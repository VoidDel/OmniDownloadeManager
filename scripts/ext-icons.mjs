// Renders the browser extension's small icons from icons/extension-toolbar.svg:
//
//   apps/extension/public/icon/16.png, 32.png
//
//   node scripts/ext-icons.mjs
//
// The browser draws those at 16 pixels (32 on high-density screens) with no room for a margin:
// the toolbar button, the context menu, the tab of the extension's pages. They get a tile that
// fills the square, as other extensions' do. 48.png and 128.png (the extensions page, the stores,
// which want a transparent margin around the 128-pixel icon) stay as they are. Rendered by the
// Chromium that Playwright installs for the extension's tests.

import { createRequire } from 'node:module';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const require = createRequire(join(root, 'apps/extension/package.json'));
const { chromium } = require('@playwright/test');

const svg = readFileSync(join(root, 'icons/extension-toolbar.svg'), 'utf8');
const browser = await chromium.launch();
try {
  for (const size of [16, 32]) {
    const page = await browser.newPage({ viewport: { width: size, height: size }, deviceScaleFactor: 1 });
    const sized = svg.replace(/width="32" height="32"/, `width="${size}" height="${size}"`);
    await page.setContent(`<!doctype html><style>html,body{margin:0;background:transparent}svg{display:block}</style>${sized}`);
    const out = join(root, `apps/extension/public/icon/${size}.png`);
    await page.screenshot({ path: out, omitBackground: true, clip: { x: 0, y: 0, width: size, height: size } });
    console.log(out);
    await page.close();
  }
} finally {
  await browser.close();
}
