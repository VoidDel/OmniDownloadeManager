// Renders the macOS menu bar icons from icons/tray-template.svg and tray-template-busy.svg:
//
//   apps/desktop/src-tauri/icons/tray/template.png, busy-template.png
//
//   node scripts/tray-icons.mjs
//
// 36×36 pixels, black on transparent: the app shows them as template images, 18 points high,
// which macOS tints like its own icons. Windows and Linux keep the app's icon. Rendered by the
// Chromium that Playwright installs for the extension's tests.

import { createRequire } from 'node:module';
import { mkdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const require = createRequire(join(root, 'apps/extension/package.json'));
const { chromium } = require('@playwright/test');

const OUT = join(root, 'apps/desktop/src-tauri/icons/tray');
const SIZE = 36;
mkdirSync(OUT, { recursive: true });
const browser = await chromium.launch();
try {
  for (const [from, to] of [
    ['tray-template.svg', 'template.png'],
    ['tray-template-busy.svg', 'busy-template.png'],
  ]) {
    const svg = readFileSync(join(root, 'icons', from), 'utf8');
    const page = await browser.newPage({ viewport: { width: SIZE, height: SIZE }, deviceScaleFactor: 1 });
    await page.setContent(`<!doctype html><style>html,body{margin:0;background:transparent}svg{display:block}</style>${svg}`);
    const out = join(OUT, to);
    await page.screenshot({ path: out, omitBackground: true, clip: { x: 0, y: 0, width: SIZE, height: SIZE } });
    console.log(out);
    await page.close();
  }
} finally {
  await browser.close();
}
