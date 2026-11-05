import { createServer } from 'node:http';
import { mkdir, mkdtemp, readFile, rm, stat, unlink } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawn } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import process from 'node:process';
import { Buffer } from 'node:buffer';
import { clearTimeout, setTimeout } from 'node:timers';

const { WebSocket, fetch } = globalThis;

const browserPath = process.argv[2];
if (!browserPath) throw new Error('usage: node scripts/browser-integration.mjs <browser executable>');
const repository = process.cwd();
const desktopPath = process.env.GRABBIT_DESKTOP ?? join(repository, 'target/release/grabbit-desktop');
const extensionId = 'hdfipadppabbjkbbodcdhjelmiifamjj';
const root = await mkdtemp(join(tmpdir(), 'grabbit-browser-test-'));
const appData = join(root, 'app-data');
const extensionPath = process.env.GRABBIT_EXTENSION
  ?? (process.env.GRABBIT_USE_PACKAGED_EXTENSION === '1'
    ? join(appData, 'browser-extensions/chromium')
    : join(repository, 'extensions/chromium/dist'));
const downloads = join(root, 'grabbit-downloads');
const browserDownloads = join(root, 'browser-downloads');
const profile = join(root, 'profile');
await Promise.all([mkdir(appData), mkdir(downloads), mkdir(browserDownloads), mkdir(profile)]);
const payload = Buffer.alloc(8 * 1024 * 1024, 0x47);

const server = createServer((request, response) => {
  const parsed = new globalThis.URL(request.url ?? '/', 'http://127.0.0.1');
  if (parsed.pathname === '/redirect.iso') {
    response.statusCode = 302;
    response.setHeader('Location', '/mirror/CentOS-Stream-9-latest-aarch64-dvd1.iso?mirror=local');
    response.end();
    return;
  }
  const name = parsed.pathname === '/attachment' ? 'content-disposition.bin' : parsed.pathname.split('/').pop() || 'fixture.bin';
  response.setHeader('Content-Type', 'application/octet-stream');
  response.setHeader('Content-Disposition', `attachment; filename="${name}"`);
  response.setHeader('Accept-Ranges', 'bytes');
  response.setHeader('ETag', '"grabbit-browser-fixture-v1"');
  const range = request.headers.range?.match(/^bytes=(\d+)-(\d*)$/);
  if (range) {
    const start = Number(range[1]); const end = range[2] ? Number(range[2]) : payload.length - 1;
    response.statusCode = 206; response.setHeader('Content-Range', `bytes ${start}-${end}/${payload.length}`);
    response.setHeader('Content-Length', end - start + 1);
    if (request.method === 'HEAD') response.end(); else response.end(payload.subarray(start, end + 1));
  } else {
    if (parsed.pathname !== '/no-content-length.bin') response.setHeader('Content-Length', payload.length);
    if (request.method === 'HEAD') { response.end(); return; }
    if ((request.headers['user-agent'] ?? '').includes('Chrome')) {
      let offset = 0;
      const timer = globalThis.setInterval(() => {
        if (offset >= payload.length || response.destroyed) { globalThis.clearInterval(timer); response.end(); return; }
        const end = Math.min(offset + 64 * 1024, payload.length);
        response.write(payload.subarray(offset, end));
        offset = end;
      }, 10);
    } else response.end(payload);
  }
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const port = server.address().port;
const origin = `http://127.0.0.1:${port}`;

const children = [];
function startDesktop() {
  const child = spawn(desktopPath, [], { env: { ...process.env, GRABBIT_DATA_DIRECTORY: appData,
    GRABBIT_DEFAULT_DOWNLOAD_DIRECTORY: downloads, GRABBIT_ENABLE_AUTOMATIC_CAPTURE: '1' }, stdio: 'ignore' });
  children.push(child); return child;
}
function startBrowser() {
  const child = spawn(browserPath, [`--user-data-dir=${profile}`, `--load-extension=${extensionPath}`,
    `--disable-extensions-except=${extensionPath}`, '--remote-debugging-port=0', '--no-first-run',
    '--no-default-browser-check', '--enable-unsafe-extension-debugging',
    '--disable-features=DisableLoadExtensionCommandLineSwitch',
    `--download-default-directory=${browserDownloads}`, `chrome-extension://${extensionId}/popup.html`],
    { env: { ...process.env, GRABBIT_IPC_ENDPOINT: join(appData, 'browser-ipc.sock') }, stdio: 'ignore' });
  children.push(child); return child;
}
async function stopChild(child) {
  if (child.exitCode != null || child.signalCode != null) return;
  const waitForExit = () => child.exitCode != null || child.signalCode != null
    ? Promise.resolve()
    : new Promise(resolve => child.once('exit', resolve));
  child.kill('SIGTERM');
  const exited = await Promise.race([
    waitForExit().then(() => true),
    new Promise(resolve => globalThis.setTimeout(() => resolve(false), 5000)),
  ]);
  if (!exited) {
    child.kill('SIGKILL');
    await waitForExit();
  }
  child.stdout?.destroy();
  child.stderr?.destroy();
}
async function waitFor(test, label, timeoutMs = 15000) {
  const end = Date.now() + timeoutMs;
  while (Date.now() < end) { try { const value = await test(); if (value) return value; } catch { /* retry transient state */ } await new Promise(r => setTimeout(r, 100)); }
  throw new Error(`timed out waiting for ${label}`);
}
async function devtoolsPort() {
  return waitFor(async () => Number((await readFile(join(profile, 'DevToolsActivePort'), 'utf8')).split('\n')[0]), 'DevTools port');
}
class Cdp {
  constructor(url) { this.id = 0; this.pending = new Map(); this.socket = new WebSocket(url); this.opened = new Promise((resolve, reject) => { this.socket.onopen = resolve; this.socket.onerror = reject; }); this.socket.onmessage = event => { const value = JSON.parse(event.data); if (value.id) { const entry = this.pending.get(value.id); this.pending.delete(value.id); if (value.error) entry.reject(new Error(value.error.message)); else entry.resolve(value.result); } }; }
  async ready() { if (this.socket.readyState === WebSocket.OPEN) return; await Promise.race([this.opened, new Promise((_, reject) => setTimeout(() => reject(new Error('CDP connection timed out')), 5000))]); }
  async send(method, params = {}) { await this.ready(); const id = ++this.id; const promise = new Promise((resolve, reject) => { const timer = setTimeout(() => { this.pending.delete(id); reject(new Error(`CDP ${method} timed out`)); }, 15000); this.pending.set(id, { resolve: value => { clearTimeout(timer); resolve(value); }, reject: error => { clearTimeout(timer); reject(error); } }); }); this.socket.send(JSON.stringify({ id, method, params })); return promise; }
  close() { this.socket.close(); }
}
async function targets(portNumber) { return fetch(`http://127.0.0.1:${portNumber}/json`).then(response => response.json()); }
async function createTarget(portNumber, url) { const response = await fetch(`http://127.0.0.1:${portNumber}/json/new?${encodeURIComponent(url)}`, { method: 'PUT' }); if (!response.ok) throw new Error(`could not create browser target: ${response.status}`); return response.json(); }
async function worker(portNumber) {
  try {
    let target;
    try {
      target = await waitFor(async () => (await targets(portNumber)).find(item => item.type === 'service_worker' && item.url.startsWith(`chrome-extension://${extensionId}/`)), 'GrabBit extension service worker', 8000);
    } catch { /* an idle service worker is expected until an extension page wakes it */ }
    target ??= await createTarget(portNumber, `chrome-extension://${extensionId}/popup.html`);
    target = await waitFor(async () => (await targets(portNumber)).find(item => item.id === target.id && item.url.startsWith(`chrome-extension://${extensionId}/`)), 'GrabBit extension target');
    const cdp = new Cdp(target.webSocketDebuggerUrl);
    try {
      await waitFor(() => evaluate(cdp, `typeof chrome !== 'undefined' && Boolean(chrome.runtime?.id)`), 'extension API');
    } catch (error) {
      const state = await evaluate(cdp, `JSON.stringify({chrome:typeof chrome, runtime:typeof chrome==='undefined'?'unavailable':typeof chrome.runtime, id:typeof chrome==='undefined'?null:chrome.runtime?.id, href:location.href})`);
      throw new Error(`${error.message}; extension API state: ${state}`, { cause: error });
    }
    return cdp;
  } catch (error) {
    const visibleTargets = (await targets(portNumber)).map(item => `${item.type}:${item.url}`).join(', ');
    throw new Error(`${error.message}; visible targets: ${visibleTargets}`, { cause: error });
  }
}
async function evaluate(cdp, expression) { const result = await cdp.send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true, userGesture: true }); if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text); return result.result.value; }
async function native(cdp, type, body) {
  const resultKey = `nativeResult_${randomUUID().replaceAll('-', '')}`;
  const request = JSON.stringify({ schemaVersion: 1, requestId: randomUUID(), type, ...body });
  await evaluate(cdp, `(()=>{chrome.runtime.sendNativeMessage('com.grabbit.native',${request},response=>chrome.storage.local.set({${JSON.stringify(resultKey)}:{response,error:chrome.runtime.lastError?.message??null}}));return true})()`);
  const result = await waitFor(() => evaluate(cdp, `chrome.storage.local.get(${JSON.stringify(resultKey)}).then(value=>value[${JSON.stringify(resultKey)}]??null)`), 'native messaging callback', 12000);
  await evaluate(cdp, `chrome.storage.local.remove(${JSON.stringify(resultKey)})`);
  return result;
}
async function updateCaptureSettings(cdp, mode, excludedSites = []) {
  const result = await native(cdp, 'UPDATE_CAPTURE_SETTINGS', { payload: { mode, excludedSites } });
  if (!result.response?.ok) throw new Error(`capture setting failed: ${JSON.stringify(result)}`);
}
async function beginBrowserDownload(cdp, url) {
  return evaluate(cdp, `chrome.tabs.create({url:${JSON.stringify(url)}}).then(tab=>tab.id)`);
}
async function searchDownloads(cdp, query) {
  return evaluate(cdp, `chrome.downloads.search(${JSON.stringify(query)})`);
}
async function clearBrowserDownload(cdp, url) {
  const items = await searchDownloads(cdp, { url });
  for (const item of items) {
    if (item.state === 'in_progress') await evaluate(cdp, `chrome.downloads.cancel(${item.id})`).catch(() => undefined);
    await evaluate(cdp, `chrome.downloads.erase({id:${item.id}})`);
  }
}
async function confirmation(portNumber, downloadId) {
  const target = await waitFor(async () => (await targets(portNumber)).find(item => item.url.includes(`confirm.html?id=${downloadId}`)), 'Ask Every Time confirmation');
  const cdp = new Cdp(target.webSocketDebuggerUrl);
  await waitFor(() => evaluate(cdp, `Boolean(document.querySelector('#grabbit')&&document.querySelector('#chrome'))`), 'Ask Every Time confirmation controls');
  return cdp;
}

try {
  let desktop = startDesktop();
  await waitFor(() => stat(join(appData, 'browser-ipc.sock')).then(value => value.isSocket()), 'desktop IPC');
  await waitFor(() => stat(join(appData, 'browser-extensions/chromium/manifest.json')), 'packaged extension sync');
  let browser = startBrowser(); let portNumber = await devtoolsPort(); let extension = await worker(portNumber);
  globalThis.console.error('Browser integration: extension loaded');
  const ping = await native(extension, 'PING', {}); if (!ping.response?.ok) throw new Error(`PING failed: ${JSON.stringify(ping)}`);
  globalThis.console.error('Browser integration: native PING passed');
  const idempotencyKey = randomUUID();
  const captureBody = { payload: { url: `${origin}/fixture.bin`, suggestedFileName: 'fixture.bin', browser: 'chrome', sourcePageOrigin: origin, idempotencyKey, automatic: false } };
  const accepted = await native(extension, 'CAPTURE_DOWNLOAD', captureBody); if (accepted.response?.code !== 'accepted') throw new Error(`capture failed: ${JSON.stringify(accepted)}`);
  const duplicate = await native(extension, 'CAPTURE_DOWNLOAD', captureBody); if (duplicate.response?.code !== 'duplicate') throw new Error(`duplicate failed: ${JSON.stringify(duplicate)}`);
  await waitFor(() => stat(join(downloads, 'fixture.bin')).then(value => value.size === payload.length), 'captured large file');
  globalThis.console.error('Browser integration: durable and duplicate capture passed');

  const captureSettings = await native(extension, 'GET_CAPTURE_SETTINGS', {});
  if (captureSettings.response?.data?.captureMode !== 'automatic') {
    throw new Error(`automatic mode was not persisted: ${JSON.stringify(captureSettings.response?.data)}`);
  }
  await evaluate(extension, `chrome.tabs.create({url:${JSON.stringify(`${origin}/automatic.bin`)}}).then(tab=>tab.id)`);
  try {
    await waitFor(() => stat(join(downloads, 'automatic.bin')).then(value => value.size === payload.length), 'automatic capture');
  } catch (error) {
    const diagnostics = await evaluate(extension, `chrome.storage.local.get('captureDiagnostics').then(result=>result.captureDiagnostics??null)`);
    const browserDownloadsState = await evaluate(extension, `chrome.downloads.search({orderBy:['-startTime'],limit:5}).then(items=>items.map(item=>({url:item.url,state:item.state,filename:item.filename})))`);
    throw new Error(`${error.message}; diagnostics: ${JSON.stringify(diagnostics)}; browser downloads: ${JSON.stringify(browserDownloadsState)}`, { cause: error });
  }
  const automaticDiagnostics = await waitFor(async () => {
    const value = await evaluate(extension, `chrome.storage.local.get('captureDiagnostics').then(result=>result.captureDiagnostics??null)`);
    return value?.decision === 'captured' ? value : null;
  }, 'automatic capture cancellation');
  if (automaticDiagnostics.cancellationResult !== 'cancelled') {
    throw new Error(`automatic browser download was not cancelled: ${JSON.stringify(automaticDiagnostics)}`);
  }
  const automaticCapture = automaticDiagnostics.cancellationResult;

  await beginBrowserDownload(extension, `${origin}/redirect.iso`);
  const isoPath = join(downloads, 'CentOS-Stream-9-latest-aarch64-dvd1.iso');
  await waitFor(() => stat(isoPath).then(value => value.size === payload.length), 'redirected ISO capture', 30000);
  if (!(await readFile(isoPath)).equals(payload)) throw new Error('redirected ISO content mismatch');
  await beginBrowserDownload(extension, `${origin}/attachment?mirror=centos`);
  await waitFor(() => stat(join(downloads, 'content-disposition.bin')).then(value => value.size === payload.length), 'Content-Disposition capture', 30000);
  await beginBrowserDownload(extension, `${origin}/no-content-length.bin?channel=stream9`);
  await waitFor(() => stat(join(downloads, 'no-content-length.bin')).then(value => value.size === payload.length), 'unknown-length capture', 30000);
  globalThis.console.error('Browser integration: redirected ISO, query, Content-Disposition, range, and unknown-length cases passed');

  await updateCaptureSettings(extension, 'off');
  const disabledUrl = `${origin}/automatic-disabled.bin`;
  await beginBrowserDownload(extension, disabledUrl);
  await waitFor(async () => (await searchDownloads(extension, { url: disabledUrl })).length > 0, 'off-mode browser fallback');
  let disabledCaptured = false; try { disabledCaptured = (await stat(join(downloads, 'automatic-disabled.bin'))).isFile(); } catch { /* expected */ }
  if (disabledCaptured) throw new Error('off mode created a GrabBit job');
  await clearBrowserDownload(extension, disabledUrl);

  await updateCaptureSettings(extension, 'ask');
  const continueUrl = `${origin}/ask-continue.bin`;
  await beginBrowserDownload(extension, continueUrl);
  const [continueItem] = await waitFor(async () => { const items = await searchDownloads(extension, { url: continueUrl }); return items.length ? items : null; }, 'Ask Every Time browser item');
  const continuePrompt = await confirmation(portNumber, continueItem.id);
  await evaluate(continuePrompt, `document.querySelector('#chrome').click()`);
  continuePrompt.close();
  let continuedCaptured = false; try { continuedCaptured = (await stat(join(downloads, 'ask-continue.bin'))).isFile(); } catch { /* expected */ }
  if (continuedCaptured) throw new Error('Continue with Chrome created a GrabBit job');
  await clearBrowserDownload(extension, continueUrl);

  const askUrl = `${origin}/ask-grabbit.bin`;
  await beginBrowserDownload(extension, askUrl);
  const [askItem] = await waitFor(async () => { const items = await searchDownloads(extension, { url: askUrl }); return items.length ? items : null; }, 'Ask Every Time GrabBit item');
  const grabbitPrompt = await confirmation(portNumber, askItem.id);
  await evaluate(grabbitPrompt, `document.querySelector('#grabbit').click()`);
  await waitFor(() => stat(join(downloads, 'ask-grabbit.bin')).then(value => value.size === payload.length), 'Ask Every Time GrabBit capture', 30000);
  grabbitPrompt.close();

  await updateCaptureSettings(extension, 'automatic', ['127.0.0.1']);
  const excludedUrl = `${origin}/excluded.bin`;
  await beginBrowserDownload(extension, excludedUrl);
  await waitFor(async () => (await searchDownloads(extension, { url: excludedUrl })).length > 0, 'excluded-site browser fallback');
  let excludedCaptured = false; try { excludedCaptured = (await stat(join(downloads, 'excluded.bin'))).isFile(); } catch { /* expected */ }
  if (excludedCaptured) throw new Error('excluded site created a GrabBit job');
  await clearBrowserDownload(extension, excludedUrl);

  await updateCaptureSettings(extension, 'automatic');
  const patchedCancellation = await evaluate(extension, `(()=>{const original=chrome.downloads.cancel;globalThis.__grabbitOriginalCancel=original;chrome.downloads.cancel=()=>Promise.reject(new Error('fixture cancellation failure'));return chrome.downloads.cancel!==original})()`);
  if (!patchedCancellation) throw new Error('could not install the cancellation-failure fixture');
  const cancellationFailureUrl = `${origin}/cancellation-failure.bin`;
  await beginBrowserDownload(extension, cancellationFailureUrl);
  await waitFor(() => stat(join(downloads, 'cancellation-failure.bin')).then(value => value.size === payload.length), 'cancellation-failure GrabBit job', 30000);
  const failureDiagnostics = await waitFor(async () => {
    const value = await evaluate(extension, `chrome.storage.local.get('captureDiagnostics').then(result=>result.captureDiagnostics??null)`);
    return value?.decision === 'duplicate_risk' ? value : null;
  }, 'cancellation-failure diagnostics');
  if (failureDiagnostics.cancellationResult !== 'failed') throw new Error(`unexpected cancellation failure diagnostics: ${JSON.stringify(failureDiagnostics)}`);
  await evaluate(extension, `chrome.downloads.cancel=globalThis.__grabbitOriginalCancel;delete globalThis.__grabbitOriginalCancel`);
  await clearBrowserDownload(extension, cancellationFailureUrl);
  globalThis.console.error('Browser integration: off, Ask Every Time, and exclusion cases passed');

  extension.close(); await stopChild(browser);
  await unlink(join(profile, 'DevToolsActivePort')).catch(() => undefined);
  browser = startBrowser(); portNumber = await devtoolsPort(); extension = await worker(portNumber);
  const restartPing = await native(extension, 'PING', {}); if (!restartPing.response?.ok) throw new Error('browser restart connection failed');
  await updateCaptureSettings(extension, 'automatic');
  await stopChild(desktop);
  const unavailable = await native(extension, 'PING', {});
  if (unavailable.response?.code !== 'app_unavailable') throw new Error(`desktop-unavailable response failed: ${JSON.stringify(unavailable)}`);
  const unavailableUrl = `${origin}/desktop-unavailable.bin`;
  await beginBrowserDownload(extension, unavailableUrl);
  await waitFor(async () => (await searchDownloads(extension, { url: unavailableUrl })).length > 0, 'desktop-unavailable browser fallback');
  let unavailableCaptured = false; try { unavailableCaptured = (await stat(join(downloads, 'desktop-unavailable.bin'))).isFile(); } catch { /* expected */ }
  if (unavailableCaptured) throw new Error('desktop-unavailable event created a GrabBit job');
  await clearBrowserDownload(extension, unavailableUrl);
  desktop = startDesktop(); await waitFor(() => stat(join(appData, 'browser-ipc.sock')).then(value => value.isSocket()), 'desktop restart IPC');
  const desktopRestartPing = await waitFor(async () => { const result = await native(extension, 'PING', {}); return result.response?.ok && result; }, 'desktop restart connection');
  extension.close(); await stopChild(browser); await stopChild(desktop);
  globalThis.console.log(JSON.stringify({ browser: browserPath, ping: ping.response.code, durableCapture: 'PASS', duplicatePrevention: 'PASS', automaticCapture, redirectedIso: 'PASS', contentDisposition: 'PASS', unknownLength: 'PASS', automaticDisabled: 'PASS', askContinueChrome: 'PASS', askGrabBit: 'PASS', siteExclusion: 'PASS', cancellationFailure: 'PASS', desktopUnavailableFallback: 'PASS', desktopUnavailableResponse: unavailable.response.code, browserRestart: restartPing.response.code, desktopRestart: desktopRestartPing.response.code, largeFileBytes: payload.length }, null, 2));
} finally {
  await Promise.all(children.map(stopChild));
  await rm(root, { recursive: true, force: true });
  server.closeAllConnections();
  server.close();
}
