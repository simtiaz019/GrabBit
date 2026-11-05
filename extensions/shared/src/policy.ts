export type CaptureMode = 'off' | 'automatic' | 'ask';
export interface CapturePolicy { mode: CaptureMode; excludedSites: string[] }

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
  return policy.mode === 'automatic' && isEligibleUrl(value) && !hostExcluded(value, policy.excludedSites);
}

export function safeSuggestedFileName(value?: string): string | null {
  return value?.split(/[\\/]/u).pop()?.slice(0, 255) || null;
}
