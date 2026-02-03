import { store } from './store.js';
import { $, el, setIf, buildFileUrl } from './utils.js';
import { updateMe, changeMyPassword, uploadAvatar } from './users.js';
import { logout, setBaseUrl } from './auth.js';
import { stopCloudsAnimation, startCloudsAnimation } from './clouds.js';
import { stopMysteriousAnimation, startMysteriousAnimation } from './mysterious.js';
import { stopRainAnimation, startRainAnimation } from './rain.js';
import { recreateCanvas } from './themeCanvas.js';
import { fetchEmojis, uploadEmoji, deleteEmoji, buildEmojiUrl } from './emojis.js';

// List of animated themes that use the background canvas
const ANIMATED_THEMES = ['clouds', 'mysterious', 'rain'];

/**
 * Stop all animated theme backgrounds
 */
function stopAllAnimatedThemes() {
    stopCloudsAnimation();
    stopMysteriousAnimation();
    stopRainAnimation();
}

/**
 * Start the appropriate animated theme
 */
function startAnimatedTheme(theme) {
    switch (theme) {
        case 'clouds':
            startCloudsAnimation();
            break;
        case 'mysterious':
            startMysteriousAnimation();
            break;
        case 'rain':
            startRainAnimation();
            break;
    }
}

/**
 * Render the list of custom emojis in the settings modal
 */
export function renderEmojiList() {
    const list = $('#customEmojiList');
    if (!list) return;
    list.innerHTML = '';

    store.customEmojis.forEach((emoji, name) => {
        const item = el('div', { class: 'emoji-item', title: `:${name}:` }, [
            el('img', { src: buildEmojiUrl(name), alt: name }),
            el('div', { class: 'emoji-name' }, name),
            el('button', {
                class: 'btn-delete-emoji',
                onclick: async () => {
                    if (confirm(`Delete emoji :${name}:?`)) {
                        try {
                            await deleteEmoji(name);
                            renderEmojiList();
                        } catch (e) { alert(e.message); }
                    }
                }
            }, el('i', { class: 'bi bi-x' }))
        ]);
        list.appendChild(item);
    });
}

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

    // Video codec preferences
    setIf('#prefVP9', 'checked', store.preferVP9);
    setIf('#prefAV1', 'checked', store.preferAV1);

    // Debug information
    setIf('#debugUserAgent', 'textContent', navigator.userAgent);

    renderEmojiList();

    $('#settingsModal').classList.remove('hidden');
}

export function closeSettings() {
    $('#settingsModal').classList.add('hidden');
}

export function applyTheme(theme) {
    store.theme = theme || 'dark';
    document.body.setAttribute('data-theme', store.theme);
    localStorage.setItem('stuffchat.theme', store.theme);

    // Stop all animated themes first
    stopAllAnimatedThemes();

    // If switching to an animated theme, recreate the canvas and start the animation
    if (ANIMATED_THEMES.includes(store.theme)) {
        // Recreate the canvas to ensure fresh context (2D vs WebGL compatibility)
        recreateCanvas();
        // Start the new animated theme
        startAnimatedTheme(store.theme);
    }
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

    // Video codec preferences
    $('#prefVP9').addEventListener('change', (e) => {
        store.preferVP9 = e.target.checked;
        localStorage.setItem('stuffchat.prefer_vp9', store.preferVP9);
    });
    $('#prefAV1').addEventListener('change', (e) => {
        store.preferAV1 = e.target.checked;
        localStorage.setItem('stuffchat.prefer_av1', store.preferAV1);
    });

    // Logout from modal
    $('#btnLogoutSettings').addEventListener('click', () => logout());

    // Emoji add
    $('#btnAddEmoji').addEventListener('click', async () => {
        const nameInput = $('#newEmojiName');
        const fileInput = $('#newEmojiFile');
        const name = nameInput.value.trim().toLowerCase();
        const file = fileInput.files && fileInput.files[0];

        if (!name) return alert('Enter emoji name.');
        if (!file) return alert('Select an image.');

        // Validate name: lowercase letters, numbers, dashes, and underscores
        if (!/^[a-z0-9_-]+$/.test(name)) {
            return alert('Name can only contain lowercase letters, numbers, dashes, and underscores.');
        }

        try {
            await uploadEmoji(name, file);
            nameInput.value = '';
            fileInput.value = '';
            renderEmojiList();
        } catch (e) { alert(e.message); }
    });
}
