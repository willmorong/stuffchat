import { store } from './store.js';
import { $ } from './utils.js';

let _logout = () => console.warn('logout not bound');
let _connectWs = () => console.warn('connectWs not bound');

export function setupApi(logoutFn, connectWsFn) {
    if (logoutFn) _logout = logoutFn;
    if (connectWsFn) _connectWs = connectWsFn;
}

export function saveTokens({ access_token, refresh_token_id, refresh_token }) {
    store.accessToken = access_token;
    store.refreshTokenId = refresh_token_id;
    store.refreshToken = refresh_token;
    localStorage.setItem('stuffchat.access_token', access_token);
    localStorage.setItem('stuffchat.refresh_token_id', refresh_token_id);
    localStorage.setItem('stuffchat.refresh_token', refresh_token);
}

export async function refreshTokens() {
    try {
        const data = await fetch(store.baseUrl + '/api/auth/refresh', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ refresh_token_id: store.refreshTokenId, refresh_token: store.refreshToken })
        }).then(r => r.ok ? r.json() : Promise.reject(r));
        saveTokens(data);
        // reconnect WS with new token
        _connectWs(true);
        return true;
    } catch (e) {
        console.warn('Refresh failed', e);
        _logout(true);
        return false;
    }
}

export async function apiFetch(path, opts = {}, retry = true) {
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

export async function checkServer(url) {
    try {
        const res = await fetch(url + '/api/health');
        if (!res.ok) throw new Error('Server not responding');
        const data = await res.json();
        if (data.config && data.config.invite_only) {
            const regInviteRow = $('#regInviteRow');
            if (regInviteRow) regInviteRow.style.display = 'flex';
        } else {
            const regInviteRow = $('#regInviteRow');
            if (regInviteRow) regInviteRow.style.display = 'none';
        }
        return data.version ? true : false;
    } catch (e) {
        console.error('Server check failed:', e);
        throw new Error('Could not connect to server');
    }
}
