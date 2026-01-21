import { store } from './store.js';
import { $ } from './utils.js';
import { sendTyping } from './socket.js'; // Just to have access to ws sending if needed, though we usully access store.ws

export class SharePlay {
    constructor() {
        this.ctx = new (window.AudioContext || window.webkitAudioContext)();
        this.gainNode = this.ctx.createGain();
        this.gainNode.connect(this.ctx.destination);

        this.currentSource = null;
        this.buffer = null;
        this.isLoading = false;
        this.fileId = null; // To track if we need to load a new file
        this.channelId = null; // Current channel for API calls
        this.localStatus = 'paused';
        this.serverState = null;

        // Bind UI
        this.ui = {
            container: $('#shareplayContainer'),
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
        };

        this.animationFrameId = null;

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
            const duration = this.buffer ? this.buffer.duration : 0;
            if (duration > 0) {
                this.sendAction('seek', (percent * duration).toString());
            }
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

                // Calculate current server position
                let serverPos = state.current_position_secs;
                if (state.start_time) {
                    const saved = new Date(state.start_time).getTime();
                    const now = Date.now() + (store.timeOffset || 0); // Assuming store.timeOffset if we had one, but we don't.
                    // Use local clock relative diff if close enough or just trust server time if synced reasonably
                    // For now simple elapsed:
                    const elapsed = (Date.now() - saved) / 1000;
                    serverPos += elapsed;
                }

                // If not playing or drifted
                if (!this.currentSource || Math.abs(this.getCurrentTime() - serverPos) > 0.5) {
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

        } else if (currentItem && !currentItem.file_path) {
            // Still downloading
            this.ui.title.textContent = `Grabbing: ${currentItem.url}...`;
            this.stop();
        } else {
            // Nothing playing
            this.ui.title.textContent = "Nothing is playing.";
            this.stop();
        }
    }

    async loadTrack(id, channelId) {
        this.stop();
        this.buffer = null;
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
            console.log(`[SharePlay] Got song ID: ${song_id}, fetching audio file...`);

            const audioRes = await fetch(store.baseUrl + `/api/shareplay/song/${song_id}`);
            if (!audioRes.ok) throw new Error(`Failed to load audio: ${audioRes.status} ${audioRes.statusText}`);

            const arrayBuffer = await audioRes.arrayBuffer();
            const decoded = await this.ctx.decodeAudioData(arrayBuffer);

            // Check if we already moved on to another track during the fetch
            if (this.fileId !== id) {
                console.warn(`[SharePlay] Track ID mismatch after load: ${this.fileId} vs ${id}. Discarding buffer.`);
                return;
            }

            this.buffer = decoded;
            this.isLoading = false;
            console.log(`[SharePlay] Track loaded successfully, buffer duration: ${this.buffer.duration}`);

            // If the server state is still playing this track, start it
            if (this.serverState?.status === 'playing' && this.serverState?.current_index !== null) {
                const currentItem = this.serverState.queue[this.serverState.current_index];
                if (currentItem && currentItem.id === id) {
                    // Re-sync position
                    let serverPos = this.serverState.current_position_secs;
                    if (this.serverState.start_time) {
                        const saved = new Date(this.serverState.start_time).getTime();
                        const elapsed = (Date.now() - saved) / 1000;
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

    play(offset = 0) {
        if (!this.buffer) return;
        // Clamp offset to valid range to prevent RangeError
        offset = Math.max(0, Math.min(offset, this.buffer.duration - 0.01));
        if (this.currentSource) this.currentSource.stop();

        this.currentSource = this.ctx.createBufferSource();
        this.currentSource.buffer = this.buffer;
        this.currentSource.connect(this.gainNode);

        this.currentSource.start(0, offset);
        this.startTime = this.ctx.currentTime - offset;

        // Start the seek bar update loop
        this.startSeekLoop();

        this.currentSource.onended = () => {
            // Determine if it ended naturally or was stopped
            if (this.ctx.currentTime - this.startTime >= this.buffer.duration - 0.5) {
                // Song ended
                this.sendAction('next');
            }
        };
    }

    stop() {
        if (this.currentSource) {
            this.currentSource.onended = null; // Prevent skipping
            try { this.currentSource.stop(); } catch (e) { }
            this.currentSource = null;
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
        this.buffer = null;
        this.fileId = null;
        this.isLoading = false;
        this.serverState = null;
        if (this.ctx && this.ctx.state !== 'closed') {
            this.ctx.suspend().catch(() => { });
        }
        if (this.ui.title) this.ui.title.textContent = "SharePlay inactive";
        if (this.ui.seek) this.ui.seek.style.background = 'var(--bg-3)';
        if (this.ui.currentTime) this.ui.currentTime.textContent = '0:00';
        if (this.ui.totalTime) this.ui.totalTime.textContent = '0:00';
        if (this.ui.queue) this.ui.queue.innerHTML = '';
    }

    getCurrentTime() {
        if (!this.currentSource) return 0;
        return this.ctx.currentTime - this.startTime;
    }

    renderQueue(state) {
        this.ui.queue.innerHTML = '';
        state.queue.forEach((item, idx) => {
            const el = document.createElement('div');
            el.className = 'shareplay-queue-item';
            if (idx === state.current_index) el.classList.add('active');

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

            el.appendChild(title);
            el.appendChild(duration);

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
        if (!this.serverState || !this.buffer) return;

        const current = this.getCurrentTime();
        const storedDuration = this.serverState.queue[this.serverState.current_index]?.duration_seconds || this.buffer.duration;
        const duration = storedDuration || 1; // avoid /0

        const percent = Math.min(100, (current / duration) * 100);
        this.ui.seek.style.background = `linear-gradient(to right, var(--accent) ${percent}%, var(--bg-3) ${percent}%)`;
    }

    startSeekLoop() {
        if (this.animationFrameId) cancelAnimationFrame(this.animationFrameId);

        const update = () => {
            this.updateSeekBar();
            this.updateTimestamps();
            if (this.serverState?.status === 'playing' && this.currentSource) {
                this.animationFrameId = requestAnimationFrame(update);
            }
        };
        update();
    }

    updateTimestamps() {
        if (!this.buffer || !this.serverState) return;
        const current = this.getCurrentTime();
        const duration = this.serverState.queue[this.serverState.current_index]?.duration_seconds
            || this.buffer.duration;
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
