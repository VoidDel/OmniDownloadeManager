// The task list (docs/development-plan.md §6.2): a virtual table (TanStack Virtual, as OmniZip's
// archive browser) so a thousand tasks scroll as smoothly as ten. Each row subscribes to its own
// task: a progress tick re-renders the rows whose numbers changed, not the list.

import { useVirtualizer } from '@tanstack/react-virtual';
import { memo, useEffect, useRef, useState, type KeyboardEvent, type MouseEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { useShallow } from 'zustand/react/shallow';

import { FileIcon, Icon } from '../../components/Icon';
import { ContextMenu, type MenuItem } from '../../components/ui';
import i18n from '../../i18n';
import { api } from '../../lib/api';
import * as fmt from '../../lib/format';
import { ROW_HEIGHT } from '../../lib/theme';
import type { Task } from '../../lib/types';
import { STATUS_COLOR, errorText, statusText } from '../../lib/words';
import { progressOf, useApp, visibleIds, type SortKey } from '../../store/app';
import { EmptyTasks } from './EmptyTasks';

/** Header cells: sort key, label (a translation key), class (numbers align right; c-* drop in narrow panes). */
const COLUMNS: [SortKey, string, string][] = [
  ['name', 'table.column.name', ''],
  ['progress', 'table.column.progress', ''],
  ['size', 'table.column.size', 'num c-size'],
  ['speed', 'table.column.speed', 'num c-speed'],
  ['added', 'table.column.eta', 'num c-eta'],
  ['status', 'table.column.status', ''],
];

const PROTO: Record<Task['kind'], string> = { http: 'HTTP', ftp: 'FTP', torrent: 'BT' };

/** The menu for the selected tasks. */
export function taskMenu(ids: string[], tasks: Record<string, Task>): MenuItem[] {
  const { run, openDialog, openDetail } = useApp.getState();
  const list = ids.map((id) => tasks[id]).filter(Boolean);
  if (!list.length) return [];
  const one = list.length === 1 ? list[0] : null;
  // Shows the details panel for the task (once, unless it is pinned).
  const details: MenuItem[] = one ? [{ label: i18n.t('table.menu.details'), icon: 'info', run: () => openDetail(one.id) }] : [];
  if (list.every((t) => t.status === 'removed')) {
    return [
      { label: i18n.t('table.menu.restore'), icon: 'refresh', run: () => void run(() => api.restore(ids)) },
      ...details,
      { label: i18n.t('table.menu.purge'), icon: 'trash', danger: true, run: () => openDialog({ kind: 'purge', ids }) },
    ];
  }
  const canPause = list.some((t) => ['queued', 'probing', 'downloading'].includes(t.status));
  const canResume = list.some((t) => t.status === 'paused' || t.status === 'failed');
  const seeding = list.filter((t) => t.status === 'seeding').map((t) => t.id);
  const changed = list.filter((t) => t.error?.code === 'remote_changed').map((t) => t.id);
  const items: MenuItem[] = [];
  if (one?.status === 'completed' || one?.status === 'seeding') items.push({ label: i18n.t('table.menu.open'), icon: 'external', kbd: 'Enter', run: () => void run(() => api.open(one.id)) });
  if (canResume) items.push({ label: i18n.t('table.menu.start'), icon: 'play', run: () => void run(() => api.resume(ids)) });
  if (canPause) items.push({ label: i18n.t('table.menu.pause'), icon: 'pause', run: () => void run(() => api.pause(ids)) });
  if (seeding.length) items.push({ label: i18n.t('table.menu.stopSeeding'), icon: 'stop', run: () => void run(() => api.pause(seeding)) });
  if (changed.length) items.push({ label: i18n.t('table.menu.redownload'), icon: 'refresh', run: () => void run(() => api.restart(changed)) });
  items.push(
    { separator: true },
    ...details,
    { label: i18n.t('table.menu.openFolder'), icon: 'folderOpen', disabled: !one, run: () => one && void run(() => api.reveal(one.id)) },
    {
      label: i18n.t('table.menu.copyLink'),
      icon: 'copy',
      run: () =>
        void navigator.clipboard
          .writeText(list.map((t) => t.source).join('\n'))
          .then(() => useApp.getState().toast('success', list.length > 1 ? i18n.t('table.menu.copiedMany', { count: list.length }) : i18n.t('table.menu.copied'))),
    },
    { separator: true },
    { label: i18n.t('table.menu.delete'), icon: 'trash', kbd: 'Del', run: () => void run(() => api.remove(ids)) },
  );
  return items;
}

/** One task. `start` is its offset in the list. */
const Row = memo(function Row({ id, start, cols }: { id: string; start: number; cols: string }) {
  const t = useApp((s) => s.tasks[id]);
  const selected = useApp((s) => s.selection.includes(id));
  if (!t) return null;
  const color = STATUS_COLOR[t.status];
  const running = t.status === 'downloading' || t.status === 'probing';
  const p = progressOf(t);
  const seeding = t.status === 'seeding';
  const pct = t.status === 'completed' || seeding ? '100%' : fmt.percent(t.downloadedBytes, t.totalBytes);
  const size = t.totalBytes != null ? fmt.bytes(t.totalBytes) : t.downloadedBytes ? fmt.bytes(t.downloadedBytes) : '—';
  const remaining = running ? fmt.duration(t.etaSecs) : t.status === 'completed' ? fmt.when(t.completedAt) : '—';
  const title = t.error ? `${errorText(t.error)}\n${t.error.message}` : undefined;
  return (
    <div
      role="row"
      className="trow task-row"
      aria-selected={selected}
      data-id={id}
      style={{ transform: `translateY(${start}px)`, gridTemplateColumns: cols }}
    >
      <span className="nm" role="gridcell">
        <FileIcon name={t.fileName} isDir={false} />
        <span className="nm-txt">
          <span title={t.fileName}>{t.fileName}</span>
        </span>
        <span className={`badge proto-${t.kind}`}>{PROTO[t.kind]}</span>
      </span>
      <span className="prog" role="gridcell">
        <span className={`bar${t.totalBytes == null && running ? ' indeterminate' : ''}`}>
          <i style={{ width: `${p * 100}%`, background: color }} />
        </span>
        <span className="pct">{pct}</span>
      </span>
      <span className="dim num c-size" role="gridcell">
        {size}
      </span>
      <span className="dim num c-speed" role="gridcell">
        {seeding ? `↑ ${fmt.speed(t.uploadSpeedBps)}` : running && t.speedBps ? fmt.speed(t.speedBps) : '—'}
      </span>
      <span className="dim num c-eta" role="gridcell">
        {remaining}
      </span>
      <span className="st" role="gridcell" style={{ color }} title={title}>
        <i className="sdot" style={{ background: color }} />
        {/* The reason is in the tooltip and the details panel; the column only says the state. */}
        {statusText(t)}
      </span>
    </div>
  );
});

export function TaskTable() {
  const ids = useApp(useShallow((s) => visibleIds(s.tasks, s.filter, s.sort)));
  const sort = useApp((s) => s.sort);
  const setSort = useApp((s) => s.setSort);
  const density = useApp((s) => s.settings.appearance.density);
  const filter = useApp((s) => s.filter);
  const rowH = ROW_HEIGHT[density];
  const scroller = useRef<HTMLDivElement>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; items: MenuItem[] } | null>(null);
  // The keyboard cursor; the selection follows it unless Ctrl is held.
  const [focus, setFocus] = useState<string | null>(null);
  const { t: tr } = useTranslation();

  // The React Compiler cannot memoize around the virtualizer's functions and skips this
  // component; that is expected (rows are memoized themselves).
  // eslint-disable-next-line react-hooks/incompatible-library
  const v = useVirtualizer({ count: ids.length, getScrollElement: () => scroller.current, estimateSize: () => rowH, overscan: 12 });
  useEffect(() => v.measure(), [rowH, v]);

  // The columns follow the pane's width (container queries in app.css).
  const cols = 'var(--task-cols)';

  const rowAt = (e: MouseEvent) => (e.target as HTMLElement).closest<HTMLElement>('.task-row')?.dataset.id ?? null;

  const onMouseDown = (e: MouseEvent) => {
    if (e.button !== 0 && e.button !== 2) return;
    const id = rowAt(e);
    const { selection, anchor, select } = useApp.getState();
    if (!id) {
      if (e.button === 0) select([], null);
      return;
    }
    scroller.current?.focus({ preventScroll: true });
    setFocus(id);
    if (e.button === 2) {
      if (!selection.includes(id)) select([id], id);
      return;
    }
    if (e.shiftKey && anchor && ids.includes(anchor)) {
      const [a, b] = [ids.indexOf(anchor), ids.indexOf(id)].sort((x, y) => x - y);
      select(ids.slice(a, b + 1));
    } else if (e.ctrlKey || e.metaKey) {
      select(selection.includes(id) ? selection.filter((x) => x !== id) : [...selection, id], id);
    } else {
      select([id], id);
    }
  };

  const onDoubleClick = (e: MouseEvent) => {
    const id = rowAt(e);
    const t = id ? useApp.getState().tasks[id] : null;
    if (!t) return;
    const { run } = useApp.getState();
    if (t.status === 'completed' || t.status === 'seeding') void run(() => api.open(t.id));
    else if (t.status === 'paused' || t.status === 'failed') void run(() => api.resume([t.id]));
    else if (t.status === 'removed') void run(() => api.restore([t.id]));
  };

  const onContextMenu = (e: MouseEvent) => {
    e.preventDefault();
    const { selection, tasks } = useApp.getState();
    const items = taskMenu(selection, tasks);
    if (items.length) setMenu({ x: e.clientX, y: e.clientY, items });
  };

  const onKeyDown = (e: KeyboardEvent) => {
    const { selection, select, run, openDialog, tasks } = useApp.getState();
    const move = (to: number) => {
      const i = Math.max(0, Math.min(ids.length - 1, to));
      const id = ids[i];
      if (!id) return;
      setFocus(id);
      v.scrollToIndex(i);
      if (e.shiftKey && useApp.getState().anchor) {
        const a = ids.indexOf(useApp.getState().anchor!);
        const [lo, hi] = [a, i].sort((x, y) => x - y);
        select(ids.slice(lo, hi + 1));
      } else if (!e.ctrlKey && !e.metaKey) select([id], id);
    };
    const cur = focus ? ids.indexOf(focus) : -1;
    switch (e.key) {
      case 'ArrowDown':
        e.preventDefault();
        move(cur + 1);
        break;
      case 'ArrowUp':
        e.preventDefault();
        move(cur < 0 ? 0 : cur - 1);
        break;
      case 'Home':
        e.preventDefault();
        move(0);
        break;
      case 'End':
        e.preventDefault();
        move(ids.length - 1);
        break;
      case 'a':
        if (e.ctrlKey || e.metaKey) {
          e.preventDefault();
          select(ids);
        }
        break;
      case 'Delete':
      case 'Backspace':
        if (!selection.length) break;
        e.preventDefault();
        if (filter === 'trash' || e.shiftKey) openDialog({ kind: 'purge', ids: selection });
        else void run(() => api.remove(selection));
        break;
      case ' ': {
        if (!selection.length) break;
        e.preventDefault();
        const running = selection.some((id) => ['queued', 'probing', 'downloading'].includes(tasks[id]?.status));
        void run(() => (running ? api.pause(selection) : api.resume(selection)));
        break;
      }
      case 'Enter': {
        const t = selection.length === 1 ? tasks[selection[0]] : null;
        if (t?.status === 'completed') void run(() => api.open(t.id));
        break;
      }
      case 'Escape':
        select([], null);
        break;
    }
  };

  if (!ids.length) return <EmptyTasks filter={filter} />;

  const items = v.getVirtualItems();
  return (
    <div className="task-table" role="grid" aria-label={tr('table.label')} aria-rowcount={ids.length} aria-multiselectable="true">
      <div className="thead" role="row" style={{ gridTemplateColumns: cols }}>
        {COLUMNS.map(([key, label, cls], i) => {
          // "剩余" is not sortable (it changes every tick); its slot keeps the grid.
          if (i === 4) {
            return (
              <span key="eta" className={cls} role="columnheader">
                {tr(label)}
              </span>
            );
          }
          const active = sort.key === key;
          return (
            <button
              key={key}
              type="button"
              role="columnheader"
              className={cls || undefined}
              aria-sort={active ? (sort.desc ? 'descending' : 'ascending') : 'none'}
              onClick={() => setSort(key)}
            >
              {tr(label)}
              <Icon name={active && sort.desc ? 'chevronDown' : 'chevronUp'} />
            </button>
          );
        })}
      </div>
      <div className="tscroll" ref={scroller} tabIndex={0} onMouseDown={onMouseDown} onDoubleClick={onDoubleClick} onContextMenu={onContextMenu} onKeyDown={onKeyDown}>
        <div className="tbody" style={{ height: v.getTotalSize() }}>
          {items.map((vi) => (
            <Row key={ids[vi.index]} id={ids[vi.index]} start={vi.start} cols={cols} />
          ))}
        </div>
      </div>
      {menu && <ContextMenu x={menu.x} y={menu.y} items={menu.items} onClose={() => setMenu(null)} />}
    </div>
  );
}
