import { envelope, NATIVE_HOST, type BrowserKind, type NativeResponse } from './contracts';
import { isEligibleUrl, type CapturePolicy } from './policy';

export const api: WebExt.Api = typeof browser !== 'undefined' ? browser : chrome;
const DEFAULT_POLICY: CapturePolicy = { enabled: true, automatic: false, excludedSites: [] };

function browserKind(): BrowserKind {
  const agent = navigator.userAgent;
  if (agent.includes('Firefox/')) return 'firefox';
  if (agent.includes('Edg/')) return 'edge';
  if (agent.includes('Brave/')) return 'brave';
  return 'chrome';
}

export async function policy(): Promise<CapturePolicy> {
  const stored = await api.storage.local.get(['enabled', 'automatic', 'excludedSites']);
  return {
    enabled: typeof stored.enabled === 'boolean' ? stored.enabled : DEFAULT_POLICY.enabled,
    automatic: typeof stored.automatic === 'boolean' ? stored.automatic : DEFAULT_POLICY.automatic,
    excludedSites: Array.isArray(stored.excludedSites) ? stored.excludedSites.filter((site): site is string => typeof site === 'string') : [],
  };
}

export async function native(type: string, payload?: unknown): Promise<NativeResponse> {
  return api.runtime.sendNativeMessage(NATIVE_HOST, envelope(type, payload)) as Promise<NativeResponse>;
}

export async function capture(url: string, suggestedFileName?: string, sourcePage?: string, automatic = false): Promise<NativeResponse> {
  if (!isEligibleUrl(url)) throw new Error('Only HTTP/HTTPS downloads can be sent to GrabBit.');
  return native('CAPTURE_DOWNLOAD', {
    url, suggestedFileName: suggestedFileName || null, browser: browserKind(),
    sourcePageOrigin: sourcePage ? new URL(sourcePage).origin : null,
    idempotencyKey: crypto.randomUUID(),
    automatic,
  });
}
