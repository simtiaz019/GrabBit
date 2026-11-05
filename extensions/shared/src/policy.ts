export interface CapturePolicy { enabled: boolean; automatic: boolean; excludedSites: string[] }

export function isEligibleUrl(value: string): boolean {
  try {
    const url = new URL(value);
    return (url.protocol === 'http:' || url.protocol === 'https:') && !url.username && !url.password;
  } catch { return false; }
}

export function hostExcluded(value: string, sites: string[]): boolean {
  let host: string;
  try { host = new URL(value).hostname.toLowerCase(); } catch { return true; }
  return sites.some((entry) => {
    const site = entry.trim().toLowerCase().replace(/^\*\./, '');
    return site !== '' && (host === site || host.endsWith(`.${site}`));
  });
}

export function shouldIntercept(value: string, policy: CapturePolicy): boolean {
  return policy.enabled && policy.automatic && isEligibleUrl(value) && !hostExcluded(value, policy.excludedSites);
}
