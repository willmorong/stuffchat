import { store } from './store.js';
import { $ } from './utils.js';
import { sendTyping } from './socket.js'; // Just to have access to ws sending if needed, though we usully access store.ws

export class SharePlay {
    constructor() {
        this.ctx = new (window.AudioContext || window.webkitAudioContext)();
        this.gainNode = this.ctx.createGain();
        this.gainNode.gain.value = 0.4; // Default to 40%
        this.gainNode.connect(this.ctx.destination);

        // HTML5 Audio element for streaming playback
        this.audio = new Audio();
        this.audio.crossOrigin = 'anonymous'; // Required for CORS with Web Audio API
        this.audio.preload = 'metadata';
        this.mediaSource = null; // MediaElementSourceNode - only create once per audio element

        this.isLoading = false;
        this.fileId = null; // To track if we need to load a new file
        this.channelId = null; // Current channel for API calls
        this.localStatus = 'paused';
        this.serverState = null;

        // Bind UI
        this.ui = {
            container: $('#shareplayModal'),
            title: $('#nowPlayingTitle'),
            seek: $('#nowPlayingSeek'),
            currentTime: $('#nowPlayingCurrentTime'),
            totalTime: $('#nowPlayingTotalTime'),
            queue: $('#shareplayQueueItems'),
            input: $('#shareplayQueueAddInput'),
            btnAdd: $('#shareplayQueueAddButton'),
            btnPrev: $('#btnSharePlayPrevious'),
            btnPause: $('#btnSharePlayPause'),
            btnNext: $('#btnSharePlayNext'),
            btnRepeat: $('#btnSharePlayRepeat'),
            volume: $('#shareplayVolume'),
            cover: $('#nowPlayingCover'),
        };

        this.animationFrameId = null;

        // Set up audio event handlers
        this.audio.addEventListener('ended', () => {
            console.log('[SharePlay] Audio ended naturally');
            this.sendAction('next');
        });

        this.audio.addEventListener('canplay', () => {
            console.log('[SharePlay] Audio can play (enough buffered)');
        });

        this.audio.addEventListener('error', (e) => {
            // MediaError code 4 with empty src is expected when clearing the source
            if (this.audio.error?.code === 4) {
                console.log('[SharePlay] Source cleared');
            } else {
                console.error('[SharePlay] Audio error:', this.audio.error);
            }
        });

        this.bindEvents();
    }

    bindEvents() {
        this.ui.btnAdd.onclick = () => this.addItem();
        this.ui.input.onkeydown = (e) => { if (e.key === 'Enter') this.addItem(); };

        this.ui.btnPause.onclick = () => {
            const action = this.serverState?.status === 'playing' ? 'pause' : 'play';
            this.sendAction(action);
        };

        this.ui.btnNext.onclick = () => this.sendAction('next');
        this.ui.btnPrev.onclick = () => this.sendAction('prev');
        this.ui.btnRepeat.onclick = () => this.sendAction('toggle_repeat');

        // Seek bar click
        this.ui.seek.onclick = (e) => {
            const rect = this.ui.seek.getBoundingClientRect();
            const x = e.clientX - rect.left;
            const percent = Math.max(0, Math.min(1, x / rect.width));
            const duration = this.audio.duration || 0;
            if (duration > 0 && isFinite(duration)) {
                this.sendAction('seek', (percent * duration).toString());
            }
        };

        this.ui.volume.oninput = () => {
            const val = parseInt(this.ui.volume.value);
            this.gainNode.gain.value = val / 200;
        };
    }

    addItem() {
        const url = this.ui.input.value.trim();
        if (!url) return;
        console.log(`[SharePlay] User adding item: ${url}`);
        this.sendAction('add', url);
        this.ui.input.value = '';

        // Optimistic UI? Or just wait for sync.
        // Plan said: "Add Item: Grabbing... appears immediately"
        // This is handled by backend adding a placeholder immediately
    }

    sendAction(type, data = null) {
        if (!store.ws || !store.callChannelId) {
            console.warn("[SharePlay] Cannot send action, no WS or callChannelId", store.ws, store.callChannelId);
            return;
        }
        console.log(`[SharePlay] Sending action: ${type}`, data);
        store.ws.send(JSON.stringify({
            type: 'shareplay_action',
            channel_id: store.callChannelId,
            action_type: type,
            data: data
        }));
    }

    async sync(state, channelId) {
        // console.log("[SharePlay] Syncing state:", state);
        this.serverState = state;
        if (channelId) {
            this.channelId = channelId; // Store for use in loadTrack
        }

        // Update UI Visibility
        // Handled in voice.js usually but we can ensure it's displayed if active
        // Logic: if active, voice.js handles showing container

        this.renderQueue(state);
        this.renderControls(state);

        const currentIndex = state.current_index;
        const currentItem = (currentIndex !== null && state.queue[currentIndex])
            ? state.queue[currentIndex]
            : null;

        // Sync Playback
        if (currentItem && currentItem.file_path) {
            // Check if we need to load new file
            if (this.fileId !== currentItem.id) {
                await this.loadTrack(currentItem.id, this.channelId);
            }

            // Sync status/position
            if (state.status === 'playing') {
                if (this.ctx.state === 'suspended') this.ctx.resume();

                // Calculate current server position using clock-synced time
                let serverPos = state.current_position_secs;
                if (state.start_time) {
                    const saved = new Date(state.start_time).getTime();
                    // Use timeOffset to convert client time to server time
                    const now = Date.now() + (store.timeOffset || 0);
                    const elapsed = (now - saved) / 1000;
                    serverPos += elapsed;
                }

                // If not playing or drifted
                const isPlaying = !this.audio.paused && !this.audio.ended;
                if (!isPlaying || Math.abs(this.getCurrentTime() - serverPos) > 0.5) {
                    if (!this.isLoading) {
                        this.play(serverPos);
                    }
                }
            } else {
                this.stop();
                if (this.ctx.state === 'running') this.ctx.suspend();
            }

            // Update Title
            this.ui.title.textContent = currentItem.title;

            // Update Seek Bar (visual only)
            requestAnimationFrame(() => this.updateSeekBar());

            // Update Cover
            if (currentItem.thumbnail_path) {
                this.ui.cover.src = store.baseUrl + `/api/shareplay/thumbnail/${currentItem.id}?t=${Date.now()}`;
                this.ui.cover.classList.remove('hidden');
            } else {
                this.ui.cover.classList.add('hidden');
                this.ui.cover.src = '';
            }

        } else if (currentItem && !currentItem.file_path) {
            // Still downloading
            this.ui.title.textContent = `Grabbing: ${currentItem.url}...`;
            this.ui.cover.classList.add('hidden');
            this.stop();
        } else {
            // Nothing playing
            this.ui.title.textContent = "Nothing is playing.";
            this.ui.cover.classList.add('hidden');
            this.stop();
        }
    }

    async loadTrack(id, channelId) {
        this.stop();
        this.fileId = id;
        this.isLoading = true;
        this.ui.title.textContent = "Loading audio...";
        console.log(`[SharePlay] Loading track ID: ${id} for channel: ${channelId}`);

        try {
            const res = await fetch(store.baseUrl + `/api/shareplay/${channelId}/current`, {
                cache: 'no-store'
            });
            if (!res.ok) throw new Error(`Failed to load track info: ${res.status} ${res.statusText}`);
            const { song_id } = await res.json();
            console.log(`[SharePlay] Got song ID: ${song_id}, setting audio source for streaming...`);

            // Check if we already moved on to another track during the fetch
            if (this.fileId !== id) {
                console.warn(`[SharePlay] Track ID mismatch after load: ${this.fileId} vs ${id}. Aborting.`);
                return;
            }

            // Set the audio source - browser handles streaming via HTTP Range requests
            this.audio.src = store.baseUrl + `/api/shareplay/song/${song_id}`;

            // Connect to Web Audio API for gain control (only once per audio element)
            if (!this.mediaSource) {
                this.mediaSource = this.ctx.createMediaElementSource(this.audio);
                this.mediaSource.connect(this.gainNode);
                console.log('[SharePlay] Connected audio element to Web Audio API');
            }

            // Wait for enough data to start playing
            await new Promise((resolve, reject) => {
                const onCanPlay = () => {
                    this.audio.removeEventListener('canplay', onCanPlay);
                    this.audio.removeEventListener('error', onError);
                    resolve();
                };
                const onError = () => {
                    this.audio.removeEventListener('canplay', onCanPlay);
                    this.audio.removeEventListener('error', onError);
                    reject(new Error('Audio failed to load'));
                };
                this.audio.addEventListener('canplay', onCanPlay);
                this.audio.addEventListener('error', onError);
                this.audio.load();
            });

            this.isLoading = false;
            console.log(`[SharePlay] Track ready for streaming, duration: ${this.audio.duration}`);

            // If the server state is still playing this track, start it
            if (this.serverState?.status === 'playing' && this.serverState?.current_index !== null) {
                const currentItem = this.serverState.queue[this.serverState.current_index];
                if (currentItem && currentItem.id === id) {
                    // Re-sync position using clock-synced time
                    let serverPos = this.serverState.current_position_secs;
                    if (this.serverState.start_time) {
                        const saved = new Date(this.serverState.start_time).getTime();
                        const now = Date.now() + (store.timeOffset || 0);
                        const elapsed = (now - saved) / 1000;
                        serverPos += elapsed;
                    }
                    this.play(serverPos);
                }
            }
        } catch (e) {
            console.error("[SharePlay] Error loading audio:", e);
            this.isLoading = false;
            this.ui.title.textContent = "Error loading audio.";
        }
    }

    async play(offset = 0) {
        if (!this.audio.src) return;
        // Resume AudioContext if suspended (browser autoplay policy)
        if (this.ctx.state === 'suspended') {
            await this.ctx.resume();
        }

        // Clamp offset to valid range
        const duration = this.audio.duration || 0;
        if (isFinite(duration) && duration > 0) {
            offset = Math.max(0, Math.min(offset, duration - 0.01));
        }

        this.audio.currentTime = offset;

        try {
            await this.audio.play();
            console.log(`[SharePlay] Playing from offset: ${offset}`);
        } catch (e) {
            console.error('[SharePlay] Playback failed:', e);
        }

        // Start the seek bar update loop
        this.startSeekLoop();
    }

    stop() {
        if (this.audio && !this.audio.paused) {
            this.audio.pause();
        }
        // Cancel the seek loop
        if (this.animationFrameId) {
            cancelAnimationFrame(this.animationFrameId);
            this.animationFrameId = null;
        }
    }

    reset() {
        console.log("[SharePlay] Resetting state");
        this.stop();
        this.audio.src = '';
        this.fileId = null;
        this.isLoading = false;
        this.serverState = null;
        if (this.ctx && this.ctx.state !== 'closed') {
            this.ctx.suspend().catch(() => { });
        }
        if (this.ui.title) this.ui.title.textContent = "SharePlay inactive";
        if (this.ui.seek) this.ui.seek.style.background = 'var(--border)';
        if (this.ui.currentTime) this.ui.currentTime.textContent = '0:00';
        if (this.ui.totalTime) this.ui.totalTime.textContent = '0:00';
        if (this.ui.queue) this.ui.queue.innerHTML = '';
    }

    getCurrentTime() {
        return this.audio?.currentTime || 0;
    }

    renderQueue(state) {
        this.ui.queue.innerHTML = '';
        state.queue.forEach((item, idx) => {
            const el = document.createElement('div');
            el.className = 'shareplay-queue-item';
            if (idx === state.current_index) el.classList.add('playing');

            // Thumbnail
            if (item.thumbnail_path) {
                const img = document.createElement('img');
                img.src = store.baseUrl + `/api/shareplay/thumbnail/${item.id}`;
                img.className = 'shareplay-queue-thumb';
                el.appendChild(img);
            } else {
                const placeholder = document.createElement('div');
                placeholder.className = 'shareplay-queue-thumb-placeholder';
                placeholder.innerHTML = '<i class="bi bi-music-note-beamed"></i>';
                el.appendChild(placeholder);
            }

            const title = document.createElement('div');
            title.className = 'title';

            // Show download status if not ready
            if (!item.file_path && item.download_status !== 'ready') {
                const status = item.download_status || 'grabbing';
                if (status === 'grabbing') {
                    title.textContent = `Getting: ${item.url}...`;
                } else if (status === 'downloading') {
                    title.textContent = `Downloading: ${item.title}...`;
                } else if (status === 'error') {
                    title.textContent = `Error: ${item.title}`;
                    el.classList.add('error');
                } else {
                    title.textContent = item.title;
                }
            } else {
                title.textContent = item.title;
            }

            const duration = document.createElement('div');
            duration.className = 'duration';
            duration.textContent = this.formatTime(item.duration_seconds);

            // Remove button (appears on hover)
            const removeBtn = document.createElement('button');
            removeBtn.className = 'shareplay-queue-remove';
            removeBtn.innerHTML = '<i class="bi bi-x"></i>';
            removeBtn.title = 'Remove from queue';
            removeBtn.onclick = (e) => {
                e.stopPropagation(); // Prevent triggering track selection
                this.sendAction('remove', idx.toString());
            };

            el.appendChild(title);
            el.appendChild(duration);
            el.appendChild(removeBtn);

            el.onclick = () => {
                this.sendAction('track', idx.toString());
            };

            this.ui.queue.appendChild(el);
        });
    }

    renderControls(state) {
        this.ui.btnPause.innerHTML = state.status === 'playing'
            ? '<i class="bi bi-pause-fill"></i>'
            : '<i class="bi bi-play-fill"></i>';

        this.ui.btnRepeat.className = 'iconbtn' + (state.repeat_mode !== 'Off' ? ' active' : '');
        // Maybe different icon for One/All
        if (state.repeat_mode === 'One') this.ui.btnRepeat.innerHTML = '<i class="bi bi-repeat-1"></i>';
        else this.ui.btnRepeat.innerHTML = '<i class="bi bi-repeat"></i>';

        // Disable skip buttons at queue boundaries when repeat is off
        const currentIdx = state.current_index ?? 0;
        const queueLen = state.queue.length;
        const currentTime = this.getCurrentTime();

        // Disable prev if at start of queue (repeat off) and < 3s in
        const canGoPrev = queueLen > 0 && (
            state.repeat_mode !== 'Off' ||
            currentIdx > 0 ||
            currentTime > 3
        );
        this.ui.btnPrev.disabled = !canGoPrev;

        // Disable next if at end of queue (repeat off)
        const canGoNext = queueLen > 0 && (
            state.repeat_mode !== 'Off' ||
            currentIdx < queueLen - 1
        );
        this.ui.btnNext.disabled = !canGoNext;
    }

    updateSeekBar() {
        if (!this.serverState || !this.audio.src) return;

        const current = this.getCurrentTime();
        const storedDuration = this.serverState.queue[this.serverState.current_index]?.duration_seconds;
        const audioDuration = this.audio.duration;
        const duration = storedDuration || (isFinite(audioDuration) ? audioDuration : 0) || 1;

        const percent = Math.min(100, (current / duration) * 100);
        this.ui.seek.style.background = `linear-gradient(to right, var(--accent) ${percent}%, var(--border) ${percent}%)`;
    }

    startSeekLoop() {
        if (this.animationFrameId) cancelAnimationFrame(this.animationFrameId);

        const update = () => {
            this.updateSeekBar();
            this.updateTimestamps();
            const isPlaying = this.audio && !this.audio.paused && !this.audio.ended;
            if (this.serverState?.status === 'playing' && isPlaying) {
                this.animationFrameId = requestAnimationFrame(update);
            }
        };
        update();
    }

    updateTimestamps() {
        if (!this.audio.src || !this.serverState) return;
        const current = this.getCurrentTime();
        const storedDuration = this.serverState.queue[this.serverState.current_index]?.duration_seconds;
        const audioDuration = this.audio.duration;
        const duration = storedDuration || (isFinite(audioDuration) ? audioDuration : 0);
        if (this.ui.currentTime) this.ui.currentTime.textContent = this.formatTime(current);
        if (this.ui.totalTime) this.ui.totalTime.textContent = this.formatTime(duration);
    }

    formatTime(secs) {
        if (!secs) return '0:00';
        const m = Math.floor(secs / 60);
        const s = Math.floor(secs % 60);
        return `${m}:${s.toString().padStart(2, '0')}`;
    }
}

export const sharePlay = new SharePlay();
