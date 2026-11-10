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
  const { run, openDialog, openDetail, openTask, missing } = useApp.getState();
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
  // The remote file changed, or the finished file was deleted: download it again.
  const changed = list.filter((t) => t.error?.code === 'remote_changed' || (t.status === 'completed' && missing[t.id])).map((t) => t.id);
  const items: MenuItem[] = [];
  if ((one?.status === 'completed' && !missing[one.id]) || one?.status === 'seeding') items.push({ label: i18n.t('table.menu.open'), icon: 'external', kbd: 'Enter', run: () => void openTask(one.id) });
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
const Row = memo(function Row({ id, start, cols, cursor }: { id: string; start: number; cols: string; cursor: boolean }) {
  const t = useApp((s) => s.tasks[id]);
  const selected = useApp((s) => s.selection.includes(id));
  const gone = useApp((s) => !!s.missing[id]);
  if (!t) return null;
  // Finished, but the file was deleted or moved since.
  const fileGone = gone && t.status === 'completed';
  const color = fileGone ? 'var(--warning)' : STATUS_COLOR[t.status];
  const running = t.status === 'downloading' || t.status === 'probing';
  const p = progressOf(t);
  const seeding = t.status === 'seeding';
  const pct = t.status === 'completed' || seeding ? '100%' : fmt.percent(t.downloadedBytes, t.totalBytes);
  const size = t.totalBytes != null ? fmt.bytes(t.totalBytes) : t.downloadedBytes ? fmt.bytes(t.downloadedBytes) : '—';
  const remaining = running ? fmt.duration(t.etaSecs) : t.status === 'completed' ? fmt.when(t.completedAt) : '—';
  // Finished: the average speed over the whole download.
  const avg = t.status === 'completed' && !fileGone ? fmt.averageSpeed(t) : null;
  const title = t.error ? `${errorText(t.error)}\n${t.error.message}` : fileGone ? i18n.t('detail.fileMissing') : undefined;
  return (
    <div
      role="row"
      id={`task-${id}`}
      className={`trow task-row${cursor ? ' cursor' : ''}`}
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
      <span className="dim num c-speed" role="gridcell" title={avg != null ? i18n.t('table.avgSpeed') : undefined}>
        {seeding ? `↑ ${fmt.speed(t.uploadSpeedBps)}` : running && t.speedBps ? fmt.speed(t.speedBps) : avg != null ? fmt.speed(avg) : '—'}
      </span>
      <span className="dim num c-eta" role="gridcell">
        {remaining}
      </span>
      <span className="st" role="gridcell" style={{ color }} title={title}>
        <i className="sdot" style={{ background: color }} />
        {/* The reason is in the tooltip and the details panel; the column only says the state. */}
        {fileGone ? i18n.t('status.fileMissing') : statusText(t)}
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
  // Keyboard focus is on the grid itself; the row it is at is `aria-activedescendant`.
  const table = useRef<HTMLDivElement>(null);
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
    table.current?.focus({ preventScroll: true });
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
    const { run, openTask } = useApp.getState();
    if (t.status === 'completed' || t.status === 'seeding') void openTask(t.id);
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
    // Keys pressed on a column header's button are that button's.
    if (e.target !== e.currentTarget) return;
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
        if (t?.status === 'completed' || t?.status === 'seeding') void useApp.getState().openTask(t.id);
        break;
      }
      case 'Escape':
        select([], null);
        break;
      case 'ContextMenu':
      case 'F10': {
        // The menu key (or Shift+F10): the menu of the selection, at the row the cursor is on.
        if (e.key === 'F10' && !e.shiftKey) break;
        e.preventDefault();
        const items = taskMenu(selection, tasks);
        if (!items.length) break;
        const row = (focus && document.getElementById(`task-${focus}`)) || table.current?.querySelector('.task-row[aria-selected="true"]');
        const r = (row ?? e.currentTarget).getBoundingClientRect();
        setMenu({ x: r.left + 48, y: r.top + Math.min(r.height, rowH), items });
        break;
      }
    }
  };

  if (!ids.length) return <EmptyTasks filter={filter} />;

  const items = v.getVirtualItems();
  return (
    <div
      className="task-table"
      role="grid"
      ref={table}
      tabIndex={0}
      aria-label={tr('table.label')}
      aria-rowcount={ids.length}
      aria-multiselectable="true"
      aria-activedescendant={focus && items.some((vi) => ids[vi.index] === focus) ? `task-${focus}` : undefined}
      onKeyDown={onKeyDown}
      onFocus={(e) => {
        // Tabbed into: the cursor starts at the selection, or at the first row.
        if (e.target === e.currentTarget && (!focus || !ids.includes(focus))) setFocus(useApp.getState().selection.find((id) => ids.includes(id)) ?? ids[0] ?? null);
      }}
    >
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
          // The header cell holds the button: a button cannot itself be a column header.
          return (
            <span key={key} role="columnheader" className={cls || undefined} aria-sort={active ? (sort.desc ? 'descending' : 'ascending') : 'none'}>
              <button type="button" onClick={() => setSort(key)}>
                {tr(label)}
                <Icon name={active && sort.desc ? 'chevronDown' : 'chevronUp'} />
              </button>
            </span>
          );
        })}
      </div>
      <div className="tscroll" role="rowgroup" ref={scroller} onMouseDown={onMouseDown} onDoubleClick={onDoubleClick} onContextMenu={onContextMenu}>
        <div className="tbody" role="presentation" style={{ height: v.getTotalSize() }}>
          {items.map((vi) => (
            <Row key={ids[vi.index]} id={ids[vi.index]} start={vi.start} cols={cols} cursor={focus === ids[vi.index]} />
          ))}
        </div>
      </div>
      {menu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          items={menu.items}
          onClose={() => {
            setMenu(null);
            // Back to the list, so the keyboard carries on from where the menu was opened.
            table.current?.focus({ preventScroll: true });
          }}
        />
      )}
    </div>
  );
}
