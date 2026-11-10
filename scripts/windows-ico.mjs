// Writes apps/desktop/src-tauri/icons/icon.ico with every size Windows asks for at the common
// display scales (100–300 %), each drawn from icons/app-icon.svg rather than resized from a
// neighbour. `tauri icon` only writes 16–64 and 256: at 125 %, 150 % or 175 % the taskbar and
// window icons were resampled from another size and looked soft. Run after `pnpm icons`:
//
//   node scripts/windows-ico.mjs [svg] [ico]
//
// (defaults: icons/app-icon.svg → apps/desktop/src-tauri/icons/icon.ico; OmniZip's icon was made
// with the same script and its own paths)
// (it uses the extension's Playwright Chromium to rasterize the SVG).

import { readFileSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

// 16 / 32 / 48 / 256 are the classic set; the rest are 16, 24 and 32 at 125–300 %.
const SIZES = [16, 20, 24, 28, 30, 32, 36, 40, 42, 48, 56, 60, 64, 72, 80, 96, 128, 256];

const root = fileURLToPath(new URL('..', import.meta.url));
// Playwright is the extension's dependency.
const { chromium } = createRequire(`${root}apps/extension/package.json`)('@playwright/test');
// The source keeps the margin macOS icons have (the tile is 896 of 1024); Windows icons fill
// their frame (Edge, VS Code, Settings: 100 %), and with the margin the taskbar button and the
// notification-area icon looked a size smaller than their neighbours. So the frame is cropped to
// the tile, the first <rect> of the source.
const source = readFileSync(process.argv[2] ?? `${root}icons/app-icon.svg`, 'utf8');
const tile = source.match(/<rect x="(\d+)" y="(\d+)" width="(\d+)" height="(\d+)"/);
if (!tile) throw new Error('no tile <rect> in the icon source');
const svg = source.replace(/viewBox="[^"]*"/, `viewBox="${tile.slice(1).join(' ')}"`);
const out = process.argv[3] ?? `${root}apps/desktop/src-tauri/icons/icon.ico`;

const browser = await chromium.launch();
const page = await browser.newPage({ deviceScaleFactor: 1 });
const pngs = [];
for (const size of SIZES) {
  await page.setViewportSize({ width: size, height: size });
  await page.setContent(
    `<html><body style="margin:0;background:transparent"><img src="data:image/svg+xml;base64,${Buffer.from(svg).toString('base64')}" width="${size}" height="${size}" style="display:block"></body></html>`,
  );
  await page.waitForFunction(() => document.querySelector('img').complete);
  pngs.push(await page.screenshot({ omitBackground: true, clip: { x: 0, y: 0, width: size, height: size } }));
}
await browser.close();

// ICO: a 6-byte header, a 16-byte entry per image, then the PNG data.
const header = Buffer.alloc(6);
header.writeUInt16LE(0, 0);
header.writeUInt16LE(1, 2);
header.writeUInt16LE(SIZES.length, 4);
let offset = 6 + 16 * SIZES.length;
const entries = SIZES.map((size, i) => {
  const e = Buffer.alloc(16);
  e.writeUInt8(size >= 256 ? 0 : size, 0);
  e.writeUInt8(size >= 256 ? 0 : size, 1);
  e.writeUInt8(0, 2);
  e.writeUInt8(0, 3);
  e.writeUInt16LE(1, 4);
  e.writeUInt16LE(32, 6);
  e.writeUInt32LE(pngs[i].length, 8);
  e.writeUInt32LE(offset, 12);
  offset += pngs[i].length;
  return e;
});
writeFileSync(out, Buffer.concat([header, ...entries, ...pngs]));
console.log(`${out}: ${SIZES.join(', ')}`);
