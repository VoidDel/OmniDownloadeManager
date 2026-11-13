// The desktop app end to end (docs/development-plan.md §11): a real build, its real window, a
// local web site. Windows only (see `app.ts`); skipped unless `OMNIDL_APP` names a build:
//
//   pnpm tauri build --debug --no-bundle
//   OMNIDL_APP=<…/debug/omnidl-desktop.exe> pnpm --filter omnidl-desktop e2e

import { expect, test, type Locator, type Page } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';

import { APP, launch, type App } from './app';
import { ACCOUNT, isSiteFile, startSite, type Site } from './site';

test.skip(process.platform !== 'win32' || !APP, 'set OMNIDL_APP to a built omnidl-desktop.exe (Windows)');
test.describe.configure({ mode: 'serial' });

let app: App;
let site: Site;
let page: Page;

test.beforeAll(async () => {
  site = await startSite();
  app = await launch();
  page = app.page;
});

test.afterAll(async () => {
  await app?.close();
  await site?.close();
});

/** The row of the task whose file is `name`. */
const row = (name: string): Locator => page.locator('.task-row', { has: page.locator(`.nm-line[title="${name}"]`) });
/** The names in the list, top to bottom. */
const names = () => page.locator('.task-row .nm-line').evaluateAll((els) => els.map((e) => e.getAttribute('title')));

/** Adds downloads through the new-download dialog: now, or for later (paused). */
async function add(urls: string[], start: boolean) {
  await page.locator('.cmdbar .btn').first().click();
  await page.fill('#nt-urls', urls.join('\n'));
  const button = page.locator(`.modal footer .btn.${start ? 'primary' : 'outline'}`);
  await expect(button).toBeEnabled();
  await button.click();
  // (Another dialog may open at once: a question about the download just added.)
  await expect(page.locator('#nt-urls')).toHaveCount(0);
}

test('downloads a file added in the new-download dialog', async () => {
  await page.locator('.cmdbar .btn').first().click();
  await page.fill('#nt-urls', site.url('/files/first.bin'));
  // The dialog asks the server about the link.
  await expect(page.locator('.nt-row .nt-sub')).toContainText('3 MB');
  await page.locator('.modal footer .btn.primary').click();
  await expect(row('first.bin').locator('.st')).toHaveText('Completed');
  expect(isSiteFile(path.join(app.downloads, 'first.bin'))).toBe(true);
  // Nothing of the download is left beside it.
  expect(fs.readdirSync(app.downloads)).toEqual(['first.bin']);
});

test('moves tasks in the list with the keyboard, the menu and the mouse', async () => {
  await add(['a.bin', 'b.bin', 'c.bin'].map((n) => site.url(`/files/${n}`)), false);
  await expect(page.locator('.task-row')).toHaveCount(4);
  expect(await names()).toEqual(['first.bin', 'a.bin', 'b.bin', 'c.bin']);

  // Alt+Up: one place up.
  await row('c.bin').click();
  await page.keyboard.press('Alt+ArrowUp');
  await expect.poll(names).toEqual(['first.bin', 'a.bin', 'c.bin', 'b.bin']);

  // The menu: to the top.
  await row('c.bin').click({ button: 'right' });
  await page.getByRole('menuitem', { name: 'Move to top' }).click();
  await expect.poll(names).toEqual(['c.bin', 'first.bin', 'a.bin', 'b.bin']);

  // Dragged by its handle: below the last row.
  const handle = row('c.bin').locator('.grip');
  await row('c.bin').hover();
  const from = (await handle.boundingBox())!;
  const last = (await row('b.bin').boundingBox())!;
  await page.mouse.move(from.x + from.width / 2, from.y + from.height / 2);
  await page.mouse.down();
  await page.mouse.move(from.x + 20, last.y + last.height / 2, { steps: 8 });
  await page.mouse.move(from.x + 20, last.y + last.height - 2, { steps: 4 });
  await expect(page.locator('.drop-line')).toHaveCount(1);
  await page.mouse.up();
  await expect.poll(names).toEqual(['first.bin', 'a.bin', 'b.bin', 'c.bin']);

  // A drag let go with Esc changes nothing.
  await row('a.bin').hover();
  const a = (await row('a.bin').locator('.grip').boundingBox())!;
  await page.mouse.move(a.x + a.width / 2, a.y + a.height / 2);
  await page.mouse.down();
  await page.mouse.move(a.x + 20, last.y + last.height - 2, { steps: 8 });
  await page.keyboard.press('Escape');
  await page.mouse.up();
  await expect(page.locator('.drop-line')).toHaveCount(0);
  expect(await names()).toEqual(['first.bin', 'a.bin', 'b.bin', 'c.bin']);

  // Sorted by a column, the rows cannot be moved; the third click brings the list's order back.
  await row('b.bin').click();
  await page.keyboard.press('Alt+ArrowUp');
  await expect.poll(names).toEqual(['first.bin', 'b.bin', 'a.bin', 'c.bin']);
  const byName = page.locator('.thead button').first();
  await byName.click();
  await expect.poll(names).toEqual(['a.bin', 'b.bin', 'c.bin', 'first.bin']);
  await expect(page.locator('.task-row .grip')).toHaveCount(0);
  await byName.click();
  await byName.click();
  await expect.poll(names).toEqual(['first.bin', 'b.bin', 'a.bin', 'c.bin']);

  // The app has the same order (it is the queue's order too).
  const tasks = await app.invoke<{ fileName: string }[]>('task_list');
  expect(tasks.map((t) => t.fileName)).toEqual(['first.bin', 'b.bin', 'a.bin', 'c.bin']);
});

test('asks for the account a server wants, then downloads', async () => {
  await add([site.url('/private/secret.bin')], true);
  // The download stops and the question comes up by itself.
  await expect(page.locator('#auth-user')).toBeVisible();
  await expect(row('secret.bin').locator('.st')).toHaveText('Sign-in needed');
  // A wrong password: asked again, saying so.
  await page.fill('#auth-user', ACCOUNT.user);
  await page.fill('#auth-pass', 'wrong');
  await page.keyboard.press('Enter');
  await expect(page.locator('.modal .dlg-text')).toContainText('did not accept');
  await expect(page.locator('#auth-user')).toHaveValue(ACCOUNT.user);
  await page.fill('#auth-pass', ACCOUNT.password);
  await page.locator('.modal footer .btn.primary').click();
  await expect(row('secret.bin').locator('.st')).toHaveText('Completed');
  expect(isSiteFile(path.join(app.downloads, 'secret.bin'))).toBe(true);
  expect(site.logins()).toBeGreaterThan(0);
  // The password is not part of what the window is told about the task.
  const tasks = await app.invoke<{ fileName: string; source: string; options: { username: string | null } }[]>('task_list');
  const task = tasks.find((t) => t.fileName === 'secret.bin')!;
  expect(task.options.username).toBe(ACCOUNT.user);
  expect(task.source).not.toContain(ACCOUNT.password);
});

test('asks what to do when the file name is taken', async () => {
  // Settings › Download › When the file already exists: Ask.
  const { settings } = await app.invoke<{ settings: { engine: { conflict: string } } }>('app_bootstrap');
  settings.engine.conflict = 'ask';
  await app.invoke('settings_set', { settings });
  const mine = path.join(app.downloads, 'report.bin');
  fs.writeFileSync(mine, 'mine');

  await add([site.url('/files/report.bin')], true);
  await expect(page.locator('.modal')).toContainText('report.bin');
  await expect(row('report.bin').locator('.st')).toHaveText('File exists');
  // Not now: it waits; double-clicking it asks again.
  await page.locator('.modal footer .btn.outline').first().click();
  await expect(page.locator('.modal')).toHaveCount(0);
  expect(fs.readFileSync(mine, 'utf8')).toBe('mine');
  await row('report.bin').dblclick();
  await page.locator('.modal footer .btn.primary').click();
  await expect(row('report (1).bin').locator('.st')).toHaveText('Completed');
  expect(fs.readFileSync(mine, 'utf8')).toBe('mine');
  expect(isSiteFile(path.join(app.downloads, 'report (1).bin'))).toBe(true);

  // Overwriting replaces the file when the download finishes.
  await add([site.url('/files/report.bin')], true);
  await expect(page.locator('.modal')).toContainText('report.bin');
  await page.locator('.modal footer .btn.outline').nth(1).click();
  await expect(page.locator('.task-row .st', { hasText: 'File exists' })).toHaveCount(0);
  await expect.poll(() => isSiteFile(mine)).toBe(true);
});

test('exports the task list and imports it again', async () => {
  const file = path.join(app.downloads, 'tasks.json');
  const before = await app.invoke<{ id: string; fileName: string; status: string }[]>('task_list');
  const count = await app.invoke<number>('tasks_export', { path: file, ids: null, withSecrets: false });
  expect(count).toBe(before.length);
  const text = fs.readFileSync(file, 'utf8');
  const exported = JSON.parse(text) as { app: string; tasks: { fileName: string; completed: boolean; options: Record<string, unknown> }[] };
  expect(exported.app).toBe('omni-download');
  expect(exported.tasks.map((t) => t.fileName)).toEqual(before.map((t) => t.fileName));
  // The account's name is there; its password is not.
  expect(text).toContain(`"username": "${ACCOUNT.user}"`);
  expect(text).not.toContain(ACCOUNT.password);

  // All there already: nothing is added.
  expect(await app.invoke('tasks_import', { path: file })).toEqual({ added: 0, skipped: before.length });
  // Into an empty list: everything comes back, finished downloads as finished, in the same order.
  await app.invoke('task_purge', { ids: before.map((t) => t.id), deleteFiles: false });
  await expect(page.locator('.task-row')).toHaveCount(0);
  expect(await app.invoke('tasks_import', { path: file })).toEqual({ added: before.length, skipped: 0 });
  await expect(page.locator('.task-row')).toHaveCount(before.length);
  expect(await names()).toEqual(before.map((t) => t.fileName));
  const after = await app.invoke<{ fileName: string; status: string }[]>('task_list');
  expect(after.map((t) => [t.fileName, t.status])).toEqual(before.map((t) => [t.fileName, t.status === 'completed' ? 'completed' : 'paused']));
  // A file that is not a task list is refused, in words.
  fs.writeFileSync(file, '{"hello": 1}');
  await expect(app.invoke('tasks_import', { path: file })).rejects.toMatchObject({ message: expect.stringContaining('not a task list') });
});
