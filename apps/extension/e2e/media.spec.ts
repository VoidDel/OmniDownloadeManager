// Media sniffing in the Chromium build, loaded unpacked: a page that plays a video, loads an
// HLS stream (with its segments and a small sound, which are not listed) and a Bilibili-style
// DASH picture and sound (whole files fetched in ranges: offered as one video to join). The list and the
// toolbar badge of its tab, a download from the popup's list (to a stand-in for the app's HTTP
// channel), and the list starting over on the next page. A player in a frame of another site:
// its media is sent with the Referer and Origin its request went out with.

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
let port = 0;
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
             // The cookie changes after the playlist was loaded with it.
             fetch('/hls/index.m3u8').then(() => { document.cookie = 'watch=2; path=/'; return fetch('/hls/seg0.ts'); });
             fetch('/sfx/click.mp3');
             for (const id of [30080, 30280]) fetch('/bili/5678-1-' + id + '.m4s', { headers: { Range: 'bytes=0-1023' } });
           </script>`,
          { 'Set-Cookie': 'watch=1; Path=/' },
        );
      // A page with a player from another site (localhost), which loads its video from here.
      case '/embed':
        return send('text/html; charset=utf-8', `<!doctype html><title>Embedded</title><iframe src="http://localhost:${port}/player"></iframe>`);
      case '/player':
        return send('text/html; charset=utf-8', `<!doctype html><title>Player</title><script>fetch('http://127.0.0.1:${port}/media/cors.mp4')</script>`);
      case '/media/cors.mp4':
        return send('video/mp4', Buffer.alloc(MB, 5), { 'Access-Control-Allow-Origin': '*' });
      // A site that moves to another video itself (history.pushState), then to an anchor.
      case '/spa':
        return send(
          'text/html; charset=utf-8',
          `<!doctype html><title>First</title><script>
             fetch('/media/clip.mp4');
             // Called by the test once the first video is listed.
             window.next = () =>
               Promise.resolve()
                 .then(() => { history.pushState({}, '', '/spa/next'); document.title = 'Next'; return fetch('/media/cors.mp4'); })
                 .then(() => { location.hash = 'comments'; document.body.dataset.done = '1'; });
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
  port = await listen(site);
  siteUrl = `http://127.0.0.1:${port}`;
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
      // The cookie the playlist was loaded with, not the one the page set afterwards.
      cookies: 'watch=1',
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

  test("keeps the media of the page the site moved to, not the one it left", async () => {
    const page = await context.newPage();
    await page.goto(`${siteUrl}/spa`);
    const tab = await tabOf(page);
    await expect.poll(async () => (await listOf(tab)).map((m) => new URL(m.url).pathname)).toEqual(['/media/clip.mp4']);
    await page.evaluate(() => (window as unknown as { next: () => Promise<void> }).next());
    await expect.poll(async () => (await listOf(tab)).map((m) => new URL(m.url).pathname)).toEqual(['/media/cors.mp4']);
    // The anchor is the same page: the list stays.
    await page.waitForTimeout(300);
    const list = (await listOf(tab)) as { url: string; pageUrl?: string }[];
    expect(list.map((m) => [new URL(m.url).pathname, m.pageUrl])).toEqual([['/media/cors.mp4', `${siteUrl}/spa/next`]]);
    await expect.poll(() => badgeOf(tab)).toBe('1');
    await page.close();
  });

  test("sends a player's media as its request went out", async () => {
    const page = await context.newPage();
    await page.goto(`${siteUrl}/embed`);
    const tab = await tabOf(page);
    await expect.poll(async () => (await listOf(tab)).map((m) => new URL(m.url).pathname)).toEqual(['/media/cors.mp4']);
    const [media] = await listOf(tab);
    const popup = await context.newPage();
    await popup.goto(`chrome-extension://${EXTENSION_ID}/popup.html`);
    const before = received.filter((r) => r.type === 'add_download').length;
    await popup.evaluate((m) => chrome.runtime.sendMessage({ type: 'download-media', media: m }), media);
    await expect.poll(() => received.filter((r) => r.type === 'add_download').length).toBe(before + 1);
    // The frame's site, as the browser sent it (cut to the origin across sites), not the page.
    expect(received.filter((r) => r.type === 'add_download').at(-1)?.payload).toMatchObject({
      url: `${siteUrl}/media/cors.mp4`,
      referer: `http://localhost:${port}/`,
      tab_url: `${siteUrl}/embed`,
      headers: { Origin: `http://localhost:${port}` },
    });
    await popup.close();
    await page.close();
  });
});
