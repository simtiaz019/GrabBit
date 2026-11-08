export async function performAutomaticHandoff(capture, cancelBrowserDownload) {
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
