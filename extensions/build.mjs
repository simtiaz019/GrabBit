import { cp, mkdir, readFile, readdir, rm } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const root = dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);
const typescriptCli = require.resolve('typescript/bin/tsc');
execFileSync(process.execPath, [typescriptCli, '-p', join(root, 'tsconfig.json')], {
  cwd: root,
  stdio: 'inherit',
});
async function validateModuleSpecifiers(directory) {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) {
      await validateModuleSpecifiers(path);
    } else if (entry.isFile() && path.endsWith('.js')) {
      const source = await readFile(path, 'utf8');
      for (const [, specifier] of source.matchAll(/^\s*import(?:\s+[^'"]+\s+from\s+)?\s*['"]([^'"]+)['"]/gmu)) {
        if (!specifier.startsWith('./') || !specifier.endsWith('.js')) {
          throw new Error(`Invalid extension module specifier "${specifier}" in ${path}`);
        }
        await readFile(resolve(dirname(path), specifier));
      }
    }
  }
}
await validateModuleSpecifiers(join(root, '.build'));
for (const browser of ['chromium', 'firefox']) {
  const dist = join(root, browser, 'dist');
  await rm(dist, { recursive: true, force: true });
  await mkdir(dist, { recursive: true });
  await cp(join(root, '.build'), dist, { recursive: true });
  await cp(join(root, 'shared', 'popup.html'), join(dist, 'popup.html'));
  await cp(join(root, 'shared', 'popup.css'), join(dist, 'popup.css'));
  await cp(join(root, 'shared', 'confirm.html'), join(dist, 'confirm.html'));
  await cp(join(root, 'shared', 'confirm.css'), join(dist, 'confirm.css'));
  await cp(join(root, 'shared', 'assets', 'media-download-button.png'), join(dist, 'media-download-button.png'));
  await cp(join(root, '..', 'public', 'branding', 'logo-icon.png'), join(dist, 'icon.png'));
  await cp(join(root, browser, 'manifest.json'), join(dist, 'manifest.json'));
}
await rm(join(root, '.build'), { recursive: true, force: true });
