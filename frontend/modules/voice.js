import { store } from './store.js';
import { $ } from './utils.js';
import { playNotificationSound, buildFileUrl, el } from './utils.js';
import { sharePlay } from './shareplay.js';

let sharedAudioCtx = null;

/**
 * Returns the shared audio context, creating it if it doesn't exist.
 */
function getAudioCtx() {
    if (!sharedAudioCtx) {
        sharedAudioCtx = new (window.AudioContext || window.webkitAudioContext)();
    }
    if (sharedAudioCtx.state === 'suspended') {
        sharedAudioCtx.resume();
    }
    return sharedAudioCtx;
}

/**
 * Periodically calculates and updates the visual volume indicator for a stream.
 */
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
        this.lastBorderSize = -1;
        this.intervalId = setInterval(() => this.update(), 16);
    }

    update() {
        if (!this.running) return;

        this.analyser.getByteFrequencyData(this.dataArray);
        let sum = 0;
        for (let i = 0; i < this.dataArray.length; i++) {
            sum += this.dataArray[i];
        }
        const average = sum / this.dataArray.length;

        const borderSize = Math.round(Math.min(5, (average / 30) * 5));
        if (borderSize !== this.lastBorderSize) {
            this.element.style.outline = borderSize > 0
                ? `${borderSize}px solid var(--accent-2)`
                : 'none';
            this.lastBorderSize = borderSize;
        }
    }

    stop() {
        this.running = false;
        if (this.intervalId) {
            clearInterval(this.intervalId);
            this.intervalId = null;
        }
        if (this.source) {
            this.source.disconnect();
        }
        if (this.analyser) {
            this.analyser.disconnect();
        }
        this.element.style.boxShadow = 'none';
    }
}

const negotiationState = new Map();

/**
 * Retrieves or initializes the perfect negotiation state for a peer connection.
 */
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

/**
 * Clears the negotiation state for a specific peer connection.
 */
function cleanupNegotiationState(pcId) {
    negotiationState.delete(pcId);
}

/**
 * Updates the visibility and content of the voice call user interface.
 */
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

        const currentUids = Array.from(callUsersSet);
        participantsDiv.querySelectorAll('.call-participant-row').forEach(row => {
            const uid = row.dataset.uid;
            if (!currentUids.includes(uid)) {
                const userStillIn = Array.from(callUsersComposite).some(cid => cid.startsWith(uid + ':'));
                if (!userStillIn) {
                    if (store.volumeMonitors.has(uid)) {
                        store.volumeMonitors.get(uid).stop();
                        store.volumeMonitors.delete(uid);
                    }
                    const singleton = $('#voiceVolumeControl');
                    if (singleton && row.contains(singleton)) {
                        singleton.classList.remove('visible');
                        singleton.style.display = 'none';
                        document.body.appendChild(singleton);
                    }
                    row.remove();
                }
            }
        });

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

                        if (singleton.dataset.uid === uid && singleton.classList.contains('visible')) {
                            singleton.classList.remove('visible');
                            setTimeout(() => {
                                if (!singleton.classList.contains('visible')) {
                                    singleton.style.display = 'none';
                                }
                            }, 500);
                            return;
                        }

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

            const avatar = row.querySelector('.avatar');
            if (uid === store.user.id && store.localStream) {
                if (!store.volumeMonitors.has(uid)) {
                    store.volumeMonitors.set(uid, new VolumeMonitor(store.localStream, avatar));
                }
            } else {
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
        const iconsDiv = $('#voiceCallParticipantsIcons');
        if (iconsDiv) iconsDiv.innerHTML = '';
    } else {
        callUI.style.display = 'none';
        store.volumeMonitors.forEach(v => v.stop());
        store.volumeMonitors.clear();

        if (isVoice) {
            btnStart.style.display = 'block';
            btnStart.style.marginLeft = '6px';
            btnStart.innerHTML = voiceUsersHere.size > 0 ? '<i class="bi bi-telephone-fill"></i>' : '<i class="bi bi-telephone"></i>';
            btnStart.className = voiceUsersHere.size > 0 ? 'iconbtn own' : 'iconbtn';

            const iconsDiv = $('#voiceCallParticipantsIcons');
            if (iconsDiv) {
                iconsDiv.innerHTML = '';
                if (voiceUsersHere.size > 0) {
                    const uniqueUids = new Set();
                    voiceUsersHere.forEach(cid => uniqueUids.add(cid.split(':')[0]));

                    uniqueUids.forEach(uid => {
                        const u = store.users.get(uid);
                        if (!u) return;
                        const icon = el('div', { class: 'participant-icon', title: u.username });
                        if (u.avatar_file_id) {
                            icon.appendChild(el('img', { src: buildFileUrl(u.avatar_file_id, 'avatar'), alt: u.username }));
                        }
                        iconsDiv.appendChild(icon);
                    });
                }
            }
        } else {
            btnStart.style.display = 'none';
            const iconsDiv = $('#voiceCallParticipantsIcons');
            if (iconsDiv) iconsDiv.innerHTML = '';
        }
    }
}

/**
 * Sends a WebRTC signaling message to a specific user session via WebSocket.
 */
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

/**
 * Modifies the SDP to prefer Opus and set specific bitrate/channels.
 */
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
        const fmtpLine = `a=fmtp:${opusPt} maxaveragebitrate=128000;stereo=0;useinbandfec=1;usedtx=1`;
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

/**
 * Creates and configures a new RTCPeerConnection for a target user.
 */
export async function createPeerConnection(targetUserId, targetSessionId, initiator) {
    const pcId = `${targetUserId}:${targetSessionId}`;
    if (store.pcs.has(pcId)) return store.pcs.get(pcId);

    const peerConnection = new RTCPeerConnection({
        iceServers: [{ urls: 'stun:stun.l.google.com:19302' }]
    });
    store.pcs.set(pcId, peerConnection);

    const state = getNegotiationState(pcId);
    state.polite = store.user.id < targetUserId;

    peerConnection.onicecandidate = (event) => {
        if (event.candidate) {
            sendSignal(targetUserId, targetSessionId, { candidate: event.candidate });
        }
    };

    peerConnection.onnegotiationneeded = async () => {
        const state = getNegotiationState(pcId);
        try {
            state.makingOffer = true;
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
            store.remoteVideoStreams.set(pcId, stream);
            updateVideoGrid();

            const cleanupVideo = () => {
                store.remoteVideoStreams.delete(pcId);
                if (store.screenShareGainNodes.has(pcId)) {
                    store.screenShareGainNodes.get(pcId).disconnect();
                    store.screenShareGainNodes.delete(pcId);
                }
                if (store.screenShareAudioSources.has(pcId)) {
                    store.screenShareAudioSources.get(pcId).disconnect();
                    store.screenShareAudioSources.delete(pcId);
                }
                updateVideoGrid();
            };

            track.onended = cleanupVideo;
            track.onmute = cleanupVideo;
            stream.onremovetrack = (e) => {
                if (e.track === track) {
                    cleanupVideo();
                }
            };
        } else if (track.kind === 'audio') {
            const isScreenShareAudio = stream.getVideoTracks().length > 0;

            if (isScreenShareAudio) {
                const ctx = getAudioCtx();
                const source = ctx.createMediaStreamSource(stream);
                const gainNode = ctx.createGain();
                source.connect(gainNode);
                gainNode.connect(ctx.destination);
                gainNode.gain.value = 0;

                store.screenShareAudioSources.set(pcId, source);
                store.screenShareGainNodes.set(pcId, gainNode);
                return;
            }

            let audio = document.getElementById(`audio-${pcId}`);
            if (!audio) {
                audio = document.createElement('audio');
                audio.id = `audio-${pcId}`;
                audio.autoplay = true;
                document.body.appendChild(audio);
            }
            audio.srcObject = stream;
            audio.muted = true;

            const ctx = getAudioCtx();
            const source = ctx.createMediaStreamSource(stream);
            const gainNode = ctx.createGain();
            source.connect(gainNode);
            gainNode.connect(ctx.destination);

            store.audioSources.set(pcId, source);
            store.gainNodes.set(pcId, gainNode);

            if (store.isDeafened) {
                gainNode.gain.value = 0;
            } else {
                const initialVol = store.userVolumes[targetUserId];
                if (initialVol !== undefined) {
                    gainNode.gain.value = initialVol;
                }
            }
            updateCallUI();
        }
    };

    if (store.localStream) {
        store.localStream.getTracks().forEach(track => peerConnection.addTrack(track, store.localStream));
    }

    if (store.localVideoStream) {
        store.localVideoStream.getTracks().forEach(track => {
            const sender = peerConnection.addTrack(track, store.localVideoStream);
            configureVideoSender(sender, peerConnection);
        });
    }

    if (initiator) {
        const state = getNegotiationState(pcId);
        try {
            state.makingOffer = true;
            await peerConnection.setLocalDescription();
            sendSignal(targetUserId, targetSessionId, { sdp: peerConnection.localDescription });
        } catch (e) {
            console.error(`Initial offer error [${pcId}]:`, e);
        } finally {
            state.makingOffer = false;
        }
    }

    updateVideoGrid();

    return peerConnection;
}

/**
 * Closes and cleans up a peer connection and its associated resources.
 */
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
    if (store.screenShareGainNodes.has(pcId)) {
        store.screenShareGainNodes.get(pcId).disconnect();
        store.screenShareGainNodes.delete(pcId);
    }
    if (store.screenShareAudioSources.has(pcId)) {
        store.screenShareAudioSources.get(pcId).disconnect();
        store.screenShareAudioSources.delete(pcId);
    }
    store.remoteVideoStreams.delete(pcId);
    store.pcs.delete(pcId);
    cleanupNegotiationState(pcId);
    updateCallUI();
    updateVideoGrid();
}

/**
 * Handles incoming signaling messages (offers, answers, ICE candidates).
 */
export async function handleSignal(userId, sessionId, data) {
    const pcId = `${userId}:${sessionId}`;
    let peerConnection = store.pcs.get(pcId);
    if (!peerConnection) {
        peerConnection = await createPeerConnection(userId, sessionId, false);
    }

    const state = getNegotiationState(pcId);

    try {
        if (data.sdp) {
            const description = new RTCSessionDescription(data.sdp);
            const readyForOffer =
                !state.makingOffer &&
                (peerConnection.signalingState === 'stable' || state.isSettingRemoteAnswerPending);
            const offerCollision = description.type === 'offer' && !readyForOffer;

            state.ignoreOffer = !state.polite && offerCollision;
            if (state.ignoreOffer) {
                console.log(`[${pcId}] Ignoring colliding offer (we are impolite)`);
                state.pendingCandidates = [];
                return;
            }

            if (offerCollision) {
                console.log(`[${pcId}] Rolling back local offer to accept remote offer (we are polite)`);
                await peerConnection.setLocalDescription({ type: 'rollback' });
            }

            state.isSettingRemoteAnswerPending = description.type === 'answer';
            await peerConnection.setRemoteDescription(description);
            state.isSettingRemoteAnswerPending = false;

            if (state.pendingCandidates.length > 0) {
                for (const candidate of state.pendingCandidates) {
                    try {
                        await peerConnection.addIceCandidate(candidate);
                    } catch (e) {
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

            updateVideoGrid();
        } else if (data.candidate) {
            if (!peerConnection.remoteDescription || !peerConnection.remoteDescription.type) {
                state.pendingCandidates.push(new RTCIceCandidate(data.candidate));
            } else {
                try {
                    await peerConnection.addIceCandidate(new RTCIceCandidate(data.candidate));
                } catch (e) {
                    if (!e.message?.includes('Unknown ufrag')) {
                        console.warn(`Failed to add ICE candidate [${pcId}]:`, e);
                    }
                }
            }
        }
    } catch (e) {
        if (e.name !== 'InvalidStateError') {
            console.error(`Signal error [${pcId}]:`, e);
        }
    }
}

/**
 * Toggles the local user's microphone mute state.
 */
export function toggleMute() {
    store.isMuted = !store.isMuted;
    if (store.localStream) {
        store.localStream.getAudioTracks().forEach(t => {
            t.enabled = !store.isMuted;
        });
    }
    updateMuteButton();
}

/**
 * Toggles the local user's deafen state, muting or unmuting all remote users.
 */
export function toggleDeafen() {
    store.isDeafened = !store.isDeafened;
    store.gainNodes.forEach((gainNode, pcId) => {
        if (store.isDeafened) {
            gainNode.gain.value = 0;
        } else {
            const [uid] = pcId.split(':');
            gainNode.gain.value = store.userVolumes[uid] !== undefined ? store.userVolumes[uid] : 1.0;
        }
    });

    updateDeafenButton();
}

/**
 * Updates the visual state of the mute button.
 */
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

/**
 * Updates the visual state of the deafen button.
 */
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

/**
 * Initiates joining a voice call, acquiring media and signaling other users.
 */
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
            const shouldInitiate = store.user.id > uid;
            createPeerConnection(uid, sid, shouldInitiate);
        }
    });
}

/**
 * Leaves the current voice call, cleaning up all peer connections and media.
 */
export function leaveCall() {
    if (!store.inCall) return;
    store.inCall = false;
    playNotificationSound('leave');

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
    sharePlay.reset();
    updateCallUI();
    updateVideoGrid();
}

/**
 * Configures the video sender for high-quality screen share.
 * Respects codec preferences from store (VP9, AV1) over default H264.
 */
async function configureVideoSender(sender, pc) {
    const transceiver = pc.getTransceivers().find(t => t.sender === sender);
    if (transceiver && transceiver.setCodecPreferences) {
        const capabilities = RTCRtpSender.getCapabilities('video');
        if (capabilities) {
            const codecs = capabilities.codecs.slice();

            // Build codec preference order based on settings
            // Higher priority codecs come first in the order array
            const order = [];
            if (store.preferAV1) order.push('video/AV1');
            if (store.preferVP9) order.push('video/VP9');
            order.push('video/H264'); // Always include H264 as fallback

            codecs.sort((a, b) => {
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

    try {
        const params = sender.getParameters();
        if (!params.encodings || params.encodings.length === 0) {
            params.encodings = [{}];
        }
        params.encodings[0].maxBitrate = 5000000;
        await sender.setParameters(params);
    } catch (e) {
        console.warn('Could not set video bitrate:', e);
    }
}

/**
 * Acquires screen share media and adds it to all active peer connections.
 * In Electron, handles source selection from the main process.
 */
export async function startScreenShare() {
    if (store.screenSharing) return;

    // Set up Electron source selection listener if running in Electron
    if (window.electronAPI?.isElectron) {
        setupElectronSourcePicker();
    }

    let stream;
    try {
        stream = await navigator.mediaDevices.getDisplayMedia({
            video: {
                cursor: 'always',
                frameRate: { ideal: 60 }
            },
            audio: true
        });
    } catch (e) {
        console.error('Failed to get screen share', e);
        return;
    }

    store.localVideoStream = stream;
    store.screenSharing = true;

    const videoTrack = stream.getVideoTracks()[0];
    const audioTrack = stream.getAudioTracks()[0];
    store.pcs.forEach((pc, pcId) => {
        const videoSender = pc.addTrack(videoTrack, stream);
        configureVideoSender(videoSender, pc);
        if (audioTrack) {
            pc.addTrack(audioTrack, stream);
        }
    });

    videoTrack.onended = () => {
        stopScreenShare();
    };

    updateScreenShareButton();
    updateVideoGrid();
}

/**
 * Sets up the Electron source picker modal listener.
 * Called before getDisplayMedia to intercept the select-source event from main.
 */
function setupElectronSourcePicker() {
    if (!window.electronAPI?.onSelectSource) return;

    window.electronAPI.onSelectSource((sources) => {
        const modal = $('#sourcePickerModal');
        const grid = $('#sourcePickerGrid');
        if (!modal || !grid) return;

        grid.innerHTML = '';
        sources.forEach(source => {
            const item = el('div', { class: 'source-picker-item' });
            const img = el('img', { src: source.thumbnail, alt: source.name });
            const name = el('div', { class: 'source-name' }, source.name);
            item.appendChild(img);
            item.appendChild(name);

            item.onclick = () => {
                window.electronAPI.selectSource(source.id);
                modal.classList.add('hidden');
            };

            grid.appendChild(item);
        });

        modal.classList.remove('hidden');

        // Set up close button
        const closeBtn = $('#btnCloseSourcePicker');
        if (closeBtn) {
            closeBtn.onclick = () => {
                modal.classList.add('hidden');
                // Signal cancellation
                window.electronAPI.selectSource(null);
            };
        }

        // Close on backdrop click
        const backdrop = modal.querySelector('.modal-backdrop');
        if (backdrop) {
            backdrop.onclick = () => {
                modal.classList.add('hidden');
                window.electronAPI.selectSource(null);
            };
        }
    });
}


/**
 * Stops screen sharing and removes the associated tracks from all peer connections.
 */
export function stopScreenShare() {
    if (!store.screenSharing) return;

    if (store.localVideoStream) {
        store.localVideoStream.getTracks().forEach(t => t.stop());
        store.localVideoStream = null;
    }

    store.screenSharing = false;

    const tracks = store.localVideoStream ? store.localVideoStream.getTracks() : [];
    store.pcs.forEach((pc) => {
        const senders = pc.getSenders();
        senders.forEach(sender => {
            if (sender.track && tracks.includes(sender.track)) {
                pc.removeTrack(sender);
            }
        });
    });

    updateScreenShareButton();
    updateVideoGrid();
}

/**
 * Updates the visual state of the screen share toggle button.
 */
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

/**
 * Updates the video grid display with local and remote video streams.
 */
export function updateVideoGrid() {
    const grid = $('#videoGrid');
    if (!grid) return;

    grid.innerHTML = '';

    if (store.localVideoStream && store.screenSharing) {
        const tile = createVideoTile('local', store.localVideoStream, store.user?.username || 'You');
        tile.classList.add('local');
        grid.appendChild(tile);
    }

    const staleIds = [];
    store.remoteVideoStreams.forEach((stream, pcId) => {
        const videoTracks = stream.getVideoTracks();
        const isStale = videoTracks.length === 0 || videoTracks.every(t => t.readyState === 'ended');
        if (isStale) {
            staleIds.push(pcId);
        }
    });
    staleIds.forEach(pcId => {
        store.remoteVideoStreams.delete(pcId);
        if (store.screenShareGainNodes.has(pcId)) {
            store.screenShareGainNodes.get(pcId).disconnect();
            store.screenShareGainNodes.delete(pcId);
        }
        if (store.screenShareAudioSources.has(pcId)) {
            store.screenShareAudioSources.get(pcId).disconnect();
            store.screenShareAudioSources.delete(pcId);
        }
    });

    store.remoteVideoStreams.forEach((stream, pcId) => {
        const [userId] = pcId.split(':');
        const user = store.users.get(userId);
        const username = user?.username || userId;
        const tile = createVideoTile(pcId, stream, username);
        grid.appendChild(tile);
    });
}

/**
 * Creates a video tile element for a stream.
 */
function createVideoTile(id, stream, username) {
    const tile = el('div', { class: 'video-tile', 'data-stream-id': id });
    const video = el('video', { autoplay: true, playsinline: true, muted: id === 'local' });
    video.srcObject = stream;
    tile.appendChild(video);

    const label = el('div', { class: 'video-label' }, username);
    tile.appendChild(label);

    tile.onclick = () => toggleVideoFullscreen(id, stream, username);

    return tile;
}

/**
 * Toggles a video stream into or out of fullscreen mode.
 */
export function toggleVideoFullscreen(id, stream, username) {
    const overlay = $('#videoFullscreen');
    if (!overlay) return;

    if (overlay.classList.contains('hidden')) {
        overlay.innerHTML = '';
        const video = el('video', { autoplay: true, playsinline: true });
        video.srcObject = stream;
        overlay.appendChild(video);
        overlay.classList.remove('hidden');

        overlay.onclick = () => {
            overlay.classList.add('hidden');
            overlay.innerHTML = '';
            const gainNode = store.screenShareGainNodes.get(id);
            if (gainNode) gainNode.gain.value = 0;
        };

        const gainNode = store.screenShareGainNodes.get(id);
        if (gainNode) {
            const [userId] = id.split(':');
            const vol = store.userVolumes[userId] !== undefined ? store.userVolumes[userId] : 1.0;
            gainNode.gain.value = vol;
        }
    } else {
        overlay.classList.add('hidden');
        overlay.innerHTML = '';
        const gainNode = store.screenShareGainNodes.get(id);
        if (gainNode) gainNode.gain.value = 0;
    }
}
