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

/** The app to test: `omnidl-desktop.exe` of a `tauri build` (`--debug --no-bundle` is enough). */
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
  const child = spawn(APP, [], {
    stdio: 'ignore',
    env: {
      ...process.env,
      OMNIDL_DATA_DIR: data,
      OMNIDL_IPC_ENDPOINT: `\\\\.\\pipe\\omnidl-e2e-${process.pid}-${port}`,
      WEBVIEW2_USER_DATA_FOLDER: path.join(work, 'webview'),
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`,
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
  try {
    let page: Page | undefined;
    const deadline = Date.now() + 60_000;
    while (!page) {
      if (child.exitCode !== null) throw new Error(`the app exited with code ${child.exitCode}`);
      if (Date.now() > deadline) throw new Error('the app window did not open');
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
