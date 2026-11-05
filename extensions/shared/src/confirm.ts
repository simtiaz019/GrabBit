import { api, capture, hostname, reportDiagnostics, safeError } from './native-client.js';

interface PendingCapture {
  downloadId: number;
  url: string;
  filename?: string;
  sourcePage?: string;
  idempotencyKey: string;
  createdAt: number;
}

const heading = document.querySelector<HTMLElement>('#filename')!;
const host = document.querySelector<HTMLElement>('#hostname')!;
const status = document.querySelector<HTMLElement>('#status')!;
const grabbit = document.querySelector<HTMLButtonElement>('#grabbit')!;
const chrome = document.querySelector<HTMLButtonElement>('#chrome')!;
const id = Number(new URL(location.href).searchParams.get('id'));
const key = `pendingCapture:${id}`;

async function pendingCapture(): Promise<PendingCapture | null> {
  const stored = await api.storage.local.get(key);
  const pending = stored[key];
  return pending && typeof pending === 'object' ? pending as PendingCapture : null;
}

async function finish(decision: 'grabbit' | 'chrome'): Promise<void> {
  grabbit.disabled = true;
  chrome.disabled = true;
  const pending = await pendingCapture();
  if (!pending) { status.textContent = 'This download decision has expired.'; return; }
  if (decision === 'chrome') {
    await api.storage.local.remove(key);
    await reportDiagnostics({ decision: 'continued_in_browser', jobAcceptance: 'not_attempted', cancellationResult: 'not_attempted', lastError: null });
    globalThis.close();
    return;
  }
  const [current] = await api.downloads.search({ id: pending.downloadId });
  if (!current || current.state === 'complete') {
    status.textContent = 'Chrome already finished this download. It was not added to GrabBit.';
    await api.storage.local.remove(key);
    await reportDiagnostics({ decision: 'browser_completed', jobAcceptance: 'not_attempted', cancellationResult: 'browser_completed' });
    return;
  }
  status.textContent = 'Adding to GrabBit…';
  try {
    const response = await capture(pending.url, pending.filename, pending.sourcePage, false, pending.idempotencyKey);
    if (!response.ok || !response.jobId) {
      status.textContent = `GrabBit could not accept the download. Chrome will continue. ${response.message}`;
      await reportDiagnostics({ decision: 'browser_continues', jobAcceptance: response.code, lastError: response.message });
      chrome.disabled = false;
      return;
    }
    try {
      await api.downloads.cancel(pending.downloadId);
      await api.downloads.erase({ id: pending.downloadId });
      await api.storage.local.remove(key);
      await reportDiagnostics({ decision: 'captured', jobAcceptance: response.code, cancellationResult: 'cancelled', lastError: null });
      status.textContent = 'Download accepted by GrabBit.';
      globalThis.setTimeout(() => globalThis.close(), 700);
    } catch (reason) {
      const error = safeError(reason);
      status.textContent = 'GrabBit accepted the job, but Chrome could not be cancelled. Check both download lists to avoid a duplicate file.';
      await reportDiagnostics({ decision: 'duplicate_risk', jobAcceptance: response.code, cancellationResult: 'failed', lastError: error });
    }
  } catch (reason) {
    const error = safeError(reason);
    status.textContent = 'GrabBit is unavailable. Chrome will continue the download.';
    await reportDiagnostics({ decision: 'browser_continues', jobAcceptance: 'unavailable', lastError: error });
    chrome.disabled = false;
  }
}

void pendingCapture().then((pending) => {
  if (!pending) { status.textContent = 'This download decision has expired.'; grabbit.disabled = true; chrome.disabled = true; return; }
  heading.textContent = pending.filename?.split(/[\\/]/u).pop() || 'New download';
  host.textContent = hostname(pending.url) ?? 'Unknown host';
});

grabbit.addEventListener('click', () => void finish('grabbit'));
chrome.addEventListener('click', () => void finish('chrome'));
