import { store } from './store.js';
import { $ } from './utils.js';
import { playNotificationSound, buildFileUrl, el } from './utils.js';

let sharedAudioCtx = null;
function getAudioCtx() {
    if (!sharedAudioCtx) {
        sharedAudioCtx = new (window.AudioContext || window.webkitAudioContext)();
    }
    if (sharedAudioCtx.state === 'suspended') {
        sharedAudioCtx.resume();
    }
    return sharedAudioCtx;
}

class VolumeMonitor {
    constructor(stream, element) {
        this.stream = stream;
        this.element = element;
        this.audioCtx = getAudioCtx();
        this.analyser = this.audioCtx.createAnalyser();
        this.source = this.audioCtx.createMediaStreamSource(stream);
        this.source.connect(this.analyser);
        this.analyser.fftSize = 256;
        this.dataArray = new Uint8Array(this.analyser.frequencyBinCount);
        this.running = true;
        this.update();
    }

    update() {
        if (!this.running) return;
        requestAnimationFrame(() => this.update());

        this.analyser.getByteFrequencyData(this.dataArray);
        let sum = 0;
        for (let i = 0; i < this.dataArray.length; i++) {
            sum += this.dataArray[i];
        }
        const average = sum / this.dataArray.length;

        // Map average volume (0-255) to border size (0-5px)
        const borderSize = Math.min(5, (average / 30) * 5);
        this.element.style.boxShadow = `0 0 0 ${borderSize}px #4dd4ac`;
    }

    stop() {
        this.running = false;
        if (this.source) {
            this.source.disconnect();
        }
        if (this.analyser) {
            this.analyser.disconnect();
        }
        this.element.style.boxShadow = 'none';
    }
}

export function updateCallUI() {
    const ch = store.channels.find(c => c.id === store.currentChannelId);
    if (!ch) return;
    const isVoice = ch.is_voice;

    const btnStart = $('#btnStartCall');
    const callUI = $('#callInterface');
    const participantsDiv = $('#callParticipants');

    if (!btnStart || !callUI || !participantsDiv) return;

    const voiceUsersHere = store.voiceUsers.get(store.currentChannelId) || new Set();

    if (store.inCall) {
        btnStart.style.display = 'none';
        callUI.style.display = 'flex';

        const callChan = store.channels.find(c => c.id === store.callChannelId);
        $('.call-status').textContent = (store.callChannelId === store.currentChannelId)
            ? 'Voice Connected'
            : `Voice Connected (${callChan?.name || 'Unknown'})`;

        const callUsersComposite = store.voiceUsers.get(store.callChannelId) || new Set();
        const callUsersSet = new Set();
        callUsersComposite.forEach(cid => callUsersSet.add(cid.split(':')[0]));

        // Remove rows for users no longer in call
        const currentUids = Array.from(callUsersSet);
        participantsDiv.querySelectorAll('.call-participant-row').forEach(row => {
            const uid = row.dataset.uid;
            if (!currentUids.includes(uid)) {
                // Check if any session for this user is still in the call
                const userStillIn = Array.from(callUsersComposite).some(cid => cid.startsWith(uid + ':'));
                if (!userStillIn) {
                    if (store.volumeMonitors.has(uid)) {
                        store.volumeMonitors.get(uid).stop();
                        store.volumeMonitors.delete(uid);
                    }
                    row.remove();
                }
            }
        });

        // Add or update rows for users in call
        currentUids.forEach(uid => {
            let row = participantsDiv.querySelector(`.call-participant-row[data-uid="${uid}"]`);
            if (!row) {
                const u = store.users.get(uid);
                row = el('div', { class: 'call-participant-row', 'data-uid': uid });

                const info = el('div', { class: 'call-participant-info' });
                const avatar = el('div', { class: 'avatar' });
                if (u && u.avatar_file_id) {
                    avatar.appendChild(el('img', { src: buildFileUrl(u.avatar_file_id, 'avatar') }));
                }
                info.appendChild(avatar);
                info.appendChild(el('div', { class: 'username' }, u?.username || uid));

                row.appendChild(info);
                participantsDiv.appendChild(row);
            }

            // Always check if we need to initialize or update the volume monitor
            const avatar = row.querySelector('.avatar');
            if (uid === store.user.id && store.localStream) {
                if (!store.volumeMonitors.has(uid)) {
                    store.volumeMonitors.set(uid, new VolumeMonitor(store.localStream, avatar));
                }
            } else {
                // Check if we have any PC for this user (any session)
                for (const [pcid, pc] of store.pcs) {
                    if (pcid.startsWith(uid + ':')) {
                        const audio = document.getElementById(`audio-${pcid}`);
                        if (audio && audio.srcObject && !store.volumeMonitors.has(uid)) {
                            store.volumeMonitors.set(uid, new VolumeMonitor(audio.srcObject, avatar));
                            break;
                        }
                    }
                }
            }
        });
    } else {
        callUI.style.display = 'none';
        // Cleanup volume monitors if not in call
        store.volumeMonitors.forEach(v => v.stop());
        store.volumeMonitors.clear();

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

function sendSignal(toUserId, toSessionId, data) {
    if (store.ws && store.ws.readyState === 1) {
        store.ws.send(JSON.stringify({
            type: 'webrtc_signal',
            channel_id: store.callChannelId,
            to_user_id: toUserId,
            to_session_id: toSessionId,
            from_session_id: store.sessionId,
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

export async function createPeerConnection(targetUserId, targetSessionId, initiator) {
    const pcId = `${targetUserId}:${targetSessionId}`;
    if (store.pcs.has(pcId)) return store.pcs.get(pcId);

    const peerConnection = new RTCPeerConnection({
        iceServers: [{ urls: 'stun:stun.l.google.com:19302' }]
    });
    store.pcs.set(pcId, peerConnection);

    peerConnection.onicecandidate = (event) => {
        if (event.candidate) sendSignal(targetUserId, targetSessionId, { candidate: event.candidate });
    };

    peerConnection.onconnectionstatechange = () => {
        console.log(`Connection State [${pcId}]: ${peerConnection.connectionState}`);
        if (peerConnection.connectionState === 'failed' || peerConnection.connectionState === 'closed') {
            const audio = document.getElementById(`audio-${pcId}`);
            if (audio) audio.remove();
            store.pcs.delete(pcId);
            updateCallUI();
        }
    };

    peerConnection.ontrack = (event) => {
        const stream = event.streams[0];
        let audio = document.getElementById(`audio-${pcId}`);
        if (!audio) {
            audio = document.createElement('audio');
            audio.id = `audio-${pcId}`;
            audio.autoplay = true;
            document.body.appendChild(audio);
        }
        audio.srcObject = stream;
        updateCallUI(); // Trigger UI update to attach volume monitor to the new stream
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
            sendSignal(targetUserId, targetSessionId, { sdp: mangledOffer });
        } catch (e) { console.error('Offer error', e); }
    }

    return peerConnection;
}

export async function handleSignal(userId, sessionId, data) {
    const pcId = `${userId}:${sessionId}`;
    let peerConnection = store.pcs.get(pcId);
    if (!peerConnection) {
        peerConnection = await createPeerConnection(userId, sessionId, false);
    }
    try {
        if (data.sdp) {
            await peerConnection.setRemoteDescription(new RTCSessionDescription(data.sdp));
            if (data.sdp.type === 'offer') {
                const answer = await peerConnection.createAnswer();
                const sdp = mangleSdp(answer.sdp);
                const mangledAnswer = { type: answer.type, sdp };
                await peerConnection.setLocalDescription(mangledAnswer);
                sendSignal(userId, sessionId, { sdp: mangledAnswer });
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
    existingUsers.forEach(cid => {
        const [uid, sid] = cid.split(':');
        if (uid !== store.user.id) {
            const shouldInitiate = store.user.id > uid;
            createPeerConnection(uid, sid, shouldInitiate);
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

    store.pcs.forEach((pc, pcid) => {
        pc.close();
        const audio = document.getElementById(`audio-${pcid}`);
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
