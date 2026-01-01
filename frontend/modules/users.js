import { store } from './store.js';
import { apiFetch } from './api.js';
import { $, buildFileUrl } from './utils.js';
import { renderMessages } from './messages.js';

export async function loadMe() {
    const me = await apiFetch('/api/users/me');
    store.user = me;
    if (me && me.id) {
        store.users.set(me.id, me);
    }
    $('#meName').textContent = me.username || 'me';
    $('#meEmail').textContent = me.email || '';
    if (me.avatar_file_id) {
        $('#meAvatar').innerHTML = `<img src="${buildFileUrl(me.avatar_file_id, 'avatar')}" alt="avatar">`;
    } else {
        $('#meAvatar').innerHTML = '';
    }
}

export async function updateMe(fields) {
    if (!fields || Object.keys(fields).length === 0) return;
    await apiFetch('/api/users/me', { method: 'PATCH', body: JSON.stringify(fields) });
    await loadMe();
}

export async function changeMyPassword(current_password, new_password) {
    await apiFetch('/api/users/me/password', { method: 'PUT', body: JSON.stringify({ current_password, new_password }) });
}

export async function fetchUser(userId) {
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

export async function prefetchUsers(userIds) {
    if (!Array.isArray(userIds) || userIds.length === 0) return;
    const unique = [...new Set(userIds)].filter(id => id && !store.users.has(id));
    if (unique.length === 0) return;
    await Promise.all(unique.map(id => fetchUser(id)));
}

export async function uploadAvatar(file) {
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

export async function fetchAllUsers() {
    try {
        const users = await apiFetch('/api/users');
        store.allUsers = users || [];
        return store.allUsers;
    } catch (e) {
        console.warn('Failed to load users', e.message);
        store.allUsers = [];
        return [];
    }
}
