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

                if (uid !== store.user.id) {
                    avatar.style.cursor = 'pointer';
                    avatar.onclick = () => {
                        const singleton = $('#voiceVolumeControl');
                        if (!singleton) return;

                        // If already open for this user, close it
                        if (singleton.dataset.uid === uid && singleton.classList.contains('visible')) {
                            singleton.classList.remove('visible');
                            setTimeout(() => {
                                if (!singleton.classList.contains('visible')) {
                                    singleton.style.display = 'none';
                                }
                            }, 500);
                            return;
                        }

                        // Otherwise, move and show it
                        singleton.dataset.uid = uid;
                        row.appendChild(singleton);

                        const slider = singleton.querySelector('.volume-slider');
                        const label = singleton.querySelector('.volume-label');
                        const initialVol = store.userVolumes[uid] !== undefined ? store.userVolumes[uid] : 1.0;

                        slider.value = initialVol;
                        label.textContent = `${Math.round(initialVol * 100)}%`;

                        slider.oninput = () => {
                            const val = parseFloat(slider.value);
                            label.textContent = `${Math.round(val * 100)}%`;
                            store.userVolumes[uid] = val;
                            localStorage.setItem('stuffchat.user_volumes', JSON.stringify(store.userVolumes));

                            for (const [pcid, pc] of store.pcs) {
                                if (pcid.startsWith(uid + ':')) {
                                    const gainNode = store.gainNodes.get(pcid);
                                    if (gainNode) {
                                        gainNode.gain.value = val;
                                    }
                                }
                            }
                        };

                        singleton.style.display = 'flex';
                        setTimeout(() => singleton.classList.add('visible'), 10);
                    };
                }

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

    peerConnection.onnegotiationneeded = async () => {
        // Only initiate negotiation if we're in a stable state to avoid glare
        if (peerConnection.signalingState !== 'stable') {
            return;
        }
        try {
            const offer = await peerConnection.createOffer();
            await peerConnection.setLocalDescription(offer);
            sendSignal(targetUserId, targetSessionId, { sdp: peerConnection.localDescription });
        } catch (e) {
            console.error('Negotiation needed error', e);
        }
    };

    peerConnection.onconnectionstatechange = () => {
        console.log(`Connection State [${pcId}]: ${peerConnection.connectionState}`);
        if (peerConnection.connectionState === 'connected') {
            // Refresh video grid when connection is fully established
            updateVideoGrid();
        }
        if (peerConnection.connectionState === 'failed' || peerConnection.connectionState === 'closed') {
            const audio = document.getElementById(`audio-${pcId}`);
            if (audio) {
                audio.remove();
            }
            if (store.gainNodes.has(pcId)) {
                store.gainNodes.get(pcId).disconnect();
                store.gainNodes.delete(pcId);
            }
            if (store.audioSources.has(pcId)) {
                store.audioSources.get(pcId).disconnect();
                store.audioSources.delete(pcId);
            }
            store.remoteVideoStreams.delete(pcId);
            store.pcs.delete(pcId);
            updateCallUI();
            updateVideoGrid();
        }
    };

    peerConnection.ontrack = (event) => {
        const track = event.track;
        const stream = event.streams[0];

        if (track.kind === 'video') {
            // Handle video track
            store.remoteVideoStreams.set(pcId, stream);
            updateVideoGrid();
            // Clean up when track ends
            track.onended = () => {
                store.remoteVideoStreams.delete(pcId);
                updateVideoGrid();
            };
        } else if (track.kind === 'audio') {
            // Handle audio track
            let audio = document.getElementById(`audio-${pcId}`);
            if (!audio) {
                audio = document.createElement('audio');
                audio.id = `audio-${pcId}`;
                audio.autoplay = true;
                document.body.appendChild(audio);
            }
            audio.srcObject = stream;
            audio.muted = true; // Use Web Audio API for playback to support boosting

            // Setup Web Audio API
            const ctx = getAudioCtx();
            const source = ctx.createMediaStreamSource(stream);
            const gainNode = ctx.createGain();
            source.connect(gainNode);
            gainNode.connect(ctx.destination);

            store.audioSources.set(pcId, source);
            store.gainNodes.set(pcId, gainNode);

            // Apply saved volume
            const initialVol = store.userVolumes[targetUserId];
            if (initialVol !== undefined) {
                gainNode.gain.value = initialVol;
            }
            updateCallUI(); // Trigger UI update to attach volume monitor to the new stream
        }
    };

    if (store.localStream) {
        store.localStream.getTracks().forEach(track => peerConnection.addTrack(track, store.localStream));
    }

    // Add video track if screen sharing is active
    if (store.localVideoStream) {
        store.localVideoStream.getTracks().forEach(track => peerConnection.addTrack(track, store.localVideoStream));
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
    updateVideoGrid();

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
            if (data.sdp.type === 'offer') {
                // Handle glare: if we're also trying to send an offer
                if (peerConnection.signalingState !== 'stable') {
                    // Polite peer (lower user ID) rolls back their offer
                    const polite = store.user.id < userId;
                    if (!polite) {
                        // We're impolite - ignore their offer, ours takes priority
                        return;
                    }
                    // We're polite - rollback and accept their offer
                    await peerConnection.setLocalDescription({ type: 'rollback' });
                }
                await peerConnection.setRemoteDescription(new RTCSessionDescription(data.sdp));
                const answer = await peerConnection.createAnswer();
                const sdp = mangleSdp(answer.sdp);
                const mangledAnswer = { type: answer.type, sdp };
                await peerConnection.setLocalDescription(mangledAnswer);
                sendSignal(userId, sessionId, { sdp: mangledAnswer });
            } else if (data.sdp.type === 'answer') {
                // Only accept answer if we have a pending offer
                if (peerConnection.signalingState !== 'have-local-offer') {
                    console.log(`Ignoring answer in state ${peerConnection.signalingState}`);
                    return;
                }
                await peerConnection.setRemoteDescription(new RTCSessionDescription(data.sdp));
            }
        } else if (data.candidate) {
            // Only add candidates if we have a remote description
            if (peerConnection.remoteDescription) {
                await peerConnection.addIceCandidate(new RTCIceCandidate(data.candidate));
            }
        }
    } catch (e) {
        // Ignore non-fatal errors like stale candidates
        if (e.name !== 'InvalidStateError' && !e.message?.includes('Unknown ufrag')) {
            console.error('Signal error', e);
        }
    }
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

    // Stop screen sharing if active
    if (store.screenSharing) {
        stopScreenShare();
    }

    if (store.localStream) {
        store.localStream.getTracks().forEach(t => t.stop());
        store.localStream = null;
    }

    store.pcs.forEach((pc, pcid) => {
        pc.close();
        const audio = document.getElementById(`audio-${pcid}`);
        if (audio) audio.remove();

        if (store.gainNodes.has(pcid)) {
            store.gainNodes.get(pcid).disconnect();
            store.gainNodes.delete(pcid);
        }
        if (store.audioSources.has(pcid)) {
            store.audioSources.get(pcid).disconnect();
            store.audioSources.delete(pcid);
        }
    });
    store.pcs.clear();
    store.remoteVideoStreams.clear();

    if (store.ws && store.ws.readyState === 1) {
        store.ws.send(JSON.stringify({ type: 'leave_call', channel_id: store.callChannelId }));
        if (store.callChannelId !== store.currentChannelId) {
            store.ws.send(JSON.stringify({ type: 'leave', channel_id: store.callChannelId }));
        }
    }

    store.callChannelId = null;
    updateCallUI();
    updateVideoGrid();
}

// Screen sharing functions
export async function startScreenShare() {
    if (store.screenSharing) return;

    let stream;
    try {
        stream = await navigator.mediaDevices.getDisplayMedia({
            video: { cursor: 'always' },
            audio: false
        });
    } catch (e) {
        console.error('Failed to get screen share', e);
        return;
    }

    store.localVideoStream = stream;
    store.screenSharing = true;

    // Add video track to all existing peer connections
    const videoTrack = stream.getVideoTracks()[0];
    store.pcs.forEach((pc, pcId) => {
        pc.addTrack(videoTrack, stream);
    });

    // Handle stream ending (user clicks browser's stop sharing)
    videoTrack.onended = () => {
        stopScreenShare();
    };

    // Renegotiate with all peers
    store.pcs.forEach(async (pc, pcId) => {
        try {
            const offer = await pc.createOffer();
            await pc.setLocalDescription(offer);
            const [userId, sessionId] = pcId.split(':');
            sendSignal(userId, sessionId, { sdp: pc.localDescription });
        } catch (e) {
            console.error('Renegotiation error', e);
        }
    });

    updateScreenShareButton();
    updateVideoGrid();
}

export function stopScreenShare() {
    if (!store.screenSharing) return;

    if (store.localVideoStream) {
        store.localVideoStream.getTracks().forEach(t => t.stop());
        store.localVideoStream = null;
    }

    store.screenSharing = false;

    // Remove video senders from all peer connections
    store.pcs.forEach((pc) => {
        const senders = pc.getSenders();
        senders.forEach(sender => {
            if (sender.track && sender.track.kind === 'video') {
                pc.removeTrack(sender);
            }
        });
    });

    updateScreenShareButton();
    updateVideoGrid();
}

function updateScreenShareButton() {
    const btn = $('#btnScreenShare');
    if (!btn) return;

    if (store.screenSharing) {
        btn.classList.add('active');
        btn.title = 'Stop Sharing';
        btn.innerHTML = '<i class="bi bi-display-fill"></i>';
    } else {
        btn.classList.remove('active');
        btn.title = 'Share Screen';
        btn.innerHTML = '<i class="bi bi-display"></i>';
    }
}

// Video grid functions
export function updateVideoGrid() {
    const grid = $('#videoGrid');
    if (!grid) return;

    // Clear existing tiles
    grid.innerHTML = '';

    // Add local video if screen sharing
    if (store.localVideoStream && store.screenSharing) {
        const tile = createVideoTile('local', store.localVideoStream, store.user?.username || 'You');
        tile.classList.add('local');
        grid.appendChild(tile);
    }

    // Add remote video streams
    store.remoteVideoStreams.forEach((stream, pcId) => {
        const [userId] = pcId.split(':');
        const user = store.users.get(userId);
        const username = user?.username || userId;
        const tile = createVideoTile(pcId, stream, username);
        grid.appendChild(tile);
    });
}

function createVideoTile(id, stream, username) {
    const tile = el('div', { class: 'video-tile', 'data-stream-id': id });
    const video = el('video', { autoplay: true, playsinline: true, muted: id === 'local' });
    video.srcObject = stream;
    tile.appendChild(video);

    const label = el('div', { class: 'video-label' }, username);
    tile.appendChild(label);

    tile.onclick = () => toggleVideoFullscreen(stream, username);

    return tile;
}

export function toggleVideoFullscreen(stream, username) {
    const overlay = $('#videoFullscreen');
    if (!overlay) return;

    if (overlay.classList.contains('hidden')) {
        // Enter fullscreen
        overlay.innerHTML = '';
        const video = el('video', { autoplay: true, playsinline: true });
        video.srcObject = stream;
        overlay.appendChild(video);
        overlay.classList.remove('hidden');

        overlay.onclick = () => {
            overlay.classList.add('hidden');
            overlay.innerHTML = '';
        };
    } else {
        // Exit fullscreen
        overlay.classList.add('hidden');
        overlay.innerHTML = '';
    }
}
