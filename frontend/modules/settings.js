import { store } from './store.js';
import { $, setIf, buildFileUrl } from './utils.js';
import { updateMe, changeMyPassword, uploadAvatar } from './users.js';
import { logout, setBaseUrl } from './auth.js';
import { initCloudsTheme } from './clouds.js';

export function openSettings() {
    // Fill current values
    setIf('#profileUsername', 'value', store.user?.username || '');
    setIf('#settingsBaseUrl', 'value', store.baseUrl || '');

    // Avatar preview in modal
    const prev = $('#settingsAvatarPreview');
    if (prev) {
        if (store.user?.avatar_file_id) {
            prev.innerHTML = `<img src="${buildFileUrl(store.user.avatar_file_id, 'avatar')}" alt="avatar">`;
        } else {
            prev.innerHTML = '';
        }
    }

    // Theme selection
    const radios = document.querySelectorAll('input[name="themeSel"]');
    radios.forEach(r => { r.checked = (r.value === store.theme); });

    // Audio preferences
    setIf('#prefNoiseSuppression', 'checked', store.noiseSuppression);
    setIf('#prefEchoCancellation', 'checked', store.echoCancellation);
    setIf('#prefAutoGainControl', 'checked', store.autoGainControl);

    $('#settingsModal').classList.remove('hidden');
}

export function closeSettings() {
    $('#settingsModal').classList.add('hidden');
}

export function applyTheme(theme) {
    store.theme = theme || 'mysterious';
    document.body.setAttribute('data-theme', store.theme === 'mysterious' ? '' : store.theme);
    localStorage.setItem('stuffchat.theme', store.theme);
    if (store.theme === 'mysterious') document.body.removeAttribute('data-theme');

    // Handle clouds theme animation
    initCloudsTheme(store.theme);
}

export function bindSettingsEvents() {
    // Avatar upload: wired in modal
    const modalAvatar = $('#setAvatarFile');
    if (modalAvatar) {
        modalAvatar.addEventListener('change', async (e) => {
            const f = e.target.files && e.target.files[0];
            if (!f) return;
            try {
                await uploadAvatar(f);
                // Reflect in modal preview too
                const prev = $('#settingsAvatarPreview');
                if (store.user?.avatar_file_id && prev) {
                    prev.innerHTML = `<img src="${buildFileUrl(store.user.avatar_file_id, 'avatar')}" alt="avatar">`;
                } else if (prev) {
                    prev.innerHTML = '';
                }
            } catch (err) { alert(err.message); }
            e.target.value = '';
        });
    }

    // Settings modal open/close
    $('#btnOpenSettings').addEventListener('click', openSettings);
    $('#btnCloseSettings').addEventListener('click', closeSettings);
    $('#settingsModal').addEventListener('click', (e) => {
        if (e.target === $('#settingsModal') || e.target === $('.modal-backdrop')) closeSettings();
    });
    window.addEventListener('keydown', (e) => {
        if (e.key === 'Escape' && !$('#settingsModal').classList.contains('hidden')) closeSettings();
    });

    // Profile save
    $('#btnSaveProfile').addEventListener('click', async () => {
        const name = $('#profileUsername').value.trim();
        try {
            await updateMe({ username: name || null });
            alert('Profile updated.');
        } catch (e) { alert('Update failed: ' + e.message); }
    });

    // Password change
    $('#btnChangePassword').addEventListener('click', async () => {
        const cur = $('#curPwd').value, nw = $('#newPwd').value;
        if (!cur || !nw) return alert('Enter current and new password.');
        try {
            await changeMyPassword(cur, nw);
            $('#curPwd').value = ''; $('#newPwd').value = '';
            alert('Password changed.');
        } catch (e) { alert('Change failed: ' + e.message); }
    });

    // Theme select
    document.querySelectorAll('input[name="themeSel"]').forEach(r => {
        r.addEventListener('change', () => applyTheme(r.value));
    });

    // Audio preferences
    $('#prefNoiseSuppression').addEventListener('change', (e) => {
        store.noiseSuppression = e.target.checked;
        localStorage.setItem('stuffchat.noise_suppression', store.noiseSuppression);
    });
    $('#prefEchoCancellation').addEventListener('change', (e) => {
        store.echoCancellation = e.target.checked;
        localStorage.setItem('stuffchat.echo_cancellation', store.echoCancellation);
    });
    $('#prefAutoGainControl').addEventListener('change', (e) => {
        store.autoGainControl = e.target.checked;
        localStorage.setItem('stuffchat.auto_gain_control', store.autoGainControl);
    });

    // Logout from modal
    $('#btnLogoutSettings').addEventListener('click', () => logout());

    // We bind Base URL save in auth.js or main.js?
    // It calls setBaseUrl which is in auth.js. import setBaseUrl from auth.js?
    // Cycle: auth -> settings (logout doesn't depend on settings, but bindSettingsEvents depends on logout).
    // settings -> auth (bind depends on logout).
    // If auth imports settings, that's bad.
    // Auth doesn't need settings.
    // But main.js will call bindSettingsEvents.
    // setBaseUrl is in... auth.js.
    // So settings.js needs setBaseUrl.
    // settings -> auth.
    // Good.
}
