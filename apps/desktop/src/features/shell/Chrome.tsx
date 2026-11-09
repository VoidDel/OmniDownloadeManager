// Window chrome (docs/development-plan.md §6.2): title bar with the paste box and window buttons,
// sidebar with filters, categories and settings sections, the command bar and the status bar.
// Same structure and classes as OmniZip's (omni-ui shell.css); labels are Chinese literals until
// i18n lands in Phase 5.

import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type { Platform } from 'omni-ui';
import { useEffect, useMemo, useState, type MouseEvent as ReactMouseEvent } from 'react';
import { useShallow } from 'zustand/react/shallow';

import { Icon, Logo, type IconName } from '../../components/Icon';
import { ContextMenu, ProgressBar, type MenuItem } from '../../components/ui';
import { api } from '../../lib/api';
import * as fmt from '../../lib/format';
import { hasLink } from '../../lib/links';
import { isDark } from '../../lib/theme';
import type { Category, ThemeMode } from '../../lib/types';
import { CATEGORY_LABEL } from '../../lib/words';
import { countAll, inFilter, useApp, visibleIds, type Filter, type SettingsTab, type SortKey } from '../../store/app';

export const FILTERS: readonly { id: Filter; label: string; icon: IconName }[] = [
  { id: 'all', label: '全部任务', icon: 'list' },
  { id: 'active', label: '进行中', icon: 'download' },
  { id: 'done', label: '已完成', icon: 'checkCircle' },
  { id: 'torrent', label: '种子', icon: 'magnet' },
  { id: 'failed', label: '失败', icon: 'alert' },
  { id: 'trash', label: '回收站', icon: 'trash' },
];

/** Categories always listed; the others appear once something is in them. */
const MAIN_CATEGORIES: Category[] = ['video', 'audio', 'document', 'archive', 'program', 'disk_image'];
const MORE_CATEGORIES: Category[] = ['image', 'other'];
const CATEGORY_ICON: Record<Category, IconName> = {
  video: 'fileVideo',
  audio: 'fileAudio',
  document: 'fileText',
  archive: 'archive',
  program: 'package',
  disk_image: 'disc',
  image: 'fileImage',
  other: 'file',
};

export const SETTINGS_TABS: readonly [SettingsTab, string, IconName][] = [
  ['general', '常规', 'sliders'],
  ['download', '下载', 'download'],
  ['network', '网络', 'globe'],
];

export function filterLabel(f: Filter): string {
  if (f.startsWith('cat:')) return CATEGORY_LABEL[f.slice(4) as Category];
  return FILTERS.find((x) => x.id === f)?.label ?? '';
}

/** A menu opened from a button, below it. A second press on the button closes it. */
function useMenu() {
  const [at, setAt] = useState<{ x: number; y: number; anchor: HTMLElement } | null>(null);
  const toggle = (e: ReactMouseEvent<HTMLElement>, alignEnd = false) => {
    const anchor = e.currentTarget;
    const r = anchor.getBoundingClientRect();
    setAt(at ? null : { x: alignEnd ? r.right - 220 : r.left, y: r.bottom + 4, anchor });
  };
  return { at, toggle, close: () => setAt(null) };
}

/** Whether the window is maximised, kept up to date (false outside Tauri). */
function useMaximized(enabled: boolean): boolean {
  const [max, setMax] = useState(false);
  useEffect(() => {
    if (!enabled || !isTauri()) return;
    const w = getCurrentWindow();
    void w.isMaximized().then(setMax);
    const un = w.onResized(() => void w.isMaximized().then(setMax));
    return () => {
      void un.then((f) => f());
    };
  }, [enabled]);
  return max;
}

const EDGES = [
  ['n', 'North'],
  ['s', 'South'],
  ['e', 'East'],
  ['w', 'West'],
  ['ne', 'NorthEast'],
  ['nw', 'NorthWest'],
  ['se', 'SouthEast'],
  ['sw', 'SouthWest'],
] as const;

/**
 * The edges a Linux window is resized by. The window has no frame there and the web view takes
 * the pointer before the window manager could start a resize from the border, so the page hands
 * the press over. Not while maximised.
 */
export function ResizeEdges({ platform }: { platform: Platform }) {
  const linux = platform === 'linux';
  const max = useMaximized(linux);
  if (!linux || max || !isTauri()) return null;
  return (
    <div className="resize-edges" aria-hidden="true">
      {EDGES.map(([cls, dir]) => (
        <div
          key={cls}
          className={cls}
          onPointerDown={(e) => {
            if (e.button !== 0) return;
            e.preventDefault();
            void getCurrentWindow().startResizeDragging(dir);
          }}
        />
      ))}
    </div>
  );
}

function WindowControls() {
  const max = useMaximized(true);
  const run = (f: (w: ReturnType<typeof getCurrentWindow>) => Promise<void>) => () => {
    if (isTauri()) void f(getCurrentWindow());
  };
  return (
    <div className="winctl">
      <button type="button" aria-label="最小化" onClick={run((w) => w.minimize())}>
        <Icon name="minus" />
      </button>
      <button type="button" aria-label={max ? '还原' : '最大化'} onClick={run((w) => w.toggleMaximize())}>
        <Icon name={max ? 'restore' : 'square'} />
      </button>
      <button type="button" className="close" aria-label="关闭" onClick={run((w) => w.close())}>
        <Icon name="x" />
      </button>
    </div>
  );
}

/** Title-bar button for light / dark / system. */
function ThemeMenu() {
  const mode = useApp((s) => s.settings.appearance.mode);
  const save = useApp((s) => s.saveSettings);
  const menu = useMenu();
  const pick = (m: ThemeMode) => () => void save((s) => (s.appearance.mode = m));
  const items: MenuItem[] = [
    { label: '浅色', icon: 'sun', checked: mode === 'light', run: pick('light') },
    { label: '深色', icon: 'moon', checked: mode === 'dark', run: pick('dark') },
    { label: '跟随系统', icon: 'monitor', checked: mode === 'system', run: pick('system') },
  ];
  return (
    <>
      <button type="button" className="iconbtn" title="外观" aria-label="外观" aria-haspopup="menu" aria-expanded={menu.at !== null} onClick={(e) => menu.toggle(e)}>
        <Icon name={isDark(mode) ? 'moon' : 'sun'} />
      </button>
      {menu.at && <ContextMenu x={menu.at.x} y={menu.at.y} items={items} anchor={menu.at.anchor} onClose={menu.close} />}
    </>
  );
}

/** The paste box: a link pasted or typed here (Enter) opens the new-download dialog with it. */
function PasteBox({ platform }: { platform: Platform }) {
  const openDialog = useApp((s) => s.openDialog);
  const [value, setValue] = useState('');
  const submit = (text: string) => {
    openDialog({ kind: 'new', text });
    setValue('');
  };
  return (
    <label className="searchbox omnibox">
      <Icon name="link" />
      <input
        id="pastebox"
        type="text"
        value={value}
        placeholder="粘贴下载链接"
        aria-label="粘贴下载链接"
        spellCheck={false}
        onChange={(e) => setValue(e.target.value)}
        onPaste={(e) => {
          const text = e.clipboardData.getData('text');
          if (hasLink(text)) {
            e.preventDefault();
            submit(text);
          }
        }}
        onKeyDown={(e) => {
          if (e.key === 'Enter') submit(value);
          if (e.key === 'Escape') setValue('');
        }}
      />
      <kbd>{platform === 'mac' ? '⌘N' : 'Ctrl+N'}</kbd>
    </label>
  );
}

export function TitleBar({ platform }: { platform: Platform }) {
  const view = useApp((s) => s.view);
  const filter = useApp((s) => s.filter);
  const count = useApp((s) => Object.values(s.tasks).reduce((n, t) => n + (inFilter(t, s.filter) ? 1 : 0), 0));
  return (
    <header className="titlebar" data-tauri-drag-region>
      <div className="tb-side" data-tauri-drag-region>
        <div className="ttl" data-tauri-drag-region>
          {view === 'settings' ? (
            <b>设置</b>
          ) : (
            <>
              <b>{filterLabel(filter)}</b>
              <span>{count} 个任务</span>
            </>
          )}
        </div>
      </div>
      <PasteBox platform={platform} />
      <div className="tb-side end" data-tauri-drag-region>
        <ThemeMenu />
        {platform !== 'mac' && <WindowControls />}
      </div>
    </header>
  );
}

function Brand({ version }: { version: string }) {
  return (
    <>
      <Logo />
      <span className="brandname">
        Omni Download<span className="ver">{version || __APP_VERSION__}</span>
      </span>
    </>
  );
}

/** Running downloads at the bottom of the sidebar: how many, overall progress. */
function TaskSummary() {
  const setFilter = useApp((s) => s.setFilter);
  const { running, done, total, unknown } = useApp(
    useShallow((s) => {
      let running = 0;
      let done = 0;
      let total = 0;
      let unknown = false;
      for (const t of Object.values(s.tasks)) {
        if (t.status !== 'downloading' && t.status !== 'probing') continue;
        running++;
        done += t.downloadedBytes;
        if (t.totalBytes == null) unknown = true;
        else total += t.totalBytes;
      }
      return { running, done, total, unknown };
    }),
  );
  if (!running) return null;
  return (
    <button type="button" className="task-sum" title="查看进行中的任务" onClick={() => setFilter('active')}>
      <div className="row">
        <Icon name="download" />
        <span className="lbl grow">{running} 个任务下载中</span>
        {!unknown && <span className="lbl sum-pct">{fmt.percent(done, total)}</span>}
      </div>
      <ProgressBar value={total ? done / total : 0} indeterminate={unknown} />
    </button>
  );
}

export function Sidebar({ platform }: { platform: Platform }) {
  const view = useApp((s) => s.view);
  const filter = useApp((s) => s.filter);
  const settingsTab = useApp((s) => s.settingsTab);
  const version = useApp((s) => s.version);
  const tasks = useApp((s) => s.tasks);
  const { setFilter, setView, setSettingsTab } = useApp(useShallow((s) => ({ setFilter: s.setFilter, setView: s.setView, setSettingsTab: s.setSettingsTab })));
  const counts = useMemo(() => countAll(tasks), [tasks]);
  const inSettings = view === 'settings';
  const categories = [...MAIN_CATEGORIES, ...MORE_CATEGORIES.filter((c) => counts.get(`cat:${c}`))];

  const item = (id: Filter, label: string, icon: IconName) => {
    const n = counts.get(id) ?? 0;
    return (
      <button key={id} type="button" className="nav-item" aria-current={!inSettings && filter === id} title={label} onClick={() => setFilter(id)}>
        <Icon name={icon} />
        <span className="lbl grow">{label}</span>
        {n > 0 && <span className="lbl cnt">{n}</span>}
      </button>
    );
  };

  return (
    <aside className="sidebar">
      {/* macOS: the traffic lights take the top row; the name has its own row below. */}
      {platform === 'mac' ? (
        <>
          <div className="sb-top" data-tauri-drag-region />
          <div className="sb-brand">
            <Brand version={version} />
          </div>
        </>
      ) : (
        <div className="sb-top" data-tauri-drag-region>
          <Brand version={version} />
        </div>
      )}
      <nav className="sb-nav" aria-label="任务筛选">
        {FILTERS.map((f) => item(f.id, f.label, f.icon))}
        <div className="sb-label lbl">分类</div>
        {categories.map((c) => item(`cat:${c}`, CATEGORY_LABEL[c], CATEGORY_ICON[c]))}
      </nav>
      <div className="sb-bottom">
        <TaskSummary />
        {/* The settings sections unfold upwards from the Settings item while settings are shown. */}
        <div id="sb-settings" className="sb-sub" data-open={inSettings} role="group" aria-label="设置" inert={!inSettings}>
          <div>
            {SETTINGS_TABS.map(([id, label, icon]) => (
              <button key={id} type="button" className="nav-item" aria-current={inSettings && settingsTab === id} title={label} onClick={() => setSettingsTab(id)}>
                <Icon name={icon} />
                <span className="lbl">{label}</span>
              </button>
            ))}
          </div>
        </div>
        <button type="button" className="nav-item" aria-expanded={inSettings} aria-controls="sb-settings" title="设置" onClick={() => setView(inSettings ? 'tasks' : 'settings')}>
          <Icon name="gear" />
          <span className="lbl grow">设置</span>
          <Icon name="chevronUp" className="chev" />
        </button>
      </div>
    </aside>
  );
}

const SPEED_PRESETS = [512 * 1024, 1024 ** 2, 2 * 1024 ** 2, 5 * 1024 ** 2, 10 * 1024 ** 2, 20 * 1024 ** 2];

/** The global download limit: presets and a custom value. */
function SpeedLimitMenu() {
  const limit = useApp((s) => s.settings.engine.speedLimit);
  const { run, openDialog } = useApp(useShallow((s) => ({ run: s.run, openDialog: s.openDialog })));
  const menu = useMenu();
  const set = (l: number | null) => () =>
    void run(async () => {
      const settings = await api.setSpeedLimit(l);
      useApp.setState({ settings });
    });
  const items: MenuItem[] = [
    { label: '不限速', checked: limit == null, run: set(null) },
    { separator: true },
    ...SPEED_PRESETS.map((p): MenuItem => ({ label: fmt.speed(p), checked: limit === p, run: set(p) })),
    { separator: true },
    { label: limit != null && !SPEED_PRESETS.includes(limit) ? `自定义（${fmt.speed(limit)}）` : '自定义…', checked: limit != null && !SPEED_PRESETS.includes(limit), run: () => openDialog({ kind: 'speed' }) },
  ];
  return (
    <>
      <button type="button" className="btn subtle" aria-haspopup="menu" aria-expanded={menu.at !== null} title="全局限速" onClick={(e) => menu.toggle(e, true)}>
        <Icon name="gauge" />
        <span className="lbl">{limit == null ? '不限速' : `限速 ${fmt.speed(limit)}`}</span>
        <Icon name="chevronDown" className="chev" />
      </button>
      {menu.at && <ContextMenu x={menu.at.x} y={menu.at.y} items={items} anchor={menu.at.anchor} onClose={menu.close} />}
    </>
  );
}

const SORTS: [SortKey, string][] = [
  ['added', '按添加时间'],
  ['name', '按名称'],
  ['size', '按大小'],
  ['progress', '按进度'],
  ['speed', '按速度'],
  ['status', '按状态'],
];

function SortMenu() {
  const sort = useApp((s) => s.sort);
  const setSort = useApp((s) => s.setSort);
  const menu = useMenu();
  const items: MenuItem[] = SORTS.map(([k, label]) => ({ label, checked: sort.key === k, run: () => setSort(k) }));
  return (
    <>
      <button type="button" className="btn subtle" aria-haspopup="menu" aria-expanded={menu.at !== null} onClick={(e) => menu.toggle(e, true)}>
        <span className="lbl">{SORTS.find(([k]) => k === sort.key)?.[1]}</span>
        <Icon name="chevronDown" className="chev" />
      </button>
      {menu.at && <ContextMenu x={menu.at.x} y={menu.at.y} items={items} anchor={menu.at.anchor} onClose={menu.close} />}
    </>
  );
}

/** Commands for the selection, or for every task when nothing is selected. */
export function CommandBar() {
  const filter = useApp((s) => s.filter);
  const selection = useApp((s) => s.selection);
  const detailOpen = useApp((s) => s.detailOpen);
  const { run, openDialog, setDetailOpen } = useApp(useShallow((s) => ({ run: s.run, openDialog: s.openDialog, setDetailOpen: s.setDetailOpen })));
  const statuses = useApp(useShallow((s) => selection.map((id) => s.tasks[id]?.status)));
  const allIds = useApp(useShallow((s) => visibleIds(s.tasks, s.filter, { key: 'added', desc: false })));
  const anyPausable = useApp((s) => Object.values(s.tasks).some((t) => ['queued', 'probing', 'downloading'].includes(t.status)));
  const anyResumable = useApp((s) => Object.values(s.tasks).some((t) => t.status === 'paused' || t.status === 'failed'));

  const sel = selection.length > 0;
  const canPause = statuses.some((s) => s === 'queued' || s === 'probing' || s === 'downloading');
  const canResume = statuses.some((s) => s === 'paused' || s === 'failed');
  const single = selection.length === 1 ? selection[0] : null;

  if (filter === 'trash') {
    return (
      <div className="cmdbar">
        <button type="button" className="btn" disabled={!sel} onClick={() => void run(() => api.restore(selection))}>
          <Icon name="refresh" />
          <span className="lbl">还原</span>
        </button>
        <button type="button" className="btn danger" disabled={!sel} onClick={() => openDialog({ kind: 'purge', ids: selection })}>
          <Icon name="trash" />
          <span className="lbl">彻底删除</span>
        </button>
        <span className="grow" />
        <button type="button" className="btn subtle" disabled={allIds.length === 0} onClick={() => openDialog({ kind: 'purge', ids: allIds })}>
          <span className="lbl">清空回收站</span>
        </button>
      </div>
    );
  }

  return (
    <div className="cmdbar">
      <button type="button" className="btn" onClick={() => openDialog({ kind: 'new', text: '' })}>
        <Icon name="plus" />
        <span className="lbl">新建下载</span>
      </button>
      {sel ? (
        <>
          <button type="button" className="btn" disabled={!canResume} onClick={() => void run(() => api.resume(selection))}>
            <Icon name="play" />
            <span className="lbl">开始</span>
          </button>
          <button type="button" className="btn" disabled={!canPause} onClick={() => void run(() => api.pause(selection))}>
            <Icon name="pause" />
            <span className="lbl">暂停</span>
          </button>
          <button type="button" className="btn" onClick={() => void run(() => api.remove(selection))}>
            <Icon name="trash" />
            <span className="lbl">删除</span>
          </button>
          <button type="button" className="btn" disabled={!single} onClick={() => single && void run(() => api.reveal(single))}>
            <Icon name="folderOpen" />
            <span className="lbl">打开文件夹</span>
          </button>
        </>
      ) : (
        <>
          <button type="button" className="btn" disabled={!anyResumable} onClick={() => void run(() => api.resume(Object.values(useApp.getState().tasks).filter((t) => t.status === 'paused').map((t) => t.id)))}>
            <Icon name="play" />
            <span className="lbl">全部开始</span>
          </button>
          <button type="button" className="btn" disabled={!anyPausable} onClick={() => void run(() => api.pause(Object.keys(useApp.getState().tasks)))}>
            <Icon name="pause" />
            <span className="lbl">全部暂停</span>
          </button>
        </>
      )}
      <span className="grow" />
      <SpeedLimitMenu />
      <SortMenu />
      <button type="button" className="iconbtn" aria-pressed={detailOpen} title={detailOpen ? '隐藏详情' : '显示详情'} aria-label="详情面板" onClick={() => setDetailOpen(!detailOpen)}>
        <Icon name="sidebar" />
      </button>
    </div>
  );
}

/** Free space where new downloads go, refreshed every 10 s. */
function useDiskFree(dir: string): number | null {
  const [free, setFree] = useState<number | null>(null);
  useEffect(() => {
    if (!dir || !isTauri()) return;
    let live = true;
    const read = () => void api.diskFree(dir).then((n) => live && setFree(n)).catch(() => undefined);
    read();
    const t = setInterval(read, 10_000);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, [dir]);
  return free;
}

/** `D:` on Windows, the last folder elsewhere. */
function volumeOf(dir: string): string {
  const m = /^([a-zA-Z]:)/.exec(dir);
  return m ? m[1].toUpperCase() : dir.split(/[\\/]/).filter(Boolean).pop() ?? '/';
}

export function StatusBar() {
  const speed = useApp((s) => s.stats.downloadSpeed);
  const dir = useApp((s) => s.settings.downloadDir || s.defaultDir);
  const selected = useApp((s) => s.selection.length);
  const { all, running, waiting } = useApp(
    useShallow((s) => {
      let all = 0;
      let running = 0;
      let waiting = 0;
      for (const t of Object.values(s.tasks)) {
        if (t.status === 'removed') continue;
        all++;
        if (t.status === 'downloading' || t.status === 'probing') running++;
        if (t.status === 'queued') waiting++;
      }
      return { all, running, waiting };
    }),
  );
  const free = useDiskFree(dir);
  const parts = [`${all} 个任务`];
  if (running) parts.push(`下载中 ${running}`);
  if (waiting) parts.push(`等待 ${waiting}`);
  if (selected) parts.push(`已选 ${selected}`);
  return (
    <footer className="statusbar">
      <div className="lft">
        <span className="speed" title="下载速度">
          <Icon name="arrowDown" />
          {fmt.speed(speed)}
        </span>
      </div>
      <div className="mid">{parts.join(' · ')}</div>
      <div className="rgt" title={dir}>
        {free != null && `${volumeOf(dir)} 剩余 ${fmt.bytes(free)}`}
      </div>
    </footer>
  );
}
