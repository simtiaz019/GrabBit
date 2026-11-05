import { createServer } from 'node:http';
import { mkdtemp, mkdir, readFile, stat, unlink } from 'node:fs/promises';
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
const extensionPath = join(repository, 'extensions/chromium/dist');
const extensionId = 'hdfipadppabbjkbbodcdhjelmiifamjj';
const root = await mkdtemp(join(tmpdir(), 'grabbit-browser-test-'));
const appData = join(root, 'app-data');
const downloads = join(root, 'grabbit-downloads');
const browserDownloads = join(root, 'browser-downloads');
const profile = join(root, 'profile');
await Promise.all([mkdir(appData), mkdir(downloads), mkdir(browserDownloads), mkdir(profile)]);
const payload = Buffer.alloc(8 * 1024 * 1024, 0x47);

const server = createServer((request, response) => {
  const name = request.url?.slice(1) || 'fixture.bin';
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
    response.setHeader('Content-Length', payload.length);
    if (request.method === 'HEAD') response.end(); else response.end(payload);
  }
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const port = server.address().port;
const origin = `http://127.0.0.1:${port}`;

const children = [];
function startDesktop() {
  const child = spawn(desktopPath, [], { env: { ...process.env, GRABBIT_DATA_DIRECTORY: appData,
    GRABBIT_DEFAULT_DOWNLOAD_DIRECTORY: downloads, GRABBIT_ENABLE_AUTOMATIC_CAPTURE: '1' }, stdio: ['ignore', 'pipe', 'pipe'] });
  children.push(child); return child;
}
function startBrowser() {
  const child = spawn(browserPath, [`--user-data-dir=${profile}`, `--load-extension=${extensionPath}`,
    `--disable-extensions-except=${extensionPath}`, '--remote-debugging-port=0', '--no-first-run',
    '--no-default-browser-check', '--enable-unsafe-extension-debugging',
    '--disable-features=DisableLoadExtensionCommandLineSwitch',
    `--download-default-directory=${browserDownloads}`, `chrome-extension://${extensionId}/popup.html`],
    { env: { ...process.env, GRABBIT_IPC_ENDPOINT: join(appData, 'browser-ipc.sock') }, stdio: ['ignore', 'pipe', 'pipe'] });
  children.push(child); return child;
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
    await waitFor(() => evaluate(cdp, `typeof chrome !== 'undefined' && Boolean(chrome.runtime?.id)`) , 'extension API');
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

try {
  let desktop = startDesktop();
  await waitFor(() => stat(join(appData, 'browser-ipc.sock')).then(value => value.isSocket()), 'desktop IPC');
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

  const hostPermission = await evaluate(extension, `chrome.permissions.contains({origins:['http://*/*','https://*/*']})`);
  const automaticCapture = hostPermission ? 'permission already granted; policy covered by extension tests' : 'manual optional-host permission required';

  extension.close(); browser.kill('SIGTERM'); await new Promise(resolve => browser.once('exit', resolve));
  await unlink(join(profile, 'DevToolsActivePort')).catch(() => undefined);
  browser = startBrowser(); portNumber = await devtoolsPort(); extension = await worker(portNumber);
  const restartPing = await native(extension, 'PING', {}); if (!restartPing.response?.ok) throw new Error('browser restart connection failed');
  desktop.kill('SIGTERM'); await new Promise(resolve => desktop.once('exit', resolve));
  const unavailable = await native(extension, 'PING', {});
  if (unavailable.response?.code !== 'app_unavailable') throw new Error(`desktop-unavailable response failed: ${JSON.stringify(unavailable)}`);
  desktop = startDesktop(); await waitFor(() => stat(join(appData, 'browser-ipc.sock')).then(value => value.isSocket()), 'desktop restart IPC');
  const desktopRestartPing = await waitFor(async () => { const result = await native(extension, 'PING', {}); return result.response?.ok && result; }, 'desktop restart connection');
  globalThis.console.log(JSON.stringify({ browser: browserPath, ping: ping.response.code, durableCapture: 'PASS', duplicatePrevention: 'PASS', automaticCapture, desktopUnavailableResponse: unavailable.response.code, browserRestart: restartPing.response.code, desktopRestart: desktopRestartPing.response.code, largeFileBytes: payload.length }, null, 2));
} finally {
  for (const child of children) if (child.exitCode == null) child.kill('SIGTERM');
  server.close();
}
