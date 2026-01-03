import { store } from './modules/store.js';
import { $ } from './modules/utils.js';
import { setupApi, checkServer } from './modules/api.js';
import {
    doLogin, doRegister, logout, showServerStep, showAuthStep, setBaseUrl
} from './modules/auth.js';
import { connectWs, sendTyping } from './modules/socket.js';
import {
    openCreateChannelModal, closeCreateChannelModal, createChannelAdvanced,
    openEditChannelModal, closeEditChannelModal, saveEditChannel, confirmDeleteChannel,
    modifyMembersInModal, loadChannels, selectChannel
} from './modules/channels.js';
import {
    sendMessage, enableComposer
} from './modules/messages.js';
import {
    startCall, leaveCall
} from './modules/voice.js';
import {
    bindSettingsEvents, applyTheme
} from './modules/settings.js';
import {
    openInviteModal, closeInviteModal, createInvite
} from './modules/invites.js';
import { heartbeat } from './modules/presence.js';

// Dependency injection to break cycles
setupApi(logout, connectWs);

// Expose for HTML (onclick handlers if any remain, but we try to replace them)
// Actually showServerStep was called by onclick in HTML: onclick="showServerStep()"
window.showServerStep = showServerStep;

async function init() {
    bindUI();

    // Apply saved theme
    applyTheme(store.theme);

    const storedUrl = localStorage.getItem('stuffchat.base_url');
    if (storedUrl) {
        store.baseUrl = storedUrl;
        const cfg = $('#cfgBaseUrl');
        const bUrl = $('#baseUrl');
        const sUrl = $('#settingsBaseUrl');
        if (cfg) cfg.value = storedUrl;
        if (bUrl) bUrl.value = storedUrl;
        if (sUrl) sUrl.value = storedUrl;

        try {
            await checkServer(storedUrl);
            showAuthStep();
            if (store.accessToken) {
                // We rely on auth.js methods which are not exported as a single bootstrap function
                // but bootstrapAfterAuth is.
                // Wait, auth.js exports bootstrapAfterAuth? Yes.
                // But we need to check validity? 
                // In frontend.js: if (store.accessToken) await bootstrapAfterAuth()
                // But we need to import bootstrapAfterAuth.
                // It was internal in frontend.js but I exported it in auth.js.
                // Let's import it.
                await import('./modules/auth.js').then(m => m.bootstrapAfterAuth());
            }
        } catch {
            showServerStep();
        }
    } else {
        const defaultUrl = `${window.location.protocol}//${window.location.hostname}:22800`;
        const cfg = $('#cfgBaseUrl');
        if (cfg) cfg.value = defaultUrl;
        showServerStep();
    }
}

function bindUI() {
    $('#btnCheckServer').addEventListener('click', () => setBaseUrl($('#cfgBaseUrl').value));
    if ($('#baseUrl')) {
        $('#baseUrl').addEventListener('change', e => setBaseUrl(e.target.value));
    }

    $('#btnLogin').addEventListener('click', async () => {
        $('#loginErr').textContent = '';
        try { await doLogin($('#loginUser').value, $('#loginPass').value); }
        catch (e) { $('#loginErr').textContent = e.message; }
    });
    $('#btnRegister').addEventListener('click', async () => {
        $('#regErr').textContent = '';
        await doRegister($('#regUser').value, $('#regEmail').value, $('#regPass').value);
    });

    if ($('#btnLogout')) $('#btnLogout').addEventListener('click', () => logout());

    // Channels
    $('#btnOpenCreateChannel').addEventListener('click', openCreateChannelModal);
    $('#btnCloseCreateChannel').addEventListener('click', closeCreateChannelModal);
    $('#createChannelModal').addEventListener('click', (e) => {
        if (e.target === $('#createChannelModal') || e.target === $('#createChannelModal .modal-backdrop')) closeCreateChannelModal();
    });

    $('#chIsPrivate').addEventListener('change', (e) => {
        const on = e.target.checked;
        $('#channelMembersSection').style.display = on ? '' : 'none';
    });

    $('#btnCreateChannelSubmit').onclick = async () => {
        const name = $('#chName').value;
        const is_voice = $('#chIsVoice').checked;
        const is_private = $('#chIsPrivate').checked;
        const members = Array.from($('#channelMembersSelect').selectedOptions).map(o => o.value);
        if (!name) return;
        try {
            const ch = await createChannelAdvanced({ name, is_private, is_voice, members });
            closeCreateChannelModal();
            // selectChannel(ch.id); // createChannelAdvanced calls this
        } catch (e) { alert(e.message); }
    };

    $('#btnCloseEditChannel').onclick = closeEditChannelModal;
    $('#btnEditChannelSave').onclick = saveEditChannel;
    $('#btnEditChannelDelete').onclick = confirmDeleteChannel;
    $('#btnAddMembers').onclick = () => modifyMembersInModal('add');
    $('#btnRemoveMembers').onclick = () => modifyMembersInModal('remove');
    $('#editChannelModal').addEventListener('click', (e) => {
        if (e.target === $('#editChannelModal') || e.target === $('#editChannelModal .modal-backdrop')) closeEditChannelModal();
    });

    // Invites
    $('#btnOpenInvites').onclick = openInviteModal;
    $('#btnCloseInvites').onclick = closeInviteModal;
    $('#btnCreateInvite').onclick = createInvite;

    // Messages
    $('#btnSend').addEventListener('click', sendMessage);
    $('#msgInput').addEventListener('keydown', e => {
        if (e.key === 'Enter' && !e.shiftKey) {
            e.preventDefault();
            sendMessage();
            sendTyping(false);
        } else {
            sendTyping(true);
        }
    });
    let typingDeb;
    $('#msgInput').addEventListener('input', () => {
        if (typingDeb) clearTimeout(typingDeb);
        typingDeb = setTimeout(() => sendTyping(false), 1000);
    });

    $('#attachFile').addEventListener('change', () => {
        if ($('#attachFile').files && $('#attachFile').files[0]) {
            const name = $('#attachFile').files[0].name;
            $('#msgInput').placeholder = 'Attached: ' + name;
        } else {
            $('#msgInput').placeholder = 'Write a message…';
        }
    });

    // Sidebar
    $('#btnToggleSidebar').addEventListener('click', () => {
        $('#sidebar').classList.toggle('open');
    });
    $('#btnCloseSidebar').addEventListener('click', () => {
        $('#sidebar').classList.remove('open');
    });

    $('#presenceSelect').addEventListener('change', heartbeat);

    // Settings
    bindSettingsEvents();

    window.addEventListener('beforeunload', () => {
        if (store.ws) try { store.ws.close(); } catch { }
    });

    // WebRTC
    $('#btnStartCall').addEventListener('click', startCall);
    $('#btnLeaveCall').addEventListener('click', leaveCall);

    document.addEventListener('visibilitychange', () => {
        if (!document.hidden) {
            // Reconnect if needed
            connectWs();

            if (store.currentChannelId) {
                const msgs = store.messages.get(store.currentChannelId);
                if (msgs && msgs.length) {
                    // Determine latest
                    const last = msgs[msgs.length - 1]; // sorted ascending
                    import('./modules/channels.js').then(m => m.markChannelRead(store.currentChannelId, last));
                }
            }
        }
    });

    if ('Notification' in window && Notification.permission === 'default') {
        // Request on interaction
        document.body.addEventListener('click', () => {
            if (Notification.permission === 'default') Notification.requestPermission();
        }, { once: true });
    }
}

init();
