import { api, capture as captureDownload, native, policy, safeError, savePolicy } from './native-client.js';
import { isEligibleUrl } from './policy.js';
const status = document.querySelector('#status');
const mode = document.querySelector('#mode');
const site = document.querySelector('#site');
const media = document.querySelector('#media');
const capture = document.querySelector('#capture');
const recent = document.querySelector('#recent');
const diagnostics = document.querySelector('#diagnostics');
let activeUrl = '';
mode.disabled = true;
site.disabled = true;
function setStatus(message, connected) {
    status.textContent = message;
    if (connected !== undefined)
        status.dataset.connected = String(connected);
}
function renderDiagnostics(value, permission) {
    const rows = [
        ['Downloads permission', permission ? 'Granted' : 'Denied'],
        ['Last event', value?.lastEventAt ? new Date(value.lastEventAt).toLocaleTimeString() : 'None'],
        ['Last hostname', value?.lastHostname ?? 'None'],
        ['Decision', value?.decision ?? 'None'],
        ['GrabBit acceptance', value?.jobAcceptance ?? 'Not attempted'],
        ['Chrome cancellation', value?.cancellationResult ?? 'Not attempted'],
        ['Last error', value?.lastError ?? 'None'],
    ];
    diagnostics.replaceChildren(...rows.map(([label, content]) => {
        const row = document.createElement('div');
        const strong = document.createElement('strong');
        strong.textContent = label;
        const span = document.createElement('span');
        span.textContent = content;
        row.append(strong, span);
        return row;
    }));
}
async function refresh() {
    try {
        const current = await policy();
        mode.value = current.mode;
        mode.disabled = false;
        const [tab] = await api.tabs.query({ active: true, currentWindow: true });
        activeUrl = tab?.url ?? '';
        capture.disabled = !isEligibleUrl(activeUrl);
        media.disabled = !isEligibleUrl(activeUrl);
        site.disabled = !isEligibleUrl(activeUrl);
        if (isEligibleUrl(activeUrl))
            site.textContent = current.excludedSites.includes(new URL(activeUrl).hostname) ? 'Enable capture on this site' : 'Disable capture on this site';
        const permission = await api.permissions.contains({ permissions: ['downloads'] });
        const stored = await api.storage.local.get('captureDiagnostics');
        renderDiagnostics(stored.captureDiagnostics, permission);
        const response = await native('PING');
        setStatus(response.ok ? 'Connected to GrabBit' : response.message, response.ok);
    }
    catch (reason) {
        setStatus(safeError(reason), false);
    }
}
mode.addEventListener('change', () => void (async () => {
    const selected = mode.value;
    if (selected !== 'off' && selected !== 'ask' && selected !== 'automatic')
        return;
    try {
        const current = await policy();
        await savePolicy({ ...current, mode: selected });
        setStatus('Capture mode saved', true);
    }
    catch (reason) {
        const error = safeError(reason);
        await refresh();
        setStatus(error, false);
    }
})());
site.addEventListener('click', () => void (async () => {
    try {
        const current = await policy();
        const host = new URL(activeUrl).hostname;
        const excludedSites = current.excludedSites.includes(host) ? current.excludedSites.filter((value) => value !== host) : [...current.excludedSites, host];
        await savePolicy({ mode: current.mode, excludedSites });
        site.textContent = excludedSites.includes(host) ? 'Enable capture on this site' : 'Disable capture on this site';
    }
    catch (reason) {
        setStatus(safeError(reason), false);
    }
})());
capture.addEventListener('click', () => void captureDownload(activeUrl, undefined, activeUrl).then((response) => { setStatus(response.message, response.ok); }).catch((reason) => setStatus(safeError(reason), false)));
media.addEventListener('click', () => void native('INSPECT_MEDIA', { url: activeUrl, authorizationConfirmed: false }).then((response) => { setStatus(response.message, response.ok); }).catch((reason) => setStatus(safeError(reason), false)));
recent.addEventListener('click', () => void (async () => {
    const downloads = await api.downloads.search({ state: 'in_progress', orderBy: ['-startTime'], limit: 1 });
    const item = downloads[0];
    if (!item || !isEligibleUrl(item.finalUrl || item.url)) {
        setStatus('No eligible active Chrome download found');
        return;
    }
    const response = await captureDownload(item.finalUrl || item.url, item.filename, item.referrer);
    if (!response.ok || !response.jobId) {
        setStatus(response.message, false);
        return;
    }
    try {
        await api.downloads.cancel(item.id);
        await api.downloads.erase({ id: item.id });
        setStatus('Active download moved to GrabBit', true);
    }
    catch {
        setStatus('GrabBit accepted the job, but Chrome could not be cancelled. Check for duplicates.', false);
    }
})().catch((reason) => setStatus(safeError(reason), false)));
document.querySelector('#open').addEventListener('click', () => void native('OPEN_APPLICATION'));
void refresh();
