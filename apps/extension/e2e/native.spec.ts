// The whole chain on Windows with a real Omni Download build (opt-in, Phase 2 acceptance):
// Edge with the extension → Native Messaging → omnidl-nm-host → the app's pipe → the
// quick-download window → a download that needs the site's login cookie.
//
//   OMNIDL_APP=<path to omnidl-desktop.exe, with omnidl-nm-host.exe next to it> pnpm e2e native
//
// The app must not be running: the host has to start it. For the test's duration Edge's host
// registration (HKCU) points at that build; the previous value is restored afterwards. The app
// and Edge talk over a private pipe name, so another running Omni Download is not involved.

import { chromium, expect, test, type Browser, type Page } from '@playwright/test';
import { execFileSync, spawn } from 'node:child_process';
import fs from 'node:fs';
import http from 'node:http';
import type { AddressInfo } from 'node:net';
import os from 'node:os';
import path from 'node:path';

const APP = process.env.OMNIDL_APP ?? '';
const EDGE = process.env.OMNIDL_EDGE ?? 'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe';
const EXTENSION = path.resolve(import.meta.dirname, '../.output/chrome-mv3');
const EXTENSION_ID = 'nfifehjghnnmamofhbbiagiaodhfeipn';
const REG_KEY = 'HKCU\\Software\\Microsoft\\Edge\\NativeMessagingHosts\\app.omnidownload.host';
const SIZE = 40 * 1024 * 1024;

const byteAt = (i: number) => (i * 31 + (i >> 8)) & 0xff;

test.skip(process.platform !== 'win32' || !APP, 'set OMNIDL_APP to a built omnidl-desktop.exe (Windows)');

/** The site: a page that logs you in (cookie) and a file only logged-in visitors get, with ranges. */
function startSite(): Promise<http.Server> {
  const server = http.createServer((req, res) => {
    if (req.url === '/') {
      res.setHeader('Set-Cookie', 'session=secret; Path=/');
      res.setHeader('Content-Type', 'text/html; charset=utf-8');
      res.end('<!doctype html><a id="file" href="/protected/data.bin">data</a>');
      return;
    }
    if (req.url !== '/protected/data.bin') {
      res.statusCode = 404;
      res.end();
      return;
    }
    if (!/(^|;\s*)session=secret(;|$)/.test(req.headers.cookie ?? '')) {
      res.statusCode = 403;
      res.end('log in first');
      return;
    }
    const m = /^bytes=(\d+)-(\d*)$/.exec(req.headers.range ?? '');
    const start = m ? Number(m[1]) : 0;
    const end = m && m[2] ? Math.min(Number(m[2]), SIZE - 1) : SIZE - 1;
    res.writeHead(m ? 206 : 200, {
      'Content-Type': 'application/octet-stream',
      'Content-Length': String(end - start + 1),
      'Content-Disposition': 'attachment; filename="data.bin"',
      'Accept-Ranges': 'bytes',
      ETag: '"v1"',
      ...(m ? { 'Content-Range': `bytes ${start}-${end}/${SIZE}` } : {}),
    });
    if (req.method === 'HEAD') {
      res.end();
      return;
    }
    let pos = start;
    const pump = () => {
      while (pos <= end) {
        const n = Math.min(256 * 1024, end - pos + 1);
        const chunk = Buffer.alloc(n);
        for (let i = 0; i < n; i++) chunk[i] = byteAt(pos + i);
        pos += n;
        if (!res.write(chunk)) {
          res.once('drain', pump);
          return;
        }
      }
      res.end();
    };
    pump();
  });
  return new Promise((resolve) => server.listen(0, '127.0.0.1', () => resolve(server)));
}

function regQuery(): string | null {
  try {
    const out = execFileSync('reg', ['query', REG_KEY, '/ve'], { encoding: 'utf8' });
    return /REG_SZ\s+(.+)$/m.exec(out)?.[1]?.trim() ?? null;
  } catch {
    return null;
  }
}

function appProcesses(): number[] {
  const out = execFileSync('powershell', ['-NoProfile', '-Command', `Get-Process omnidl-desktop -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq '${APP.replace(/'/g, "''")}' } | ForEach-Object { $_.Id }; exit 0`], { encoding: 'utf8' });
  return out.split(/\s+/).filter(Boolean).map(Number);
}

/** The DevTools address of the Chromium using `profile` (msedge.exe hands over to another process,
 *  so the port is read from the profile rather than from its output). */
async function devtools(profile: string): Promise<string> {
  const file = path.join(profile, 'DevToolsActivePort');
  for (let i = 0; i < 300; i++) {
    const port = fs.existsSync(file) ? fs.readFileSync(file, 'utf8').split(/\r?\n/)[0].trim() : '';
    if (port) return `http://127.0.0.1:${port}`;
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error('Edge did not start');
}

/** Stops the processes whose command line mentions `marker` (Edge's, by its profile folder). */
function killByCommandLine(marker: string) {
  execFileSync('powershell', ['-NoProfile', '-Command', `Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -like '*${marker.replace(/'/g, "''")}*' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }; exit 0`]);
}

/** Undone after the test, also when it times out. */
const cleanup: (() => unknown)[] = [];
test.afterEach(async () => {
  for (const f of cleanup.splice(0).reverse()) {
    try {
      await f();
    } catch {
      // keep going
    }
  }
});

test('Edge hands a logged-in download to Omni Download, which it starts', async () => {
  test.setTimeout(180_000);
  expect(fs.existsSync(path.join(path.dirname(APP), 'omnidl-nm-host.exe')), 'omnidl-nm-host.exe next to the app').toBe(true);
  expect(appProcesses(), 'the app must not be running').toEqual([]);

  const work = fs.mkdtempSync(path.join(os.tmpdir(), 'omnidl-native-'));
  const saveDir = path.join(work, 'saved');
  fs.mkdirSync(saveDir);
  // Edge's host registration → this build's host.
  const manifest = path.join(work, 'host.json');
  fs.writeFileSync(
    manifest,
    JSON.stringify({ name: 'app.omnidownload.host', description: 'e2e', path: path.join(path.dirname(APP), 'omnidl-nm-host.exe'), type: 'stdio', allowed_origins: [`chrome-extension://${EXTENSION_ID}/`] }),
  );
  const previous = regQuery();
  execFileSync('reg', ['add', REG_KEY, '/ve', '/d', manifest, '/f']);
  cleanup.push(() => (previous ? execFileSync('reg', ['add', REG_KEY, '/ve', '/d', previous, '/f']) : execFileSync('reg', ['delete', REG_KEY, '/f'])));

  const site = await startSite();
  cleanup.push(() => site.close());
  const siteUrl = `http://127.0.0.1:${(site.address() as AddressInfo).port}`;
  const appDebugPort = 9400 + (process.pid % 500);
  const env = {
    ...process.env,
    // Inherited by the host Edge starts and by the app the host starts.
    OMNIDL_IPC_ENDPOINT: `\\\\.\\pipe\\omnidl-e2e-${process.pid}`,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${appDebugPort}`,
  };
  const profile = path.join(work, 'edge');
  fs.mkdirSync(path.join(profile, 'Default'), { recursive: true });
  fs.writeFileSync(path.join(profile, 'Default', 'Preferences'), JSON.stringify({ download: { default_directory: path.join(work, 'edge-downloads'), prompt_for_download: false } }));
  spawn(EDGE, ['--headless=new', '--remote-debugging-port=0', `--user-data-dir=${profile}`, `--disable-extensions-except=${EXTENSION}`, `--load-extension=${EXTENSION}`, '--no-first-run', '--no-default-browser-check', 'about:blank'], { env, stdio: 'ignore' });
  cleanup.push(() => killByCommandLine(profile));
  cleanup.push(() => {
    // With its WebView2 processes: left running, they keep the profile busy for the next start.
    for (const pid of appProcesses()) execFileSync('taskkill', ['/PID', String(pid), '/T', '/F']);
  });
  {
    const browser = await chromium.connectOverCDP(await devtools(profile));
    cleanup.push(() => browser.close());
    let app: Browser | undefined;
    cleanup.push(() => app?.close());
    const cdp = await browser.newBrowserCDPSession();
    await cdp.send('Browser.setDownloadBehavior', { behavior: 'default' });
    const context = browser.contexts()[0];
    const isOurs = (w: { url(): string }) => w.url().startsWith(`chrome-extension://${EXTENSION_ID}/`);
    const worker = context.serviceWorkers().find(isOurs) ?? (await context.waitForEvent('serviceworker', { predicate: isOurs, timeout: 20_000 }));
    await worker.evaluate(() => chrome.storage.local.set({ transport: 'native', pairing: null }));

    const page = await context.newPage();
    await page.goto(`${siteUrl}/`);
    await page.click('#file');

    // The host starts the app in the background; its quick-download window asks.
    let quick: Page | undefined;
    await expect
      .poll(
        async () => {
          try {
            app ??= await chromium.connectOverCDP(`http://127.0.0.1:${appDebugPort}`);
            for (const p of app.contexts().flatMap((c) => c.pages())) {
              if (await p.locator('.quick footer .btn.primary').count()) quick = p;
            }
          } catch {
            // not up yet
          }
          return !!quick;
        },
        { timeout: 60_000 },
      )
      .toBe(true);
    expect(appProcesses().length).toBe(1);
    await expect(quick!.locator('#qa-name')).toHaveValue('data.bin');
    await expect(quick!.locator('.q-info')).toContainText('40 MB');
    await expect(quick!.locator('.q-info')).toContainText('已附带 Cookie 与 Referer');
    await expect(quick!.locator('.q-info')).toContainText('支持断点续传');
    await quick!.fill('#qa-dir', saveDir);
    await quick!.screenshot({ path: test.info().outputPath('quick-download.png') });
    await quick!.click('.quick footer .btn.primary');

    // Edge dropped its copy; the app downloaded the file with the cookie.
    await expect.poll(() => worker.evaluate(() => chrome.downloads.search({}).then((l) => l.length))).toBe(0);
    const target = path.join(saveDir, 'data.bin');
    await expect.poll(() => fs.existsSync(target), { timeout: 60_000 }).toBe(true);
    const data = fs.readFileSync(target);
    expect(data.length).toBe(SIZE);
    for (let i = 0; i < SIZE; i += 4093) expect(data[i]).toBe(byteAt(i));
  }
});
