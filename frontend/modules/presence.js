import { store } from './store.js';
import { apiFetch } from './api.js';
import { $, presenceClass, buildFileUrl } from './utils.js';
import { updatePresenceBadges } from './messages.js';

export async function heartbeat() {
    const status = $('#presenceSelect').value || 'online';

    // Update local cache immediately for instant feedback
    if (store.user?.id) {
        store.presenceCache.set(store.user.id, status);
        updatePresenceBadges();
    }

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

export async function openMembersModal() {
    const channelId = store.currentChannelId;
    const ids = store.members.get(channelId) || [];
    const membersList = $('#membersList');

    // Fetch and populate channel info
    try {
        const info = await apiFetch(`/api/channels/${channelId}/info`);
        $('#channelInfoName').textContent = info.name;
        $('#channelInfoOwner').textContent = info.owner_username;
        $('#channelInfoCreated').textContent = new Date(info.created_at).toLocaleDateString();
        $('#channelInfoMessages').textContent = info.message_count.toLocaleString();
    } catch (e) {
        console.warn('Failed to fetch channel info', e);
    }

    if (ids.length === 0) {
        membersList.innerHTML = '<div class="hint">No members in this channel</div>';
    } else {
        membersList.innerHTML = ids.map(id => {
            const user = store.users.get(id);
            const username = user?.username || id.substring(0, 8);
            const avatarUrl = user?.avatar_file_id ? buildFileUrl(user.avatar_file_id, 'avatar') : null;
            const status = store.presenceCache.get(id) || 'offline';

            const avatarHtml = avatarUrl
                ? `<img src="${avatarUrl}" alt="${username}" onerror="this.style.display='none'">`
                : '';

            return `
                <div class="member-item">
                    <div class="avatar">
                        ${avatarHtml}
                        <div class="presence-badge ${presenceClass(status)}"></div>
                    </div>
                    <span class="member-name">${username}</span>
                    <span class="member-status">${status}</span>
                </div>
            `;
        }).join('');
    }

    $('#membersModal').classList.remove('hidden');
}

export function closeMembersModal() {
    $('#membersModal').classList.add('hidden');
}

export function setupMembersModalListeners() {
    $('#btnMemberInfo').addEventListener('click', openMembersModal);
    $('#btnCloseMembers').addEventListener('click', closeMembersModal);
    $('#membersModal .modal-backdrop').addEventListener('click', closeMembersModal);
}

export async function fetchPresenceForUsers(userIds) {
    if (!userIds || userIds.length === 0) return;
    const query = '?ids=' + encodeURIComponent(userIds.join(','));
    try {
        const res = await apiFetch('/api/presence/users' + query);
        res.forEach(p => {
            store.presenceCache.set(p.user_id, p.status);
        });
        refreshMembersModalIfOpen();
        updatePresenceBadges();
    } catch (e) { console.warn('Presence fetch failed', e.message); }
}

function refreshMembersModalIfOpen() {
    const modal = $('#membersModal');
    if (modal && !modal.classList.contains('hidden')) {
        openMembersModal();
    }
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
