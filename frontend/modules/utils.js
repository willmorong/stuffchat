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
    const file = type === 'join' ? 'audio/stuffchat_join_v3.wav' : 'audio/stuffchat_leave_v3.wav';
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
