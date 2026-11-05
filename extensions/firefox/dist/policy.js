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
    return policy.enabled && policy.automatic && isEligibleUrl(value) && !hostExcluded(value, policy.excludedSites);
}
