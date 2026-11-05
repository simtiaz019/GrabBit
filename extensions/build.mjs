import { cp, mkdir, rm } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new globalThis.URL('.', import.meta.url));
execFileSync(join(root, '..', 'node_modules', '.bin', 'tsc'), ['-p', join(root, 'tsconfig.json')], { stdio: 'inherit' });
for (const browser of ['chromium', 'firefox']) {
  const dist = join(root, browser, 'dist');
  await rm(dist, { recursive: true, force: true });
  await mkdir(dist, { recursive: true });
  await cp(join(root, '.build'), dist, { recursive: true });
  await cp(join(root, 'shared', 'popup.html'), join(dist, 'popup.html'));
  await cp(join(root, 'shared', 'popup.css'), join(dist, 'popup.css'));
  await cp(join(root, browser, 'manifest.json'), join(dist, 'manifest.json'));
}
await rm(join(root, '.build'), { recursive: true, force: true });
