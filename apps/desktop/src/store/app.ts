// Application state (zustand). Tasks are kept by id; each row subscribes to its own task, so a
// progress tick re-renders only the rows whose numbers changed. Lists (filtered, sorted) are
// derived where they are shown.

import type { Platform, ToastItem, ToastTone } from 'omni-ui';
import { create } from 'zustand';

import { api, asError, listenApp, listenTasks } from '../lib/api';
import { applyTheme } from '../lib/theme';
import type { AppError, Category, Prefill, Settings, Stats, Task, TaskStatus } from '../lib/types';
import { errorText } from '../lib/words';

export type Filter = 'all' | 'active' | 'done' | 'torrent' | 'failed' | 'trash' | `cat:${Category}`;
export type SortKey = 'added' | 'name' | 'size' | 'progress' | 'speed' | 'status';
export type SettingsTab = 'general' | 'download' | 'network' | 'torrent' | 'browser';
export type View = 'tasks' | 'settings';

export type Dialog =
  /** New downloads, with the text pasted so far (and request details from a browser batch). */
  | ({ kind: 'new' } & Prefill)
  /** Delete for good, optionally with the files. */
  | { kind: 'purge'; ids: string[] }
  /** A custom global speed limit. */
  | { kind: 'speed' }
  | null;

export const DEFAULT_SETTINGS: Settings = {
  appearance: { mode: 'system', lightTheme: 'aurora', darkTheme: 'midnight', density: 'comfortable' },
  downloadDir: '',
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
  sort: { key: SortKey; desc: boolean };
  selection: string[];
  /** Where a shift-click range starts. */
  anchor: string | null;
  /** The details panel opens for every selected task (the switch in its header). */
  detailPinned: boolean;
  /** The task the panel was opened for once (command bar, menu); selecting anything else (or
   *  clicking empty space) closes it for good. */
  detailFor: string | null;
  dialog: Dialog;
  toasts: ToastItem[];
  /** Browsers whose extension has been in touch since the app started: [name, Unix ms]. */
  seenBrowsers: [string, number][];

  /** Loads settings and tasks and listens for changes. Resolves to whether the app was started
   *  in the background by the browser bridge (the main window then stays hidden). */
  boot: (platform: Platform) => Promise<boolean>;
  setView: (v: View) => void;
  setSettingsTab: (t: SettingsTab) => void;
  setFilter: (f: Filter) => void;
  setSort: (key: SortKey) => void;
  select: (ids: string[], anchor?: string | null) => void;
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
  /** Changes and saves the settings (the window applies them at once). */
  saveSettings: (change: (s: Settings) => void) => Promise<void>;
  /** The folder new downloads go to. */
  downloadDir: () => string;
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
  sort: { key: 'added', desc: false },
  selection: [],
  anchor: null,
  detailPinned: readPinned(),
  detailFor: null,
  dialog: null,
  toasts: [],
  seenBrowsers: [],

  boot: async (platform) => {
    set({ platform });
    let background = false;
    try {
      await listenApp({
        newTask: (p) => set({ dialog: { kind: 'new', ...p }, view: 'tasks' }),
        browserSeen: (seenBrowsers) => set({ seenBrowsers }),
        settingsChanged: (settings) => set({ settings }),
      });
      // Listen first: nothing that happens while the list loads is missed.
      await listenTasks({
        added: (t) => set((s) => ({ tasks: withTask(s.tasks, t) })),
        changed: (t) =>
          set((s) => {
            if (!s.tasks[t.id]) return {};
            if (!['probing', 'downloading'].includes(t.status)) history.delete(t.id);
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
      });
      // Opened with magnet links or .torrent files.
      if (boot.openLinks.length > 0) set({ dialog: { kind: 'new', text: boot.openLinks.join('\n') } });
      background = boot.background;
    } catch (e) {
      // Outside the app (the page in a browser) or a failed start: an empty list.
      console.error('bootstrap', e);
    }
    applyTheme(get().settings.appearance, platform);
    set({ ready: true });
    return background;
  },

  setView: (view) => set({ view }),
  setSettingsTab: (settingsTab) => set({ settingsTab, view: 'settings' }),
  setFilter: (filter) => set({ filter, view: 'tasks', selection: [], anchor: null }),
  setSort: (key) => set((s) => ({ sort: { key, desc: s.sort.key === key ? !s.sort.desc : key === 'speed' || key === 'size' } })),
  select: (selection, anchor) =>
    set((s) => ({
      selection,
      anchor: anchor === undefined ? s.anchor : anchor,
      detailFor: selection.length === 1 && selection[0] === s.detailFor ? s.detailFor : null,
    })),
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
    setTimeout(() => get().dismiss(id), tone === 'error' ? 8000 : 4000);
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

  saveSettings: async (change) => {
    const before = get().settings;
    const next: Settings = structuredClone(before);
    change(next);
    set({ settings: next });
    if (next.appearance !== before.appearance && JSON.stringify(next.appearance) !== JSON.stringify(before.appearance)) {
      applyTheme(next.appearance, get().platform, true);
    }
    const saved = await get().run(() => api.setSettings(next));
    if (saved) set({ settings: saved });
    else {
      set({ settings: before });
      applyTheme(before.appearance, get().platform);
    }
  },

  downloadDir: () => get().settings.downloadDir || get().defaultDir,
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

/** Ids of the tasks under `filter`, sorted. */
export function visibleIds(tasks: Record<string, Task>, filter: Filter, sort: State['sort']): string[] {
  const list = Object.values(tasks).filter((t) => inFilter(t, filter));
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
