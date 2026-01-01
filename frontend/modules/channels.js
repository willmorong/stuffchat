import { store } from './store.js';
import { apiFetch } from './api.js';
import { $, el, textIf, setIf, truncateId } from './utils.js';
import { updateCallUI } from './voice.js';
import { renderMessages, fetchMessagesPage, enableComposer } from './messages.js';
import { fetchPresenceForUsers } from './presence.js';
import { prefetchUsers, fetchAllUsers } from './users.js';

export async function loadChannels() {
    const list = await apiFetch('/api/channels');
    store.channels = list;
    renderChannelList();
    if (list.length && !store.currentChannelId) {
        selectChannel(list[0].id);
    }
}

export function renderChannelList() {
    const wrap = $('#channels');
    wrap.innerHTML = '';
    store.channels.forEach(ch => {
        const isActive = ch.id === store.currentChannelId;
        const li = el('div', { class: 'channel' + (isActive ? ' active' : ''), onclick: () => selectChannel(ch.id) }, [
            el('i', { class: 'bi ' + (ch.is_voice ? 'bi-mic' : 'bi-hash') }),
            el('div', { style: 'flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;' }, ch.name),
            ch.is_owner ? el('div', {
                class: 'settings-icon',
                onclick: (e) => {
                    e.stopPropagation();
                    openEditChannelModal(ch.id);
                }
            }, [el('i', { class: 'bi bi-gear-fill' })]) : null
        ]);
        wrap.appendChild(li);
    });
}

export async function selectChannel(channelId) {
    if (store.currentChannelId && store.ws && store.ws.readyState === 1) {
        if (store.currentChannelId !== store.callChannelId) {
            store.ws.send(JSON.stringify({ type: 'leave', channel_id: store.currentChannelId }));
        }
    }
    store.currentChannelId = channelId;
    renderChannelList();
    updateCallUI();

    const ch = store.channels.find(c => c.id === channelId);
    $('#channelName').textContent = ch ? (ch.is_voice ? '' : '# ') + ch.name : 'Channel';

    try {
        const members = await apiFetch(`/api/channels/${channelId}/members`);
        const ids = members.map(m => m.user_id);
        store.members.set(channelId, ids);
        prefetchUsers(ids).catch(() => { });
        fetchPresenceForUsers(ids);
    } catch (e) { store.members.set(channelId, []); }

    store.messages.set(channelId, []);
    store.oldestMessageId.set(channelId, null);
    $('#messages').innerHTML = '';
    // addLoadOlderButton is called inside renderMessages
    // But renderMessages isn't called yet.
    // fetchMessagesPage calls renderMessages.

    if (store.ws && store.ws.readyState === 1) {
        store.ws.send(JSON.stringify({ type: 'join', channel_id: channelId }));
    }

    await fetchMessagesPage(channelId);

    enableComposer(true);
    $('#msgInput').focus();
}

export async function createChannelAdvanced({ name, is_private, is_voice, members }) {
    if (!name || !name.trim()) throw new Error('Name required');
    const payload = { name: name.trim(), is_private: !!is_private, is_voice: !!is_voice };
    if (payload.is_private && Array.isArray(members)) payload.members = members;
    const res = await apiFetch('/api/channels', { method: 'POST', body: JSON.stringify(payload) });
    await loadChannels();
    selectChannel(res.id);
    return res;
}

// --- Modal Handlers ---

function renderCreateChannelMembers() {
    const sel = $('#channelMembersSelect');
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

export function openCreateChannelModal() {
    setIf('#chName', 'value', '');
    $('#chIsVoice').checked = false;
    $('#chIsPrivate').checked = false;
    $('#channelMembersSection').style.display = 'none';

    // We can call loadAllUsers here
    fetchAllUsers().then(() => renderCreateChannelMembers());

    $('#createChannelModal').classList.remove('hidden');
}

export function closeCreateChannelModal() { $('#createChannelModal').classList.add('hidden'); }

async function loadEditChannelMembers(channelId) {
    try {
        const membersList = await apiFetch(`/api/channels/${channelId}/members`);
        const meId = store.user?.id;
        await prefetchUsers(membersList.map(m => m.user_id));

        const curSel = $('#editChannelMembersCurrent');
        curSel.innerHTML = '';
        membersList.forEach(m => {
            if (m.user_id === meId) return;
            const u = store.users.get(m.user_id);
            const opt = document.createElement('option');
            opt.value = m.user_id;
            opt.textContent = u ? (u.username || truncateId(m.user_id)) : truncateId(m.user_id);
            curSel.appendChild(opt);
        });

        if (store.allUsers.length === 0) await fetchAllUsers();

        const addSel = $('#editChannelMembersAdd');
        addSel.innerHTML = '';
        const memberIds = new Set(membersList.map(m => m.user_id));
        store.allUsers.forEach(u => {
            if (memberIds.has(u.id)) return;
            const opt = document.createElement('option');
            opt.value = u.id;
            opt.textContent = u.username || truncateId(u.id);
            addSel.appendChild(opt);
        });
    } catch (e) {
        console.error('Failed to load channel members', e);
    }
}

export async function openEditChannelModal(channelId) {
    const ch = store.channels.find(c => c.id === channelId);
    if (!ch) return;

    setIf('#editChName', 'value', ch.name);
    $('#editChIsVoice').checked = ch.is_voice;
    $('#editChIsPrivate').checked = ch.is_private;
    $('#editChannelModal').setAttribute('data-channel-id', channelId);

    await loadEditChannelMembers(channelId);
    $('#editChannelModal').classList.remove('hidden');
}

export function closeEditChannelModal() {
    $('#editChannelModal').classList.add('hidden');
}

export async function saveEditChannel() {
    const id = $('#editChannelModal').getAttribute('data-channel-id');
    const name = $('#editChName').value.trim();
    const is_voice = $('#editChIsVoice').checked;
    const is_private = $('#editChIsPrivate').checked;

    if (!name) return alert('Name required');

    try {
        await apiFetch(`/api/channels/${id}`, {
            method: 'PATCH',
            body: JSON.stringify({ name, is_voice, is_private })
        });
        await loadChannels();
        closeEditChannelModal();
        if (id === store.currentChannelId) {
            $('#channelName').textContent = (is_voice ? '' : '# ') + name;
        }
    } catch (e) { alert('Failed to save channel: ' + e.message); }
}

export async function confirmDeleteChannel() {
    const id = $('#editChannelModal').getAttribute('data-channel-id');
    const ch = store.channels.find(c => c.id === id);
    if (!confirm(`Are you sure you want to delete channel "${ch?.name || id}"?`)) return;

    try {
        await apiFetch(`/api/channels/${id}`, { method: 'DELETE' });
        await loadChannels();
        closeEditChannelModal();
        if (id === store.currentChannelId) {
            store.currentChannelId = null;
            $('#channelName').textContent = 'Select a channel';
            $('#messages').innerHTML = '';
            enableComposer(false);
        }
    } catch (e) { alert('Delete failed: ' + e.message); }
}

export async function modifyMembersInModal(action) {
    const channelId = $('#editChannelModal').getAttribute('data-channel-id');
    let sel;
    let payload;

    if (action === 'add') {
        sel = $('#editChannelMembersAdd');
        const ids = Array.from(sel.selectedOptions).map(o => o.value);
        if (ids.length === 0) return;
        payload = { add: ids };
    } else {
        sel = $('#editChannelMembersCurrent');
        const ids = Array.from(sel.selectedOptions).map(o => o.value);
        if (ids.length === 0) return;
        payload = { remove: ids };
    }

    try {
        await apiFetch(`/api/channels/${channelId}/members`, {
            method: 'POST',
            body: JSON.stringify(payload)
        });
        await loadEditChannelMembers(channelId);
    } catch (e) { alert('Failed to modify members: ' + e.message); }
}
