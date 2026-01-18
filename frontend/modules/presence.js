import { store } from './store.js';
import { apiFetch } from './api.js';
import { $, presenceClass } from './utils.js';
import { updatePresenceBadges } from './messages.js';

export async function heartbeat() {
    const status = $('#presenceSelect').value || 'online';
    try { await apiFetch('/api/presence/heartbeat', { method: 'POST', body: JSON.stringify({ status }) }); } catch (e) { console.warn('Heartbeat failed', e.message); }
    const meDot = $('#mePresence');
    meDot.className = 'presence-dot ' + presenceClass(status);
    meDot.title = status;
}

let _heartbeatLoopActive = false;
export async function startHeartbeatLoop() {
    if (_heartbeatLoopActive) return;
    _heartbeatLoopActive = true;
    // Initial heartbeat
    await heartbeat();
    while (store.accessToken) {
        await new Promise(r => setTimeout(r, PRESENCE_INTERVAL));
        if (!store.accessToken) break;
        await heartbeat();
    }
    _heartbeatLoopActive = false;
}

export function renderMemberInfo() {
    const ids = store.members.get(store.currentChannelId) || [];
    const onlineCount = ids.filter(id => (store.presenceCache.get(id) || 'offline') !== 'offline').length;
    $('#memberInfo').textContent = `${ids.length} members • ${onlineCount} online`;
}

export async function fetchPresenceForUsers(userIds) {
    if (!userIds || userIds.length === 0) return;
    const query = '?ids=' + encodeURIComponent(userIds.join(','));
    try {
        const res = await apiFetch('/api/presence/users' + query);
        res.forEach(p => {
            store.presenceCache.set(p.user_id, p.status);
        });
        renderMemberInfo();
        updatePresenceBadges();
    } catch (e) { console.warn('Presence fetch failed', e.message); }
}

export const PRESENCE_INTERVAL = 30000;
export const POLL_INTERVAL = 15000;

let _pollLoopActive = false;
export async function presencePollLoop() {
    if (_pollLoopActive) return;
    _pollLoopActive = true;
    while (store.accessToken) {
        const members = store.members.get(store.currentChannelId) || [];
        await fetchPresenceForUsers(members);
        await new Promise(r => setTimeout(r, POLL_INTERVAL));
    }
    _pollLoopActive = false;
}
