import { store } from './store.js';
import { toWsUrl, $, truncateId, playNotificationSound } from './utils.js';
import { fetchUser } from './users.js';
import { renderMessages, renderMessageItem, isScrolledToBottom, scrollToBottom } from './messages.js';
import { updateCallUI, createPeerConnection, handleSignal } from './voice.js';

export function connectWs(reconnect = false) {
    const url = toWsUrl(store.baseUrl);
    if (!url || !store.accessToken) return;
    try {
        if (store.ws) {
            console.log('WebSocket already open');
            return;
        }
        const ws = new WebSocket(url + '?token=' + encodeURIComponent(store.accessToken));
        store.ws = ws;
        ws.onopen = () => {
            // Rejoin current channel
            if (store.currentChannelId) {
                ws.send(JSON.stringify({ type: 'join', channel_id: store.currentChannelId }));
            }
            // Rejoin call channel if different
            if (store.callChannelId && store.callChannelId !== store.currentChannelId) {
                ws.send(JSON.stringify({ type: 'join', channel_id: store.callChannelId }));
            }
        };
        ws.onmessage = (ev) => {
            try {
                const msg = JSON.parse(ev.data);
                handleWsMessage(msg);
            } catch (e) { console.warn('WS parse error', e) }
        };
        ws.onclose = () => {
            if (store.inCall) {
                console.warn('WebSocket closed while in call, cleaning up');
                store.inCall = false;
                if (store.localStream) {
                    store.localStream.getTracks().forEach(t => t.stop());
                    store.localStream = null;
                }
                store.pcs.forEach((pc, pcid) => {
                    pc.close();
                    const audio = document.getElementById(`audio-${pcid}`);
                    if (audio) audio.remove();
                });
                store.visualizers.forEach(v => v.stop());
                store.visualizers.clear();
                store.pcs.clear();
                store.callChannelId = null;
                updateCallUI();
            }
            if (store.accessToken) {
                setTimeout(() => connectWs(true), 2000);
            }
        };
    } catch (e) { console.warn('WS connect error', e.message); }
}

export function handleWsMessage(ev) {
    switch (ev.type) {
        case 'message_created': {
            if (!ev.file_url && ev.file_id) {
                ev.file_url = `/files/${ev.file_id}/file`;
            }
            if (ev.user_id && !store.users.has(ev.user_id)) fetchUser(ev.user_id);
            const arr = store.messages.get(ev.channel_id) || [];
            if (!arr.some(m => m.id === ev.id)) {
                arr.push(ev);
                store.messages.set(ev.channel_id, arr);
            }
            if (ev.channel_id === store.currentChannelId) {
                const atBottom = isScrolledToBottom();
                $('#messages').appendChild(renderMessageItem(ev));
                if (atBottom) scrollToBottom();
            }
            break;
        }
        case 'message_edited': {
            const arr = store.messages.get(ev.channel_id) || [];
            const m = arr.find(x => x.id === ev.id);
            if (m) { m.content = ev.content; m.edited_at = ev.edited_at; }
            if (ev.channel_id === store.currentChannelId) renderMessages(ev.channel_id);
            break;
        }
        case 'message_deleted': {
            const arr = store.messages.get(ev.channel_id) || [];
            const idx = arr.findIndex(x => x.id === ev.id);
            if (idx >= 0) { arr.splice(idx, 1); }
            if (ev.channel_id === store.currentChannelId) renderMessages(ev.channel_id);
            break;
        }
        case 'chat_message': {
            if (ev.channel_id === store.currentChannelId) {
                const pseudo = {
                    id: 'ephemeral-' + Math.random().toString(36).slice(2),
                    channel_id: ev.channel_id,
                    user_id: ev.user_id,
                    content: '[ephemeral] ' + ev.content,
                    created_at: new Date().toISOString()
                };
                const atBottom = isScrolledToBottom();
                $('#messages').appendChild(renderMessageItem(pseudo));
                if (atBottom) scrollToBottom();
            }
            break;
        }
        case 'typing': {
            if (ev.channel_id !== store.currentChannelId) break;
            if (ev.started) {
                store.typingUsers.add(ev.user_id);
                updateTypingIndicator();
                if (store.typingTimers.has(ev.user_id)) clearTimeout(store.typingTimers.get(ev.user_id));
                store.typingTimers.set(ev.user_id, setTimeout(() => {
                    store.typingUsers.delete(ev.user_id);
                    updateTypingIndicator();
                }, 3000));
            } else {
                store.typingUsers.delete(ev.user_id);
                updateTypingIndicator();
            }
            break;
        }
        case 'pong': break;
        case 'connection_metadata': {
            store.sessionId = ev.session_id;
            console.log('Session ID:', store.sessionId);
            break;
        }
        case 'room_state': {
            const chanId = ev.channel_id || store.currentChannelId;
            // Room state now contains pairs of [user_id, session_id]
            const users = new Set();
            (ev.voice_users || []).forEach(([uid, sid]) => {
                users.add(`${uid}:${sid}`);
            });
            store.voiceUsers.set(chanId, users);
            updateCallUI();
            break;
        }
        case 'voice_joined': {
            if (!store.voiceUsers.has(ev.channel_id)) store.voiceUsers.set(ev.channel_id, new Set());
            const compositeid = `${ev.user_id}:${ev.session_id}`;
            store.voiceUsers.get(ev.channel_id).add(compositeid);
            updateCallUI();
            if (store.inCall && ev.channel_id === store.callChannelId && ev.user_id !== store.user.id) {
                // If we are in call, and someone joins, we might need to connect to them.
                // We use (user_id, session_id) for the peer connection.
                const shouldInitiate = store.user.id > ev.user_id;
                createPeerConnection(ev.user_id, ev.session_id, shouldInitiate);
            }
            playNotificationSound('join');
            break;
        }
        case 'voice_left': {
            if (store.voiceUsers.has(ev.channel_id)) {
                // Remove all sessions for this user
                const users = store.voiceUsers.get(ev.channel_id);
                for (const cid of users) {
                    if (cid.startsWith(ev.user_id + ':')) {
                        users.delete(cid);
                        const sid = cid.split(':')[1];
                        if (ev.channel_id === store.callChannelId && store.pcs.has(`${ev.user_id}:${sid}`)) {
                            store.pcs.get(`${ev.user_id}:${sid}`).close();
                            store.pcs.delete(`${ev.user_id}:${sid}`);
                        }
                    }
                }
            }
            updateCallUI();
            playNotificationSound('leave');
            break;
        }
        case 'webrtc_signal': {
            if (ev.channel_id !== store.callChannelId) break;
            if (!store.inCall) break;
            if (ev.from_user_id === store.user.id && ev.from_session_id === store.sessionId) break;

            // If it's targeted at us, or untargeted (legacy/broadcast)
            if (!ev.to_session_id || ev.to_session_id === store.sessionId) {
                handleSignal(ev.from_user_id, ev.from_session_id, ev.data);
            }
            break;
        }
        default: break;
    }
}

export function updateTypingIndicator() {
    const elTip = $('#typingIndicator');
    if (store.typingUsers.size) {
        const sample = [...store.typingUsers][0];
        let name = '';
        if (sample === store.user?.id) {
            name = store.user?.username || 'You';
        } else {
            const u = store.users.get(sample);
            name = u?.username || truncateId(sample);
        }
        elTip.textContent = (store.typingUsers.size > 1) ? 'Several people are typing…' : (name + ' is typing…');
        elTip.style.display = '';
    } else {
        elTip.style.display = 'none';
    }
}

export function sendTyping(started) {
    if (store.ws && store.ws.readyState === 1 && store.currentChannelId) {
        store.ws.send(JSON.stringify({ type: 'typing', channel_id: store.currentChannelId, started }));
    }
}
