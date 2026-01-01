import { store } from './store.js';
import { $ } from './utils.js';
import { playNotificationSound, buildFileUrl, el } from './utils.js';

export function updateCallUI() {
    const ch = store.channels.find(c => c.id === store.currentChannelId);
    if (!ch) return;
    const isVoice = ch.is_voice;

    const btnStart = $('#btnStartCall');
    const callUI = $('#callInterface');
    const participantsDiv = $('#callParticipants');

    // Safety check if elements exist (in case UI not fully bound yet)
    if (!btnStart || !callUI || !participantsDiv) return;

    const voiceUsersHere = store.voiceUsers.get(store.currentChannelId) || new Set();

    if (store.inCall) {
        btnStart.style.display = 'none';
        callUI.style.display = 'flex';

        const callChan = store.channels.find(c => c.id === store.callChannelId);
        $('.call-status').textContent = (store.callChannelId === store.currentChannelId)
            ? 'Voice Connected'
            : `Voice Connected (${callChan?.name || 'Unknown'})`;

        participantsDiv.innerHTML = '';
        const callUsers = store.voiceUsers.get(store.callChannelId) || new Set();
        callUsers.forEach(uid => {
            const u = store.users.get(uid);
            const div = el('div', { class: 'call-participant', title: u?.username || uid });
            if (u && u.avatar_file_id) {
                div.appendChild(el('img', { src: buildFileUrl(u.avatar_file_id, 'avatar') }));
            }
            participantsDiv.appendChild(div);
        });
    } else {
        callUI.style.display = 'none';
        if (isVoice) {
            btnStart.style.display = 'block';
            btnStart.style.marginLeft = 'auto';
            btnStart.style.width = 'fit-content';
            btnStart.textContent = voiceUsersHere.size > 0 ? 'Join Call' : 'Start Call';
            btnStart.className = voiceUsersHere.size > 0 ? 'button small success' : 'button small';
        } else {
            btnStart.style.display = 'none';
        }
    }
}

function sendSignal(toUserId, data) {
    if (store.ws && store.ws.readyState === 1) {
        store.ws.send(JSON.stringify({
            type: 'webrtc_signal',
            channel_id: store.callChannelId,
            to_user_id: toUserId,
            data
        }));
    }
}

function mangleSdp(sdp) {
    const lines = sdp.split('\n');
    let opusPt = null;
    for (let l of lines) {
        if (l.startsWith('a=rtpmap:') && l.includes('opus/48000/2')) {
            opusPt = l.split(':')[1].split(' ')[0];
            break;
        }
    }
    if (opusPt) {
        const fmtpLine = `a=fmtp:${opusPt} maxaveragebitrate=96000;stereo=1;useinbandfec=1;usedtx=1`;
        let found = false;
        for (let i = 0; i < lines.length; i++) {
            if (lines[i].startsWith(`a=fmtp:${opusPt}`)) {
                lines[i] = lines[i].trim() + ';maxaveragebitrate=96000;usedtx=1';
                found = true;
                break;
            }
        }
        if (!found) {
            for (let i = 0; i < lines.length; i++) {
                if (lines[i].startsWith(`a=rtpmap:${opusPt}`)) {
                    lines.splice(i + 1, 0, fmtpLine);
                    break;
                }
            }
        }
    }
    return lines.join('\n');
}

export async function createPeerConnection(targetUserId, initiator) {
    if (store.pcs.has(targetUserId)) return store.pcs.get(targetUserId);

    const peerConnection = new RTCPeerConnection({
        iceServers: [{ urls: 'stun:stun.l.google.com:19302' }]
    });
    store.pcs.set(targetUserId, peerConnection);

    peerConnection.onicecandidate = (event) => {
        if (event.candidate) sendSignal(targetUserId, { candidate: event.candidate });
    };

    peerConnection.onconnectionstatechange = () => {
        console.log(`Connection State [${targetUserId}]: ${peerConnection.connectionState}`);
        if (peerConnection.connectionState === 'failed' || peerConnection.connectionState === 'closed') {
            const audio = document.getElementById(`audio-${targetUserId}`);
            if (audio) audio.remove();
            store.pcs.delete(targetUserId);
        }
    };

    peerConnection.ontrack = (event) => {
        const stream = event.streams[0];
        let audio = document.getElementById(`audio-${targetUserId}`);
        if (!audio) {
            audio = document.createElement('audio');
            audio.id = `audio-${targetUserId}`;
            audio.autoplay = true;
            document.body.appendChild(audio);
        }
        audio.srcObject = stream;
    };

    if (store.localStream) {
        store.localStream.getTracks().forEach(track => peerConnection.addTrack(track, store.localStream));
    }

    if (initiator) {
        try {
            const offer = await peerConnection.createOffer();
            const sdp = mangleSdp(offer.sdp);
            const mangledOffer = { type: offer.type, sdp };
            await peerConnection.setLocalDescription(mangledOffer);
            sendSignal(targetUserId, { sdp: mangledOffer });
        } catch (e) { console.error('Offer error', e); }
    }

    return peerConnection;
}

export async function handleSignal(userId, data) {
    let peerConnection = store.pcs.get(userId);
    if (!peerConnection) {
        peerConnection = await createPeerConnection(userId, false);
    }
    try {
        if (data.sdp) {
            await peerConnection.setRemoteDescription(new RTCSessionDescription(data.sdp));
            if (data.sdp.type === 'offer') {
                const answer = await peerConnection.createAnswer();
                const sdp = mangleSdp(answer.sdp);
                const mangledAnswer = { type: answer.type, sdp };
                await peerConnection.setLocalDescription(mangledAnswer);
                sendSignal(userId, { sdp: mangledAnswer });
            }
        } else if (data.candidate) {
            await peerConnection.addIceCandidate(new RTCIceCandidate(data.candidate));
        }
    } catch (e) { console.error('Signal error', e); }
}

export async function startCall() {
    if (store.inCall) return;
    let stream;
    try {
        stream = await navigator.mediaDevices.getUserMedia({
            audio: {
                echoCancellation: store.echoCancellation,
                noiseSuppression: store.noiseSuppression,
                autoGainControl: store.autoGainControl
            },
            video: false
        });
    } catch (e) {
        console.error('Failed to get media', e);
        alert('Could not access microphone: ' + e.message);
        return;
    }

    store.localStream = stream;
    store.callChannelId = store.currentChannelId;
    store.inCall = true;

    updateCallUI();
    store.ws.send(JSON.stringify({ type: 'join_call', channel_id: store.callChannelId }));

    const existingUsers = store.voiceUsers.get(store.callChannelId) || new Set();
    existingUsers.forEach(uid => {
        if (uid !== store.user.id) {
            const shouldInitiate = store.user.id > uid;
            createPeerConnection(uid, shouldInitiate);
        }
    });
}

export function leaveCall() {
    if (!store.inCall) return;
    store.inCall = false;

    if (store.localStream) {
        store.localStream.getTracks().forEach(t => t.stop());
        store.localStream = null;
    }

    store.pcs.forEach((pc) => {
        pc.close();
    });
    // Remove all audio elements
    store.pcs.forEach((_, uid) => {
        const audio = document.getElementById(`audio-${uid}`);
        if (audio) audio.remove();
    });
    store.pcs.clear();

    if (store.ws && store.ws.readyState === 1) {
        store.ws.send(JSON.stringify({ type: 'leave_call', channel_id: store.callChannelId }));
        if (store.callChannelId !== store.currentChannelId) {
            store.ws.send(JSON.stringify({ type: 'leave', channel_id: store.callChannelId }));
        }
    }

    store.callChannelId = null;
    updateCallUI();
}
