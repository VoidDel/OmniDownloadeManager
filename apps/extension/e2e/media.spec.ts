// Media sniffing in the Chromium build, loaded unpacked: a page that plays a video, loads an
// HLS stream (with its segments and a small sound, which are not listed) and a Bilibili-style
// DASH picture and sound (whole files fetched in ranges: offered as one video to join). The list and the
// toolbar badge of its tab, a download from the popup's list (to a stand-in for the app's HTTP
// channel), and the list starting over on the next page.

import { chromium, expect, test, type Browser, type BrowserContext, type Page, type Worker } from '@playwright/test';
import { spawn, type ChildProcess } from 'node:child_process';
import fs from 'node:fs';
import http from 'node:http';
import type { AddressInfo } from 'node:net';
import os from 'node:os';
import path from 'node:path';

const EXTENSION = path.resolve(import.meta.dirname, '../.output/chrome-mv3');
const EXTENSION_ID = 'nfifehjghnnmamofhbbiagiaodhfeipn';
const TOKEN = 'e2e'.padEnd(64, '0');
const MB = 1024 * 1024;

let site: http.Server;
let bridge: http.Server;
let siteUrl = '';
let bridgePort = 0;
const received: { type: string; payload?: Record<string, unknown> }[] = [];

function listen(server: http.Server): Promise<number> {
  return new Promise((resolve) => server.listen(0, '127.0.0.1', () => resolve((server.address() as AddressInfo).port)));
}

test.beforeAll(async () => {
  site = http.createServer((req, res) => {
    const send = (type: string, body: Buffer | string, extra: Record<string, string> = {}) => {
      res.writeHead(200, { 'Content-Type': type, 'Content-Length': String(Buffer.byteLength(body)), ...extra });
      res.end(body);
    };
    switch (req.url) {
      case '/watch':
        return send(
          'text/html; charset=utf-8',
          `<!doctype html><title>Big Buck Bunny</title>
           <video src="/media/clip.mp4" preload="auto" muted></video>
           <script>
             fetch('/hls/index.m3u8').then(() => fetch('/hls/seg0.ts'));
             fetch('/sfx/click.mp3');
             for (const id of [30080, 30280]) fetch('/bili/5678-1-' + id + '.m4s', { headers: { Range: 'bytes=0-1023' } });
           </script>`,
        );
      case '/other':
        return send('text/html; charset=utf-8', '<!doctype html><title>Other</title>');
      case '/media/clip.mp4':
        return send('video/mp4', Buffer.alloc(2 * MB, 1), { 'Accept-Ranges': 'bytes' });
      case '/hls/index.m3u8':
        return send('application/vnd.apple.mpegurl', '#EXTM3U\n#EXTINF:2,\nseg0.ts\n#EXT-X-ENDLIST\n');
      case '/hls/seg0.ts':
        return send('video/mp2t', Buffer.alloc(200 * 1024, 2));
      case '/bili/5678-1-30080.m4s':
      case '/bili/5678-1-30280.m4s': {
        const total = req.url.includes('30080') ? 2 * MB : MB;
        res.writeHead(206, { 'Content-Type': 'video/mp4', 'Content-Range': `bytes 0-1023/${total}`, 'Content-Length': '1024' });
        return res.end(Buffer.alloc(1024, 4));
      }
      case '/sfx/click.mp3':
        return send('audio/mpeg', Buffer.alloc(10 * 1024, 3));
      default:
        res.statusCode = 404;
        res.end();
    }
  });
  siteUrl = `http://127.0.0.1:${await listen(site)}`;
  bridge = http.createServer((req, res) => {
    let body = '';
    req.on('data', (c) => (body += c));
    req.on('end', () => {
      const msg = JSON.parse(body) as { id: string; type: string; payload?: Record<string, unknown> };
      received.push(msg);
      res.setHeader('Content-Type', 'application/json');
      const result = msg.type === 'add_download' ? { action: 'started', asked: true, task_id: 't' } : { app_version: '9.9.9' };
      res.end(JSON.stringify({ id: msg.id, ok: true, result }));
    });
  });
  bridgePort = await listen(bridge);
});

test.afterAll(() => {
  site?.close();
  bridge?.close();
});

test.describe('media sniffing', () => {
  let proc: ChildProcess;
  let browser: Browser;
  let context: BrowserContext;
  let worker: Worker;

  test.beforeAll(async () => {
    const profile = fs.mkdtempSync(path.join(os.tmpdir(), 'omnidl-e2e-media-'));
    proc = spawn(
      chromium.executablePath(),
      ['--headless=new', '--remote-debugging-port=0', `--user-data-dir=${profile}`, `--disable-extensions-except=${EXTENSION}`, `--load-extension=${EXTENSION}`, '--no-first-run', '--no-default-browser-check', '--autoplay-policy=no-user-gesture-required', 'about:blank'],
      { stdio: ['ignore', 'ignore', 'pipe'] },
    );
    const ws = await new Promise<string>((resolve) => {
      let buf = '';
      proc.stderr!.on('data', (d) => {
        buf += d;
        const m = /DevTools listening on (ws:\/\/\S+)/.exec(buf);
        if (m) resolve(m[1]);
      });
    });
    browser = await chromium.connectOverCDP(ws);
    context = browser.contexts()[0];
    const isOurs = (w: Worker) => w.url().startsWith(`chrome-extension://${EXTENSION_ID}/`);
    worker = context.serviceWorkers().find(isOurs) ?? (await context.waitForEvent('serviceworker', { predicate: isOurs }));
    await worker.evaluate(([port, token]) => chrome.storage.local.set({ pairing: { port, token }, transport: 'http' }), [bridgePort, TOKEN] as const);
  });

  test.afterAll(async () => {
    await browser?.close();
    proc?.kill();
  });

  const tabOf = (page: Page) => worker.evaluate((url) => chrome.tabs.query({ url }).then((t) => t[0]?.id ?? -1), page.url());
  const listOf = (tabId: number) =>
    worker.evaluate((id) => chrome.storage.session.get(`media:${id}`).then((r) => ((r[`media:${id}`] ?? []) as { url: string; kind: string; size: number | null; title: string | null; track?: string }[])), tabId);
  const badgeOf = (tabId: number) => worker.evaluate((id) => chrome.action.getBadgeText({ tabId: id }), tabId);

  test('lists the video and the streams, not the pieces, with a badge', async () => {
    const page = await context.newPage();
    await page.goto(`${siteUrl}/watch`);
    const tab = await tabOf(page);
    await expect.poll(async () => (await listOf(tab)).map((m) => `${m.kind} ${new URL(m.url).pathname}`).sort()).toEqual(['dash /bili/5678-1-30080.m4s', 'dash /bili/5678-1-30280.m4s', 'hls /hls/index.m3u8', 'video /media/clip.mp4']);
    const list = await listOf(tab);
    expect(list.find((m) => m.kind === 'video')?.size).toBe(2 * MB);
    expect(list.find((m) => m.kind === 'hls')?.title).toBe('Big Buck Bunny');
    expect(list.filter((m) => m.kind === 'dash').map((m) => `${m.track} ${m.size}`).sort()).toEqual([`audio ${MB}`, `video ${2 * MB}`]);
    // The picture and its sound count once.
    await expect.poll(() => badgeOf(tab)).toBe('3');

    // The popup's download button: the stream goes to the app named after the page.
    const popup = await context.newPage();
    await popup.goto(`chrome-extension://${EXTENSION_ID}/popup.html`);
    const hls = list.find((m) => m.kind === 'hls');
    await popup.evaluate((media) => chrome.runtime.sendMessage({ type: 'download-media', media }), hls);
    await expect.poll(() => received.filter((r) => r.type === 'add_download').length).toBe(1);
    expect(received.find((r) => r.type === 'add_download')?.payload).toMatchObject({
      url: `${siteUrl}/hls/index.m3u8`,
      filename: 'Big Buck Bunny.mp4',
      referer: `${siteUrl}/watch`,
    });
    // The DASH picture goes with its sound, to be joined.
    const picture = list.find((m) => m.kind === 'dash' && m.track === 'video');
    const sound = list.find((m) => m.kind === 'dash' && m.track === 'audio');
    await popup.evaluate(([media, audio]) => chrome.runtime.sendMessage({ type: 'download-media', media, audio, name: 'Big Buck Bunny.mp4' }), [picture, sound] as const);
    await expect.poll(() => received.filter((r) => r.type === 'add_download').length).toBe(2);
    expect(received.filter((r) => r.type === 'add_download')[1]?.payload).toMatchObject({
      url: `${siteUrl}/bili/5678-1-30080.m4s`,
      audio_url: `${siteUrl}/bili/5678-1-30280.m4s`,
      filename: 'Big Buck Bunny.mp4',
      size: 3 * MB,
    });
    await popup.close();

    // Another page in the tab: a new list.
    await page.goto(`${siteUrl}/other`);
    await expect.poll(async () => (await listOf(tab)).length).toBe(0);
    await expect.poll(() => badgeOf(tab)).toBe('');
  });
});
