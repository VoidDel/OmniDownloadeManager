// Window chrome (docs/development-plan.md §6.2): title bar with the search box and window buttons,
// sidebar with filters, categories and settings sections, the command bar and the status bar.
// Same structure and classes as OmniZip's (omni-ui shell.css); labels come from locales/*/app.json
// (`shell`).

import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { builtinThemes, summarize, type Platform, type ThemeSummary } from 'omni-ui';
import { useEffect, useMemo, useRef, useState, type MouseEvent as ReactMouseEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { useShallow } from 'zustand/react/shallow';

import { Icon, Logo, type IconName } from '../../components/Icon';
import { ContextMenu, ProgressBar, type MenuItem } from '../../components/ui';
import i18n, { LANGUAGE_NAMES, SUPPORTED, systemLang } from '../../i18n';
import { api } from '../../lib/api';
import { CATEGORY_ICON } from '../../lib/category';
import * as fmt from '../../lib/format';
import { isDark } from '../../lib/theme';
import type { Category, ThemeMode } from '../../lib/types';
import { categoryLabel, checksumFailed } from '../../lib/words';
import { countAll, detailTask, inFilter, matches, useApp, visibleIds, type Filter, type SettingsTab, type SortKey } from '../../store/app';

/** The fixed filters; their labels are `shell.filters.<id>` (see `filterLabel`). */
export const FILTERS: readonly { id: Filter; icon: IconName }[] = [
  { id: 'all', icon: 'list' },
  { id: 'active', icon: 'download' },
  { id: 'done', icon: 'checkCircle' },
  { id: 'torrent', icon: 'magnet' },
  { id: 'failed', icon: 'alert' },
  { id: 'trash', icon: 'trash' },
];

/** Categories always listed; the others appear once something is in them. */
const MAIN_CATEGORIES: Category[] = ['video', 'audio', 'document', 'archive', 'program', 'disk_image'];
const MORE_CATEGORIES: Category[] = ['image', 'other'];

/** The settings sections: id, translation key of the label, icon. For labels use
 *  `settingsTabs()` or `settingsTabLabel(id)`. */
export const SETTINGS_TABS: readonly [SettingsTab, string, IconName][] = [
  ['general', 'shell.settingsTabs.general', 'layout'],
  ['appearance', 'shell.settingsTabs.appearance', 'palette'],
  ['download', 'shell.settingsTabs.download', 'download'],
  ['network', 'shell.settingsTabs.network', 'globe'],
  ['torrent', 'shell.settingsTabs.torrent', 'magnet'],
  ['browser', 'shell.settingsTabs.browser', 'plug'],
];

export function settingsTabLabel(id: SettingsTab): string {
  return i18n.t(`shell.settingsTabs.${id}`);
}

/** `SETTINGS_TABS` with the labels in the UI language: id, label, icon. */
export function settingsTabs(): [SettingsTab, string, IconName][] {
  return SETTINGS_TABS.map(([id, key, icon]) => [id, i18n.t(key), icon]);
}

export function filterLabel(f: Filter): string {
  if (f.startsWith('cat:')) return categoryLabel(f.slice(4) as Category);
  return FILTERS.some((x) => x.id === f) ? i18n.t(`shell.filters.${f}`) : '';
}

/** A menu opened from a button, below it (`up`: above it, from the status bar). A second press
 *  on the button closes it. */
function useMenu() {
  const [at, setAt] = useState<{ x: number; y: number; anchor: HTMLElement } | null>(null);
  const toggle = (e: ReactMouseEvent<HTMLElement>, alignEnd = false, up = false) => {
    const anchor = e.currentTarget;
    const r = anchor.getBoundingClientRect();
    // Upwards: the menu does not fit below, so it ends just above the button.
    setAt(at ? null : { x: alignEnd ? r.right - 220 : r.left, y: up ? r.top - 4 : r.bottom + 4, anchor });
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
  const { t } = useTranslation();
  const max = useMaximized(true);
  const run = (f: (w: ReturnType<typeof getCurrentWindow>) => Promise<void>) => () => {
    if (isTauri()) void f(getCurrentWindow());
  };
  return (
    <div className="winctl">
      <button type="button" aria-label={t('shell.window.minimize')} onClick={run((w) => w.minimize())}>
        <Icon name="minus" />
      </button>
      <button type="button" aria-label={max ? t('shell.window.restore') : t('shell.window.maximize')} onClick={run((w) => w.toggleMaximize())}>
        <Icon name={max ? 'restore' : 'square'} />
      </button>
      <button type="button" className="close" aria-label={t('shell.window.close')} onClick={run((w) => w.close())}>
        <Icon name="x" />
      </button>
    </div>
  );
}

/** A title-bar button with a menu (as OmniZip's): the items are made when it opens. */
function MenuButton({ icon, label, items }: { icon: IconName; label: string; items: () => MenuItem[] }) {
  const menu = useMenu();
  return (
    <>
      <button type="button" className="iconbtn" title={label} aria-label={label} aria-haspopup="menu" aria-expanded={menu.at !== null} onClick={(e) => menu.toggle(e, true)}>
        <Icon name={icon} />
      </button>
      {menu.at && <ContextMenu x={menu.at.x} y={menu.at.y} items={items()} anchor={menu.at.anchor} onClose={menu.close} />}
    </>
  );
}

const THEMES: ThemeSummary[] = builtinThemes.map((th) => summarize(th, true));

/** Light / dark / system, then the themes of the appearance in effect (as in OmniZip and
 *  Settings → Appearance), and the interface language. */
function TitleMenus() {
  const { t, i18n: { language } } = useTranslation();
  const appearance = useApp((s) => s.settings.appearance);
  const lang = useApp((s) => s.settings.language || 'auto');
  const save = useApp((s) => s.saveSettings);
  const dark = isDark(appearance.mode);
  const themeItems = (): MenuItem[] => {
    const mode = (m: ThemeMode) => () => void save((s) => (s.appearance.mode = m));
    const key = dark ? 'darkTheme' : 'lightTheme';
    return [
      { label: t('shell.theme.light'), icon: 'sun', checked: appearance.mode === 'light', run: mode('light') },
      { label: t('shell.theme.dark'), icon: 'moon', checked: appearance.mode === 'dark', run: mode('dark') },
      { label: t('shell.theme.system'), icon: 'monitor', checked: appearance.mode === 'system', run: mode('system') },
      { separator: true },
      ...THEMES.filter((th) => th.appearance === (dark ? 'dark' : 'light')).map(
        (th): MenuItem => ({
          label: th.name[language] ?? th.name['en-US'],
          checked: appearance[key] === th.id,
          run: () => void save((s) => (s.appearance[key] = th.id)),
        }),
      ),
    ];
  };
  const langItems = (): MenuItem[] => {
    const pick = (l: string) => () => void save((s) => (s.language = l));
    return [
      { label: t('settings.language.auto', { lang: LANGUAGE_NAMES[systemLang()] }), checked: lang === 'auto', run: pick('auto') },
      { separator: true },
      ...SUPPORTED.map((l): MenuItem => ({ label: LANGUAGE_NAMES[l], checked: lang === l, run: pick(l) })),
    ];
  };
  return (
    <>
      <MenuButton icon={dark ? 'moon' : 'sun'} label={t('shell.theme.title')} items={themeItems} />
      <MenuButton icon="globe" label={t('shell.language')} items={langItems} />
    </>
  );
}

/** The search box: narrows the list (within the sidebar's filter) to tasks whose name, link or
 *  folder has every word typed. Escape clears it; Enter or ↓ moves to the list. */
function SearchBox({ platform }: { platform: Platform }) {
  const { t } = useTranslation();
  const query = useApp((s) => s.query);
  const setQuery = useApp((s) => s.setQuery);
  return (
    <label className="searchbox">
      <Icon name="search" />
      <input
        id="search"
        type="search"
        value={query}
        placeholder={t('shell.search.placeholder')}
        aria-label={t('shell.search.label')}
        spellCheck={false}
        onChange={(e) => setQuery(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Escape') {
            if (query) setQuery('');
            else e.currentTarget.blur();
          }
          if (e.key === 'Enter' || e.key === 'ArrowDown') {
            e.preventDefault();
            document.querySelector<HTMLElement>('.task-table')?.focus();
          }
        }}
      />
      {!query && <kbd>{platform === 'mac' ? '⌘F' : 'Ctrl+F'}</kbd>}
    </label>
  );
}

export function TitleBar({ platform }: { platform: Platform }) {
  const { t } = useTranslation();
  const view = useApp((s) => s.view);
  const filter = useApp((s) => s.filter);
  const count = useApp((s) => Object.values(s.tasks).reduce((n, t) => n + (inFilter(t, s.filter) ? 1 : 0), 0));
  const found = useApp((s) => (s.query.trim() ? Object.values(s.tasks).reduce((n, t) => n + (inFilter(t, s.filter) && matches(t, s.query) ? 1 : 0), 0) : null));
  return (
    <header className="titlebar" data-tauri-drag-region>
      <div className="tb-side" data-tauri-drag-region>
        <div className="ttl" data-tauri-drag-region>
          {view === 'settings' ? (
            <b role="heading" aria-level={1}>
              {t('shell.settings')}
            </b>
          ) : (
            <>
              <b role="heading" aria-level={1}>
                {filterLabel(filter)}
              </b>
              <span>{found == null ? t('shell.taskCount', { count }) : t('shell.search.found', { found, count })}</span>
            </>
          )}
        </div>
      </div>
      {view === 'tasks' && <SearchBox platform={platform} />}
      <div className="tb-side end" data-tauri-drag-region>
        <TitleMenus />
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
  const { t } = useTranslation();
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
    <button type="button" className="task-sum" title={t('shell.summary.title')} onClick={() => setFilter('active')}>
      <div className="row">
        <Icon name="download" />
        <span className="lbl grow">{t('shell.summary.running', { count: running })}</span>
        {!unknown && <span className="lbl sum-pct">{fmt.percent(done, total)}</span>}
      </div>
      <ProgressBar value={total ? done / total : 0} indeterminate={unknown} />
    </button>
  );
}

/** Whether a scrolling element has more below what it shows (its last rows then fade out). */
function useMoreBelow<T extends HTMLElement>() {
  const ref = useRef<T>(null);
  const [more, setMore] = useState(false);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const check = () => setMore(el.scrollTop + el.clientHeight < el.scrollHeight - 2);
    check();
    const sizes = new ResizeObserver(check);
    sizes.observe(el);
    el.addEventListener('scroll', check, { passive: true });
    return () => {
      sizes.disconnect();
      el.removeEventListener('scroll', check);
    };
  }, []);
  return [ref, more] as const;
}

export function Sidebar({ platform }: { platform: Platform }) {
  const { t } = useTranslation();
  const [nav, more] = useMoreBelow<HTMLElement>();
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
      <nav className="sb-nav" ref={nav} data-more={more} aria-label={t('shell.sidebar.filters')}>
        {FILTERS.map((f) => item(f.id, filterLabel(f.id), f.icon))}
        <div className="sb-label lbl">{t('shell.sidebar.categories')}</div>
        {categories.map((c) => item(`cat:${c}`, categoryLabel(c), CATEGORY_ICON[c]))}
      </nav>
      <div className="sb-bottom">
        <TaskSummary />
        {/* The settings sections unfold upwards from the Settings item while settings are shown. */}
        <div id="sb-settings" className="sb-sub" data-open={inSettings} role="group" aria-label={t('shell.settings')} inert={!inSettings}>
          <div>
            {settingsTabs().map(([id, label, icon]) => (
              <button key={id} type="button" className="nav-item" aria-current={inSettings && settingsTab === id} title={label} onClick={() => setSettingsTab(id)}>
                <Icon name={icon} />
                <span className="lbl">{label}</span>
              </button>
            ))}
          </div>
        </div>
        <button type="button" className="nav-item" aria-expanded={inSettings} aria-controls="sb-settings" title={t('shell.settings')} onClick={() => setView(inSettings ? 'tasks' : 'settings')}>
          <Icon name="gear" />
          <span className="lbl grow">{t('shell.settings')}</span>
          <Icon name="chevronUp" className="chev" />
        </button>
      </div>
    </aside>
  );
}

const SPEED_PRESETS = [512 * 1024, 1024 ** 2, 2 * 1024 ** 2, 5 * 1024 ** 2, 10 * 1024 ** 2, 20 * 1024 ** 2];

/** The global download limit, in the status bar: presets and a custom value. */
function SpeedLimitMenu() {
  const { t } = useTranslation();
  const limit = useApp((s) => s.settings.engine.speedLimit);
  const { run, openDialog } = useApp(useShallow((s) => ({ run: s.run, openDialog: s.openDialog })));
  const menu = useMenu();
  const set = (l: number | null) => () =>
    void run(async () => {
      const settings = await api.setSpeedLimit(l);
      useApp.setState({ settings });
    });
  const items: MenuItem[] = [
    { label: t('shell.speedLimit.unlimited'), checked: limit == null, run: set(null) },
    { separator: true },
    ...SPEED_PRESETS.map((p): MenuItem => ({ label: fmt.speed(p), checked: limit === p, run: set(p) })),
    { separator: true },
    { label: limit != null && !SPEED_PRESETS.includes(limit) ? t('shell.speedLimit.customValue', { speed: fmt.speed(limit) }) : t('shell.speedLimit.custom'), checked: limit != null && !SPEED_PRESETS.includes(limit), run: () => openDialog({ kind: 'speed' }) },
  ];
  return (
    <>
      <button
        type="button"
        className="sb-limit"
        data-on={limit != null}
        aria-haspopup="menu"
        aria-expanded={menu.at !== null}
        title={t('shell.speedLimit.title')}
        onClick={(e) => menu.toggle(e, true, true)}
      >
        <Icon name="gauge" />
        {limit == null ? t('shell.speedLimit.unlimited') : t('shell.speedLimit.limited', { speed: fmt.speed(limit) })}
        <Icon name="chevronUp" className="chev" />
      </button>
      {menu.at && <ContextMenu x={menu.at.x} y={menu.at.y} items={items} anchor={menu.at.anchor} onClose={menu.close} />}
    </>
  );
}

/** The sort orders; their labels are `shell.sort.<key>`. */
const SORTS: SortKey[] = ['added', 'name', 'size', 'progress', 'speed', 'status'];

function SortMenu() {
  const { t } = useTranslation();
  const sort = useApp((s) => s.sort);
  const setSort = useApp((s) => s.setSort);
  const menu = useMenu();
  const items: MenuItem[] = SORTS.map((k) => ({ label: t(`shell.sort.${k}`), checked: sort.key === k, run: () => setSort(k) }));
  return (
    <>
      <button type="button" className="btn subtle sort" aria-haspopup="menu" aria-expanded={menu.at !== null} onClick={(e) => menu.toggle(e, true)}>
        <span className="lbl">{SORTS.includes(sort.key) ? t(`shell.sort.${sort.key}`) : undefined}</span>
        <Icon name="chevronDown" className="chev" />
      </button>
      {menu.at && <ContextMenu x={menu.at.x} y={menu.at.y} items={items} anchor={menu.at.anchor} onClose={menu.close} />}
    </>
  );
}

/** Commands for the selection, or for every task when nothing is selected. */
export function CommandBar() {
  const { t } = useTranslation();
  const filter = useApp((s) => s.filter);
  const selection = useApp((s) => s.selection);
  const pinned = useApp((s) => s.detailPinned);
  const shown = useApp(detailTask);
  const { run, openDialog, openDetail, setDetailPinned } = useApp(
    useShallow((s) => ({ run: s.run, openDialog: s.openDialog, openDetail: s.openDetail, setDetailPinned: s.setDetailPinned })),
  );
  const statuses = useApp(useShallow((s) => selection.map((id) => s.tasks[id]?.status)));
  // Selected tasks to download again: the finished file was deleted, or the remote file changed.
  const again = useApp(
    useShallow((s) =>
      selection.filter((id) => {
        const t = s.tasks[id];
        return t && ((t.status === 'completed' && s.missing[id]) || t.error?.code === 'remote_changed' || checksumFailed(t));
      }),
    ),
  );
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
        <button type="button" title={t('shell.cmd.restore')} className="btn" disabled={!sel} onClick={() => void run(() => api.restore(selection))}>
          <Icon name="refresh" />
          <span className="lbl">{t('shell.cmd.restore')}</span>
        </button>
        <button type="button" title={t('shell.cmd.purge')} className="btn danger" disabled={!sel} onClick={() => openDialog({ kind: 'purge', ids: selection })}>
          <Icon name="trash" />
          <span className="lbl">{t('shell.cmd.purge')}</span>
        </button>
        <span className="grow" />
        <button type="button" className="btn subtle" disabled={allIds.length === 0} onClick={() => openDialog({ kind: 'purge', ids: allIds })}>
          <span className="lbl">{t('shell.cmd.emptyTrash')}</span>
        </button>
      </div>
    );
  }

  return (
    <div className="cmdbar">
      <button type="button" className="btn" onClick={() => openDialog({ kind: 'new', text: '' })}>
        <Icon name="plus" />
        <span className="lbl">{t('shell.cmd.newDownload')}</span>
      </button>
      {sel ? (
        <>
          <button type="button" title={t('shell.cmd.start')} className="btn" disabled={!canResume} onClick={() => void run(() => api.resume(selection))}>
            <Icon name="play" />
            <span className="lbl">{t('shell.cmd.start')}</span>
          </button>
          <button type="button" title={t('shell.cmd.pause')} className="btn" disabled={!canPause} onClick={() => void run(() => api.pause(selection))}>
            <Icon name="pause" />
            <span className="lbl">{t('shell.cmd.pause')}</span>
          </button>
          {again.length > 0 && (
            <button type="button" title={t('shell.cmd.redownload')} className="btn" onClick={() => void run(() => api.restart(again))}>
              <Icon name="refresh" />
              <span className="lbl">{t('shell.cmd.redownload')}</span>
            </button>
          )}
          <button type="button" title={t('shell.cmd.delete')} className="btn" onClick={() => void useApp.getState().removeTasks(selection)}>
            <Icon name="trash" />
            <span className="lbl">{t('shell.cmd.delete')}</span>
          </button>
          <button type="button" title={t('shell.cmd.openFolder')} className="btn" disabled={!single} onClick={() => single && void run(() => api.reveal(single))}>
            <Icon name="folderOpen" />
            <span className="lbl">{t('shell.cmd.openFolder')}</span>
          </button>
        </>
      ) : (
        <>
          <button type="button" title={t('shell.cmd.startAll')} className="btn" disabled={!anyResumable} onClick={() => void run(() => api.resume(Object.values(useApp.getState().tasks).filter((t) => t.status === 'paused').map((t) => t.id)))}>
            <Icon name="play" />
            <span className="lbl">{t('shell.cmd.startAll')}</span>
          </button>
          <button type="button" title={t('shell.cmd.pauseAll')} className="btn" disabled={!anyPausable} onClick={() => void run(() => api.pause(Object.keys(useApp.getState().tasks)))}>
            <Icon name="pause" />
            <span className="lbl">{t('shell.cmd.pauseAll')}</span>
          </button>
        </>
      )}
      <span className="grow" />
      <SortMenu />
      {/* While the panel shows, the same switch is in its header, in the same place. Here it opens the
          panel once for the selected task; highlighted, the panel opens for every task (a press here
          turns that off). */}
      {!shown && (
        <button
          type="button"
          className="iconbtn"
          aria-pressed={pinned}
          disabled={!pinned && !single}
          title={pinned ? t('shell.cmd.detailPinned') : t('shell.cmd.showDetail')}
          aria-label={t('shell.cmd.detailPanel')}
          onClick={() => (pinned ? setDetailPinned(false) : single && openDetail(single))}
        >
          <Icon name="sidebar" />
        </button>
      )}
    </div>
  );
}

export function StatusBar() {
  const { t } = useTranslation();
  const speed = useApp((s) => s.stats.downloadSpeed);
  const upload = useApp((s) => s.stats.uploadSpeed);
  // The selection is the list's: not shown over the settings.
  const selected = useApp((s) => (s.view === 'tasks' ? s.selection.length : 0));
  const { all, running, waiting, seeding } = useApp(
    useShallow((s) => {
      let all = 0;
      let running = 0;
      let waiting = 0;
      let seeding = 0;
      for (const t of Object.values(s.tasks)) {
        if (t.status === 'removed') continue;
        all++;
        if (t.status === 'downloading' || t.status === 'probing') running++;
        if (t.status === 'queued') waiting++;
        if (t.status === 'seeding') seeding++;
      }
      return { all, running, waiting, seeding };
    }),
  );
  const seen = useApp((s) => s.seenBrowsers);
  const names = seen.map(([b]) => ({ chrome: 'Chrome', edge: 'Edge', firefox: 'Firefox', brave: 'Brave', chromium: 'Chromium' })[b] ?? b);
  const parts = [t('shell.taskCount', { count: all })];
  if (running) parts.push(t('shell.status.running', { count: running }));
  if (waiting) parts.push(t('shell.status.waiting', { count: waiting }));
  if (seeding) parts.push(t('shell.status.seeding', { count: seeding }));
  if (selected) parts.push(t('shell.status.selected', { count: selected }));
  return (
    <footer className="statusbar">
      <div className="lft">
        {names.length > 0 && (
          <span className="seen" title={t('shell.status.extensionConnected')}>
            <i aria-hidden="true" />
            {t('shell.status.browsersConnected', { names: names.join(t('shell.status.listSeparator')) })}
          </span>
        )}
      </div>
      <div className="mid">{parts.join(' · ')}</div>
      <div className="rgt">
        {/* The speeds next to the limit that governs them. */}
        <span className="speed" title={t('shell.status.downloadSpeed')}>
          <Icon name="arrowDown" />
          {fmt.speed(speed)}
        </span>
        <span className="speed" title={t('shell.status.uploadSpeed')}>
          <Icon name="arrowUp" />
          {fmt.speed(upload)}
        </span>
        <SpeedLimitMenu />
      </div>
    </footer>
  );
}
