import { store } from './store.js';
import { apiFetch } from './api.js';
import { $, el, localizeDate } from './utils.js';
import { selectChannel } from './channels.js';

let searchDebounce = null;
let loadingMore = false;

function getTimezoneOffsetString() {
    const minutesEast = -new Date().getTimezoneOffset();
    const sign = minutesEast >= 0 ? '+' : '-';
    const abs = Math.abs(minutesEast);
    const hh = String(Math.floor(abs / 60)).padStart(2, '0');
    const mm = String(abs % 60).padStart(2, '0');
    return `${sign}${hh}:${mm}`;
}

function setSearchStatus(text) {
    const n = $('#searchStatus');
    if (n) n.textContent = text;
}

function renderSearchResults() {
    const wrap = $('#searchResults');
    if (!wrap) return;
    wrap.innerHTML = '';

    if (store.search.loading && store.search.results.length === 0) {
        wrap.appendChild(el('div', { class: 'search-empty' }, 'Searching...'));
        return;
    }

    if (store.search.error) {
        wrap.appendChild(el('div', { class: 'search-empty' }, store.search.error));
        return;
    }

    if (!store.search.query.trim()) {
        wrap.appendChild(el('div', { class: 'search-empty' }, 'Type a query to search messages.'));
        return;
    }

    if (store.search.results.length === 0) {
        wrap.appendChild(el('div', { class: 'search-empty' }, 'No results.'));
        return;
    }

    for (const hit of store.search.results) {
        const item = el('div', {
            class: 'search-result-item',
            tabindex: '0',
            onclick: () => openResult(hit),
            onkeydown: (e) => {
                if (e.key === 'Enter') openResult(hit);
            }
        }, [
            el('div', { class: 'search-result-meta' }, [
                el('span', { class: 'search-pill' }, `#${hit.channel_name}`),
                el('span', { class: 'search-pill' }, `from:${hit.username}`),
                el('span', { class: 'search-pill' }, localizeDate(hit.created_at)),
                hit.has_attachment ? el('span', { class: 'search-pill' }, 'has:attachment') : null,
            ]),
            el('div', { class: 'search-result-preview' }, hit.content_preview || '(no text content)')
        ]);
        wrap.appendChild(item);
    }
}

async function runSearch({ append = false } = {}) {
    const input = $('#searchInput');
    if (!input) return;

    const query = input.value || '';
    store.search.query = query;

    if (!query.trim()) {
        store.search.results = [];
        store.search.nextCursor = null;
        store.search.error = '';
        store.search.loading = false;
        setSearchStatus('Modifiers: from:, before:, after:, in:, has:attachment');
        renderSearchResults();
        return;
    }

    const params = new URLSearchParams();
    params.set('q', query);
    params.set('tz', getTimezoneOffsetString());
    params.set('limit', '25');
    if (append && store.search.nextCursor) {
        params.set('cursor', store.search.nextCursor);
    }

    const reqId = ++store.search.requestSeq;
    store.search.loading = true;
    store.search.error = '';
    setSearchStatus('Searching...');
    renderSearchResults();

    try {
        const data = await apiFetch(`/api/messages/search?${params.toString()}`);
        if (reqId !== store.search.requestSeq) return;

        const incoming = Array.isArray(data?.results) ? data.results : [];
        if (append) {
            const seen = new Set(store.search.results.map(r => r.id));
            for (const r of incoming) {
                if (!seen.has(r.id)) store.search.results.push(r);
            }
        } else {
            store.search.results = incoming;
        }
        store.search.nextCursor = data?.next_cursor || null;
        store.search.loading = false;
        store.search.error = '';
        setSearchStatus(store.search.nextCursor ? 'Scroll for more results.' : 'Results loaded.');
        renderSearchResults();
    } catch (e) {
        if (reqId !== store.search.requestSeq) return;
        store.search.loading = false;
        store.search.error = e.message || 'Search failed';
        setSearchStatus('Search failed.');
        renderSearchResults();
    }
}

function scheduleSearch() {
    if (searchDebounce) clearTimeout(searchDebounce);
    searchDebounce = setTimeout(() => runSearch({ append: false }), 250);
}

function openResult(hit) {
    closeSearchModal();
    store.pendingFocusMessageId = hit.id;
    selectChannel(hit.channel_id, { focusMessageId: hit.id });
}

export function openSearchModal() {
    const modal = $('#searchModal');
    if (!modal) return;
    modal.classList.remove('hidden');
    const input = $('#searchInput');
    if (input) {
        input.focus();
        input.select();
    }
    renderSearchResults();
}

export function closeSearchModal() {
    const modal = $('#searchModal');
    if (!modal) return;
    modal.classList.add('hidden');
}

export function initSearch() {
    const btn = $('#btnOpenSearch');
    const closeBtn = $('#btnCloseSearch');
    const modal = $('#searchModal');
    const input = $('#searchInput');
    const results = $('#searchResults');
    if (!btn || !closeBtn || !modal || !input || !results) return;

    btn.addEventListener('click', openSearchModal);
    closeBtn.addEventListener('click', closeSearchModal);
    modal.addEventListener('click', (e) => {
        if (e.target === modal || e.target === $('#searchModal .modal-backdrop')) closeSearchModal();
    });

    input.addEventListener('input', scheduleSearch);
    input.addEventListener('keydown', (e) => {
        if (e.key === 'Enter') {
            e.preventDefault();
            runSearch({ append: false });
        } else if (e.key === 'Escape') {
            closeSearchModal();
        }
    });

    results.addEventListener('scroll', async () => {
        if (loadingMore || store.search.loading || !store.search.nextCursor) return;
        const nearBottom = results.scrollHeight - results.scrollTop - results.clientHeight < 120;
        if (!nearBottom) return;
        loadingMore = true;
        try {
            await runSearch({ append: true });
        } finally {
            loadingMore = false;
        }
    });

    document.addEventListener('keydown', (e) => {
        if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
            e.preventDefault();
            openSearchModal();
            return;
        }
        if (e.key === 'Escape' && !modal.classList.contains('hidden')) {
            closeSearchModal();
        }
    });

    renderSearchResults();
}
