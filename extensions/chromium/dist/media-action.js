"use strict";
const runtime = typeof browser !== 'undefined' ? browser.runtime : chrome.runtime;
const buttonId = 'grabbit-media-action';
const cleanupKey = '__grabbitMediaActionCleanup';
const minimumWidth = 240;
const minimumHeight = 135;
const scanLimit = 25;
const observationDelayMs = 150;
let player = null;
let previousPosition = '';
let button = null;
let observationTimer;
let resizeObserver = null;
function isEligiblePage() {
    return location.protocol === 'http:' || location.protocol === 'https:';
}
function videoArea(video) {
    const rect = video.getBoundingClientRect();
    return rect.width * rect.height;
}
function isSupportedVideo(video) {
    if (!video.isConnected || video.mediaKeys)
        return false;
    const rect = video.getBoundingClientRect();
    return rect.width >= minimumWidth && rect.height >= minimumHeight;
}
function findVideo() {
    return Array.from(document.querySelectorAll('video'))
        .slice(0, scanLimit)
        .filter(isSupportedVideo)
        .sort((left, right) => videoArea(right) - videoArea(left))[0] ?? null;
}
function playerForVideo(video) {
    const youtubePlayer = video.closest('#movie_player, .html5-video-player, ytd-player');
    return youtubePlayer ?? video.parentElement ?? video;
}
function isYouTubeMediaRoute() {
    const mediaPath = location.pathname === '/watch'
        || location.pathname.startsWith('/shorts/')
        || location.pathname.startsWith('/live/');
    return mediaPath && Boolean(document.querySelector('ytd-app, ytd-player, #movie_player'));
}
function findYouTubePlayer() {
    if (!isYouTubeMediaRoute())
        return null;
    const videos = Array.from(document.querySelectorAll('video')).slice(0, scanLimit);
    if (videos.some((video) => video.mediaKeys))
        return null;
    return Array.from(document.querySelectorAll('#movie_player, ytd-player .html5-video-player, ytd-player'))
        .slice(0, scanLimit)
        .filter((candidate) => {
        const rect = candidate.getBoundingClientRect();
        return candidate.isConnected && rect.width >= minimumWidth && rect.height >= minimumHeight;
    })
        .sort((left, right) => {
        const leftRect = left.getBoundingClientRect();
        const rightRect = right.getBoundingClientRect();
        return rightRect.width * rightRect.height - leftRect.width * leftRect.height;
    })[0] ?? null;
}
function findTargetPlayer() {
    const video = findVideo();
    return video ? playerForVideo(video) : findYouTubePlayer();
}
function restorePlayer() {
    resizeObserver?.disconnect();
    resizeObserver = null;
    if (player?.isConnected)
        player.style.position = previousPosition;
    player = null;
    previousPosition = '';
}
function removeButton() {
    button?.remove();
    button = null;
    restorePlayer();
}
function createButton() {
    const action = document.createElement('button');
    action.id = buttonId;
    action.type = 'button';
    action.title = 'Download with GrabBit';
    action.setAttribute('aria-label', 'Download with GrabBit');
    const icon = document.createElement('img');
    icon.src = runtime.getURL('media-download-button.png');
    icon.alt = '';
    Object.assign(icon.style, { width: '36px', height: '36px', display: 'block', flex: '0 0 auto' });
    const label = document.createElement('span');
    label.textContent = 'GrabBit Download';
    Object.assign(label.style, {
        maxWidth: '0', marginLeft: '0', overflow: 'hidden', opacity: '0', whiteSpace: 'nowrap',
        color: '#fff', font: '700 13px system-ui, sans-serif', transition: 'max-width 140ms ease, opacity 120ms ease, margin 140ms ease',
    });
    action.append(icon, label);
    Object.assign(action.style, {
        position: 'absolute', right: '12px', bottom: '54px', zIndex: '2147483647',
        display: 'flex', alignItems: 'center', width: '44px', height: '44px', padding: '4px', overflow: 'hidden',
        border: '1px solid rgb(52 211 153 / 75%)', borderRadius: '12px', background: '#033c2d',
        boxShadow: '0 4px 16px rgb(0 0 0 / 48%)', cursor: 'pointer', opacity: '0.94',
        transition: 'width 140ms ease, opacity 120ms ease, transform 120ms ease, box-shadow 120ms ease',
    });
    const expand = () => {
        action.style.width = '164px';
        action.style.opacity = '1';
        action.style.transform = 'translateY(-1px)';
        action.style.boxShadow = '0 6px 20px rgb(0 0 0 / 55%), 0 0 0 2px rgb(52 211 153 / 35%)';
        label.style.maxWidth = '112px';
        label.style.marginLeft = '7px';
        label.style.opacity = '1';
    };
    const collapse = () => {
        action.style.width = '44px';
        action.style.opacity = '0.9';
        action.style.transform = '';
        action.style.boxShadow = '0 4px 16px rgb(0 0 0 / 48%)';
        label.style.maxWidth = '0';
        label.style.marginLeft = '0';
        label.style.opacity = '0';
    };
    action.addEventListener('mouseenter', expand);
    action.addEventListener('mouseleave', () => { if (document.activeElement !== action)
        collapse(); });
    action.addEventListener('focus', () => {
        action.style.outline = '3px solid rgb(110 231 183 / 80%)';
        action.style.outlineOffset = '2px';
        expand();
    });
    action.addEventListener('blur', () => {
        action.style.outline = '';
        action.style.outlineOffset = '';
        collapse();
    });
    action.addEventListener('click', (event) => {
        event.preventDefault();
        event.stopPropagation();
        void runtime.sendMessage({ type: 'GRABBIT_INSPECT_PAGE_MEDIA', url: location.href }).catch(() => undefined);
    });
    return action;
}
function positionButton(action, target) {
    action.style.bottom = target.getBoundingClientRect().height < 240 ? '42px' : '54px';
}
function reconcile() {
    if (!isEligiblePage()) {
        removeButton();
        return;
    }
    const nextPlayer = findTargetPlayer();
    if (!nextPlayer) {
        removeButton();
        return;
    }
    if (button?.isConnected && player === nextPlayer) {
        positionButton(button, nextPlayer);
        return;
    }
    removeButton();
    player = nextPlayer;
    previousPosition = player.style.position;
    if (getComputedStyle(player).position === 'static')
        player.style.position = 'relative';
    button = createButton();
    positionButton(button, player);
    player.append(button);
    if (typeof ResizeObserver !== 'undefined') {
        resizeObserver = new ResizeObserver(() => {
            if (button && player)
                positionButton(button, player);
        });
        resizeObserver.observe(player);
    }
}
function scheduleReconcile() {
    globalThis.clearTimeout(observationTimer);
    observationTimer = globalThis.setTimeout(reconcile, observationDelayMs);
}
const existingCleanup = window[cleanupKey];
existingCleanup?.();
reconcile();
const observer = new MutationObserver(scheduleReconcile);
observer.observe(document.documentElement, { childList: true, subtree: true });
window.addEventListener('resize', scheduleReconcile);
window.addEventListener('popstate', scheduleReconcile);
window.addEventListener('hashchange', scheduleReconcile);
window.addEventListener('yt-navigate-finish', scheduleReconcile);
window.addEventListener('yt-page-data-updated', scheduleReconcile);
window[cleanupKey] = () => {
    observer.disconnect();
    globalThis.clearTimeout(observationTimer);
    window.removeEventListener('resize', scheduleReconcile);
    window.removeEventListener('popstate', scheduleReconcile);
    window.removeEventListener('hashchange', scheduleReconcile);
    window.removeEventListener('yt-navigate-finish', scheduleReconcile);
    window.removeEventListener('yt-page-data-updated', scheduleReconcile);
    removeButton();
};
