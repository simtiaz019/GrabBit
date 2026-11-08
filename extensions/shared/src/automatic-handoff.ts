import type { NativeResponse } from './contracts.js';

export interface AutomaticHandoffResult {
  accepted: boolean;
  decision: 'captured' | 'browser_continues' | 'duplicate_risk';
  jobAcceptance: string;
  cancellationResult: 'cancelled' | 'browser_completed' | 'failed' | 'not_attempted';
  lastError: string | null;
}

export async function performAutomaticHandoff(
  capture: () => Promise<NativeResponse>,
  cancelBrowserDownload: () => Promise<'cancelled' | 'browser_completed' | 'failed'>,
): Promise<AutomaticHandoffResult> {
  const response = await capture();
  if (!response.ok || !response.jobId) {
    return {
      accepted: false,
      decision: 'browser_continues',
      jobAcceptance: response.code || 'rejected',
      cancellationResult: 'not_attempted',
      lastError: response.message,
    };
  }
  const cancellationResult = await cancelBrowserDownload();
  return {
    accepted: true,
    decision: cancellationResult === 'cancelled' ? 'captured' : 'duplicate_risk',
    jobAcceptance: response.code,
    cancellationResult,
    lastError: cancellationResult === 'cancelled'
      ? null
      : 'The browser could not be cancelled after GrabBit accepted the job.',
  };
}
