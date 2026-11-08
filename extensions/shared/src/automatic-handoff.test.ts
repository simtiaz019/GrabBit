import { describe, expect, it, vi } from 'vitest';
import { performAutomaticHandoff } from './automatic-handoff';
import { captureDecision } from './policy';

const response = (ok: boolean) => ({
  schemaVersion: 1, requestId: 'request', ok,
  code: ok ? 'accepted' : 'app_unavailable',
  message: ok ? 'durably queued' : 'GrabBit is unavailable',
  jobId: ok ? 'job-id' : null, data: null,
});

describe('automatic download handoff', () => {
  it('cancels the browser only after durable acceptance', async () => {
    const cancel = vi.fn().mockResolvedValue('cancelled' as const);
    const result = await performAutomaticHandoff(() => Promise.resolve(response(true)), cancel);
    expect(result).toMatchObject({ accepted: true, decision: 'captured', cancellationResult: 'cancelled' });
    expect(cancel).toHaveBeenCalledOnce();
  });

  it('preserves the browser download when handoff fails', async () => {
    const cancel = vi.fn().mockResolvedValue('cancelled' as const);
    const result = await performAutomaticHandoff(() => Promise.resolve(response(false)), cancel);
    expect(result).toMatchObject({ accepted: false, decision: 'browser_continues', cancellationResult: 'not_attempted' });
    expect(cancel).not.toHaveBeenCalled();
  });

  it('rejects a repeated browser event before creating another job', () => {
    expect(captureDecision('https://mirror.example/file.iso', { mode: 'automatic', excludedSites: [] }, true, false)).toBe('duplicate_event');
  });

  it('leaves automatic capture disabled when the desktop policy is off', () => {
    expect(captureDecision('https://example.com/file.zip', { mode: 'off', excludedSites: [] }, false, false)).toBe('disabled');
  });
});
