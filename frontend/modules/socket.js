import { store } from './store.js';
import { toWsUrl, $, truncateId, playNotificationSound } from './utils.js';
import { fetchUser } from './users.js';
import { renderMessages, renderMessageItem, isScrolledToBottom, scrollToBottom, updateMessageReactions } from './messages.js';
import { closeAllPeerConnections, updateCallUI, createPeerConnection, closePeerConnection, handleSignal } from './voice.js';
import { renderChannelList, markChannelRead } from './channels.js';
import { sharePlay } from './shareplay.js';

let reconnectTimer = null;
let heartbeatTimer = null;
let heartbeatTimeout = null;

const WS_HEARTBEAT_INTERVAL_MS = 15000;
const WS_HEARTBEAT_TIMEOUT_MS = 30000;

function clearHeartbeatTimeout() {
    if (heartbeatTimeout) {
        clearTimeout(heartbeatTimeout);
        heartbeatTimeout = null;
    }
}

function startHeartbeat() {
    if (heartbeatTimer) return;
    heartbeatTimer = setInterval(() => {
        if (!store.ws || store.ws.readyState !== WebSocket.OPEN) return;
        if (heartbeatTimeout) {
            console.warn('WebSocket heartbeat timeout elapsed, reconnecting');
            try {
                store.ws.close();
            } catch { }
            clearHeartbeatTimeout();
            return;
        }
        try {
            store.ws.send(JSON.stringify({ type: 'ping' }));
            heartbeatTimeout = setTimeout(() => {
                if (store.ws && store.ws.readyState === WebSocket.OPEN) {
                    console.warn('WebSocket heartbeat timeout elapsed, reconnecting');
                    try {
                        store.ws.close();
                    } catch { }
                }
            }, WS_HEARTBEAT_TIMEOUT_MS);
        } catch (e) {
            console.warn('Failed to send heartbeat:', e);
        }
    }, WS_HEARTBEAT_INTERVAL_MS);
}

function stopHeartbeat() {
    if (heartbeatTimer) {
        clearInterval(heartbeatTimer);
        heartbeatTimer = null;
    }
    clearHeartbeatTimeout();
}

function scheduleReconnect() {
    if (reconnectTimer || !store.accessToken) return;
    reconnectTimer = setTimeout(() => {
        reconnectTimer = null;
        if (!store.ws && store.accessToken) {
            connectWs();
        }
    }, 2000);
}

export function connectWs(reconnect = false) {
    const url = toWsUrl(store.baseUrl);
    if (!url || !store.accessToken) return;
    try {
        if (reconnectTimer) {
            clearTimeout(reconnectTimer);
            reconnectTimer = null;
        }
        if (store.ws) {
            if (!reconnect && (store.ws.readyState === WebSocket.OPEN || store.ws.readyState === WebSocket.CONNECTING)) {
                // Console log disabled to reduce noise
                // console.log('WebSocket already open or connecting');
                return;
            }

            const previousWs = store.ws;
            store.ws = null;
            previousWs.onclose = null;
            stopHeartbeat();
            try { previousWs.close(); } catch { }
        }
        const ws = new WebSocket(url + '?token=' + encodeURIComponent(store.accessToken));
        store.ws = ws;
        ws.onopen = () => {
            if (store.ws !== ws) return;
            startHeartbeat();
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
            if (store.ws !== ws) return;
            store.ws = null;
            stopHeartbeat();
            if (store.inCall) {
                console.warn('WebSocket closed while in call, cleaning up');
                store.inCall = false;
                if (store.localStream) {
                    store.localStream.getTracks().forEach(t => t.stop());
                    store.localStream = null;
                }
                closeAllPeerConnections();
                store.volumeMonitors.forEach(v => v.stop());
                store.volumeMonitors.clear();
                store.callChannelId = null;
                updateCallUI();
            }
            if (store.accessToken) {
                scheduleReconnect();
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

            // Update channel last_message_at
            const chItem = store.channels.find(c => c.id === ev.channel_id);
            if (chItem) chItem.last_message_at = ev.created_at;

            if (ev.channel_id === store.currentChannelId) {
                const atBottom = isScrolledToBottom();
                $('#messages').appendChild(renderMessageItem(ev));
                if (atBottom) scrollToBottom();

                // Mark read immediately if in channel and visible
                if (!document.hidden) {
                    markChannelRead(ev.channel_id, ev);
                }
            } else {
                renderChannelList();
            }

            // Show notification if message is unread (different channel OR document hidden)
            const shouldNotify = ev.channel_id !== store.currentChannelId || document.hidden;
            if (shouldNotify && Notification.permission === 'granted' && chItem && ev.user_id !== store.user.id) {
                const n = new Notification(`${store.users.get(ev.user_id)?.username || 'Someone'} (#${chItem.name})`, {
                    body: `${ev.content || 'Sent a file'}`,
                    icon: '/img/favicon.png'
                });
                n.onclick = () => { window.focus(); };
                playNotificationSound('message');
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
        case 'reaction_updated': {
            const arr = store.messages.get(ev.channel_id) || [];
            const m = arr.find(x => x.id === ev.message_id);
            if (m) m.reactions = ev.reactions;
            if (ev.channel_id === store.currentChannelId) {
                updateMessageReactions(ev.message_id, ev.reactions);
            }
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
        case 'pong': {
            clearHeartbeatTimeout();
            break;
        }
        case 'connection_metadata': {
            store.sessionId = ev.session_id;
            console.log('Session ID:', store.sessionId);
            // Calculate clock offset for SharePlay sync
            if (ev.server_time) {
                const serverTime = new Date(ev.server_time).getTime();
                const clientTime = Date.now();
                store.timeOffset = serverTime - clientTime;
                console.log('Clock offset:', store.timeOffset, 'ms (positive = server ahead)');
            }
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
        case 'shareplay_state': {
            if (ev.channel_id === store.callChannelId) {
                sharePlay.sync(ev.state, ev.channel_id);

                $('#btnSharePlay').innerHTML = '<i class="bi bi-collection-play-fill"></i>';
            }
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
            if (store.inCall && ev.channel_id === store.callChannelId) {
                playNotificationSound('join');
            }

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
                        if (ev.channel_id === store.callChannelId) {
                            closePeerConnection(`${ev.user_id}:${sid}`);
                        }
                    }
                }
            }
            updateCallUI();
            if (store.inCall && ev.channel_id === store.callChannelId) {
                playNotificationSound('leave');
            }

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
        case 'shareplay_update': {
            if (ev.channel_id === store.callChannelId) {
                sharePlay.sync(ev.state, ev.channel_id);
                if (ev.state.status === 'playing' || ev.state.queue.length > 0) {

                    $('#btnSharePlay').innerHTML = '<i class="bi bi-collection-play-fill"></i>';
                }
            }
            break;
        }
        case 'user_updated': {
            // Force fetch updated user data (this re-renders messages if in current channel)
            fetchUser(ev.user_id, true);
            // If it's the current user, refresh sidebar "me" section
            if (ev.user_id === store.user?.id) {
                import('./users.js').then(m => m.loadMe());
            }
            // Refresh members modal if it happens to be open
            import('./presence.js').then(m => m.refreshMembersModalIfOpen());
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
