import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import process from 'node:process';
import { describe, expect, it } from 'vitest';

describe.each(['chromium', 'firefox'])('%s extension manifest', (browser) => {
  it('declares the user-visible media action content script', async () => {
    const manifest = JSON.parse(await readFile(join(process.cwd(), 'extensions', browser, 'manifest.json'), 'utf8')) as {
      host_permissions?: string[];
      content_scripts?: Array<{ matches: string[]; js: string[] }>;
      web_accessible_resources?: Array<{ resources: string[]; matches: string[] }>;
    };
    expect(manifest.host_permissions).toEqual(expect.arrayContaining(['http://*/*', 'https://*/*']));
    expect(manifest.content_scripts).toContainEqual(expect.objectContaining({ js: ['media-action.js'] }));
    expect(manifest.web_accessible_resources).toContainEqual({
      resources: ['media-download-button.png'],
      matches: ['http://*/*', 'https://*/*'],
    });
  });
});
