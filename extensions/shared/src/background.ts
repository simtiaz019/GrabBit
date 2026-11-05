import { api, capture, native, policy } from './native-client.js';
import { isEligibleUrl, shouldIntercept } from './policy.js';

const recentlyCaptured = new Map<string, number>();

api.downloads.onCreated.addListener((item) => {
  void (async () => {
    const url = item.finalUrl || item.url;
    const current = await policy();
    if (!shouldIntercept(url, current) || item.byExtensionId) return;
    const last = recentlyCaptured.get(url) ?? 0;
    if (Date.now() - last < 15_000) return;
    const response = await capture(url, item.filename, undefined, true);
    if (!response.ok || !response.jobId) return;
    recentlyCaptured.set(url, Date.now());
    // The browser transfer remains intact unless durable queue creation was confirmed.
    try { await api.downloads.cancel(item.id); await api.downloads.erase({ id: item.id }); } catch { /* GrabBit already owns a durable copy; surface via popup status. */ }
  })().catch(() => undefined);
});

void api.contextMenus.removeAll().then(() => {
  api.contextMenus.create({ id: 'grabbit-link', title: 'Download with GrabBit', contexts: ['link'] });
  api.contextMenus.create({ id: 'grabbit-media', title: 'Download video/audio with GrabBit', contexts: ['video', 'audio', 'page'] });
});

api.contextMenus.onClicked.addListener((info, tab) => {
  const url = info.linkUrl || info.srcUrl || info.pageUrl || tab?.url;
  if (!url || !isEligibleUrl(url)) return;
  if (info.menuItemId === 'grabbit-media') {
    void native('INSPECT_MEDIA', { url, authorizationConfirmed: false });
  } else {
    void capture(url, undefined, tab?.url);
  }
});

