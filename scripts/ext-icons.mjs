// Renders the browser extension's toolbar icons from icons/extension-toolbar.svg:
//
//   apps/extension/public/icon/toolbar-16.png, toolbar-32.png   (the action's default_icon)
//
//   node scripts/ext-icons.mjs
//
// The toolbar draws its buttons at 16 pixels (32 on high-density screens) with no room for a
// margin, so the button gets a tile that fills the square; the other sizes (icon/*.png, the
// extensions page and the stores, which want a transparent margin) stay as they are. Rendered by
// the Chromium that Playwright installs for the extension's tests.

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
    const out = join(root, `apps/extension/public/icon/toolbar-${size}.png`);
    await page.screenshot({ path: out, omitBackground: true, clip: { x: 0, y: 0, width: size, height: size } });
    console.log(out);
    await page.close();
  }
} finally {
  await browser.close();
}
