import { api, capture as captureDownload, native, policy } from './native-client';
import { isEligibleUrl } from './policy';

const status = document.querySelector<HTMLElement>('#status')!;
const enabled = document.querySelector<HTMLInputElement>('#enabled')!;
const automatic = document.querySelector<HTMLInputElement>('#automatic')!;
const site = document.querySelector<HTMLButtonElement>('#site')!;
const media = document.querySelector<HTMLButtonElement>('#media')!;
const capture = document.querySelector<HTMLButtonElement>('#capture')!;
let activeUrl = '';

async function refresh() {
  const current = await policy(); enabled.checked = current.enabled; automatic.checked = current.automatic;
  const [tab] = await api.tabs.query({ active: true, currentWindow: true }); activeUrl = tab?.url ?? '';
  capture.disabled = !isEligibleUrl(activeUrl); media.disabled = !isEligibleUrl(activeUrl); site.disabled = !isEligibleUrl(activeUrl);
  if (isEligibleUrl(activeUrl)) site.textContent = current.excludedSites.includes(new URL(activeUrl).hostname) ? 'Enable capture on this site' : 'Disable capture on this site';
  try { const response = await native('PING'); status.textContent = response.ok ? 'Connected to GrabBit' : response.message; status.dataset.connected = String(response.ok); }
  catch { status.textContent = 'GrabBit is not connected'; status.dataset.connected = 'false'; }
}

enabled.addEventListener('change', () => void api.storage.local.set({ enabled: enabled.checked }));
automatic.addEventListener('change', () => void api.storage.local.set({ automatic: automatic.checked }));
site.addEventListener('click', () => void (async () => {
  const current = await policy(); const host = new URL(activeUrl).hostname;
  const excluded = current.excludedSites.includes(host) ? current.excludedSites.filter((value) => value !== host) : [...current.excludedSites, host];
  await api.storage.local.set({ excludedSites: excluded }); site.textContent = excluded.includes(host) ? 'Enable capture on this site' : 'Disable capture on this site';
})());
capture.addEventListener('click', () => void captureDownload(activeUrl, undefined, activeUrl).then((response) => { status.textContent = response.message; }));
media.addEventListener('click', () => void native('INSPECT_MEDIA', { url: activeUrl, authorizationConfirmed: false }).then((response) => { status.textContent = response.message; }));
document.querySelector('#open')!.addEventListener('click', () => void native('OPEN_APPLICATION'));
void refresh();
