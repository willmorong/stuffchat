import { fetchEmojis } from './emojis.js';
import { store } from './store.js';

export const $ = sel => document.querySelector(sel);

export const el = (tag, attrs = {}, children = []) => {
    const e = document.createElement(tag);
    Object.entries(attrs).forEach(([k, v]) => {
        if (k === 'class') e.className = v;
        else if (k === 'style') e.style.cssText = v;
        else if (k.startsWith('on') && typeof v === 'function') e.addEventListener(k.slice(2), v);
        else if (v !== undefined && v !== null) e.setAttribute(k, v);
    });
    (Array.isArray(children) ? children : [children]).forEach(c => {
        if (c === null || c === undefined) return;
        if (typeof c === 'string') e.appendChild(document.createTextNode(c));
        else e.appendChild(c);
    });
    return e;
};

export const truncateId = id => id ? id.slice(0, 8) : 'unknown';

export const sleep = ms => new Promise(r => setTimeout(r, ms));

export const toWsUrl = (httpUrl) => {
    if (!httpUrl) return '';
    try {
        const u = new URL(httpUrl);
        u.protocol = (u.protocol === 'https:') ? 'wss:' : 'ws:';
        u.pathname = '/ws';
        u.search = '';
        return u.toString();
    } catch { return ''; }
};

export const presenceClass = status => ({
    online: 'presence-online',
    away: 'presence-away',
    dnd: 'presence-dnd',
    invisible: 'presence-invisible',
    offline: 'presence-offline'
}[status] || 'presence-offline');

export function playNotificationSound(type) {
    let file = 'audio/stuffchat_join_v3.wav';
    if (type === 'leave') file = 'audio/stuffchat_leave_v3.wav';
    if (type === 'message') file = 'audio/stuffchat_message.wav';

    const audio = new Audio(file);
    audio.volume = 0.25;
    audio.play().catch(e => console.warn('Audio playback failed', e));
}

// Build absolute file URL supporting new endpoint (/files/{id}/{filename})
// If only an ID is available, we add a dummy filename segment ("file") since the backend ignores it.
export const buildFileUrl = (id, filename = 'file') => {
    if (!id) return '';
    const safe = encodeURIComponent(filename || 'file');
    return `${store.baseUrl}/files/${encodeURIComponent(id)}/${safe}`;
};

// Normalize server-provided file_url into an absolute URL
export const absFileUrl = (file_url) => {
    if (!file_url) return '';
    try {
        // Absolute already?
        const u = new URL(file_url, store.baseUrl);
        return u.toString();
    } catch {
        return (store.baseUrl || '') + file_url;
    }
};

export const setIf = (sel, prop, val) => { const n = $(sel); if (n) n[prop] = val; };
export const textIf = (sel, val) => { const n = $(sel); if (n) n.textContent = val; };

/**
 * Convert URLs in text into an array of strings and anchor elements.
 * Suitable for passing to el() as children.
 */
export const linkifyText = (text) => {
    if (!text) return [];
    const urlRegex = /(https?:\/\/[^\s<>"{}|\\^`[\]]+)/g;
    const parts = [];
    let lastIndex = 0;
    let match;
    while ((match = urlRegex.exec(text)) !== null) {
        if (match.index > lastIndex) {
            parts.push(text.slice(lastIndex, match.index));
        }
        parts.push(el('a', { href: match[0], target: '_blank', rel: 'noopener noreferrer' }, match[0]));
        lastIndex = match.index + match[0].length;
    }
    if (lastIndex < text.length) {
        parts.push(text.slice(lastIndex));
    }
    return parts.length ? parts : [text];
};

/**
 * Handle both emojis and links in text.
 */
export const replaceEmojisAndLinkify = (text) => {
    if (!text) return [];
    const linkified = linkifyText(text);
    const finalParts = [];

    linkified.forEach(part => {
        if (typeof part !== 'string') {
            finalParts.push(part);
            return;
        }

        const emojiRegex = /:([a-z0-9_-]+):/g;
        let lastIndex = 0;
        let match;
        while ((match = emojiRegex.exec(part)) !== null) {
            fetchEmojis();
            const emojiName = match[1];
            if (store.customEmojis.has(emojiName)) {
                if (match.index > lastIndex) {
                    finalParts.push(part.slice(lastIndex, match.index));
                }
                const url = `${store.baseUrl}/emojis/${encodeURIComponent(emojiName)}/image`;
                finalParts.push(el('img', { src: url, class: 'emoji-inline', alt: `:${emojiName}:`, title: `:${emojiName}:` }));
                lastIndex = match.index + match[0].length;
            }
        }
        if (lastIndex < part.length) {
            finalParts.push(part.slice(lastIndex));
        }
    });

    return finalParts;
};

/**
 * Check if a message consists only of emojis (custom or native) and whitespace.
 */
export const isEmojiOnly = (text) => {
    if (!text) return false;

    // Remove custom emoji patterns
    let remaining = text.replace(/:([a-z0-9_-]+):/g, (match, name) => {
        return store.customEmojis.has(name) ? ' ' : match;
    });

    // Remove all whitespace
    remaining = remaining.replace(/\s+/g, '');

    if (remaining.length === 0) return true;

    // Check if what's left is only native emojis
    // This is a broad regex for common emojis
    const nativeEmojiRegex = /^(\u2714\uFE0F|\u2714|\u2122\uFE0F|\u2122|[\u203C-\u3299]|[\uD83C-\uD83E][\uDC00-\uDFFF])+$/;
    return nativeEmojiRegex.test(remaining);
};

export const localizeDate = (dateInput) => {
    const d = new Date(dateInput);
    if (isNaN(d.getTime())) return '';
    const now = new Date();
    const diffMs = now - d;
    const diffSec = Math.floor(diffMs / 1000);
    const diffMin = Math.floor(diffSec / 60);
    const diffHrs = Math.floor(diffMin / 60);

    if (diffHrs == -1) return 'Just now';

    if (diffHrs < 24 && diffHrs >= 0) {
        if (diffMin < 1) return 'Just now';
        if (diffMin < 60) return `${diffMin} minute${diffMin === 1 ? '' : 's'} ago`;
        return `${diffHrs} hour${diffHrs === 1 ? '' : 's'} ago`;
    }
    return d.toLocaleString();
};
