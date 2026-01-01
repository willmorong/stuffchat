import { store } from './store.js';
import { $ } from './utils.js';
import { playNotificationSound, buildFileUrl, el } from './utils.js';

class Visualizer {
    constructor(stream, canvas) {
        this.stream = stream;
        this.canvas = canvas;
        this.ctx = canvas.getContext('2d');
        this.audioCtx = new (window.AudioContext)();
        this.analyser = this.audioCtx.createAnalyser();
        this.source = this.audioCtx.createMediaStreamSource(stream);
        this.source.connect(this.analyser);
        this.analyser.fftSize = 256;
        this.bufferLength = this.analyser.frequencyBinCount;
        this.dataArray = new Uint8Array(this.bufferLength);
        this.running = true;
        this.draw();
    }

    draw() {
        if (!this.running) return;
        requestAnimationFrame(() => this.draw());

        const { width, height } = this.canvas;
        this.analyser.getByteTimeDomainData(this.dataArray);

        this.ctx.clearRect(0, 0, width, height);

        this.ctx.lineWidth = 2;
        this.ctx.strokeStyle = '#4dd4ac';
        this.ctx.beginPath();

        const sliceWidth = width * 1.0 / this.bufferLength;
        let x = 0;

        for (let i = 0; i < this.bufferLength; i++) {
            const v = this.dataArray[i] / 128.0;
            const y = v * height / 2;

            if (i === 0) {
                this.ctx.moveTo(x, y);
            } else {
                this.ctx.lineTo(x, y);
            }

            x += sliceWidth;
        }

        this.ctx.lineTo(width, height / 2);
        this.ctx.stroke();
    }

    stop() {
        this.running = false;
        if (this.audioCtx.state !== 'closed') {
            this.audioCtx.close();
        }
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
                    if (store.visualizers.has(uid)) {
                        store.visualizers.get(uid).stop();
                        store.visualizers.delete(uid);
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

                const waveformContainer = el('div', { class: 'call-waveform-container' });
                const canvas = el('canvas', { class: 'call-waveform-canvas', width: 120, height: 44 });
                waveformContainer.appendChild(canvas);

                row.appendChild(info);
                row.appendChild(waveformContainer);
                participantsDiv.appendChild(row);

                // Initialize visualizer if stream is available
                if (uid === store.user.id && store.localStream) {
                    if (store.visualizers.has(uid)) store.visualizers.get(uid).stop();
                    store.visualizers.set(uid, new Visualizer(store.localStream, canvas));
                } else {
                    // Check if we have any PC for this user (any session)
                    let foundStream = false;
                    for (const [pcid, pc] of store.pcs) {
                        if (pcid.startsWith(uid + ':')) {
                            const audio = document.getElementById(`audio-${pcid}`);
                            if (audio && audio.srcObject) {
                                if (store.visualizers.has(uid)) store.visualizers.get(uid).stop();
                                store.visualizers.set(uid, new Visualizer(audio.srcObject, canvas));
                                foundStream = true;
                                break;
                            }
                        }
                    }
                }
            } else {
                // Adjust canvas size if window resized? Or just ensure it matches its container
                const canvas = row.querySelector('.call-waveform-canvas');
                const container = row.querySelector('.call-waveform-container');
                if (canvas.width !== container.clientWidth || canvas.height !== container.clientHeight) {
                    canvas.width = container.clientWidth;
                    canvas.height = container.clientHeight;
                }
            }
        });
    } else {
        callUI.style.display = 'none';
        // Cleanup visualizers if not in call
        store.visualizers.forEach(v => v.stop());
        store.visualizers.clear();

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
        updateCallUI(); // Trigger UI update to attach visualizer to the new stream
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
