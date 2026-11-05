import { envelope, NATIVE_HOST, type BrowserKind, type NativeResponse } from './contracts.js';
import { isEligibleUrl, safeSuggestedFileName, type CapturePolicy } from './policy.js';

export const api: WebExt.Api = typeof browser !== 'undefined' ? browser : chrome;

export interface CaptureDiagnostics {
  automaticEnabled: boolean;
  downloadsPermissionGranted: boolean;
  lastEventAt: number | null;
  lastHostname: string | null;
  decision: string;
  jobAcceptance: string;
  cancellationResult: string;
  lastError: string | null;
}

function browserKind(): BrowserKind {
  const agent = navigator.userAgent;
  if (agent.includes('Firefox/')) return 'firefox';
  if (agent.includes('Edg/')) return 'edge';
  if (agent.includes('Brave/')) return 'brave';
  return 'chrome';
}

export async function policy(): Promise<CapturePolicy> {
  const response = await native('GET_CAPTURE_SETTINGS');
  const data = response.data as Record<string, unknown> | null;
  if (!response.ok || !data || !Array.isArray(data.excludedSites)) {
    throw new Error(response.ok ? 'GrabBit returned invalid capture settings.' : response.message);
  }
  const mode = data.captureMode === 'off' || data.captureMode === 'ask' || data.captureMode === 'automatic'
    ? data.captureMode
    : data.enabled === false ? 'off' : data.mode === 'automatic' ? 'automatic' : data.mode === 'ask' ? 'ask' : null;
  if (!mode) throw new Error('GrabBit returned an unsupported capture mode.');
  return {
    mode,
    excludedSites: data.excludedSites.filter((site): site is string => typeof site === 'string'),
  };
}

export async function savePolicy(next: CapturePolicy): Promise<NativeResponse> {
  const response = await native('UPDATE_CAPTURE_SETTINGS', next);
  if (!response.ok) throw new Error(response.message);
  return response;
}

export async function native(type: string, payload?: unknown): Promise<NativeResponse> {
  const request = api.runtime.sendNativeMessage(NATIVE_HOST, envelope(type, payload)) as Promise<NativeResponse>;
  return Promise.race([
    request,
    new Promise<never>((_, reject) => globalThis.setTimeout(() => reject(new Error('GrabBit did not respond in time')), 9_000)),
  ]);
}

export async function capture(url: string, suggestedFileName?: string, sourcePage?: string, automatic = false, idempotencyKey: string = crypto.randomUUID()): Promise<NativeResponse> {
  if (!isEligibleUrl(url)) throw new Error('Only HTTP/HTTPS downloads can be sent to GrabBit.');
  const fileName = safeSuggestedFileName(suggestedFileName);
  return native('CAPTURE_DOWNLOAD', {
    url, suggestedFileName: fileName, browser: browserKind(),
    sourcePageOrigin: sourcePage ? new URL(sourcePage).origin : null,
    idempotencyKey,
    automatic,
  });
}

export function hostname(value: string): string | null {
  try { return new URL(value).hostname.toLowerCase(); } catch { return null; }
}

export function safeError(reason: unknown): string {
  const message = reason instanceof Error ? reason.message : String(reason);
  return message.replace(/https?:\/\/[^\s]+/giu, '[redacted URL]').slice(0, 240);
}

export async function reportDiagnostics(update: Partial<CaptureDiagnostics>): Promise<void> {
  const stored = await api.storage.local.get('captureDiagnostics');
  const previous = stored.captureDiagnostics && typeof stored.captureDiagnostics === 'object' ? stored.captureDiagnostics as Partial<CaptureDiagnostics> : {};
  const diagnostics: CaptureDiagnostics = {
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
  try { await native('REPORT_CAPTURE_DIAGNOSTICS', diagnostics); } catch { /* Keep local diagnostics while GrabBit is offline. */ }
}
