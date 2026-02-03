import { store } from './store.js';
import { apiFetch } from './api.js';
import { connectWs } from './socket.js';
import { $, el, absFileUrl, buildFileUrl, setIf, truncateId, presenceClass, localizeDate, replaceEmojisAndLinkify, isEmojiOnly, formatFileSize } from './utils.js';
import { prefetchUsers } from './users.js';

export function enableComposer(enabled) {
    const input = $('#msgInput');
    const btn = $('#btnSend');
    if (input) input.disabled = !enabled;
    if (btn) btn.disabled = !enabled;
}

export function isScrolledToBottom() {
    const wrap = $('#messages');
    return wrap.scrollHeight - wrap.scrollTop - wrap.clientHeight < 10;
}

export function scrollToBottom() {
    const wrap = $('#messages');
    wrap.scrollTop = wrap.scrollHeight;
}

function addLoadOlderButton() {
    const wrap = $('#messages');
    // Ensure we don't have duplicates? The previous logic cleared innerHTML then added it.
    // If we call this, we should append it probably, or prepend?
    // In original code: $('#messages').appendChild(btn); (Called in renderMessages/selectChannel after clearing).
    // It says "Load older" so it should be at the TOP?
    // Original code: `$('#messages').appendChild(btn);`. Wait.
    // CSS might handle order (flex-direction: column-reverse? No).
    // If it's "Load older", it usually goes at the top.
    // Let's check original logic.
    /*
    function renderMessages(channelId) {
        ...
        wrap.innerHTML = '';
        addLoadOlderButton();
        arr.forEach(msg => { wrap.appendChild(renderMessageItem(msg)); });
    }
    */
    // So it's the first child.
    const btn = el('div', { class: 'load-older', id: 'loadOlder', onclick: () => loadOlder() }, [
        el('i', { class: 'bi bi-chevron-up' }), ' Load older'
    ]);
    wrap.appendChild(btn);
}

export async function loadOlder() {
    const chan = store.currentChannelId;
    const arr = store.messages.get(chan) || [];
    const before = arr.length ? arr[0].id : null;
    await fetchMessagesPage(chan, before);
}

function renderAttachment(message) {
    const box = el('div', { class: 'attachment' }, []);
    const url = message.file_url
        ? absFileUrl(message.file_url)
        : buildFileUrl(message.file_id, 'file');

    // If we have metadata, use it
    const filename = message.filename || 'attachment';
    const size = message.file_size !== undefined ? message.file_size : null;
    let ext = (filename.split('.').pop() || '').toLowerCase();
    // If no filename from server, try to guess from URL
    if (ext === 'attachment' || !ext) {
        ext = (url.split('?')[0].split('.').pop() || '').toLowerCase();
    }

    const clearBox = () => {
        while (box.firstChild) box.removeChild(box.firstChild);
    };

    const renderFileCard = () => {
        clearBox();
        const sizeStr = size !== null ? formatFileSize(size) : '';
        const info = el('div', { class: 'file-info' }, [
            el('div', { class: 'file-name' }, filename),
            sizeStr ? el('div', { class: 'file-size' }, sizeStr) : null
        ]);
        const icon = el('i', { class: 'bi bi-file-earmark-arrow-down file-icon' });

        // We'll use a flex container for the card
        const card = el('a', {
            href: url,
            target: '_blank',
            rel: 'noopener noreferrer',
            class: 'file-card'
        }, [icon, info]);

        box.replaceChildren(card);
    };

    // If it's clearly not media, just show the card immediately.
    // Use a known list of media extensions.
    const imageExts = ['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'svg', 'avif', 'heic', 'heif', 'tiff', 'tif', 'ico', 'jfif', 'jxl'];
    const videoExts = ['mp4', 'webm', 'ogv', 'mov', 'm4v', 'avi', 'wmv', 'mkv'];
    const audioExts = ['mp3', 'wav', 'ogg', 'm4a', 'aac', 'flac', 'opus'];

    const isImage = imageExts.includes(ext);
    const isVideo = videoExts.includes(ext);
    const isAudio = audioExts.includes(ext);
    const isMedia = isImage || isVideo || isAudio;

    // If we have filename and it's not media, render card immediately.
    if (message.filename && !isMedia) {
        renderFileCard();
        return box;
    }

    const showLink = () => {
        renderFileCard();
    };

    const tryImage = () =>
        new Promise((resolve, reject) => {
            const img = new Image();
            img.alt = filename;
            img.onload = () => resolve(img);
            img.onerror = reject;
            img.src = url;
            // Add a timeout in case it hangs? 
            // relying on standard timeout might be enough, but let's be safe
            setTimeout(reject, 10000);
        });

    const createMedia = (tag) => new Promise((resolve, reject) => {
        const m = el(tag, { src: url, controls: true, preload: 'metadata' });
        const onOK = () => { cleanup(); resolve(m); };
        const onErr = () => { cleanup(); reject(); };
        const cleanup = () => {
            m.removeEventListener('loadedmetadata', onOK);
            m.removeEventListener('canplay', onOK);
            m.removeEventListener('error', onErr);
        };
        m.addEventListener('loadedmetadata', onOK, { once: true });
        m.addEventListener('canplay', onOK, { once: true });
        m.addEventListener('error', onErr, { once: true });
        try { m.load(); } catch { }

        // Timeout for media too
        setTimeout(() => { cleanup(); reject(); }, 5000);
    });
    const tryVideo = () => createMedia('video');
    const tryAudio = () => createMedia('audio');

    const tryInOrder = (fns) =>
        fns.reduce((p, fn) => p.catch(() => fn()), Promise.reject());

    const handleByType = (ct) => {
        if (!ct) {
            // No type hint, just try based on extension or fallback
            if (isImage) return tryImage();
            if (isVideo) return tryVideo();
            if (isAudio) return tryAudio();
            // If unknown extension and no CT, try standard order or just show link?
            // "Download attachment" logic was a fallback.
            // If we already know it's likely not media, we shouldn't be here (handled above).
            return tryInOrder([tryImage, tryVideo, tryAudio]);
        }
        if (ct.startsWith('image/')) return tryImage();
        if (ct.startsWith('video/')) return tryInOrder([tryVideo, tryAudio, tryImage]);
        if (ct.startsWith('audio/')) return tryInOrder([tryAudio, tryVideo, tryImage]);
        return tryInOrder([tryImage, tryVideo, tryAudio]);
    };

    // If we think it's media, try to load it.
    // We can skip the HEAD request if we trust the extension, 
    // BUT the HEAD request is useful for Content-Type if extension is missing/wrong.
    // To speed up: if extension is strong indicator, try that first?
    // The original code did HEAD request first.

    // Optimization: If we have an extension, try that specific media loader FIRST before doing HEAD.
    // If that fails, then maybe fallback or show link.
    // This avoids the HEAD request latency for valid images/videos.

    let promise = Promise.reject();
    if (isImage) promise = tryImage();
    else if (isVideo) promise = tryVideo();
    else if (isAudio) promise = tryAudio();
    else {
        // No clear extension match, do HEAD to find out mime
        promise = fetch(url, { method: 'HEAD' })
            .then((res) => {
                if (!res.ok) throw new Error('HEAD failed');
                return handleByType((res.headers.get('Content-Type') || '').toLowerCase());
            });
    }

    promise
        .then((node) => {
            clearBox();
            if (node && node.tagName === 'IMG') {
                // Wrap images in link too? 
                // Original: box.appendChild(el('a', { href: url, target: '_blank', rel: 'noopener noreferrer' }, node));
                box.appendChild(el('a', { href: url, target: '_blank', rel: 'noopener noreferrer' }, node));
            } else {
                box.appendChild(node);
            }
        })
        .catch(() => {
            // Fallback if media load failed (e.g. 404 or corrupted) or if it wasn't media
            showLink();
        });

    return box;
}

export function renderMessageItem(m) {
    const own = (m.user_id === (store.user && store.user.id));
    const avatar = el('div', { class: 'avatar' });
    const user = own ? store.user : store.users.get(m.user_id);
    if (user && user.avatar_file_id) {
        const src = buildFileUrl(user.avatar_file_id, 'avatar');
        avatar.appendChild(el('img', {
            src,
            alt: user.username || 'avatar',
            onerror: () => { avatar.innerHTML = ''; }
        }));
    }

    // Add presence badge (only for visible statuses: online, away, dnd)
    const status = store.presenceCache.get(m.user_id) || 'offline';
    if (status === 'online' || status === 'away' || status === 'dnd') {
        const badge = el('span', { class: 'presence-badge ' + presenceClass(status) });
        avatar.appendChild(badge);
    }

    const createdAt = m.created_at || new Date().toISOString();
    const meta = el('div', { class: 'meta' }, [
        el('strong', {}, own ? (store.user?.username || 'me') : (user?.username || truncateId(m.user_id))),
        el('span', { class: 'msg-timestamp', 'data-created-at': createdAt }, localizeDate(createdAt)),
        m.edited_at ? el('span', { class: 'pill' }, 'edited') : null
    ]);
    const jumbo = isEmojiOnly(m.content);
    const content = el('div', { class: jumbo ? 'content jumbo' : 'content' }, replaceEmojisAndLinkify(m.content));
    const hasAttach = !!(m.file_url || m.file_id);
    const attach = hasAttach ? renderAttachment(m) : null;

    const tools = el('div', { class: 'tools' }, []);
    if (own) {
        tools.append(
            el('button', { class: 'iconbtn', title: 'Edit', onclick: () => editMessage(m) }, el('i', { class: 'bi bi-pencil' })),
            el('button', { class: 'iconbtn', title: 'Delete', onclick: () => deleteMessage(m) }, el('i', { class: 'bi bi-trash' }))
        );
    }

    return el('div', { class: 'msg' + (own ? ' highlight' : ''), 'data-user-id': m.user_id }, [
        avatar, el('div', { class: 'msg-right' }, [meta, content, attach]), tools
    ]);
}

/** Update presence badges in the DOM without re-rendering messages */
export function updatePresenceBadges() {
    const messages = document.querySelectorAll('.msg[data-user-id]');
    messages.forEach(msgEl => {
        const userId = msgEl.getAttribute('data-user-id');
        const avatar = msgEl.querySelector('.avatar');
        if (!avatar) return;

        // Remove existing badge
        const existingBadge = avatar.querySelector('.presence-badge');
        if (existingBadge) existingBadge.remove();

        // Add badge if status is visible (online, away, dnd)
        const status = store.presenceCache.get(userId) || 'offline';
        if (status === 'online' || status === 'away' || status === 'dnd') {
            const badge = el('span', { class: 'presence-badge ' + presenceClass(status) });
            avatar.appendChild(badge);
        }
    });
}

/** Update all message timestamps in the DOM */
export function updateAllTimestamps() {
    const timestamps = document.querySelectorAll('.msg-timestamp[data-created-at]');
    timestamps.forEach(span => {
        const createdAt = span.getAttribute('data-created-at');
        if (createdAt) {
            span.textContent = localizeDate(createdAt);
        }
    });
}

// Update timestamps every minute
setInterval(updateAllTimestamps, 60000);

export function renderMessages(channelId) {
    if (channelId !== store.currentChannelId) return;
    const wrap = $('#messages');
    wrap.innerHTML = '';
    addLoadOlderButton();
    const arr = store.messages.get(channelId) || [];
    arr.forEach(msg => {
        wrap.appendChild(renderMessageItem(msg));
    });
}

export async function fetchMessagesPage(channelId, beforeId = null) {
    let url = `/api/channels/${channelId}/messages?limit=50`;
    if (beforeId) url += '&before=' + encodeURIComponent(beforeId);
    let page = await apiFetch(url);
    page = page.map(m => {
        if (!m.file_url && m.file_id) return { ...m, file_url: `/files/${m.file_id}/file` };
        return m;
    });
    page.reverse();
    try { prefetchUsers(page.map(m => m.user_id)); } catch (_) { }

    const list = store.messages.get(channelId) || [];
    const isFirstLoad = list.length === 0;
    const atBottom = isScrolledToBottom();

    const have = new Set(list.map(m => m.id));
    const merged = beforeId ? [...page.filter(m => !have.has(m.id)), ...list] : [...list, ...page.filter(m => !have.has(m.id))];
    store.messages.set(channelId, merged);
    if (page.length) {
        store.oldestMessageId.set(channelId, merged[0].id);
    }

    renderMessages(channelId);
    if (isFirstLoad || atBottom) scrollToBottom();
}

export async function sendMessage() {
    connectWs(); // Ensure we are connected
    const content = $('#msgInput').value.trim();
    const fileInput = $('#attachFile');
    let file_id = null;

    // Check for file from file input OR from paste/drop (pendingAttachment)
    const fileToUpload = (fileInput.files && fileInput.files[0]) || store.pendingAttachment;

    if (fileToUpload) {
        const fd = new FormData();
        fd.append('file', fileToUpload);
        const res = await fetch(store.baseUrl + '/api/files', {
            method: 'POST',
            headers: store.accessToken ? { 'Authorization': 'Bearer ' + store.accessToken } : {},
            body: fd
        });
        if (!res.ok) {
            let msg = 'File upload failed';
            try { const d = await res.json(); if (d.error) msg = d.error; } catch { }
            alert(msg);
            return;
        }
        const data = await res.json();
        file_id = data.file_id;
    }

    if (!content && !file_id) return;
    $('#msgInput').value = '';
    $('#msgInput').placeholder = ' ';
    fileInput.value = '';
    store.pendingAttachment = null; // Clear pending attachment

    try {
        await apiFetch(`/api/channels/${store.currentChannelId}/messages`, {
            method: 'POST',
            body: JSON.stringify({ content: content || null, file_id: file_id || null })
        });
    } catch (e) {
        alert('Send failed: ' + e.message);
    }
    scrollToBottom();
}

export async function editMessage(m) {
    const text = prompt('Edit message:', m.content || '');
    if (text === null) return;
    try {
        await apiFetch(`/api/messages/${m.id}`, { method: 'PATCH', body: JSON.stringify({ content: text }) });
    } catch (e) { alert('Edit failed: ' + e.message); }
}

export async function deleteMessage(m) {
    if (!confirm('Delete this message?')) return;
    try {
        await apiFetch(`/api/messages/${m.id}`, { method: 'DELETE' });
    } catch (e) { alert('Delete failed: ' + e.message); }
}
