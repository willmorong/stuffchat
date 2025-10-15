// --- Simple state/store ---
const store = {
    baseUrl: localStorage.getItem('stuffchat.base_url') || '',
    accessToken: localStorage.getItem('stuffchat.access_token') || '',
    refreshTokenId: localStorage.getItem('stuffchat.refresh_token_id') || '',
    refreshToken: localStorage.getItem('stuffchat.refresh_token') || '',
    user: null,
    ws: null,
    channels: [],
    allUsers: [], // for channel creation modal
    currentChannelId: null,
    messages: new Map(), // channelId -> array of messages (ascending by created_at)
    oldestMessageId: new Map(), // channelId -> oldest id loaded (for pagination)
    users: new Map(), // userId -> { id, username, avatar_file_id, ... }
    members: new Map(), // channelId -> array of user_ids
    presenceCache: new Map(), // userId -> status
    typingTimers: new Map(), // userId -> timeout
    typingUsers: new Set(), // currently typing in current channel
    theme: localStorage.getItem('stuffchat.theme') || 'mysterious',
};

// --- Utilities ---
const $ = sel => document.querySelector(sel);
const el = (tag, attrs = {}, children = []) => {
    const e = document.createElement(tag);
    Object.entries(attrs).forEach(([k, v]) => {
        if (k === 'class') e.className = v;
        else if (k === 'style') e.style.cssText = v;
        else if (k.startsWith('on') && typeof v === 'function') e.addEventListener(k.slice(2), v);
        else if (v !== undefined && v !== null) e.setAttribute(k, v);
    });
    (Array.isArray(children) ? children : [children]).forEach(c => {
        if (c === null || c === undefined) return;
        if (typeof c === 'string') e.appendChild(document.createTextNode(c));
        else e.appendChild(c);
    });
    return e;
};
const truncateId = id => id ? id.slice(0, 8) : 'unknown';
const sleep = ms => new Promise(r => setTimeout(r, ms));
const toWsUrl = (httpUrl) => {
    if (!httpUrl) return '';
    try {
        const u = new URL(httpUrl);
        u.protocol = (u.protocol === 'https:') ? 'wss:' : 'ws:';
        u.pathname = '/ws';
        u.search = '';
        return u.toString();
    } catch { return ''; }
};
const presenceClass = status => ({
    online: 'presence-online',
    away: 'presence-away',
    dnd: 'presence-dnd',
    invisible: 'presence-invisible',
    offline: 'presence-offline'
}[status] || 'presence-offline');

// Build absolute file URL supporting new endpoint (/files/{id}/{filename})
// If only an ID is available, we add a dummy filename segment ("file") since the backend ignores it.
const buildFileUrl = (id, filename = 'file') => {
    if (!id) return '';
    console.log("building file url because we don't have one");
    const safe = encodeURIComponent(filename || 'file');
    return `${store.baseUrl}/files/${encodeURIComponent(id)}/${safe}`;
};
// Normalize server-provided file_url into an absolute URL
const absFileUrl = (file_url) => {
    if (!file_url) return '';
    try {
        // Absolute already?
        const u = new URL(file_url, store.baseUrl);
        return u.toString();
    } catch {
        return (store.baseUrl || '') + file_url;
    }
};

const setIf = (sel, prop, val) => { const n = $(sel); if (n) n[prop] = val; };
const textIf = (sel, val) => { const n = $(sel); if (n) n.textContent = val; };

function saveTokens({ access_token, refresh_token_id, refresh_token }) {
    store.accessToken = access_token;
    store.refreshTokenId = refresh_token_id;
    store.refreshToken = refresh_token;
    localStorage.setItem('stuffchat.access_token', access_token);
    localStorage.setItem('stuffchat.refresh_token_id', refresh_token_id);
    localStorage.setItem('stuffchat.refresh_token', refresh_token);
}

async function apiFetch(path, opts = {}, retry = true) {
    if (!store.baseUrl) throw new Error('Base URL not set');
    const headers = new Headers(opts.headers || {});
    headers.set('Content-Type', headers.get('Content-Type') || 'application/json');
    if (store.accessToken) headers.set('Authorization', 'Bearer ' + store.accessToken);
    const res = await fetch(store.baseUrl + path, { ...opts, headers });
    if (res.status === 204) return null;
    if (res.ok) {
        const ct = res.headers.get('Content-Type') || '';
        return ct.includes('application/json') ? res.json() : res.text();
    }
    if (res.status === 401 && retry && store.refreshTokenId && store.refreshToken) {
        const ok = await refreshTokens();
        if (ok) return apiFetch(path, opts, false);
    }
    let errMsg = 'Request failed';
    try { const data = await res.json(); if (data && data.error) errMsg = data.error; } catch { }
    throw new Error(errMsg + ' (' + res.status + ')');
}

async function checkServer(url) {
    try {
        const res = await fetch(url + '/api/health');
        if (!res.ok) throw new Error('Server not responding');
        const data = await res.json();
        return data.version ? true : false;
    } catch (e) {
        console.error('Server check failed:', e);
        throw new Error('Could not connect to server');
    }
}

function showServerStep() {
    $('#serverStep').style.display = 'block';
    $('#authStep').style.display = 'none';
    $('#serverError').textContent = '';
}

function showAuthStep() {
    $('#serverStep').style.display = 'none';
    $('#authStep').style.display = 'block';
    $('#serverIdentifier').textContent = `Connected to: ${store.baseUrl}`;
}

function applyTheme(theme) {
    store.theme = theme || 'mysterious';
    document.body.setAttribute('data-theme', store.theme === 'mysterious' ? '' : store.theme);
    localStorage.setItem('stuffchat.theme', store.theme);
    if (store.theme === 'mysterious') document.body.removeAttribute('data-theme');
}

async function refreshTokens() {
    try {
        const data = await fetch(store.baseUrl + '/api/auth/refresh', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ refresh_token_id: store.refreshTokenId, refresh_token: store.refreshToken })
        }).then(r => r.ok ? r.json() : Promise.reject(r));
        saveTokens(data);
        // reconnect WS with new token
        connectWs(true);
        return true;
    } catch (e) {
        console.warn('Refresh failed', e);
        logout(true);
        return false;
    }
}

async function setBaseUrl(url) {
    url = url.trim().replace(/\/+$/, '');
    if (!url) return;

    const busyBtn = $('#btnCheckServer') || $('#btnSaveBaseUrl');
    try {
        if (busyBtn) { busyBtn.disabled = true; busyBtn.textContent = 'Connecting...'; }
        await checkServer(url);

        const prev = localStorage.getItem('stuffchat.base_url');
        store.baseUrl = url;
        localStorage.setItem('stuffchat.base_url', url);
        setIf('#baseUrl', 'value', url);
        setIf('#cfgBaseUrl', 'value', url);
        setIf('#settingsBaseUrl', 'value', url);

        if (prev && prev !== url) {
            logout(true);
        }

        showAuthStep();
    } catch (e) {
        textIf('#serverError', e.message);
    } finally {
        if (busyBtn) { busyBtn.disabled = false; busyBtn.textContent = 'Connect'; }
    }
}


// --- Auth flow ---
async function doLogin(username_or_email, password) {
    const data = await apiFetch('/api/auth/login', {
        method: 'POST',
        body: JSON.stringify({ username_or_email, password })
    }, false);
    saveTokens(data);
    await bootstrapAfterAuth();
}
async function doRegister(username, email, password) {
    const data = await apiFetch('/api/auth/register', {
        method: 'POST',
        body: JSON.stringify({ username, email, password })
    }, false);
    saveTokens(data);
    await bootstrapAfterAuth();
}
async function logout(silent = false) {
    try {
        if (store.refreshTokenId) {
            await apiFetch('/api/auth/logout', { method: 'POST', body: JSON.stringify({ refresh_token_id: store.refreshTokenId }) });
        }
    } catch (e) { if (!silent) alert('Logout error: ' + e.message); }
    localStorage.removeItem('stuffchat.access_token');
    localStorage.removeItem('stuffchat.refresh_token_id');
    localStorage.removeItem('stuffchat.refresh_token');
    store.accessToken = ''; store.refreshTokenId = ''; store.refreshToken = ''; store.user = null;
    if (store.ws) { try { store.ws.close(); } catch { } store.ws = null; }
    $('#appView').style.display = 'none';
    $('#authView').style.display = 'flex';
}

async function bootstrapAfterAuth() {
    $('#authView').style.display = 'none';
    $('#appView').style.display = 'flex';
    await loadMe();
    await loadChannels();
    connectWs();
    enableComposer(false);
    heartbeat(); // send initial presence
    presencePollLoop(); // periodic presence refresh for members
}

async function updateMe(fields) {
    if (!fields || Object.keys(fields).length === 0) return;
    await apiFetch('/api/users/me', { method: 'PATCH', body: JSON.stringify(fields) });
    await loadMe();
}
async function changeMyPassword(current_password, new_password) {
    await apiFetch('/api/users/me/password', { method: 'PUT', body: JSON.stringify({ current_password, new_password }) });
}

// --- Presence ---
async function heartbeat() {
    const status = $('#presenceSelect').value || 'online';
    try { await apiFetch('/api/presence/heartbeat', { method: 'POST', body: JSON.stringify({ status }) }); } catch (e) { console.warn('Heartbeat failed', e.message); }
    // reflect my status badge
    const meDot = $('#mePresence');
    meDot.className = 'presence-dot ' + presenceClass(status);
    meDot.title = status;
    const settingsMeDot = $('#mePresenceSettings');
    settingsMeDot.className = 'presence-dot ' + presenceClass(status);
    settingsMeDot.title = status;
}
setInterval(heartbeat, 30000);

async function fetchPresenceForUsers(userIds) {
    if (!userIds || userIds.length === 0) return;
    const query = '?ids=' + encodeURIComponent(userIds.join(','));
    try {
        const res = await apiFetch('/api/presence/users' + query);
        res.forEach(p => {
            store.presenceCache.set(p.user_id, p.status);
        });
        renderMemberInfo();
    } catch (e) { console.warn('Presence fetch failed', e.message); }
}

async function presencePollLoop() {
    while (store.accessToken) {
        const members = store.members.get(store.currentChannelId) || [];
        await fetchPresenceForUsers(members);
        await sleep(15000);
    }
}

// --- Me/User ---
async function loadMe() {
    const me = await apiFetch('/api/users/me');
    store.user = me;
    if (me && me.id) {
        store.users.set(me.id, me);
    }
    $('#meName').textContent = me.username || 'me';
    $('#meEmail').textContent = me.email || '';
    if (me.avatar_file_id) {
        // Use new two-segment file route; filename is arbitrary and ignored by backend
        $('#meAvatar').innerHTML = `<img src="${buildFileUrl(me.avatar_file_id, 'avatar')}" alt="avatar">`;
    } else {
        $('#meAvatar').innerHTML = '';
    }
}

// --- User profiles (for names/avatars) ---
async function fetchUser(userId) {
    if (!userId) return null;
    if (store.users.has(userId)) return store.users.get(userId);
    try {
        const u = await apiFetch(`/api/users/${encodeURIComponent(userId)}`);
        store.users.set(userId, u);
        // Re-render current channel to show username/avatar once loaded
        if (store.currentChannelId) renderMessages(store.currentChannelId);
        return u;
    } catch (e) {
        // Cache a placeholder to avoid repeated fetches on errors
        store.users.set(userId, { id: userId });
        return null;
    }
}

async function prefetchUsers(userIds) {
    if (!Array.isArray(userIds) || userIds.length === 0) return;
    const unique = [...new Set(userIds)].filter(id => id && !store.users.has(id));
    if (unique.length === 0) return;
    await Promise.all(unique.map(id => fetchUser(id)));
}



async function uploadAvatar(file) {
    const fd = new FormData();
    fd.append('file', file);
    const res = await fetch(store.baseUrl + '/api/users/me/avatar', {
        method: 'PUT',
        headers: store.accessToken ? { 'Authorization': 'Bearer ' + store.accessToken } : {},
        body: fd
    });
    if (!res.ok) {
        let msg = 'Upload failed';
        try { const d = await res.json(); if (d.error) msg = d.error; } catch { }
        throw new Error(msg);
    }
    const data = await res.json();
    if (data && data.avatar_file_id) {
        $('#meAvatar').innerHTML = `<img src="${buildFileUrl(data.avatar_file_id, 'avatar')}" alt="avatar">`;
    }
}

// -- Settings ---
function openSettings() {
    // Fill current values
    setIf('#profileUsername', 'value', store.user?.username || '');
    setIf('#settingsBaseUrl', 'value', store.baseUrl || '');
    // Avatar preview in modal
    const prev = $('#settingsAvatarPreview');
    if (prev) {
        if (store.user?.avatar_file_id) {
            prev.innerHTML = `<img src="${buildFileUrl(store.user.avatar_file_id, 'avatar')}" alt="avatar">`;
        } else {
            prev.innerHTML = '';
        }
    }
    // Theme selection
    const radios = document.querySelectorAll('input[name="themeSel"]');
    radios.forEach(r => { r.checked = (r.value === store.theme); });

    $('#settingsModal').classList.remove('hidden');
}
function closeSettings() {
    $('#settingsModal').classList.add('hidden');
}

// --- Channels ---
async function loadChannels() {
    const list = await apiFetch('/api/channels');
    store.channels = list;
    renderChannelList();
    // Auto-select first channel
    if (list.length && !store.currentChannelId) {
        selectChannel(list[0].id);
    }
}

function renderChannelList() {
    const wrap = $('#channels');
    wrap.innerHTML = '';
    store.channels.forEach(ch => {
        const isActive = ch.id === store.currentChannelId;
        const li = el('div', { class: 'channel' + (isActive ? ' active' : ''), onclick: () => selectChannel(ch.id) }, [
            el('i', { class: 'bi ' + (ch.is_voice ? 'bi-mic' : 'bi-hash') }),
            el('div', {}, ch.name)
        ]);
        wrap.appendChild(li);
    });
}

async function createChannelAdvanced({ name, is_private, is_voice, members }) {
    if (!name || !name.trim()) throw new Error('Name required');
    const payload = { name: name.trim(), is_private: !!is_private, is_voice: !!is_voice };
    if (payload.is_private && Array.isArray(members)) payload.members = members;
    const res = await apiFetch('/api/channels', { method: 'POST', body: JSON.stringify(payload) });
    await loadChannels();
    selectChannel(res.id);
}

async function loadAllUsers() {
    try {
        const users = await apiFetch('/api/users');
        store.allUsers = users || [];
        renderCreateChannelMembers();
    } catch (e) {
        console.warn('Failed to load users', e.message);
        store.allUsers = [];
        renderCreateChannelMembers();
    }
}

function openCreateChannelModal() {
    // reset fields
    setIf('#chName', 'value', '');
    $('#chIsVoice').checked = false;
    $('#chIsPrivate').checked = false;
    $('#chMembersSection').style.display = 'none';
    renderCreateChannelMembers();
    // lazy load users
    loadAllUsers();
    $('#createChannelModal').classList.remove('hidden');
}
function closeCreateChannelModal() { $('#createChannelModal').classList.add('hidden'); }

function renderCreateChannelMembers() {
    const sel = $('#chMembersSelect');
    if (!sel) return;
    sel.innerHTML = '';
    const meId = store.user?.id;
    store.allUsers
        .filter(u => u.id !== meId)
        .forEach(u => {
            const opt = document.createElement('option');
            opt.value = u.id;
            opt.textContent = u.username || truncateId(u.id);
            sel.appendChild(opt);
        });
}

// --- Messages ---
async function selectChannel(channelId) {
    if (store.currentChannelId && store.ws) {
        store.ws.send(JSON.stringify({ type: 'leave', channel_id: store.currentChannelId }));
    }
    store.currentChannelId = channelId;
    renderChannelList();

    const ch = store.channels.find(c => c.id === channelId);
    $('#channelName').textContent = ch ? '# ' + ch.name : 'Channel';

    // load members
    try {
        const members = await apiFetch(`/api/channels/${channelId}/members`);
        const ids = members.map(m => m.user_id);
        store.members.set(channelId, ids);
        prefetchUsers(ids).catch(() => {});
        fetchPresenceForUsers(ids);
    } catch (e) { store.members.set(channelId, []); }

    // reset messages state
    store.messages.set(channelId, []);
    store.oldestMessageId.set(channelId, null);
    $('#messages').innerHTML = '';
    addLoadOlderButton();

    // Join WS room
    if (store.ws && store.ws.readyState === 1) {
        store.ws.send(JSON.stringify({ type: 'join', channel_id: channelId }));
    }

    // fetch first page
    await fetchMessagesPage(channelId);

    enableComposer(true);
    $('#msgInput').focus();
}

function addLoadOlderButton() {
    const btn = el('div', { class: 'load-older', id: 'loadOlder', onclick: () => loadOlder() }, [
        el('i', { class: 'bi bi-chevron-up' }), ' Load older'
    ]);
    $('#messages').appendChild(btn);
}

async function loadOlder() {
    const chan = store.currentChannelId;
    const arr = store.messages.get(chan) || [];
    const before = arr.length ? arr[0].id : null;
    await fetchMessagesPage(chan, before);
}

async function fetchMessagesPage(channelId, beforeId = null) {
    let url = `/api/channels/${channelId}/messages?limit=50`;
    if (beforeId) url += '&before=' + encodeURIComponent(beforeId);
    let page = await apiFetch(url);
    // Normalize messages to have file_url if server still sends file_id (back-compat)
    page = page.map(m => {
        if (!m.file_url && m.file_id) {
            return { ...m, file_url: `/files/${m.file_id}/file` };
        }
        return m;
    });
    // API returns newest-first; we want ascending:
    page.reverse();
    // Prefetch authors so we can show names/avatars
    try {
        prefetchUsers(page.map(m => m.user_id));
    } catch (_) {}
    const list = store.messages.get(channelId) || [];
    const isFirstLoad = list.length === 0;
    const atBottom = isScrolledToBottom();

    // Merge while avoiding duplicates
    const have = new Set(list.map(m => m.id));
    const merged = beforeId ? [...page.filter(m => !have.has(m.id)), ...list] : [...list, ...page.filter(m => !have.has(m.id))];
    store.messages.set(channelId, merged);
    if (page.length) {
        store.oldestMessageId.set(channelId, merged[0].id);
    }

    renderMessages(channelId);

    if (isFirstLoad || atBottom) scrollToBottom();
}

function renderMessages(channelId) {
    if (channelId !== store.currentChannelId) return;
    const wrap = $('#messages');
    wrap.innerHTML = '';
    addLoadOlderButton();
    const arr = store.messages.get(channelId) || [];
    arr.forEach(msg => {
        wrap.appendChild(renderMessageItem(msg));
    });
}

function renderMessageItem(m) {
    const own = (m.user_id === (store.user && store.user.id));
    const avatar = el('div', { class: 'avatar' });
    const user = own ? store.user : store.users.get(m.user_id);
    // Show avatar for any user if we have avatar_file_id
    if (user && user.avatar_file_id) {
        const src = buildFileUrl(user.avatar_file_id, 'avatar');
        const img = el('img', {
            src,
            alt: user.username ? `${user.username}'s avatar` : 'avatar',
            onerror: () => { avatar.innerHTML = ''; }
        });
        avatar.appendChild(img);
    }

    const meta = el('div', { class: 'meta' }, [
        el('strong', {},
            own
                ? (store.user?.username || 'me')
                : (user?.username || truncateId(m.user_id))
        ),
        el('span', {}, '•'),
        el('span', {}, new Date(m.created_at || Date.now()).toLocaleString()),
        m.edited_at ? el('span', { class: 'pill' }, 'edited') : null
    ]);
    const content = el('div', { class: 'content' }, m.content || '');
    const hasAttach = !!(m.file_url || m.file_id);
    const attach = hasAttach ? renderAttachment(m) : null;

    const tools = el('div', { class: 'tools' }, []);
    if (own) {
        const editBtn = el('button', { class: 'iconbtn', title: 'Edit', onclick: () => editMessage(m) }, el('i', { class: 'bi bi-pencil' }));
        const delBtn = el('button', { class: 'iconbtn', title: 'Delete', onclick: () => deleteMessage(m) }, el('i', { class: 'bi bi-trash' }));
        tools.append(editBtn, delBtn);
    }

    const right = el('div', { class: 'msg-right' }, [meta, content, attach]);
    const row = el('div', { class: 'msg' + (own ? ' own' : '') }, [
        avatar, right, tools
    ]);
    return row;
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

    const tryVideo = () =>
        new Promise((resolve, reject) => {
            const v = el('video', {
                src: url,
                controls: true,
                preload: 'metadata',
                playsinline: false,
            });

            const onOK = () => done(true);
            const onErr = () => done(false);
            const done = (ok) => {
                v.removeEventListener('loadedmetadata', onOK);
                v.removeEventListener('canplay', onOK);
                v.removeEventListener('error', onErr);
                ok ? resolve(v) : reject();
            };

            v.addEventListener('loadedmetadata', onOK, { once: true });
            v.addEventListener('canplay', onOK, { once: true });
            v.addEventListener('error', onErr, { once: true });

            try { v.load(); } catch (_) { }
        });

    const tryAudio = () =>
        new Promise((resolve, reject) => {
            const a = el('audio', {
                src: url,
                controls: true,
                preload: 'metadata',
            });

            const onOK = () => done(true);
            const onErr = () => done(false);
            const done = (ok) => {
                a.removeEventListener('loadedmetadata', onOK);
                a.removeEventListener('canplay', onOK);
                a.removeEventListener('error', onErr);
                ok ? resolve(a) : reject();
            };

            a.addEventListener('loadedmetadata', onOK, { once: true });
            a.addEventListener('canplay', onOK, { once: true });
            a.addEventListener('error', onErr, { once: true });

            try { a.load(); } catch (_) { }
        });

    // Try functions in order, returning the first that resolves.
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
            const ct = (res.headers.get('Content-Type') || '').toLowerCase();
            return handleByType(ct);
        })
        .catch(() => {
            // HEAD blocked or unknown content-type: guess by extension, then try fallbacks
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
                const link = el('a', {
                    href: url,
                    target: '_blank',
                    rel: 'noopener noreferrer',
                    title: 'Open image in new tab',
                }, node);
                box.appendChild(link);
            } else {
                box.appendChild(node);
            }
        })
        .catch(showLink);

    return box;
}

function isScrolledToBottom() {
    const wrap = $('#messages');
    return wrap.scrollHeight - wrap.scrollTop - wrap.clientHeight < 10;
}

function scrollToBottom() {
    const wrap = $('#messages');
    wrap.scrollTop = wrap.scrollHeight;
}

async function sendMessage() {
    const content = $('#msgInput').value.trim();
    const fileInput = $('#attachFile');
    let file_id = null;

    // Upload first attached file if present
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
        // We rely on message_created event to render; if missed, we could fallback to refetch
    } catch (e) {
        alert('Send failed: ' + e.message);
    }
    scrollToBottom();
}

async function editMessage(m) {
    const text = prompt('Edit message:', m.content || '');
    if (text === null) return;
    try {
        await apiFetch(`/api/messages/${m.id}`, { method: 'PATCH', body: JSON.stringify({ content: text }) });
        // WS will emit message_edited
    } catch (e) { alert('Edit failed: ' + e.message); }
}

async function deleteMessage(m) {
    if (!confirm('Delete this message?')) return;
    try {
        await apiFetch(`/api/messages/${m.id}`, { method: 'DELETE' });
        // WS will emit message_deleted
    } catch (e) { alert('Delete failed: ' + e.message); }
}

function enableComposer(enabled) {
    $('#msgInput').disabled = !enabled;
    $('#btnSend').disabled = !enabled;
}

// --- WebSocket ---
function connectWs(reconnect = false) {
    const url = toWsUrl(store.baseUrl);
    if (!url || !store.accessToken) return;
    try {
        if (store.ws) { try { store.ws.close(); } catch { } }
        const ws = new WebSocket(url + '?token=' + encodeURIComponent(store.accessToken));
        store.ws = ws;
        ws.onopen = () => {
            // Rejoin current channel
            if (store.currentChannelId) {
                ws.send(JSON.stringify({ type: 'join', channel_id: store.currentChannelId }));
            }
        };
        ws.onmessage = (ev) => {
            try {
                const msg = JSON.parse(ev.data);
                handleWsMessage(msg);
            } catch (e) { console.warn('WS parse error', e) }
        };
        ws.onclose = () => {
            // Try to reconnect after a delay
            if (store.accessToken) {
                setTimeout(() => connectWs(true), 2000);
            }
        };
    } catch (e) { console.warn('WS connect error', e.message); }
}

function handleWsMessage(ev) {
    switch (ev.type) {
        case 'message_created': {
            // Back-compat: normalize to include file_url if only file_id is provided
            if (!ev.file_url && ev.file_id) {
                ev.file_url = `/files/${ev.file_id}/file`;
            }
            if (ev.user_id && !store.users.has(ev.user_id)) fetchUser(ev.user_id);
            const arr = store.messages.get(ev.channel_id) || [];
            if (!arr.some(m => m.id === ev.id)) {
                arr.push(ev); // created_at is present; append at end (ascending)
                store.messages.set(ev.channel_id, arr);
            }
            if (ev.channel_id === store.currentChannelId) {
                const atBottom = isScrolledToBottom();
                $('#messages').appendChild(renderMessageItem(ev));
                if (atBottom) scrollToBottom();
            }
            break;
        }
        case 'message_edited': {
            const arr = store.messages.get(ev.channel_id) || [];
            const m = arr.find(x => x.id === ev.id);
            if (m) { m.content = ev.content; m.edited_at = ev.edited_at; }
            if (ev.channel_id === store.currentChannelId) renderMessages(ev.channel_id);
            break;
        }
        case 'message_deleted': {
            const arr = store.messages.get(ev.channel_id) || [];
            const idx = arr.findIndex(x => x.id === ev.id);
            if (idx >= 0) { arr.splice(idx, 1); }
            if (ev.channel_id === store.currentChannelId) renderMessages(ev.channel_id);
            break;
        }
        case 'chat_message': {
            // Ephemeral echo; show with a distinctive pill
            if (ev.channel_id === store.currentChannelId) {
                const pseudo = {
                    id: 'ephemeral-' + Math.random().toString(36).slice(2),
                    channel_id: ev.channel_id,
                    user_id: ev.user_id,
                    content: '[ephemeral] ' + ev.content,
                    created_at: new Date().toISOString()
                };
                const atBottom = isScrolledToBottom();
                $('#messages').appendChild(renderMessageItem(pseudo));
                if (atBottom) scrollToBottom();
            }
            break;
        }
        case 'typing': {
            if (ev.channel_id !== store.currentChannelId) break;
            if (ev.started) {
                store.typingUsers.add(ev.user_id);
                updateTypingIndicator();
                // auto-clear after a couple seconds without further events
                if (store.typingTimers.has(ev.user_id)) clearTimeout(store.typingTimers.get(ev.user_id));
                store.typingTimers.set(ev.user_id, setTimeout(() => {
                    store.typingUsers.delete(ev.user_id);
                    updateTypingIndicator();
                }, 3000));
            } else {
                store.typingUsers.delete(ev.user_id);
                updateTypingIndicator();
            }
            break;
        }
        case 'pong': break;
        default: break;
    }
}

function updateTypingIndicator() {
    const elTip = $('#typingIndicator');
    if (store.typingUsers.size) {
        const sample = [...store.typingUsers][0];
        let name = '';
        if (sample === store.user?.id) {
            name = store.user?.username || 'You';
        } else {
            const u = store.users.get(sample);
            name = u?.username || truncateId(sample);
        }
        elTip.textContent = (store.typingUsers.size > 1) ? 'Several people are typing…' : (name + ' is typing…');
        elTip.style.display = '';
    } else {
        elTip.style.display = 'none';
    }
}

function sendTyping(started) {
    if (store.ws && store.ws.readyState === 1 && store.currentChannelId) {
        store.ws.send(JSON.stringify({ type: 'typing', channel_id: store.currentChannelId, started }));
    }
}

// --- UI bindings ---
function renderMemberInfo() {
    const ids = store.members.get(store.currentChannelId) || [];
    const onlineCount = ids.filter(id => (store.presenceCache.get(id) || 'offline') !== 'offline').length;
    $('#memberInfo').textContent = `${ids.length} members • ${onlineCount} online`;
}

function bindUI() {
    $('#btnCheckServer').addEventListener('click', () => setBaseUrl($('#cfgBaseUrl').value));
    // Sidebar baseUrl field removed; guard the old binding if present:
    if ($('#baseUrl')) {
        $('#baseUrl').addEventListener('change', e => setBaseUrl(e.target.value));
    }

    $('#btnLogin').addEventListener('click', async () => {
        $('#loginErr').textContent = '';
        try { await doLogin($('#loginUser').value, $('#loginPass').value); }
        catch (e) { $('#loginErr').textContent = e.message; }
    });
    $('#btnRegister').addEventListener('click', async () => {
        $('#regErr').textContent = '';
        try { await doRegister($('#regUser').value, $('#regEmail').value, $('#regPass').value); }
        catch (e) { $('#regErr').textContent = e.message; }
    });

    // Old direct logout button in sidebar removed; keep global if still present
    if ($('#btnLogout')) $('#btnLogout').addEventListener('click', () => logout());

    // Create Channel modal open
    $('#btnOpenCreateChannel').addEventListener('click', openCreateChannelModal);

    // Create Channel modal close handlers
    $('#btnCloseCreateChannel').addEventListener('click', closeCreateChannelModal);
    $('#createChannelModal').addEventListener('click', (e) => {
        if (e.target === $('#createChannelModal') || e.target === $('#createChannelModal .modal-backdrop')) closeCreateChannelModal();
    });

    // Toggle members section when privacy changes
    $('#chIsPrivate').addEventListener('change', (e) => {
        const on = e.target.checked;
        $('#chMembersSection').style.display = on ? '' : 'none';
    });

    // Submit create channel
    $('#btnCreateChannelSubmit').addEventListener('click', async () => {
        const name = $('#chName').value.trim();
        const is_private = $('#chIsPrivate').checked;
        const is_voice = $('#chIsVoice').checked;
        let members = [];
        if (is_private) {
            const opts = Array.from($('#chMembersSelect').selectedOptions || []);
            members = opts.map(o => o.value);
        }
        try {
            await createChannelAdvanced({ name, is_private, is_voice, members });
            closeCreateChannelModal();
        } catch (e) {
            alert('Create failed: ' + e.message);
        }
    });

    $('#btnSend').addEventListener('click', sendMessage);
    $('#msgInput').addEventListener('keydown', e => {
        if (e.key === 'Enter' && !e.shiftKey) {
            e.preventDefault();
            sendMessage();
            sendTyping(false);
        } else {
            sendTyping(true);
        }
    });
    let typingDeb;
    $('#msgInput').addEventListener('input', () => {
        if (typingDeb) clearTimeout(typingDeb);
        typingDeb = setTimeout(() => sendTyping(false), 1000);
    });

    // Avatar upload: old #avatarFile removed; now wired in modal
    const modalAvatar = $('#setAvatarFile');
    if (modalAvatar) {
        modalAvatar.addEventListener('change', async (e) => {
            const f = e.target.files && e.target.files[0];
            if (!f) return;
            try {
                await uploadAvatar(f);
                // Reflect in modal preview too
                const prev = $('#settingsAvatarPreview');
                if (store.user?.avatar_file_id && prev) {
                    prev.innerHTML = `<img src="${buildFileUrl(store.user.avatar_file_id, 'avatar')}" alt="avatar">`;
                } else if (prev) {
                    prev.innerHTML = '';
                }
            } catch (err) { alert(err.message); }
            e.target.value = '';
        });
    }

    $('#attachFile').addEventListener('change', () => {
        if ($('#attachFile').files && $('#attachFile').files[0]) {
            const name = $('#attachFile').files[0].name;
            $('#msgInput').placeholder = 'Attached: ' + name;
        } else {
            $('#msgInput').placeholder = 'Write a message…';
        }
    });

    $('#btnToggleSidebar').addEventListener('click', () => {
        $('#sidebar').classList.toggle('open');
    });

    $('#presenceSelect').addEventListener('change', heartbeat);

    // Settings modal open/close
    $('#btnOpenSettings').addEventListener('click', openSettings);
    $('#btnCloseSettings').addEventListener('click', closeSettings);
    $('#settingsModal').addEventListener('click', (e) => {
        if (e.target === $('#settingsModal') || e.target === $('.modal-backdrop')) closeSettings();
    });
    window.addEventListener('keydown', (e) => {
        if (e.key === 'Escape' && !$('#settingsModal').classList.contains('hidden')) closeSettings();
    });

    // Profile save
    $('#btnSaveProfile').addEventListener('click', async () => {
        const name = $('#profileUsername').value.trim();
        try {
            await updateMe({ username: name || null });
            alert('Profile updated.');
        } catch (e) { alert('Update failed: ' + e.message); }
    });

    // Password change
    $('#btnChangePassword').addEventListener('click', async () => {
        const cur = $('#curPwd').value, nw = $('#newPwd').value;
        if (!cur || !nw) return alert('Enter current and new password.');
        try {
            await changeMyPassword(cur, nw);
            $('#curPwd').value = ''; $('#newPwd').value = '';
            alert('Password changed.');
        } catch (e) { alert('Change failed: ' + e.message); }
    });

    // Theme select
    document.querySelectorAll('input[name="themeSel"]').forEach(r => {
        r.addEventListener('change', () => applyTheme(r.value));
    });

    // Server save from modal
    $('#btnSaveBaseUrl').addEventListener('click', () => setBaseUrl($('#settingsBaseUrl').value));

    // Logout from modal
    $('#btnLogoutSettings').addEventListener('click', () => logout());

    window.addEventListener('beforeunload', () => {
        if (store.ws) try { store.ws.close(); } catch { }
    });
}

// --- Init ---
async function init() {
    bindUI();

    // Apply saved theme
    applyTheme(store.theme);

    const storedUrl = localStorage.getItem('stuffchat.base_url');
    if (storedUrl) {
        store.baseUrl = storedUrl;
        setIf('#cfgBaseUrl', 'value', storedUrl);
        setIf('#baseUrl', 'value', storedUrl);
        setIf('#settingsBaseUrl', 'value', storedUrl);
        try {
            await checkServer(storedUrl);
            showAuthStep();
            if (store.accessToken) {
                await bootstrapAfterAuth();
            }
        } catch {
            showServerStep();
        }
    } else {
        showServerStep();
    }
}

init();