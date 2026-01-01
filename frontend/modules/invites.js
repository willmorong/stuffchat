import { apiFetch } from './api.js';
import { $, el } from './utils.js';

export async function loadInvites() {
    try {
        const invites = await apiFetch('/api/invites');
        const list = $('#inviteList');
        list.innerHTML = '';
        invites.forEach(inv => {
            const tr = el('tr', {}, [
                el('td', { style: 'font-family: monospace; user-select: all;' }, [inv.code]),
                el('td', {}, [inv.joined_username || el('span', { class: 'hint' }, ['—'])]),
                el('td', { class: 'hint' }, [new Date(inv.created_at).toLocaleString()])
            ]);
            list.appendChild(tr);
        });
    } catch (e) {
        console.error('failed to load invites', e);
    }
}

export function openInviteModal() {
    $('#inviteModal').classList.remove('hidden');
    loadInvites();
}

export function closeInviteModal() {
    $('#inviteModal').classList.add('hidden');
}

export async function createInvite() {
    try {
        await apiFetch('/api/invites', { method: 'POST' });
        loadInvites();
    } catch (e) {
        alert('Failed to create invite: ' + e.message);
    }
}
