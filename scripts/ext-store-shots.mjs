// The browser extension's store screenshots (docs/extension-publishing.md), in each language:
//
//   apps/extension/store/<lang>/screenshot-1-media.png       the page's media, two chosen
//   apps/extension/store/<lang>/screenshot-2-downloads.png   the latest downloads
//   apps/extension/store/<lang>/screenshot-3-options.png     the options, connected
//
//   pnpm ext:build && node scripts/ext-store-shots.mjs
//
// 1280×800 each. The built extension (apps/extension/.output/chrome-mv3) runs in the Chromium
// that Playwright installs, in the language of the screenshot, against a local page that plays a
// stream, a video and a song (served as video.example.com) and a stand-in for the app.

import { spawn } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import http from 'node:http';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const require = createRequire(join(root, 'apps/extension/package.json'));
const { chromium } = require('@playwright/test');

const EXT = join(root, 'apps/extension/.output/chrome-mv3');
const STORE = join(root, 'apps/extension/store');
const ID = 'nfifehjghnnmamofhbbiagiaodhfeipn';
const MB = 1024 * 1024;

/** The store's folder, the browser's language, and the page's title in it. */
const LANGS = [
  ['zh-CN', 'zh-CN', '城市夜景延时摄影 4K'],
  ['zh-TW', 'zh-TW', '城市夜景縮時攝影 4K'],
  ['en', 'en-US', 'City Lights Time-lapse 4K'],
  ['ja', 'ja', '夜景タイムラプス 4K'],
];

const listen = (s) => new Promise((r) => s.listen(0, '127.0.0.1', () => r(s.address().port)));

// The site: a page that loads a stream (three qualities), a video and a song, as a player does.
let title = '';
const site = http.createServer((req, res) => {
  const send = (type, body) => {
    res.writeHead(200, { 'Content-Type': type, 'Content-Length': Buffer.byteLength(body) });
    res.end(body);
  };
  const path = req.url.split('?')[0];
  if (path === '/watch') {
    return send(
      'text/html; charset=utf-8',
      `<!doctype html><title>${title}</title><script>
        fetch('/hls/master.m3u8').then((r) => r.text())
          .then(() => fetch('/media/city-lights-1080p.mp4')).then((r) => r.arrayBuffer())
          .then(() => fetch('/media/night-ambience.mp3')).then((r) => r.arrayBuffer());
      </script>`,
    );
  }
  if (path === '/hls/master.m3u8') {
    return send(
      'application/vnd.apple.mpegurl',
      '#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=6000000,RESOLUTION=1920x1080\n1080p.m3u8\n' +
        '#EXT-X-STREAM-INF:BANDWIDTH=3000000,RESOLUTION=1280x720\n720p.m3u8\n' +
        '#EXT-X-STREAM-INF:BANDWIDTH=900000,RESOLUTION=640x360\n360p.m3u8\n',
    );
  }
  if (path === '/media/city-lights-1080p.mp4') return send('video/mp4', Buffer.alloc(Math.round(86.4 * MB)));
  if (path === '/media/night-ambience.mp3') return send('audio/mpeg', Buffer.alloc(Math.round(7.6 * MB)));
  res.statusCode = 404;
  res.end();
});

// The app: two downloads under way, one paused, two done.
const bridge = http.createServer((req, res) => {
  let b = '';
  req.on('data', (c) => (b += c));
  req.on('end', () => {
    const m = JSON.parse(b);
    const status = {
      app_version: '0.2.0',
      running: 2,
      download_speed: 18.4 * MB,
      recent: [
        { id: '1', name: 'ubuntu-24.04.3-desktop-amd64.iso', status: 'downloading', progress: 0.64 },
        { id: '2', name: 'Blender-4.5-Studio-Demo.mp4', status: 'downloading', progress: 0.27 },
        { id: '3', name: 'node-v22.11.0-x64.msi', status: 'paused', progress: 0.42 },
        { id: '4', name: 'datasheet-RTL8152B.pdf', status: 'completed', progress: 1 },
        { id: '5', name: 'city-lights-1080p.mp4', status: 'completed', progress: 1 },
      ],
    };
    const result = m.type === 'get_status' ? status : m.type === 'ping' ? { app_version: '0.2.0', capabilities: ['post', 'dash'] } : {};
    res.setHeader('Content-Type', 'application/json');
    res.end(JSON.stringify({ id: m.id, ok: true, result }));
  });
});

/** The popup's picture on the store's 1280×800 canvas: enlarged, centred, on a soft ground. */
async function framed(browser, png, out) {
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
  const src = `data:image/png;base64,${png.toString('base64')}`;
  await page.setContent(`<!doctype html><style>
    html, body { margin: 0; width: 1280px; height: 800px; }
    body { display: flex; align-items: center; justify-content: center;
      background: radial-gradient(120% 90% at 30% 20%, #f3f6fc 0%, #e4eaf5 60%, #d9e2f2 100%); }
    img { display: block; max-height: 704px; border-radius: 26px; box-shadow: 0 30px 70px rgb(30 50 90 / 0.18), 0 6px 18px rgb(30 50 90 / 0.08); }
  </style><img src="${src}">`);
  await page.locator('img').evaluate((img) => img.decode());
  await page.screenshot({ path: out });
  await page.close();
}

const sitePort = await listen(site);
const bridgePort = await listen(bridge);
const shooter = await chromium.launch();
try {
  for (const [folder, lang, pageTitle] of LANGS) {
    title = pageTitle;
    const dir = join(STORE, folder);
    mkdirSync(dir, { recursive: true });
    const profile = mkdtempSync(join(tmpdir(), 'omnidl-store-'));
    const proc = spawn(
      chromium.executablePath(),
      [
        '--headless=new',
        '--remote-debugging-port=0',
        `--user-data-dir=${profile}`,
        `--disable-extensions-except=${EXT}`,
        `--load-extension=${EXT}`,
        `--lang=${lang}`,
        '--host-resolver-rules=MAP video.example.com 127.0.0.1',
        '--no-first-run',
        'about:blank',
      ],
      { stdio: ['ignore', 'ignore', 'pipe'], env: { ...process.env, LANG: lang.replace('-', '_') } },
    );
    try {
      const ws = await new Promise((resolve) => {
        let buf = '';
        proc.stderr.on('data', (d) => {
          buf += d;
          const m = /DevTools listening on (ws:\/\/\S+)/.exec(buf);
          if (m) resolve(m[1]);
        });
      });
      const browser = await chromium.connectOverCDP(ws);
      const ctx = browser.contexts()[0];
      const isOurs = (w) => w.url().startsWith(`chrome-extension://${ID}/`);
      const worker = ctx.serviceWorkers().find(isOurs) ?? (await ctx.waitForEvent('serviceworker', { predicate: isOurs }));
      await worker.evaluate(([port]) => chrome.storage.local.set({ pairing: { port, token: 'x'.repeat(64) }, transport: 'http' }), [bridgePort]);

      const video = await ctx.newPage();
      await video.goto(`http://video.example.com:${sitePort}/watch`);
      const tab = await worker.evaluate((url) => chrome.tabs.query({ url }).then((t) => t[0].id), video.url());
      // The three items listed, the stream's qualities read.
      for (let i = 0; i < 100; i++) {
        const list = await worker.evaluate((id) => chrome.storage.session.get(`media:${id}`).then((r) => r[`media:${id}`] ?? []), tab);
        if (list.length >= 3 && list.some((m) => m.variants?.length)) break;
        await new Promise((r) => setTimeout(r, 100));
      }

      // Laid out at twice its size, for a sharp enlarged picture.
      const popup = await ctx.newPage();
      await popup.setViewportSize({ width: 680, height: 1800 });
      await popup.goto(`chrome-extension://${ID}/popup.html?tab=${tab}`);
      await popup.addStyleTag({ content: 'html { zoom: 2; }' });
      await popup.waitForSelector('.pstate.ok');
      // Two chosen (the song and the video); the stream shows its tile and qualities.
      await popup.locator('.mrow').nth(0).locator('.mtile').click();
      await popup.locator('.mrow').nth(1).locator('.mtile').click();
      await popup.mouse.move(0, 0);
      const shot = async (name) => {
        await popup.waitForTimeout(300);
        const png = await popup.locator('.popup').screenshot({ animations: 'disabled' });
        await framed(shooter, png, join(dir, name));
      };
      await shot('screenshot-1-media.png');
      await popup.locator('.views [role="tab"]').nth(1).click();
      await shot('screenshot-2-downloads.png');
      await popup.close();

      const options = await ctx.newPage();
      await options.setViewportSize({ width: 1280, height: 800 });
      await options.goto(`chrome-extension://${ID}/options.html`);
      await options.waitForSelector('.ostatus.ok');
      // The connection's state and the takeover rules, ending where that group ends.
      await options.addStyleTag({ content: 'html { scrollbar-width: none; } .options > h2:nth-of-type(2), .options > h2:nth-of-type(2) ~ * { display: none; }' });
      await options.waitForTimeout(300);
      await options.screenshot({ path: join(dir, 'screenshot-3-options.png'), animations: 'disabled' });
      await browser.close().catch(() => undefined);
      console.log(`${folder}: done`);
    } finally {
      proc.kill();
      await new Promise((r) => setTimeout(r, 500));
      rmSync(profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
    }
  }
} finally {
  await shooter.close();
  site.close();
  bridge.close();
}
