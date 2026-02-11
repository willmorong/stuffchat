import { store } from './store.js';
import { apiFetch } from './api.js';
import { $, buildFileUrl } from './utils.js';

let adminUsers = [];
let adminRoles = [];
let adminLoaded = false;

export function hasAdminRole() {
    return !!store.user?.roles?.some(r => r.name === 'admin');
}

export function refreshAdminVisibility() {
    const btn = $('#btnOpenAdmin');
    if (!btn) return;
    btn.style.display = hasAdminRole() ? 'inline-flex' : 'none';
}

function setStatus(msg, isError = false) {
    const el = $('#adminStatus');
    if (!el) return;
    el.textContent = msg || '';
    el.classList.toggle('error', isError);
}

function closeAdminModal() {
    $('#adminModal').classList.add('hidden');
}

async function openAdminModal() {
    if (!hasAdminRole()) return;
    $('#adminModal').classList.remove('hidden');
    await loadAdminData();
}

async function loadAdminData() {
    try {
        adminUsers = await apiFetch('/api/admin/users');
        adminRoles = await apiFetch('/api/admin/roles');
        renderAdminUsers();
        renderAdminRoles();
        adminLoaded = true;
        setStatus('');
    } catch (e) {
        setStatus(e.message, true);
    }
}

function renderAdminUsers() {
    const select = $('#adminUserSelect');
    if (!select) return;
    const current = select.value;
    select.innerHTML = '';
    const placeholder = document.createElement('option');
    placeholder.value = '';
    placeholder.textContent = 'Select a user…';
    select.appendChild(placeholder);

    adminUsers.forEach(u => {
        const opt = document.createElement('option');
        opt.value = u.id;
        opt.textContent = u.username || u.id.slice(0, 8);
        select.appendChild(opt);
    });

    if (current) select.value = current;
    if (!select.value && adminUsers.length) {
        select.value = adminUsers[0].id;
    }
    renderSelectedUser();
}

function renderSelectedUser() {
    const select = $('#adminUserSelect');
    if (!select) return;
    const user = adminUsers.find(u => u.id === select.value);

    const avatar = $('#adminUserAvatarPreview');
    if (avatar) {
        if (user?.avatar_file_id) {
            avatar.innerHTML = `<img src="${buildFileUrl(user.avatar_file_id, 'avatar')}" alt="avatar">`;
        } else {
            avatar.innerHTML = '';
        }
    }

    $('#adminUsername').value = user?.username || '';
    $('#adminEmail').value = user?.email || '';
    $('#adminNewPassword').value = '';
    $('#adminAvatarFile').value = '';

    renderUserRoles(user);
}

function renderUserRoles(user) {
    const select = $('#adminUserRoles');
    if (!select) return;
    select.innerHTML = '';
    adminRoles.forEach(r => {
        const opt = document.createElement('option');
        opt.value = r.id;
        opt.textContent = r.name;
        select.appendChild(opt);
    });
    const roleIds = new Set((user?.roles || []).map(r => r.id));
    Array.from(select.options).forEach(o => {
        o.selected = roleIds.has(o.value);
    });
}

function renderAdminRoles() {
    const body = $('#adminRoleList');
    if (!body) return;
    body.innerHTML = '';
    adminRoles.forEach(r => {
        const tr = document.createElement('tr');
        tr.innerHTML = `
            <td style="padding: 8px; font-family: monospace;">${r.id}</td>
            <td style="padding: 8px;">${r.name}</td>
            <td style="padding: 8px; text-align:right;">
                <button class="iconbtn danger" data-role-id="${r.id}" title="Delete role">
                    <i class="bi bi-trash"></i>
                </button>
            </td>
        `;
        body.appendChild(tr);
    });

    body.querySelectorAll('button[data-role-id]').forEach(btn => {
        btn.addEventListener('click', async () => {
            const roleId = btn.getAttribute('data-role-id');
            if (!roleId) return;
            try {
                await apiFetch(`/api/admin/roles/${encodeURIComponent(roleId)}`, { method: 'DELETE' });
                await loadAdminData();
            } catch (e) {
                setStatus(e.message, true);
            }
        });
    });
}

async function saveUserInfo() {
    const userId = $('#adminUserSelect').value;
    if (!userId) return;
    const username = $('#adminUsername').value.trim();
    const email = $('#adminEmail').value.trim();
    try {
        await apiFetch(`/api/admin/users/${encodeURIComponent(userId)}`, {
            method: 'PATCH',
            body: JSON.stringify({
                username: username || null,
                email: email || null,
            })
        });
        await loadAdminData();
    } catch (e) {
        setStatus(e.message, true);
    }
}

async function setUserPassword() {
    const userId = $('#adminUserSelect').value;
    if (!userId) return;
    const new_password = $('#adminNewPassword').value;
    if (!new_password) return;
    try {
        await apiFetch(`/api/admin/users/${encodeURIComponent(userId)}/password`, {
            method: 'PUT',
            body: JSON.stringify({ new_password })
        });
        $('#adminNewPassword').value = '';
        setStatus('Password updated');
    } catch (e) {
        setStatus(e.message, true);
    }
}

async function uploadUserAvatar() {
    const userId = $('#adminUserSelect').value;
    const file = $('#adminAvatarFile').files?.[0];
    if (!userId || !file) return;
    const fd = new FormData();
    fd.append('file', file);
    const res = await fetch(store.baseUrl + `/api/admin/users/${encodeURIComponent(userId)}/avatar`, {
        method: 'PUT',
        headers: store.accessToken ? { 'Authorization': 'Bearer ' + store.accessToken } : {},
        body: fd
    });
    if (!res.ok) {
        let msg = 'Upload failed';
        try { const d = await res.json(); if (d.error) msg = d.error; } catch { }
        setStatus(msg, true);
        return;
    }
    await loadAdminData();
}

async function updateUserRoles() {
    const userId = $('#adminUserSelect').value;
    if (!userId) return;
    const roleIds = Array.from($('#adminUserRoles').selectedOptions).map(o => o.value);
    try {
        await apiFetch(`/api/admin/users/${encodeURIComponent(userId)}/roles`, {
            method: 'PUT',
            body: JSON.stringify({ role_ids: roleIds })
        });
        await loadAdminData();
    } catch (e) {
        setStatus(e.message, true);
    }
}

async function createRole() {
    const name = $('#adminRoleName').value.trim();
    if (!name) return;
    try {
        await apiFetch('/api/admin/roles', {
            method: 'POST',
            body: JSON.stringify({ name })
        });
        $('#adminRoleName').value = '';
        await loadAdminData();
    } catch (e) {
        setStatus(e.message, true);
    }
}

export function bindAdminEvents() {
    const btn = $('#btnOpenAdmin');
    const modal = $('#adminModal');
    if (!btn || !modal) return;

    btn.addEventListener('click', openAdminModal);
    $('#btnCloseAdmin').addEventListener('click', closeAdminModal);
    modal.addEventListener('click', (e) => {
        if (e.target === modal || e.target === $('#adminModal .modal-backdrop')) closeAdminModal();
    });

    $('#btnAdminRefresh').addEventListener('click', loadAdminData);
    $('#adminUserSelect').addEventListener('change', renderSelectedUser);

    $('#btnAdminSaveUser').addEventListener('click', saveUserInfo);
    $('#btnAdminSetPassword').addEventListener('click', setUserPassword);
    $('#btnAdminUploadAvatar').addEventListener('click', uploadUserAvatar);
    $('#btnAdminSaveRoles').addEventListener('click', updateUserRoles);

    $('#btnAdminCreateRole').addEventListener('click', createRole);

    refreshAdminVisibility();
    if (adminLoaded) renderAdminUsers();
}
