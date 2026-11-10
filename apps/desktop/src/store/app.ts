// Application state (zustand). Tasks are kept by id; each row subscribes to its own task, so a
// progress tick re-renders only the rows whose numbers changed. Lists (filtered, sorted) are
// derived where they are shown.

import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type { Platform, ToastItem, ToastTone } from 'omni-ui';
import { create } from 'zustand';

import { api, asError, listenApp, listenTasks } from '../lib/api';
import i18n, { initI18n, resolveLang, setLanguage } from '../i18n';
import { categoryOf } from '../lib/category';
import { applyTheme, setSystemLook } from '../lib/theme';
import type { AppError, Category, Prefill, Settings, Stats, Task, TaskStatus, UpdateInfo, UpdateProgress } from '../lib/types';
import { errorText } from '../lib/words';

export type Filter = 'all' | 'active' | 'done' | 'torrent' | 'failed' | 'trash' | `cat:${Category}`;
export type SortKey = 'added' | 'name' | 'size' | 'progress' | 'speed' | 'status';
export type SettingsTab = 'general' | 'appearance' | 'download' | 'network' | 'torrent' | 'browser';
export type View = 'tasks' | 'settings';

export type Dialog =
  /** New downloads, with the text pasted so far (and request details from a browser batch). */
  | ({ kind: 'new' } & Prefill)
  /** Delete for good, optionally with the files. */
  | { kind: 'purge'; ids: string[] }
  /** A custom global speed limit. */
  | { kind: 'speed' }
  /** A newer version: what is new, then installing it. */
  | { kind: 'update' }
  /** The first-run guide. */
  | { kind: 'welcome' }
  | null;

export const DEFAULT_SETTINGS: Settings = {
  language: 'auto',
  general: { closeToTray: true, notifyDone: true, notifyFailed: true, shortcutShow: '', shortcutNew: '', checkUpdates: true, onboarded: true, soundDone: true, soundFile: '' },
  appearance: { mode: 'system', lightTheme: 'aurora', darkTheme: 'midnight', density: 'comfortable', material: true, materialKind: 'mica', accent: 'theme' },
  downloadDir: '',
  categoryDirs: {},
  engine: {
    maxConcurrent: 3,
    connections: 8,
    speedLimit: null,
    userAgent: '',
    proxy: { mode: 'system' },
    connectTimeoutSecs: 15,
    readTimeoutSecs: 30,
    maxRetries: 10,
    conflict: 'rename',
    resumeOnStartup: true,
    syncWrites: true,
    seeding: true,
    seedRatio: 1.5,
    seedMinutes: 1440,
    uploadLimit: null,
    btPort: 6881,
    btDht: true,
    btUpnp: true,
    ftpConnections: 2,
    ftpActive: false,
  },
  browser: { quickDialog: true, silentSites: [], httpBridge: false, httpPort: 0, httpToken: '' },
};

interface State {
  ready: boolean;
  platform: Platform;
  version: string;
  defaultDir: string;
  logDir: string;
  settings: Settings;
  tasks: Record<string, Task>;
  stats: Stats;
  view: View;
  settingsTab: SettingsTab;
  filter: Filter;
  /** The title bar's search: narrows the list within the filter (see `matches`). */
  query: string;
  sort: { key: SortKey; desc: boolean };
  selection: string[];
  /** Where a shift-click range starts. */
  anchor: string | null;
  /** The task the list should scroll to; `n` makes each request a new one. */
  reveal: { id: string; n: number } | null;
  /** The details panel opens for every selected task (the switch in its header). */
  detailPinned: boolean;
  /** The task the panel was opened for once (command bar, menu); selecting anything else (or
   *  clicking empty space) closes it for good. */
  detailFor: string | null;
  dialog: Dialog;
  toasts: ToastItem[];
  /** Browsers whose extension has been in touch since the app started: [name, Unix ms]. */
  seenBrowsers: [string, number][];
  /** Window materials exist on this system. */
  materialSupported: boolean;
  /** The system's accent colour, when known. */
  systemAccent: string | null;
  /** The app starts with the system. */
  autostart: boolean;
  /** The newer version found, and its package downloading (null before and after). */
  update: UpdateInfo | null;
  updateProgress: UpdateProgress | null;
  /** Finished downloads whose file was deleted or moved (checked at start and when the window
   *  comes back to the front). */
  missing: Record<string, true>;

  /** Loads settings and tasks and listens for changes. Resolves to whether the app was started
   *  in the background by the browser bridge (the main window then stays hidden). */
  boot: (platform: Platform) => Promise<boolean>;
  setView: (v: View) => void;
  setSettingsTab: (t: SettingsTab) => void;
  setFilter: (f: Filter) => void;
  /** Searches the list (shown, if the settings were). The selection keeps only what still shows. */
  setQuery: (q: string) => void;
  setSort: (key: SortKey) => void;
  select: (ids: string[], anchor?: string | null) => void;
  /** Moves tasks to the recycle bin and says so, with a way back. */
  removeTasks: (ids: string[]) => Promise<void>;
  /** Scrolls the list to a task (one just added). */
  revealTask: (id: string) => void;
  /** Shows the panel for task `id` this once. */
  openDetail: (id: string) => void;
  /** Hides the panel; pinned, it also stops opening for every task (or it would come straight back). */
  closeDetail: () => void;
  setDetailPinned: (pinned: boolean) => void;
  openDialog: (d: Dialog) => void;
  closeDialog: () => void;
  toast: (tone: ToastTone, text: string, action?: ToastItem['action']) => void;
  dismiss: (id: number) => void;
  /** Runs a command; a failure becomes an error toast. Resolves to undefined on failure. */
  run: <T>(op: () => Promise<T>) => Promise<T | undefined>;
  setAutostart: (enabled: boolean) => Promise<void>;
  /** Looks for a newer version; `manual` (the settings button) also says when there is none. */
  checkUpdate: (manual: boolean) => Promise<void>;
  /** Looks for finished downloads whose file is gone. */
  checkFiles: () => Promise<void>;
  /** Opens a finished download; a file that is gone is marked and offered for downloading again. */
  openTask: (id: string) => Promise<void>;
  /** Changes and saves the settings (the window applies them at once). */
  saveSettings: (change: (s: Settings) => void) => Promise<void>;
  /** The folder new downloads go to. */
  downloadDir: () => string;
  /** The folder a new download of `fileName` goes to: its category's, else the download folder. */
  dirFor: (fileName: string) => string;
}

/** Download speeds of each running task, 4 samples a second for the last two minutes. */
const history = new Map<string, number[]>();
const HISTORY = 480;

export function speedHistory(id: string): readonly number[] {
  return history.get(id) ?? [];
}

let toastId = 0;

// The panel switch is this viewer's preference: kept in the page's storage, which may be absent.
const PINNED_KEY = 'omnidl.detailPinned';

function readPinned(): boolean {
  try {
    return localStorage.getItem(PINNED_KEY) === '1';
  } catch {
    return false;
  }
}

function savePinned(pinned: boolean) {
  try {
    localStorage.setItem(PINNED_KEY, pinned ? '1' : '0');
  } catch {
    // Not remembered; it still applies now.
  }
}

/** The task the details panel shows, if it is shown. */
export function detailTask(s: Pick<State, 'selection' | 'detailPinned' | 'detailFor'>): string | null {
  const single = s.selection.length === 1 ? s.selection[0] : null;
  return single && (s.detailPinned || s.detailFor === single) ? single : null;
}

const withTask = (tasks: Record<string, Task>, t: Task) => ({ ...tasks, [t.id]: t });

export const useApp = create<State>((set, get) => ({
  ready: false,
  platform: 'win',
  version: '',
  defaultDir: '',
  logDir: '',
  settings: DEFAULT_SETTINGS,
  tasks: {},
  stats: { downloadSpeed: 0, uploadSpeed: 0, running: 0, seeding: 0 },
  view: 'tasks',
  settingsTab: 'general',
  filter: 'all',
  query: '',
  sort: { key: 'added', desc: false },
  selection: [],
  anchor: null,
  reveal: null,
  detailPinned: readPinned(),
  detailFor: null,
  dialog: null,
  toasts: [],
  seenBrowsers: [],
  materialSupported: false,
  systemAccent: null,
  autostart: false,
  update: null,
  updateProgress: null,
  missing: {},

  boot: async (platform) => {
    set({ platform });
    let background = false;
    try {
      await listenApp({
        newTask: (p) => set({ dialog: { kind: 'new', ...p }, view: 'tasks' }),
        browserSeen: (seenBrowsers) => set({ seenBrowsers }),
        settingsChanged: (settings) => set({ settings }),
        updateProgress: (updateProgress) => set({ updateProgress }),
      });
      // Listen first: nothing that happens while the list loads is missed.
      await listenTasks({
        added: (t) => set((s) => ({ tasks: withTask(s.tasks, t) })),
        changed: (t) =>
          set((s) => {
            if (!s.tasks[t.id]) return {};
            if (!['probing', 'downloading'].includes(t.status)) history.delete(t.id);
            // Downloading it again brings the file back.
            if (s.missing[t.id] && t.status !== 'completed') {
              const missing = { ...s.missing };
              delete missing[t.id];
              return { tasks: withTask(s.tasks, t), missing };
            }
            return { tasks: withTask(s.tasks, t) };
          }),
        deleted: (ids) =>
          set((s) => {
            const tasks = { ...s.tasks };
            for (const id of ids) {
              delete tasks[id];
              history.delete(id);
            }
            const gone = new Set(ids);
            return { tasks, selection: s.selection.filter((id) => !gone.has(id)) };
          }),
        progress: ({ items, stats }) =>
          set((s) => {
            const tasks = { ...s.tasks };
            for (const p of items) {
              const t = tasks[p.id];
              if (!t) continue;
              tasks[p.id] = {
                ...t,
                downloadedBytes: p.downloaded,
                totalBytes: p.total,
                speedBps: p.speed,
                etaSecs: p.eta,
                connections: p.connections,
                uploadedBytes: p.uploaded,
                uploadSpeedBps: p.uploadSpeed,
              };
              const h = history.get(p.id) ?? [];
              h.push(p.speed);
              if (h.length > HISTORY) h.splice(0, h.length - HISTORY);
              history.set(p.id, h);
            }
            return { tasks, stats };
          }),
        resync: () => {
          void api.listTasks().then((list) => set({ tasks: Object.fromEntries(list.map((t) => [t.id, t])) }));
        },
      });
      const boot = await api.bootstrap();
      set({
        version: boot.version,
        defaultDir: boot.defaultDir,
        logDir: boot.logDir,
        settings: boot.settings,
        tasks: Object.fromEntries(boot.tasks.map((t) => [t.id, t])),
        seenBrowsers: boot.seenBrowsers,
        materialSupported: boot.materialSupported,
        systemAccent: boot.systemAccent,
        autostart: boot.autostart,
      });
      setSystemLook(boot);
      // Opened with magnet links or .torrent files.
      if (boot.newTask) set({ dialog: { kind: 'new', ...boot.newTask, text: [boot.newTask.text, ...boot.openLinks].filter(Boolean).join('\n') } });
      else if (boot.openLinks.length > 0) set({ dialog: { kind: 'new', text: boot.openLinks.join('\n') } });
      background = boot.background;
      void get().checkFiles();
      if (isTauri() && getCurrentWindow().label === 'main') {
        // First run: the guide (language, folder, browsers) before anything else.
        if (!boot.settings.general.onboarded && !get().dialog) set({ dialog: { kind: 'welcome' } });
        // Not while the app is starting up: a little later, and quietly unless there is news.
        // Development builds don't: the release they'd find is not what they run. Once per run,
        // not each time the window comes back from the notification area.
        if (import.meta.env.PROD && boot.first && boot.settings.general.checkUpdates) setTimeout(() => void get().checkUpdate(false), 8000);
      }
    } catch (e) {
      // Outside the app (the page in a browser) or a failed start: an empty list.
      console.error('bootstrap', e);
    }
    await initI18n(resolveLang(get().settings.language));
    applyTheme(get().settings.appearance, platform);
    set({ ready: true });
    return background;
  },

  setView: (view) => set({ view }),
  setSettingsTab: (settingsTab) => set({ settingsTab, view: 'settings' }),
  setFilter: (filter) => set({ filter, view: 'tasks', selection: [], anchor: null }),
  setQuery: (query) =>
    set((s) => {
      const keep = s.selection.filter((id) => s.tasks[id] && matches(s.tasks[id], query));
      return {
        query,
        view: query.trim() ? 'tasks' : s.view,
        selection: keep,
        anchor: keep.includes(s.anchor ?? '') ? s.anchor : null,
        detailFor: s.detailFor && keep.includes(s.detailFor) ? s.detailFor : null,
      };
    }),
  setSort: (key) => set((s) => ({ sort: { key, desc: s.sort.key === key ? !s.sort.desc : key === 'speed' || key === 'size' } })),
  select: (selection, anchor) =>
    set((s) => ({
      selection,
      anchor: anchor === undefined ? s.anchor : anchor,
      detailFor: selection.length === 1 && selection[0] === s.detailFor ? s.detailFor : null,
    })),
  removeTasks: async (ids) => {
    if (!ids.length) return;
    await get().run(async () => {
      await api.remove(ids);
      get().toast('info', i18n.t('shell.removed', { count: ids.length }), {
        label: i18n.t('shell.undo'),
        run: () => void get().run(() => api.restore(ids)),
      });
    });
  },
  revealTask: (id) => set((s) => ({ reveal: { id, n: (s.reveal?.n ?? 0) + 1 } })),
  openDetail: (detailFor) => set({ detailFor }),
  closeDetail: () => {
    savePinned(false);
    set({ detailFor: null, detailPinned: false });
  },
  setDetailPinned: (detailPinned) => {
    savePinned(detailPinned);
    // Unpinned, the panel stays for the task it shows now.
    set((s) => ({ detailPinned, detailFor: detailPinned ? s.detailFor : (s.selection.length === 1 ? s.selection[0] : null) }));
  },
  openDialog: (dialog) => set({ dialog }),
  closeDialog: () => set({ dialog: null }),

  toast: (tone, text, action) => {
    const id = ++toastId;
    set((s) => ({ toasts: [...s.toasts.slice(-3), { id, tone, text, action }] }));
    // Errors, and anything with a button to press, stay longer.
    setTimeout(() => get().dismiss(id), tone === 'error' || action ? 8000 : 4000);
  },
  dismiss: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),

  run: async (op) => {
    try {
      return await op();
    } catch (e) {
      const err: AppError = asError(e);
      get().toast('error', errorText(err));
      console.error(err);
      return undefined;
    }
  },

  setAutostart: async (enabled) => {
    const now = await get().run(() => api.setAutostart(enabled));
    if (now !== undefined) set({ autostart: now });
  },

  checkUpdate: async (manual) => {
    let found: UpdateInfo | null;
    try {
      found = await api.updateCheck();
    } catch (e) {
      // Offline, or no release yet: only an asked-for check says so (the details are in the log).
      console.warn('update check', asError(e).message);
      if (manual) get().toast('error', i18n.t('update.checkFailed'));
      return;
    }
    set({ update: found });
    if (!found) {
      if (manual) get().toast('success', i18n.t('update.none'));
    } else if (manual) {
      set({ dialog: { kind: 'update' } });
    } else {
      get().toast('info', i18n.t('update.available', { version: found.version }), {
        label: i18n.t('update.show'),
        run: () => set({ dialog: { kind: 'update' } }),
      });
    }
  },

  checkFiles: async () => {
    try {
      const ids = await api.missingFiles();
      set({ missing: Object.fromEntries(ids.map((id) => [id, true as const])) });
    } catch (e) {
      console.error('missing files', e);
    }
  },

  openTask: async (id) => {
    try {
      await api.open(id);
    } catch (e) {
      const err: AppError = asError(e);
      if (err.code !== 'file_missing') {
        get().toast('error', errorText(err));
        console.error(err);
        return;
      }
      set((s) => ({ missing: { ...s.missing, [id]: true } }));
      get().toast('error', i18n.t('errors.fileGone'), {
        label: i18n.t('table.menu.redownload'),
        run: () => void get().run(() => api.restart([id])),
      });
    }
  },

  saveSettings: async (change) => {
    const before = get().settings;
    const next: Settings = structuredClone(before);
    change(next);
    set({ settings: next });
    if (next.appearance !== before.appearance && JSON.stringify(next.appearance) !== JSON.stringify(before.appearance)) {
      applyTheme(next.appearance, get().platform, true);
    }
    if (next.language !== before.language) void setLanguage(resolveLang(next.language));
    const saved = await get().run(() => api.setSettings(next));
    if (saved) set({ settings: saved });
    else {
      set({ settings: before });
      applyTheme(before.appearance, get().platform);
      void setLanguage(resolveLang(before.language));
    }
  },

  downloadDir: () => get().settings.downloadDir || get().defaultDir,
  dirFor: (fileName) => get().settings.categoryDirs?.[categoryOf(fileName)]?.trim() || get().downloadDir(),
}));

const ACTIVE: TaskStatus[] = ['queued', 'probing', 'downloading', 'paused'];

/** Whether a task shows under a sidebar entry. */
export function inFilter(t: Task, f: Filter): boolean {
  if (f === 'trash') return t.status === 'removed';
  if (t.status === 'removed') return false;
  switch (f) {
    case 'all':
      return true;
    case 'active':
      return ACTIVE.includes(t.status);
    case 'done':
      return t.status === 'completed' || t.status === 'seeding';
    case 'failed':
      return t.status === 'failed';
    case 'torrent':
      return t.kind === 'torrent';
    default:
      return t.category === f.slice(4);
  }
}

const STATUS_ORDER: TaskStatus[] = ['downloading', 'probing', 'queued', 'paused', 'failed', 'seeding', 'completed', 'removed'];

/** Whether a task matches the search: every word of `query` (any case) is in its name, its link
 *  or its folder. An empty query matches everything. */
export function matches(t: Task, query: string): boolean {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return true;
  const hay = `${t.fileName}\n${t.source}\n${t.finalUrl ?? ''}\n${t.saveDir}`.toLowerCase();
  return words.every((w) => hay.includes(w));
}

/** Ids of the tasks under `filter` that match `query`, sorted. */
export function visibleIds(tasks: Record<string, Task>, filter: Filter, sort: State['sort'], query = ''): string[] {
  const list = Object.values(tasks).filter((t) => inFilter(t, filter) && matches(t, query));
  const by: Record<SortKey, (a: Task, b: Task) => number> = {
    added: (a, b) => a.createdAt - b.createdAt,
    name: (a, b) => a.fileName.localeCompare(b.fileName, 'zh-CN', { numeric: true }),
    size: (a, b) => (a.totalBytes ?? -1) - (b.totalBytes ?? -1),
    progress: (a, b) => progressOf(a) - progressOf(b),
    speed: (a, b) => a.speedBps - b.speedBps,
    status: (a, b) => STATUS_ORDER.indexOf(a.status) - STATUS_ORDER.indexOf(b.status),
  };
  const cmp = by[sort.key];
  list.sort((a, b) => (sort.desc ? -cmp(a, b) : cmp(a, b)) || a.createdAt - b.createdAt);
  return list.map((t) => t.id);
}

export function progressOf(t: Task): number {
  if (t.status === 'completed' || t.status === 'seeding') return 1;
  return t.totalBytes ? Math.min(1, t.downloadedBytes / t.totalBytes) : 0;
}

/** How many tasks each sidebar entry shows. */
export function countAll(tasks: Record<string, Task>): Map<Filter, number> {
  const m = new Map<Filter, number>();
  const bump = (f: Filter) => m.set(f, (m.get(f) ?? 0) + 1);
  for (const t of Object.values(tasks)) {
    if (t.status === 'removed') {
      bump('trash');
      continue;
    }
    bump('all');
    bump(`cat:${t.category}`);
    if (ACTIVE.includes(t.status)) bump('active');
    if (t.status === 'completed' || t.status === 'seeding') bump('done');
    if (t.status === 'failed') bump('failed');
    if (t.kind === 'torrent') bump('torrent');
  }
  return m;
}
