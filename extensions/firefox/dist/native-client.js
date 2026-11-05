import { envelope, NATIVE_HOST } from './contracts.js';
import { isEligibleUrl } from './policy.js';
export const api = typeof browser !== 'undefined' ? browser : chrome;
const DEFAULT_POLICY = { enabled: true, automatic: false, excludedSites: [] };
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
    const stored = await api.storage.local.get(['enabled', 'automatic', 'excludedSites']);
    return {
        enabled: typeof stored.enabled === 'boolean' ? stored.enabled : DEFAULT_POLICY.enabled,
        automatic: typeof stored.automatic === 'boolean' ? stored.automatic : DEFAULT_POLICY.automatic,
        excludedSites: Array.isArray(stored.excludedSites) ? stored.excludedSites.filter((site) => typeof site === 'string') : [],
    };
}
export async function native(type, payload) {
    return api.runtime.sendNativeMessage(NATIVE_HOST, envelope(type, payload));
}
export async function capture(url, suggestedFileName, sourcePage, automatic = false) {
    if (!isEligibleUrl(url))
        throw new Error('Only HTTP/HTTPS downloads can be sent to GrabBit.');
    return native('CAPTURE_DOWNLOAD', {
        url, suggestedFileName: suggestedFileName || null, browser: browserKind(),
        sourcePageOrigin: sourcePage ? new URL(sourcePage).origin : null,
        idempotencyKey: crypto.randomUUID(),
        automatic,
    });
}
