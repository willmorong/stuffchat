import { store } from './store.js';
import { apiFetch } from './api.js';
import { $, el } from './utils.js';

/**
 * Fetch all custom emojis and update the store
 */
export async function fetchEmojis() {
    try {
        const emojis = await apiFetch('/api/emojis');
        store.customEmojis.clear();
        emojis.forEach(e => {
            store.customEmojis.set(e.name, e);
        });
    } catch (e) {
        console.error('Failed to fetch emojis:', e);
    }
}

/**
 * Upload a new emoji
 */
export async function uploadEmoji(name, file) {
    const fd = new FormData();
    fd.append('name', name);
    fd.append('file', file);

    const res = await fetch(store.baseUrl + '/api/emojis', {
        method: 'POST',
        headers: store.accessToken ? { 'Authorization': 'Bearer ' + store.accessToken } : {},
        body: fd
    });

    if (!res.ok) {
        let msg = 'Emoji upload failed';
        try { const d = await res.json(); if (d.error) msg = d.error; } catch { }
        throw new Error(msg);
    }

    await fetchEmojis();
}

/**
 * Delete an emoji
 */
export async function deleteEmoji(name) {
    await apiFetch(`/api/emojis/${encodeURIComponent(name)}`, { method: 'DELETE' });
    await fetchEmojis();
}

/**
 * Build URL for an emoji image
 */
export function buildEmojiUrl(name) {
    return `${store.baseUrl}/emojis/${encodeURIComponent(name)}/image`;
}

/**
 * Render the emoji picker content
 */
export function renderEmojiPicker() {
    const picker = $('#emojiPicker');
    if (!picker) return;
    picker.innerHTML = '';

    if (store.customEmojis.size === 0) {
        picker.appendChild(el('div', { style: 'grid-column: 1/-1; padding: 12px; color: var(--text-dim); text-align: center;' }, 'No custom emojis yet.'));
        return;
    }

    store.customEmojis.forEach((emoji, name) => {
        const item = el('div', {
            class: 'picker-item',
            title: `:${name}:`,
            onclick: (e) => {
                e.stopPropagation();
                insertEmojiAtCursor(name);
                picker.classList.add('hidden');
            }
        }, [
            el('img', { src: buildEmojiUrl(name), alt: name })
        ]);
        picker.appendChild(item);
    });
}

function insertEmojiAtCursor(name) {
    const input = $('#msgInput');
    if (!input) return;

    const emojiText = `:${name}: `;
    const start = input.selectionStart;
    const end = input.selectionEnd;
    const text = input.value;

    input.value = text.slice(0, start) + emojiText + text.slice(end);
    input.selectionStart = input.selectionEnd = start + emojiText.length;
    input.focus();
}

/**
 * Initialize emoji picker events
 */
export function initEmojiPicker() {
    const btn = $('#btnEmojiPicker');
    const picker = $('#emojiPicker');
    if (!btn || !picker) return;

    btn.addEventListener('click', (e) => {
        e.stopPropagation();
        const isHidden = picker.classList.contains('hidden');
        if (isHidden) {
            renderEmojiPicker();
            picker.classList.remove('hidden');
        } else {
            picker.classList.add('hidden');
        }
    });

    document.addEventListener('click', (e) => {
        if (!picker.contains(e.target) && e.target !== btn) {
            picker.classList.add('hidden');
        }
    });

    window.addEventListener('keydown', (e) => {
        if (e.key === 'Escape') picker.classList.add('hidden');
    });
}
