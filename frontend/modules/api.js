import { store } from './store.js';
import { $ } from './utils.js';

let _clearSession = () => console.warn('clearSession not bound');
let _connectWs = () => console.warn('connectWs not bound');
let refreshPromise = null;

export function setupApi(clearSessionFn, connectWsFn) {
    if (clearSessionFn) _clearSession = clearSessionFn;
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

function hasSessionTokens() {
    return Boolean(store.accessToken || store.refreshTokenId || store.refreshToken);
}

function shouldSetJsonContentType(body) {
    return body != null
        && !(body instanceof FormData)
        && !(body instanceof Blob)
        && !(body instanceof URLSearchParams)
        && !(body instanceof ArrayBuffer);
}

async function runRefresh(snapshot) {
    try {
        const res = await fetch(store.baseUrl + '/api/auth/refresh', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
                refresh_token_id: snapshot.refreshTokenId,
                refresh_token: snapshot.refreshToken,
            }),
        });

        if (!res.ok) {
            throw new Error(`Refresh failed (${res.status})`);
        }

        const data = await res.json();
        if (
            store.refreshTokenId !== snapshot.refreshTokenId
            || store.refreshToken !== snapshot.refreshToken
        ) {
            return false;
        }

        saveTokens(data);
        _connectWs(true);
        return true;
    } catch (e) {
        console.warn('Refresh failed', e);
        if (
            store.refreshTokenId === snapshot.refreshTokenId
            && store.refreshToken === snapshot.refreshToken
        ) {
            _clearSession(true);
        }
        return false;
    }
}

export async function refreshTokens() {
    if (!store.refreshTokenId || !store.refreshToken) {
        return false;
    }

    if (!refreshPromise) {
        const snapshot = {
            refreshTokenId: store.refreshTokenId,
            refreshToken: store.refreshToken,
        };

        refreshPromise = runRefresh(snapshot).finally(() => {
            if (refreshPromise) {
                refreshPromise = null;
            }
        });
    }

    return refreshPromise;
}

export async function apiRequest(path, opts = {}, retry = true, requiresAuth = true) {
    if (!store.baseUrl) throw new Error('Base URL not set');

    const headers = new Headers(opts.headers || {});
    if (requiresAuth && store.accessToken) {
        headers.set('Authorization', 'Bearer ' + store.accessToken);
    }
    if (!headers.has('Content-Type') && shouldSetJsonContentType(opts.body)) {
        headers.set('Content-Type', 'application/json');
    }

    const res = await fetch(store.baseUrl + path, { ...opts, headers });

    if (res.status === 401 && requiresAuth) {
        if (retry && store.refreshTokenId && store.refreshToken) {
            const ok = await refreshTokens();
            if (ok) return apiRequest(path, opts, false, requiresAuth);
        } else if (hasSessionTokens()) {
            _clearSession(true);
        }
    }

    return res;
}

export async function apiFetch(path, opts = {}, retry = true, requiresAuth = true) {
    const res = await apiRequest(path, opts, retry, requiresAuth);
    if (res.status === 204) return null;
    if (res.ok) {
        const ct = res.headers.get('Content-Type') || '';
        return ct.includes('application/json') ? res.json() : res.text();
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
