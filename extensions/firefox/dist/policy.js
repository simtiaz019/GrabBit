export function isEligibleUrl(value) {
    try {
        const url = new URL(value);
        return (url.protocol === 'http:' || url.protocol === 'https:') && !url.username && !url.password;
    }
    catch {
        return false;
    }
}
export function hostExcluded(value, sites) {
    let host;
    try {
        host = new URL(value).hostname.toLowerCase();
    }
    catch {
        return true;
    }
    return sites.some((entry) => {
        const site = entry.trim().toLowerCase().replace(/^\*\./, '');
        return site !== '' && (host === site || host.endsWith(`.${site}`));
    });
}
export function shouldIntercept(value, policy) {
    return policy.mode === 'automatic' && isEligibleUrl(value) && !hostExcluded(value, policy.excludedSites);
}
export function captureDecision(value, policy, alreadyHandled, browserCompleted) {
    if (policy.mode === 'off')
        return 'disabled';
    if (hostExcluded(value, policy.excludedSites))
        return 'excluded';
    if (alreadyHandled)
        return 'duplicate_event';
    if (browserCompleted)
        return 'browser_completed';
    return policy.mode;
}
export function safeSuggestedFileName(value) {
    return value?.split(/[\\/]/u).pop()?.slice(0, 255) || null;
}
