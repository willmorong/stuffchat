import { store } from './store.js';
import { apiFetch, apiRequest, saveTokens, checkServer } from './api.js';
import { $, setIf, textIf } from './utils.js';
import { connectWs } from './socket.js';
import { loadMe } from './users.js';
import { refreshAdminVisibility } from './admin.js';
import { loadChannels } from './channels.js';
import { enableComposer } from './messages.js';
import { heartbeat, presencePollLoop, startHeartbeatLoop } from './presence.js';
import { fetchEmojis } from './emojis.js';
import { closeAllPeerConnections, updateCallUI } from './voice.js';
import { sharePlay } from './shareplay.js';

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

export function clearSession(silent = false) {
    localStorage.removeItem('stuffchat.access_token');
    localStorage.removeItem('stuffchat.refresh_token_id');
    localStorage.removeItem('stuffchat.refresh_token');

    store.accessToken = '';
    store.refreshTokenId = '';
    store.refreshToken = '';
    store.user = null;
    store.channels = [];
    store.allUsers = [];
    store.currentChannelId = null;
    store.messages.clear();
    store.unread.clear();
    store.oldestMessageId.clear();
    store.users.clear();
    store.members.clear();
    store.presenceCache.clear();
    store.typingTimers.forEach(timer => clearTimeout(timer));
    store.typingTimers.clear();
    store.typingUsers.clear();
    store.sessionId = null;
    store.voiceUsers.clear();
    store.customEmojis.clear();
    store.pendingAttachment = null;
    store.pendingReplyTo = null;
    store.pendingFocusMessageId = null;

    if (store.localStream) {
        store.localStream.getTracks().forEach(track => track.stop());
        store.localStream = null;
    }
    if (store.localVideoStream) {
        store.localVideoStream.getTracks().forEach(track => track.stop());
        store.localVideoStream = null;
    }
    closeAllPeerConnections();
    store.volumeMonitors.forEach(monitor => monitor.stop());
    store.volumeMonitors.clear();
    store.gainNodes.clear();
    store.audioSources.clear();
    store.screenShareGainNodes.clear();
    store.screenShareAudioSources.clear();
    store.callChannelId = null;
    store.inCall = false;
    store.callReconnecting = false;
    store.screenSharing = false;
    sharePlay.reset();
    updateCallUI();
    document.querySelectorAll('[id^="audio-"]').forEach(el => el.remove());

    const activeWs = store.ws;
    store.ws = null;
    if (activeWs) {
        activeWs.onclose = null;
        try { activeWs.close(); } catch { }
    }

    const appView = $('#appView');
    const authView = $('#authView');
    if (appView) appView.style.display = 'none';
    if (authView) authView.style.display = 'flex';
    if (store.baseUrl) {
        showAuthStep();
    } else {
        showServerStep();
    }

    if (!silent) {
        const loginErr = $('#loginErr');
        if (loginErr) {
            loginErr.textContent = 'Your session expired. Please sign in again.';
        }
    }
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
    }, false, false);
    saveTokens(data);
    await bootstrapAfterAuth();
}

export async function doRegister(username, email, password) {
    const invite_code = $('#regInviteRow').style.display !== 'none' ? $('#regInvite').value : null;
    try {
        const data = await apiFetch('/api/auth/register', {
            method: 'POST',
            body: JSON.stringify({ username, email, password, invite_code })
        }, false, false);
        saveTokens(data);
        await bootstrapAfterAuth();
    } catch (e) {
        $('#regErr').textContent = e.message;
    }
}

export async function logout(silent = false) {
    try {
        if (store.refreshTokenId) {
            const res = await apiRequest('/api/auth/logout', {
                method: 'POST',
                body: JSON.stringify({ refresh_token_id: store.refreshTokenId })
            });
            if (!res.ok) {
                let errMsg = 'Logout failed';
                try { const data = await res.json(); if (data && data.error) errMsg = data.error; } catch { }
                throw new Error(errMsg + ' (' + res.status + ')');
            }
        }
    } catch (e) { if (!silent) alert('Logout error: ' + e.message); }
    clearSession(true);
}

export async function bootstrapAfterAuth() {
    $('#authView').style.display = 'none';
    $('#appView').style.display = 'flex';
    await loadMe();
    refreshAdminVisibility();
    await loadChannels();
    connectWs();
    enableComposer(false);
    startHeartbeatLoop();
    presencePollLoop();
    fetchEmojis();
}
