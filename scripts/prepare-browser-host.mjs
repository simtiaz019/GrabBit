import { cp, mkdir, rm } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { join } from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new globalThis.URL('..', import.meta.url));
const target = process.env.TAURI_ENV_TARGET_TRIPLE;
if (!target) throw new Error('TAURI_ENV_TARGET_TRIPLE is required to package the native messaging host.');
const windowsTarget = target.includes('-windows-');
const cargoExecutable = process.platform === 'win32' ? 'cargo.exe' : 'cargo';

execFileSync(cargoExecutable, [
  'build',
  '--manifest-path', join(root, 'Cargo.toml'),
  '--release',
  '--target', target,
  '--bin', 'grabbit-browser-host',
], { cwd: root, stdio: 'inherit' });

const executable = windowsTarget ? 'grabbit-browser-host.exe' : 'grabbit-browser-host';
const source = join(root, 'target', target, 'release', executable);
const resources = join(root, 'target', 'browser-native-host-resources');
await rm(resources, { recursive: true, force: true });
await mkdir(resources, { recursive: true });
await cp(source, join(resources, executable));
