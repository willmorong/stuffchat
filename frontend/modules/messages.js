import { store } from './store.js';
import { apiFetch } from './api.js';
import { $, el, absFileUrl, buildFileUrl, setIf, truncateId } from './utils.js';
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

    const clearBox = () => {
        while (box.firstChild) box.removeChild(box.firstChild);
    };

    const showLink = () => {
        clearBox();
        box.appendChild(
            el('a', { href: url, target: '_blank', rel: 'noopener noreferrer' }, 'Download attachment')
        );
    };

    const tryImage = () =>
        new Promise((resolve, reject) => {
            const img = new Image();
            img.alt = 'attachment';
            img.onload = () => resolve(img);
            img.onerror = reject;
            img.src = url;
        });
    // ... video/audio helpers ...
    // Simplified: reuse logic or copy it. Copying for robust module.
    // To save lines, I'll be concise.
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
    });
    const tryVideo = () => createMedia('video');
    const tryAudio = () => createMedia('audio');

    const tryInOrder = (fns) =>
        fns.reduce((p, fn) => p.catch(() => fn()), Promise.reject());

    const handleByType = (ct) => {
        if (!ct) return tryInOrder([tryImage, tryVideo, tryAudio]);
        if (ct.startsWith('image/')) return tryImage();
        if (ct.startsWith('video/')) return tryInOrder([tryVideo, tryAudio, tryImage]);
        if (ct.startsWith('audio/')) return tryInOrder([tryAudio, tryVideo, tryImage]);
        return tryInOrder([tryImage, tryVideo, tryAudio]);
    };

    fetch(url, { method: 'HEAD' })
        .then((res) => {
            if (!res.ok) throw new Error('HEAD failed');
            return handleByType((res.headers.get('Content-Type') || '').toLowerCase());
        })
        .catch(() => {
            const ext = (url.split('?')[0].split('.').pop() || '').toLowerCase();
            let guess = '';
            if (['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'svg', 'avif'].includes(ext)) guess = 'image/';
            else if (['mp4', 'webm', 'ogv', 'mov', 'm4v'].includes(ext)) guess = 'video/';
            else if (['mp3', 'wav', 'ogg', 'm4a', 'aac', 'flac', 'opus'].includes(ext)) guess = 'audio/';
            return handleByType(guess);
        })
        .then((node) => {
            clearBox();
            if (node && node.tagName === 'IMG') {
                box.appendChild(el('a', { href: url, target: '_blank', rel: 'noopener noreferrer' }, node));
            } else {
                box.appendChild(node);
            }
        })
        .catch(showLink);

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

    const meta = el('div', { class: 'meta' }, [
        el('strong', {}, own ? (store.user?.username || 'me') : (user?.username || truncateId(m.user_id))),
        el('span', {}, '•'),
        el('span', {}, new Date(m.created_at || Date.now()).toLocaleString()),
        m.edited_at ? el('span', { class: 'pill' }, 'edited') : null
    ]);
    const content = el('div', { class: 'content' }, m.content || '');
    const hasAttach = !!(m.file_url || m.file_id);
    const attach = hasAttach ? renderAttachment(m) : null;

    const tools = el('div', { class: 'tools' }, []);
    if (own) {
        tools.append(
            el('button', { class: 'iconbtn', title: 'Edit', onclick: () => editMessage(m) }, el('i', { class: 'bi bi-pencil' })),
            el('button', { class: 'iconbtn', title: 'Delete', onclick: () => deleteMessage(m) }, el('i', { class: 'bi bi-trash' }))
        );
    }

    return el('div', { class: 'msg' + (own ? ' own' : '') }, [
        avatar, el('div', { class: 'msg-right' }, [meta, content, attach]), tools
    ]);
}

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
    const content = $('#msgInput').value.trim();
    const fileInput = $('#attachFile');
    let file_id = null;

    if (fileInput.files && fileInput.files[0]) {
        const fd = new FormData();
        fd.append('file', fileInput.files[0]);
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
    $('#msgInput').placeholder = 'Write a message…';
    fileInput.value = '';

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
