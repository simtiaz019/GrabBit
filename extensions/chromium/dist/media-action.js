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
function findPlayer(video) {
    const youtubePlayer = video.closest('#movie_player, .html5-video-player, ytd-player');
    return youtubePlayer ?? video.parentElement ?? video;
}
function restorePlayer() {
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
    icon.src = runtime.getURL('icon.png');
    icon.alt = '';
    Object.assign(icon.style, { width: '20px', height: '20px', display: 'block' });
    action.append(icon);
    Object.assign(action.style, {
        position: 'absolute', right: '10px', bottom: '48px', zIndex: '2147483647',
        width: '34px', height: '34px', padding: '7px', border: '1px solid rgb(255 255 255 / 42%)',
        borderRadius: '9px', background: '#064e3b', boxShadow: '0 3px 12px rgb(0 0 0 / 38%)',
        cursor: 'pointer', opacity: '0.9', transition: 'opacity 120ms ease, transform 120ms ease',
    });
    action.addEventListener('mouseenter', () => {
        action.style.opacity = '1';
        action.style.transform = 'translateY(-1px)';
    });
    action.addEventListener('mouseleave', () => {
        action.style.opacity = '0.9';
        action.style.transform = '';
    });
    action.addEventListener('click', (event) => {
        event.preventDefault();
        event.stopPropagation();
        void runtime.sendMessage({ type: 'GRABBIT_INSPECT_PAGE_MEDIA', url: location.href }).catch(() => undefined);
    });
    return action;
}
function reconcile() {
    if (!isEligiblePage()) {
        removeButton();
        return;
    }
    const video = findVideo();
    if (!video) {
        removeButton();
        return;
    }
    const nextPlayer = findPlayer(video);
    if (button?.isConnected && player === nextPlayer)
        return;
    removeButton();
    player = nextPlayer;
    previousPosition = player.style.position;
    if (getComputedStyle(player).position === 'static')
        player.style.position = 'relative';
    button = createButton();
    player.append(button);
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
window[cleanupKey] = () => {
    observer.disconnect();
    globalThis.clearTimeout(observationTimer);
    window.removeEventListener('resize', scheduleReconcile);
    window.removeEventListener('popstate', scheduleReconcile);
    window.removeEventListener('hashchange', scheduleReconcile);
    window.removeEventListener('yt-navigate-finish', scheduleReconcile);
    removeButton();
};
