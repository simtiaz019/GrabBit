const runtime = typeof browser !== 'undefined' ? browser.runtime : chrome.runtime;
const buttonId = 'grabbit-media-action';

function isSupportedPage(): boolean {
  if (location.protocol !== 'http:' && location.protocol !== 'https:') return false;
  const host = location.hostname.toLowerCase();
  const knownMediaPage = host === 'youtu.be'
    || (host.endsWith('youtube.com') && ['/watch', '/shorts/', '/live/'].some((path) => location.pathname.startsWith(path)));
  return knownMediaPage || Boolean(document.querySelector('video, audio'));
}

function installAction(): void {
  if (!isSupportedPage() || document.getElementById(buttonId)) return;
  const button = document.createElement('button');
  button.id = buttonId;
  button.type = 'button';
  button.textContent = 'Download with GrabBit';
  button.title = 'Inspect authorized media with GrabBit';
  Object.assign(button.style, {
    position: 'fixed', right: '18px', bottom: '18px', zIndex: '2147483647',
    border: '1px solid #10b981', borderRadius: '8px', background: '#064e3b', color: '#fff',
    padding: '10px 14px', font: '600 13px system-ui, sans-serif', boxShadow: '0 8px 24px rgb(0 0 0 / 28%)',
    cursor: 'pointer',
  });
  button.addEventListener('click', () => {
    button.disabled = true;
    button.textContent = 'Opening GrabBit…';
    void runtime.sendMessage({ type: 'GRABBIT_INSPECT_PAGE_MEDIA', url: location.href })
      .then((response: unknown) => {
        const result = response as { ok?: boolean; message?: string } | undefined;
        button.textContent = result?.ok ? 'Opened in GrabBit' : (result?.message ?? 'GrabBit unavailable');
      })
      .catch(() => { button.textContent = 'GrabBit unavailable'; })
      .finally(() => {
        globalThis.setTimeout(() => { button.disabled = false; button.textContent = 'Download with GrabBit'; }, 2500);
      });
  });
  document.documentElement.append(button);
}

installAction();
const observer = new MutationObserver(installAction);
observer.observe(document.documentElement, { childList: true, subtree: true });
