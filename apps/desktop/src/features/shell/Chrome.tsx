// Window chrome (docs/development-plan.md §6.2): title bar with the paste box and window buttons,
// sidebar with the task filters, command bar and status bar. Same structure and classes as
// OmniZip's (omni-ui shell.css); labels are Chinese literals until i18n lands in Phase 5.

import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type { Platform } from 'omni-ui';
import { useEffect, useState } from 'react';

import { Icon, Logo, type IconName } from '../../components/Icon';
import { ContextMenu, type MenuItem } from '../../components/ui';
import { isDark, type ThemeMode } from '../../lib/theme';

export type Filter = 'all' | 'active' | 'done' | 'torrent' | 'failed' | 'trash';

export const FILTERS: readonly { id: Filter; label: string; icon: IconName }[] = [
  { id: 'all', label: '全部任务', icon: 'list' },
  { id: 'active', label: '进行中', icon: 'download' },
  { id: 'done', label: '已完成', icon: 'checkCircle' },
  { id: 'torrent', label: '种子', icon: 'magnet' },
  { id: 'failed', label: '失败', icon: 'alert' },
  { id: 'trash', label: '回收站', icon: 'trash' },
];

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

/** Title-bar button that opens the light / dark / system menu below itself. */
function ThemeMenu({ mode, onMode }: { mode: ThemeMode; onMode: (m: ThemeMode) => void }) {
  // Where the menu opens, and the button that opened it (a press on it toggles, not closes).
  const [at, setAt] = useState<{ x: number; y: number; anchor: HTMLElement } | null>(null);
  const items: MenuItem[] = [
    { label: '浅色', icon: 'sun', checked: mode === 'light', run: () => onMode('light') },
    { label: '深色', icon: 'moon', checked: mode === 'dark', run: () => onMode('dark') },
    { label: '跟随系统', icon: 'monitor', checked: mode === 'system', run: () => onMode('system') },
  ];
  return (
    <>
      <button
        type="button"
        className="iconbtn"
        title="外观"
        aria-label="外观"
        aria-haspopup="menu"
        aria-expanded={at !== null}
        onClick={(e) => {
          const anchor = e.currentTarget;
          const r = anchor.getBoundingClientRect();
          setAt(at ? null : { x: r.left, y: r.bottom + 4, anchor });
        }}
      >
        <Icon name={isDark(mode) ? 'moon' : 'sun'} />
      </button>
      {at && <ContextMenu x={at.x} y={at.y} items={items} anchor={at.anchor} onClose={() => setAt(null)} />}
    </>
  );
}

export function TitleBar({ platform, title, mode, onMode }: {
  platform: Platform;
  title: string;
  mode: ThemeMode;
  onMode: (m: ThemeMode) => void;
}) {
  return (
    <header className="titlebar" data-tauri-drag-region>
      <div className="tb-side" data-tauri-drag-region>
        <div className="ttl" data-tauri-drag-region>
          <b>{title}</b>
        </div>
      </div>
      {/* The paste box: links and magnet links become tasks here once the engines exist (Phase 1). */}
      <label className="searchbox omnibox">
        <Icon name="link" />
        <input type="text" placeholder="粘贴链接、磁力链接，或拖入种子文件" aria-label="新建下载的链接" disabled />
        <kbd>{platform === 'mac' ? '⌘N' : 'Ctrl+N'}</kbd>
      </label>
      <div className="tb-side end" data-tauri-drag-region>
        <ThemeMenu mode={mode} onMode={onMode} />
        {platform !== 'mac' && <WindowControls />}
      </div>
    </header>
  );
}

function Brand() {
  return (
    <>
      <Logo />
      <span className="brandname">
        Omni Download<span className="ver">{__APP_VERSION__}</span>
      </span>
    </>
  );
}

export function Sidebar({ platform, filter, onFilter }: { platform: Platform; filter: Filter; onFilter: (f: Filter) => void }) {
  return (
    <aside className="sidebar">
      {/* macOS: the traffic lights take the top row; the name has its own row below. */}
      {platform === 'mac' ? (
        <>
          <div className="sb-top" data-tauri-drag-region />
          <div className="sb-brand">
            <Brand />
          </div>
        </>
      ) : (
        <div className="sb-top" data-tauri-drag-region>
          <Brand />
        </div>
      )}
      <nav className="sb-nav" aria-label="任务筛选">
        {FILTERS.map((f) => (
          <button key={f.id} type="button" className="nav-item" aria-current={filter === f.id} onClick={() => onFilter(f.id)}>
            <Icon name={f.icon} />
            <span className="lbl">{f.label}</span>
          </button>
        ))}
      </nav>
      <div className="sb-bottom">
        <button type="button" className="nav-item" disabled title="设置在后续阶段提供">
          <Icon name="gear" />
          <span className="lbl">设置</span>
        </button>
      </div>
    </aside>
  );
}

export function CommandBar() {
  return (
    <div className="cmdbar">
      <button type="button" className="btn" disabled>
        <Icon name="plus" />
        <span className="lbl">新建下载</span>
      </button>
      <button type="button" className="btn" disabled>
        <Icon name="play" />
        <span className="lbl">全部开始</span>
      </button>
      <button type="button" className="btn" disabled>
        <Icon name="pause" />
        <span className="lbl">全部暂停</span>
      </button>
      <button type="button" className="btn" disabled>
        <Icon name="trash" />
        <span className="lbl">删除</span>
      </button>
      <span className="grow" />
      <button type="button" className="btn" disabled>
        <Icon name="gauge" />
        <span className="lbl">不限速</span>
      </button>
    </div>
  );
}

export function StatusBar() {
  return (
    <footer className="statusbar">
      <div className="lft">
        <span className="speed">
          <Icon name="arrowDown" />0 B/s
        </span>
        <span className="speed">
          <Icon name="arrowUp" />0 B/s
        </span>
      </div>
      <div className="mid">0 个任务</div>
      <div className="rgt">开发预览</div>
    </footer>
  );
}
