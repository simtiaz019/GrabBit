import { api, capture, hostname, native, policy, reportDiagnostics, safeError } from './native-client.js';
import { hostExcluded, isEligibleUrl } from './policy.js';
const pendingKey = (id) => `pendingCapture:${id}`;
const handledKey = (id) => `handledCapture:${id}`;
function eligibleItem(item) {
    return !item.byExtensionId && !item.incognito && isEligibleUrl(item.finalUrl || item.url);
}
async function refreshed(item) {
    await new Promise((resolve) => globalThis.setTimeout(resolve, 250));
    const [current] = await api.downloads.search({ id: item.id });
    return current ?? item;
}
async function cancelBrowserDownload(item) {
    const [current] = await api.downloads.search({ id: item.id });
    if (!current || current.state === 'complete')
        return 'browser_completed';
    try {
        await api.downloads.cancel(item.id);
        await api.downloads.erase({ id: item.id });
        return 'cancelled';
    }
    catch {
        return 'failed';
    }
}
async function automaticCapture(item, idempotencyKey) {
    const response = await capture(item.finalUrl || item.url, item.filename, item.referrer, true, idempotencyKey);
    if (!response.ok || !response.jobId) {
        await reportDiagnostics({ decision: 'browser_continues', jobAcceptance: response.code || 'rejected', lastError: response.message });
        return;
    }
    const cancellation = await cancelBrowserDownload(item);
    await api.storage.local.set({ [handledKey(item.id)]: { idempotencyKey, accepted: true, createdAt: Date.now() } });
    await reportDiagnostics({
        decision: cancellation === 'cancelled' ? 'captured' : 'duplicate_risk',
        jobAcceptance: response.code,
        cancellationResult: cancellation,
        lastError: cancellation === 'cancelled' ? null : 'Chrome could not be cancelled after GrabBit accepted the job.',
    });
}
async function askEveryTime(item, idempotencyKey) {
    const pending = {
        downloadId: item.id,
        url: item.finalUrl || item.url,
        filename: item.filename,
        sourcePage: item.referrer,
        idempotencyKey,
        createdAt: Date.now(),
    };
    await api.storage.local.set({
        [pendingKey(item.id)]: pending,
        [handledKey(item.id)]: { idempotencyKey, awaitingUser: true, createdAt: Date.now() },
    });
    await api.windows.create({
        url: api.runtime.getURL(`confirm.html?id=${item.id}`),
        type: 'popup',
        width: 440,
        height: 420,
    });
    await reportDiagnostics({ decision: 'awaiting_user', jobAcceptance: 'not_attempted', cancellationResult: 'not_attempted' });
}
async function handleCreated(initial) {
    const eventAt = Date.now();
    const item = await refreshed(initial);
    const url = item.finalUrl || item.url;
    const permission = await api.permissions.contains({ permissions: ['downloads'] });
    await reportDiagnostics({
        downloadsPermissionGranted: permission,
        lastEventAt: eventAt,
        lastHostname: hostname(url),
        decision: 'evaluating',
        jobAcceptance: 'not_attempted',
        cancellationResult: 'not_attempted',
        lastError: null,
    });
    if (!permission || !eligibleItem(item)) {
        await reportDiagnostics({ decision: permission ? 'unsupported' : 'permission_denied' });
        return;
    }
    const current = await policy();
    await reportDiagnostics({ automaticEnabled: current.mode === 'automatic' });
    if (current.mode === 'off') {
        await reportDiagnostics({ decision: 'disabled' });
        return;
    }
    if (hostExcluded(url, current.excludedSites)) {
        await reportDiagnostics({ decision: 'excluded' });
        return;
    }
    const existing = await api.storage.local.get(handledKey(item.id));
    if (existing[handledKey(item.id)]) {
        await reportDiagnostics({ decision: 'duplicate_event' });
        return;
    }
    if (item.state === 'complete') {
        await reportDiagnostics({ decision: 'browser_completed' });
        return;
    }
    const idempotencyKey = crypto.randomUUID();
    await api.storage.local.set({ [handledKey(item.id)]: { idempotencyKey, processing: true, createdAt: Date.now() } });
    if (current.mode === 'automatic')
        await automaticCapture(item, idempotencyKey);
    else
        await askEveryTime(item, idempotencyKey);
}
// MV3 requires event listeners to be registered synchronously at module evaluation time.
api.downloads.onCreated.addListener((item) => {
    void handleCreated(item).catch(async (reason) => {
        await reportDiagnostics({ decision: 'browser_continues', lastError: safeError(reason) });
    });
});
api.runtime.onMessage.addListener((message, _sender, sendResponse) => {
    const request = message;
    if (request.type !== 'GRABBIT_INSPECT_PAGE_MEDIA' || typeof request.url !== 'string' || !isEligibleUrl(request.url))
        return;
    void native('INSPECT_MEDIA', { url: request.url, authorizationConfirmed: false })
        .then((response) => sendResponse(response))
        .catch((reason) => sendResponse({ ok: false, message: safeError(reason) }));
    return true;
});
void api.contextMenus.removeAll().then(() => {
    api.contextMenus.create({ id: 'grabbit-link', title: 'Download with GrabBit', contexts: ['link'] });
    api.contextMenus.create({ id: 'grabbit-media', title: 'Download video/audio with GrabBit', contexts: ['video', 'audio', 'page'] });
});
api.contextMenus.onClicked.addListener((info, tab) => {
    const url = info.linkUrl || info.srcUrl || info.pageUrl || tab?.url;
    if (!url || !isEligibleUrl(url))
        return;
    if (info.menuItemId === 'grabbit-media')
        void native('INSPECT_MEDIA', { url, authorizationConfirmed: false });
    else
        void capture(url, undefined, tab?.url);
});
