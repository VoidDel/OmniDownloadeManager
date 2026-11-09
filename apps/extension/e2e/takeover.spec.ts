// The Chromium build of the extension, loaded unpacked, against a local site and a stand-in for
// the app's HTTP channel (the same requests the Native Messaging host relays).
//
// The main tests run a Chromium that Playwright attaches to over DevTools: when Playwright
// launches it, Playwright names downloads itself and Chromium never asks the extension
// (`onDeterminingFilename`). That situation is the last test: the extension's fallback, which
// takes the download over after it started.

import { chromium, expect, test, type Browser, type BrowserContext, type Worker } from '@playwright/test';
import { spawn, type ChildProcess } from 'node:child_process';
import fs from 'node:fs';
import http from 'node:http';
import type { AddressInfo } from 'node:net';
import os from 'node:os';
import path from 'node:path';

const EXTENSION = path.resolve(import.meta.dirname, '../.output/chrome-mv3');
/** Fixed by the manifest key (wxt.config.ts); the app's host manifests allow it. */
const EXTENSION_ID = 'nfifehjghnnmamofhbbiagiaodhfeipn';
const TOKEN = 'e2e'.padEnd(64, '0');
const MB = 1024 * 1024;

interface Received {
  type: string;
  browser?: string;
  payload?: Record<string, unknown>;
}

let site: http.Server;
let bridge: http.Server;
let siteUrl = '';
let bridgePort = 0;
const received: Received[] = [];
/** What the stand-in app answers to the next `add_download`. */
let nextAction: 'started' | 'browser' = 'started';
let unauthorized = 0;

function listen(server: http.Server): Promise<number> {
  return new Promise((resolve) => server.listen(0, '127.0.0.1', () => resolve((server.address() as AddressInfo).port)));
}

test.beforeAll(async () => {
  site = http.createServer((req, res) => {
    if (req.url === '/') {
      res.setHeader('Set-Cookie', 'session=abc123; Path=/');
      res.setHeader('Content-Type', 'text/html; charset=utf-8');
      res.end('<!doctype html><a id="big" href="/files/big.bin">big</a> <a id="small" href="/files/small.txt">small</a>');
      return;
    }
    const big = req.url === '/files/big.bin';
    if (big || req.url === '/files/small.txt') {
      const size = big ? 3 * MB : 10 * 1024;
      res.writeHead(200, {
        'Content-Type': 'application/octet-stream',
        'Content-Length': String(size),
        'Content-Disposition': `attachment; filename="${big ? 'big.bin' : 'small.txt'}"`,
      });
      res.end(Buffer.alloc(size, 7));
      return;
    }
    res.statusCode = 404;
    res.end();
  });
  siteUrl = `http://127.0.0.1:${await listen(site)}`;

  bridge = http.createServer((req, res) => {
    let body = '';
    req.on('data', (c) => (body += c));
    req.on('end', () => {
      if (req.headers.authorization !== `Bearer ${TOKEN}`) {
        unauthorized++;
        res.statusCode = 401;
        res.end(JSON.stringify({ id: '', ok: false, error: { code: 'unauthorized', message: 'no' } }));
        return;
      }
      const msg = JSON.parse(body) as Received & { id: string };
      received.push(msg);
      res.setHeader('Content-Type', 'application/json');
      if (msg.type === 'add_download') {
        res.end(JSON.stringify({ id: msg.id, ok: true, result: { action: nextAction, asked: true, task_id: nextAction === 'started' ? 'task-1' : undefined } }));
      } else if (msg.type === 'get_status') {
        res.end(JSON.stringify({ id: msg.id, ok: true, result: { app_version: '9.9.9', running: 1, download_speed: 5 * MB, recent: [{ id: 't', name: 'big.bin', status: 'downloading', progress: 0.4 }] } }));
      } else {
        res.end(JSON.stringify({ id: msg.id, ok: true, result: { app_version: '9.9.9' } }));
      }
    });
  });
  bridgePort = await listen(bridge);
});

test.afterAll(() => {
  site?.close();
  bridge?.close();
});

const adds = () => received.filter((r) => r.type === 'add_download');
const isOurs = (w: Worker) => w.url().startsWith(`chrome-extension://${EXTENSION_ID}/`);

async function extensionWorker(context: BrowserContext): Promise<Worker> {
  const w = context.serviceWorkers().find(isOurs) ?? (await context.waitForEvent('serviceworker', { predicate: isOurs }));
  // Talk to the stand-in over the HTTP channel (Native Messaging is not registered here).
  await w.evaluate(([port, token]) => chrome.storage.local.set({ pairing: { port, token }, transport: 'http' }), [bridgePort, TOKEN] as const);
  return w;
}

const downloads = (w: Worker) => w.evaluate(() => chrome.downloads.search({}).then((l) => l.map((d) => ({ name: d.filename.split(/[\\/]/).pop(), state: d.state }))));

test.describe('Chromium asks the extension', () => {
  let proc: ChildProcess;
  let browser: Browser;
  let context: BrowserContext;
  let worker: Worker;
  let downloadDir = '';

  test.beforeAll(async () => {
    const profile = fs.mkdtempSync(path.join(os.tmpdir(), 'omnidl-e2e-'));
    downloadDir = path.join(profile, 'downloads');
    fs.mkdirSync(path.join(profile, 'Default'), { recursive: true });
    fs.writeFileSync(path.join(profile, 'Default', 'Preferences'), JSON.stringify({ download: { default_directory: downloadDir, prompt_for_download: false } }));
    proc = spawn(
      chromium.executablePath(),
      ['--headless=new', '--remote-debugging-port=0', `--user-data-dir=${profile}`, `--disable-extensions-except=${EXTENSION}`, `--load-extension=${EXTENSION}`, '--no-first-run', '--no-default-browser-check', 'about:blank'],
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
    // Playwright takes downloads over when it attaches; give them back to the browser so that it
    // asks the extension, as for real users.
    const cdp = await browser.newBrowserCDPSession();
    await cdp.send('Browser.setDownloadBehavior', { behavior: 'default' });
    context = browser.contexts()[0];
    worker = await extensionWorker(context);
  });

  test.afterAll(async () => {
    await browser?.close();
    proc?.kill();
  });

  test('has the fixed extension ID', () => {
    expect(worker.url()).toContain(`chrome-extension://${EXTENSION_ID}/`);
  });

  test('hands a download over with its cookies and page, and cancels the browser download', async () => {
    nextAction = 'started';
    const page = await context.newPage();
    await page.goto(`${siteUrl}/`);
    const before = adds().length;
    await page.click('#big');
    await expect.poll(() => adds().length).toBe(before + 1);
    const msg = adds().at(-1)!;
    expect(msg.browser).toBe('chromium');
    expect(msg.payload).toMatchObject({
      url: `${siteUrl}/files/big.bin`,
      filename: 'big.bin',
      size: 3 * MB,
      referer: `${siteUrl}/`,
      cookies: 'session=abc123',
      silent: false,
    });
    expect(String(msg.payload?.user_agent)).toContain('Chrome');
    // The browser's own download is gone.
    await expect.poll(() => downloads(worker)).toEqual([]);
    await page.close();
  });

  test('lets the browser download when the user picks the browser', async () => {
    nextAction = 'browser';
    const page = await context.newPage();
    await page.goto(`${siteUrl}/`);
    const before = adds().length;
    await page.click('#big');
    await expect.poll(() => adds().length).toBe(before + 1);
    await expect.poll(() => downloads(worker), { timeout: 15_000 }).toContainEqual({ name: 'big.bin', state: 'complete' });
    expect(fs.statSync(path.join(downloadDir, 'big.bin')).size).toBe(3 * MB);
    await page.close();
  });

  test('leaves small files to the browser', async () => {
    const page = await context.newPage();
    await page.goto(`${siteUrl}/`);
    const before = adds().length;
    await page.click('#small');
    await expect.poll(() => downloads(worker), { timeout: 15_000 }).toContainEqual({ name: 'small.txt', state: 'complete' });
    expect(adds().length).toBe(before);
    await page.close();
  });

  test('shows the app status in the popup', async () => {
    const page = await context.newPage();
    await page.goto(`chrome-extension://${EXTENSION_ID}/popup.html`);
    await expect(page.locator('.pstate.ok')).toContainText('9.9.9');
    await expect(page.locator('.pline')).toContainText('1 个任务下载中');
    await expect(page.locator('.precent')).toContainText('big.bin');
    await page.close();
    expect(unauthorized).toBe(0);
  });
});

test.describe('another party names the download', () => {
  test('takes it over after it started (fallback)', async () => {
    nextAction = 'started';
    // Playwright names downloads itself: Chromium does not ask the extension.
    const context = await chromium.launchPersistentContext('', {
      channel: 'chromium',
      headless: true,
      acceptDownloads: true,
      args: [`--disable-extensions-except=${EXTENSION}`, `--load-extension=${EXTENSION}`],
    });
    try {
      const worker = await extensionWorker(context);
      const page = await context.newPage();
      await page.goto(`${siteUrl}/`);
      const before = adds().length;
      await page.click('#big');
      await expect.poll(() => adds().length).toBe(before + 1);
      expect(adds().at(-1)!.payload).toMatchObject({ url: `${siteUrl}/files/big.bin`, cookies: 'session=abc123', filename: null });
      await expect.poll(() => downloads(worker)).toEqual([]);
    } finally {
      await context.close();
    }
  });
});
