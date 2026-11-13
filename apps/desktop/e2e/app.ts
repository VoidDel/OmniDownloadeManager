// Starts a build of Omni Download for a test and connects to its window. On Windows the window is
// a WebView2, which speaks the DevTools protocol when asked to: Playwright drives the real app,
// commands and all, as it drives a browser.
//
// The app keeps everything in a folder of the test's own (`OMNIDL_DATA_DIR`) and listens for
// browsers on a pipe of its own (`OMNIDL_IPC_ENDPOINT`, so it registers nothing with them): it
// does not touch an installed Omni Download's tasks or settings. An installed copy with the same
// identifier must not be running, though (the second start would hand over to it): build the
// test copy under another identifier when it is, as CONTRIBUTING.md says.

import { chromium, type Browser, type Page } from '@playwright/test';
import { execFileSync, spawn } from 'node:child_process';
import fs from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';

/** The app to test: `omnidl-desktop.exe` of `tauri build --debug --no-bundle` (a debug build: only
 *  those open WebView2's DevTools port). */
export const APP = process.env.OMNIDL_APP ?? '';

export interface App {
  /** The main window. */
  page: Page;
  /** Where the app saves downloads. */
  downloads: string;
  /** Calls a command of the app, as its window does. */
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  close(): Promise<void>;
}

function freePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const s = net.createServer();
    s.once('error', reject);
    s.listen(0, '127.0.0.1', () => {
      const { port } = s.address() as net.AddressInfo;
      s.close(() => resolve(port));
    });
  });
}

/** A path as a TOML literal string. */
const toml = (p: string) => `'${p}'`;

/** Starts the app with fresh data; `engine` adds lines to the `[engine]` table of its settings. */
export async function launch(engine = ''): Promise<App> {
  const work = fs.mkdtempSync(path.join(os.tmpdir(), 'omnidl-e2e-'));
  const data = path.join(work, 'data');
  const downloads = path.join(work, 'downloads');
  fs.mkdirSync(data);
  fs.mkdirSync(downloads);
  // English, so texts the tests look for are the same everywhere; nothing that reaches outside
  // the test (updates, notifications, sounds, the system proxy).
  fs.writeFileSync(
    path.join(data, 'settings.toml'),
    [
      'language = "en-US"',
      `downloadDir = ${toml(downloads)}`,
      '[general]',
      'onboarded = true',
      'checkUpdates = false',
      'closeToTray = false',
      'notifyDone = false',
      'notifyFailed = false',
      'soundDone = false',
      '[engine]',
      engine,
      '[engine.proxy]',
      'mode = "none"',
      '',
    ].join('\n'),
  );
  const port = await freePort();
  // What the app prints, for when it does not come up.
  const output = fs.openSync(path.join(work, 'output.txt'), 'w');
  const child = spawn(APP, [], {
    stdio: ['ignore', output, output],
    env: {
      ...process.env,
      OMNIDL_DATA_DIR: data,
      OMNIDL_IPC_ENDPOINT: `\\\\.\\pipe\\omnidl-e2e-${process.pid}-${port}`,
      WEBVIEW2_USER_DATA_FOLDER: path.join(work, 'webview'),
      // A debug build opens WebView2's DevTools port when asked (`with_devtools` in lib.rs).
      OMNIDL_DEVTOOLS_PORT: String(port),
    },
  });
  let browser: Browser | undefined;
  const close = async () => {
    await browser?.close().catch(() => undefined);
    // With its WebView2 processes: left behind, they keep the folder busy.
    if (child.pid && child.exitCode === null) {
      try {
        execFileSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore' });
      } catch {
        // Gone already.
      }
    }
    fs.rmSync(work, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
  };
  /** What can be learned of a start that went wrong: the DevTools port's pages, the app's log. */
  const why = async () => {
    const lines: string[] = [];
    try {
      const r = await fetch(`http://127.0.0.1:${port}/json/list`);
      const pages = (await r.json()) as { type: string; url: string; title: string }[];
      lines.push(`DevTools port: ${pages.map((p) => `${p.type} ${p.url} "${p.title}"`).join('; ') || 'no pages'}`);
    } catch (e) {
      lines.push(`DevTools port ${port}: ${e instanceof Error ? e.message : e}`);
    }
    const tail = (file: string) => (fs.existsSync(file) ? fs.readFileSync(file, 'utf8').split(/\r?\n/).slice(-40).join('\n') : '');
    const logs = path.join(data, 'logs');
    for (const f of fs.existsSync(logs) ? fs.readdirSync(logs) : []) lines.push(`--- ${f}\n${tail(path.join(logs, f))}`);
    lines.push(`--- output\n${tail(path.join(work, 'output.txt'))}`);
    // WebView2: its processes (the browser's with its arguments) and its version.
    const ps = (script: string) => {
      try {
        return execFileSync('powershell', ['-NoProfile', '-Command', script], { encoding: 'utf8', timeout: 20_000 }).trim();
      } catch (e) {
        return `(${e instanceof Error ? e.message.split('\n')[0] : e})`;
      }
    };
    lines.push(`--- WebView2 processes\n${ps("Get-CimInstance Win32_Process -Filter \"Name='msedgewebview2.exe'\" | ForEach-Object { $_.ProcessId.ToString() + ' ' + $(if ($_.CommandLine -notmatch '--type=') { $_.CommandLine } else { $_.CommandLine.Substring(0, [Math]::Min(160, $_.CommandLine.Length)) }) }") || 'none'}`);
    lines.push(`--- WebView2 runtime\n${ps("foreach ($k in 'HKLM:\\SOFTWARE\\WOW6432Node\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}','HKCU:\\Software\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}') { if (Test-Path $k) { $k + ' ' + (Get-ItemProperty $k).pv } }") || 'not found'}`);
    const webview = path.join(work, 'webview');
    const files = fs.existsSync(webview) ? fs.readdirSync(webview, { recursive: true }).map(String) : [];
    lines.push(`--- WebView2 folder: ${files.length} entries ${files.slice(0, 15).join(', ')}`);
    return lines.join('\n');
  };
  try {
    let page: Page | undefined;
    const deadline = Date.now() + 60_000;
    while (!page) {
      if (child.exitCode !== null) throw new Error(`the app exited with code ${child.exitCode}\n${await why()}`);
      if (Date.now() > deadline) throw new Error(`the app window did not open\n${await why()}`);
      try {
        browser ??= await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
        for (const p of browser.contexts().flatMap((c) => c.pages())) {
          if (await p.locator('.cmdbar').count()) page = p;
        }
      } catch {
        // Not up yet.
      }
      if (!page) await new Promise((r) => setTimeout(r, 250));
    }
    const main = page;
    return {
      page: main,
      downloads,
      invoke: async (command, args = {}) => {
        // A command fails with `{ code, message }`, which is not an `Error`: it is carried out of
        // the page as a value and thrown here.
        const r = await main.evaluate(
          ([c, a]) =>
            (window as unknown as { __TAURI_INTERNALS__: { invoke(c: string, a: unknown): Promise<unknown> } }).__TAURI_INTERNALS__.invoke(c as string, a).then(
              (ok) => ({ ok }),
              (failed) => ({ failed: failed as { code?: string; message?: string } }),
            ),
          [command, args] as const,
        );
        if ('failed' in r) throw Object.assign(new Error(r.failed?.message ?? String(r.failed)), { code: r.failed?.code });
        return r.ok as never;
      },
      close,
    };
  } catch (e) {
    await close();
    throw e;
  }
}
