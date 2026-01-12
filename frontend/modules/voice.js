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

// Perfect Negotiation state per peer connection
// Maps pcId -> { makingOffer, ignoreOffer, isSettingRemoteAnswerPending, polite, pendingCandidates }
const negotiationState = new Map();

function getNegotiationState(pcId) {
    if (!negotiationState.has(pcId)) {
        negotiationState.set(pcId, {
            makingOffer: false,
            ignoreOffer: false,
            isSettingRemoteAnswerPending: false,
            polite: false,
            pendingCandidates: []
        });
    }
    return negotiationState.get(pcId);
}

function cleanupNegotiationState(pcId) {
    negotiationState.delete(pcId);
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

        updateMuteButton();
        updateDeafenButton();
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
        const fmtpLine = `a=fmtp:${opusPt} maxaveragebitrate=128000;stereo=1;useinbandfec=1;usedtx=1`;
        let found = false;
        for (let i = 0; i < lines.length; i++) {
            if (lines[i].startsWith(`a=fmtp:${opusPt}`)) {
                lines[i] = lines[i].trim() + ';maxaveragebitrate=128000;usedtx=1';
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

    // Initialize negotiation state for this connection
    // Polite peer has lower user ID - they will yield during glare
    const state = getNegotiationState(pcId);
    state.polite = store.user.id < targetUserId;

    peerConnection.onicecandidate = (event) => {
        if (event.candidate) {
            sendSignal(targetUserId, targetSessionId, { candidate: event.candidate });
        }
    };

    // Perfect Negotiation: onnegotiationneeded handler
    peerConnection.onnegotiationneeded = async () => {
        const state = getNegotiationState(pcId);
        try {
            state.makingOffer = true;
            // setLocalDescription with no arguments creates and sets offer automatically
            await peerConnection.setLocalDescription();
            sendSignal(targetUserId, targetSessionId, { sdp: peerConnection.localDescription });
        } catch (e) {
            console.error(`Negotiation error [${pcId}]:`, e);
        } finally {
            state.makingOffer = false;
        }
    };

    peerConnection.onconnectionstatechange = () => {
        console.log(`Connection State [${pcId}]: ${peerConnection.connectionState}`);
        if (peerConnection.connectionState === 'connected') {
            // Refresh video grid when connection is fully established
            updateVideoGrid();
        }
        if (peerConnection.connectionState === 'failed' || peerConnection.connectionState === 'closed') {
            cleanupPeerConnection(pcId);
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
            if (store.isDeafened) {
                gainNode.gain.value = 0;
            } else {
                const initialVol = store.userVolumes[targetUserId];
                if (initialVol !== undefined) {
                    gainNode.gain.value = initialVol;
                }
            }
            updateCallUI(); // Trigger UI update to attach volume monitor to the new stream
        }
    };

    // Add local audio track
    if (store.localStream) {
        store.localStream.getTracks().forEach(track => peerConnection.addTrack(track, store.localStream));
    }

    // Add video track if screen sharing is active
    if (store.localVideoStream) {
        store.localVideoStream.getTracks().forEach(track => {
            const sender = peerConnection.addTrack(track, store.localVideoStream);
            configureVideoSender(sender, peerConnection);
        });
    }

    // Initial offer only if we are the initiator
    // After this, onnegotiationneeded will handle all subsequent negotiations
    if (initiator) {
        // Trigger onnegotiationneeded by adding transceiver if no tracks yet
        // The onnegotiationneeded handler will create and send the offer
        // Since we already added tracks above, onnegotiationneeded will fire automatically
    }

    updateVideoGrid();

    return peerConnection;
}

function cleanupPeerConnection(pcId) {
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
    cleanupNegotiationState(pcId);
    updateCallUI();
    updateVideoGrid();
}

export async function handleSignal(userId, sessionId, data) {
    const pcId = `${userId}:${sessionId}`;
    let peerConnection = store.pcs.get(pcId);
    if (!peerConnection) {
        peerConnection = await createPeerConnection(userId, sessionId, false);
    }

    const state = getNegotiationState(pcId);

    try {
        if (data.sdp) {
            // Perfect Negotiation: handle offer/answer with glare detection
            const description = new RTCSessionDescription(data.sdp);
            const readyForOffer =
                !state.makingOffer &&
                (peerConnection.signalingState === 'stable' || state.isSettingRemoteAnswerPending);
            const offerCollision = description.type === 'offer' && !readyForOffer;

            state.ignoreOffer = !state.polite && offerCollision;
            if (state.ignoreOffer) {
                console.log(`[${pcId}] Ignoring colliding offer (we are impolite)`);
                // Clear any pending candidates - they're for the ignored offer's session
                // and will have mismatched ufrag/pwd when we receive the answer for our offer
                state.pendingCandidates = [];
                return;
            }

            state.isSettingRemoteAnswerPending = description.type === 'answer';
            await peerConnection.setRemoteDescription(description);
            state.isSettingRemoteAnswerPending = false;

            // Process any queued ICE candidates now that we have remote description
            if (state.pendingCandidates.length > 0) {
                for (const candidate of state.pendingCandidates) {
                    try {
                        await peerConnection.addIceCandidate(candidate);
                    } catch (e) {
                        // Ignore stale candidate errors
                        if (!e.message?.includes('Unknown ufrag')) {
                            console.warn(`Failed to add queued candidate [${pcId}]:`, e);
                        }
                    }
                }
                state.pendingCandidates = [];
            }

            if (description.type === 'offer') {
                await peerConnection.setLocalDescription();
                sendSignal(userId, sessionId, { sdp: peerConnection.localDescription });
            }
        } else if (data.candidate) {
            // Queue candidates if we don't have a remote description yet
            if (!peerConnection.remoteDescription || !peerConnection.remoteDescription.type) {
                state.pendingCandidates.push(new RTCIceCandidate(data.candidate));
            } else {
                try {
                    await peerConnection.addIceCandidate(new RTCIceCandidate(data.candidate));
                } catch (e) {
                    // Ignore stale candidate errors
                    if (!e.message?.includes('Unknown ufrag')) {
                        console.warn(`Failed to add ICE candidate [${pcId}]:`, e);
                    }
                }
            }
        }
    } catch (e) {
        // Log unexpected errors
        if (e.name !== 'InvalidStateError') {
            console.error(`Signal error [${pcId}]:`, e);
        }
    }
}

export function toggleMute() {
    store.isMuted = !store.isMuted;
    if (store.localStream) {
        store.localStream.getAudioTracks().forEach(t => {
            t.enabled = !store.isMuted;
        });
    }
    updateMuteButton();
}

export function toggleDeafen() {
    store.isDeafened = !store.isDeafened;

    // Apply to all current gain nodes
    store.gainNodes.forEach((gainNode, pcId) => {
        if (store.isDeafened) {
            gainNode.gain.value = 0;
        } else {
            // Restore saved volume or 1.0
            const [uid] = pcId.split(':');
            gainNode.gain.value = store.userVolumes[uid] !== undefined ? store.userVolumes[uid] : 1.0;
        }
    });

    updateDeafenButton();
}

function updateMuteButton() {
    const btn = $('#btnMute');
    if (!btn) return;
    if (store.isMuted) {
        btn.classList.add('danger');
        btn.innerHTML = '<i class="bi bi-mic-mute-fill"></i>';
        btn.title = 'Unmute';
    } else {
        btn.classList.remove('danger');
        btn.innerHTML = '<i class="bi bi-mic"></i>';
        btn.title = 'Mute';
    }
}

function updateDeafenButton() {
    const btn = $('#btnDeafen');
    if (!btn) return;
    if (store.isDeafened) {
        btn.classList.add('danger');
        btn.innerHTML = '<i class="bi bi-volume-off-fill"></i>';
        btn.title = 'Undeafen';
    } else {
        btn.classList.remove('danger');
        btn.innerHTML = '<i class="bi bi-volume-up-fill"></i>';
        btn.title = 'Deafen';
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
    if (store.isMuted) {
        stream.getAudioTracks().forEach(t => t.enabled = false);
    }
    store.callChannelId = store.currentChannelId;
    store.inCall = true;

    updateCallUI();
    store.ws.send(JSON.stringify({ type: 'join_call', channel_id: store.callChannelId }));

    const existingUsers = store.voiceUsers.get(store.callChannelId) || new Set();
    existingUsers.forEach(cid => {
        const [uid, sid] = cid.split(':');
        if (uid !== store.user.id) {
            // Higher user ID initiates the connection
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
        cleanupPeerConnection(pcid);
    });
    store.pcs.clear();
    store.remoteVideoStreams.clear();
    negotiationState.clear();

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

// Configure video sender for high-quality screensharing with AV1 preference
async function configureVideoSender(sender, pc) {
    // Set codec preference to AV1 if supported
    const transceiver = pc.getTransceivers().find(t => t.sender === sender);
    if (transceiver && transceiver.setCodecPreferences) {
        const capabilities = RTCRtpSender.getCapabilities('video');
        if (capabilities) {
            // Sort codecs to prefer AV1, then VP9, then VP8
            const codecs = capabilities.codecs.slice();
            codecs.sort((a, b) => {
                const order = ['video/AV1', 'video/VP9', 'video/VP8', 'video/H264'];
                const aIndex = order.findIndex(c => a.mimeType.includes(c.split('/')[1]));
                const bIndex = order.findIndex(c => b.mimeType.includes(c.split('/')[1]));
                return (aIndex === -1 ? 999 : aIndex) - (bIndex === -1 ? 999 : bIndex);
            });
            try {
                transceiver.setCodecPreferences(codecs);
            } catch (e) {
                console.warn('Could not set codec preferences:', e);
            }
        }
    }

    // Set high bitrate for smooth screensharing (8 Mbps max)
    try {
        const params = sender.getParameters();
        if (!params.encodings || params.encodings.length === 0) {
            params.encodings = [{}];
        }
        params.encodings[0].maxBitrate = 8_000_000; // 8 Mbps
        await sender.setParameters(params);
    } catch (e) {
        console.warn('Could not set video bitrate:', e);
    }
}

// Screen sharing functions
export async function startScreenShare() {
    if (store.screenSharing) return;

    let stream;
    try {
        stream = await navigator.mediaDevices.getDisplayMedia({
            video: {
                cursor: 'always',
                frameRate: { ideal: 60 }
            },
            audio: false
        });
    } catch (e) {
        console.error('Failed to get screen share', e);
        return;
    }

    store.localVideoStream = stream;
    store.screenSharing = true;

    // Add video track to all existing peer connections
    // This will trigger onnegotiationneeded automatically - no manual renegotiation needed
    const videoTrack = stream.getVideoTracks()[0];
    store.pcs.forEach((pc, pcId) => {
        const sender = pc.addTrack(videoTrack, stream);
        configureVideoSender(sender, pc);
    });

    // Handle stream ending (user clicks browser's stop sharing)
    videoTrack.onended = () => {
        stopScreenShare();
    };

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
    // This will trigger onnegotiationneeded automatically
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
