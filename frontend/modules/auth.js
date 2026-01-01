import { store } from './store.js';
import { apiFetch, saveTokens, checkServer } from './api.js';
import { $, setIf, textIf } from './utils.js';
import { connectWs } from './socket.js';
import { loadMe } from './users.js';
import { loadChannels } from './channels.js';
import { enableComposer } from './messages.js';
import { heartbeat, presencePollLoop } from './presence.js';

export function showServerStep() {
    $('#serverStep').style.display = 'block';
    $('#authStep').style.display = 'none';
    $('#serverError').textContent = '';
}

export function showAuthStep() {
    $('#serverStep').style.display = 'none';
    $('#authStep').style.display = 'block';
    $('#serverIdentifier').textContent = `Connected to: ${store.baseUrl}`;
}

export async function setBaseUrl(url) {
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

export async function doLogin(username_or_email, password) {
    const data = await apiFetch('/api/auth/login', {
        method: 'POST',
        body: JSON.stringify({ username_or_email, password })
    }, false);
    saveTokens(data);
    await bootstrapAfterAuth();
}

export async function doRegister(username, email, password) {
    const invite_code = $('#regInviteRow').style.display !== 'none' ? $('#regInvite').value : null;
    try {
        const data = await apiFetch('/api/auth/register', {
            method: 'POST',
            body: JSON.stringify({ username, email, password, invite_code })
        });
        saveTokens(data);
        await bootstrapAfterAuth();
    } catch (e) {
        $('#regErr').textContent = e.message;
    }
}

export async function logout(silent = false) {
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

export async function bootstrapAfterAuth() {
    $('#authView').style.display = 'none';
    $('#appView').style.display = 'flex';
    await loadMe();
    await loadChannels();
    connectWs();
    enableComposer(false);
    heartbeat();
    presencePollLoop();
}
