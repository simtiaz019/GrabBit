import { envelope, NATIVE_HOST } from './contracts.js';
import { isEligibleUrl, safeSuggestedFileName } from './policy.js';
export const api = typeof browser !== 'undefined' ? browser : chrome;
function browserKind() {
    const agent = navigator.userAgent;
    if (agent.includes('Firefox/'))
        return 'firefox';
    if (agent.includes('Edg/'))
        return 'edge';
    if (agent.includes('Brave/'))
        return 'brave';
    return 'chrome';
}
export async function policy() {
    const response = await native('GET_CAPTURE_SETTINGS');
    const data = response.data;
    if (!response.ok || !data || !Array.isArray(data.excludedSites)) {
        throw new Error(response.ok ? 'GrabBit returned invalid capture settings.' : response.message);
    }
    const mode = data.captureMode === 'off' || data.captureMode === 'ask' || data.captureMode === 'automatic'
        ? data.captureMode
        : data.enabled === false ? 'off' : data.mode === 'automatic' ? 'automatic' : data.mode === 'ask' ? 'ask' : null;
    if (!mode)
        throw new Error('GrabBit returned an unsupported capture mode.');
    return {
        mode,
        excludedSites: data.excludedSites.filter((site) => typeof site === 'string'),
    };
}
export async function savePolicy(next) {
    const response = await native('UPDATE_CAPTURE_SETTINGS', next);
    if (!response.ok)
        throw new Error(response.message);
    return response;
}
export async function native(type, payload) {
    const request = api.runtime.sendNativeMessage(NATIVE_HOST, envelope(type, payload));
    return Promise.race([
        request,
        new Promise((_, reject) => globalThis.setTimeout(() => reject(new Error('GrabBit did not respond in time')), 9_000)),
    ]);
}
export async function capture(url, suggestedFileName, sourcePage, automatic = false, idempotencyKey = crypto.randomUUID()) {
    if (!isEligibleUrl(url))
        throw new Error('Only HTTP/HTTPS downloads can be sent to GrabBit.');
    const fileName = safeSuggestedFileName(suggestedFileName);
    return native('CAPTURE_DOWNLOAD', {
        url, suggestedFileName: fileName, browser: browserKind(),
        sourcePageOrigin: sourcePage ? new URL(sourcePage).origin : null,
        idempotencyKey,
        automatic,
    });
}
export function hostname(value) {
    try {
        return new URL(value).hostname.toLowerCase();
    }
    catch {
        return null;
    }
}
export function safeError(reason) {
    const message = reason instanceof Error ? reason.message : String(reason);
    return message.replace(/https?:\/\/[^\s]+/giu, '[redacted URL]').slice(0, 240);
}
export async function reportDiagnostics(update) {
    const stored = await api.storage.local.get('captureDiagnostics');
    const previous = stored.captureDiagnostics && typeof stored.captureDiagnostics === 'object' ? stored.captureDiagnostics : {};
    const diagnostics = {
        automaticEnabled: false,
        downloadsPermissionGranted: true,
        lastEventAt: null,
        lastHostname: null,
        decision: 'none',
        jobAcceptance: 'not_attempted',
        cancellationResult: 'not_attempted',
        lastError: null,
        ...previous,
        ...update,
    };
    await api.storage.local.set({ captureDiagnostics: diagnostics });
    try {
        await native('REPORT_CAPTURE_DIAGNOSTICS', diagnostics);
    }
    catch { /* Keep local diagnostics while GrabBit is offline. */ }
}
