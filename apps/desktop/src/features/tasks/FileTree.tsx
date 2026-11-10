// A torrent's files as a folder tree with check boxes: which files to download, in the
// new-download dialog and the details panel (where it also shows each file's progress).

import { useEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';

import { FileIcon, Icon } from '../../components/Icon';
import * as fmt from '../../lib/format';

export interface TreeFile {
  index: number;
  /** Relative, `/`-separated. */
  path: string;
  size: number;
  /** Verified bytes, while the task runs. */
  done?: number | null;
}

interface Folder {
  name: string;
  key: string;
  folders: Folder[];
  files: TreeFile[];
  /** Every file below, for the folder's check box and size. */
  all: number[];
  size: number;
}

function build(files: readonly TreeFile[]): Folder {
  const root: Folder = { name: '', key: '', folders: [], files: [], all: [], size: 0 };
  for (const f of files) {
    const parts = f.path.split('/');
    let node = root;
    node.all.push(f.index);
    node.size += f.size;
    for (const part of parts.slice(0, -1)) {
      const key = `${node.key}/${part}`;
      let next = node.folders.find((d) => d.key === key);
      if (!next) {
        next = { name: part, key, folders: [], files: [], all: [], size: 0 };
        node.folders.push(next);
      }
      next.all.push(f.index);
      next.size += f.size;
      node = next;
    }
    node.files.push(f);
  }
  return root;
}

function Check({ state, label, onChange, disabled }: { state: 'on' | 'off' | 'some'; label: string; onChange: (on: boolean) => void; disabled?: boolean }) {
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (ref.current) ref.current.indeterminate = state === 'some';
  }, [state]);
  return <input ref={ref} type="checkbox" aria-label={label} checked={state === 'on'} disabled={disabled} onChange={(e) => onChange(e.target.checked)} />;
}

const stateOf = (ids: number[], selected: ReadonlySet<number>) => {
  const n = ids.filter((i) => selected.has(i)).length;
  return n === 0 ? 'off' : n === ids.length ? 'on' : 'some';
};

/** `onChange` absent: read only. At least one file stays chosen. */
export function FileTree({ files, selected, onChange }: { files: readonly TreeFile[]; selected: ReadonlySet<number>; onChange?: (next: Set<number>) => void }) {
  const { t } = useTranslation();
  const root = useMemo(() => build(files), [files]);
  const [closed, setClosed] = useState<ReadonlySet<string>>(new Set());
  const set = (ids: number[], on: boolean) => {
    if (!onChange) return;
    const next = new Set(selected);
    for (const i of ids) {
      if (on) next.add(i);
      else next.delete(i);
    }
    if (next.size > 0) onChange(next);
  };
  const toggle = (key: string) =>
    setClosed((c) => {
      const next = new Set(c);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  const rows: ReactNode[] = [];
  const walk = (folder: Folder, depth: number) => {
    for (const d of folder.folders) {
      const open = !closed.has(d.key);
      rows.push(
        <div key={d.key} className="ft-row" style={{ '--depth': depth } as CSSProperties}>
          <Check state={stateOf(d.all, selected)} label={d.name} disabled={!onChange} onChange={(on) => set(d.all, on)} />
          <button type="button" className="ft-fold" aria-expanded={open} onClick={() => toggle(d.key)}>
            <Icon name="chevronRight" />
            <Icon name={open ? 'folderOpen' : 'folder'} className="ft-ic" />
            <span className="ft-name">{d.name}</span>
          </button>
          <span className="ft-size">{fmt.bytes(d.size)}</span>
        </div>,
      );
      if (open) walk(d, depth + 1);
    }
    for (const f of folder.files) {
      const name = f.path.split('/').pop() ?? f.path;
      const on = selected.has(f.index);
      rows.push(
        <label key={f.index} className="ft-row" data-off={!on} style={{ '--depth': depth } as CSSProperties}>
          <Check state={on ? 'on' : 'off'} label={name} disabled={!onChange} onChange={(v) => set([f.index], v)} />
          <span className="ft-file">
            <FileIcon name={name} />
            <span className="ft-name" title={f.path}>
              {name}
            </span>
          </span>
          <span className="ft-size">
            {f.done != null && on && f.size > 0 && <span className="ft-pct">{Math.floor((f.done / f.size) * 100)}%</span>}
            {fmt.bytes(f.size)}
          </span>
        </label>,
      );
    }
  };
  walk(root, 0);

  const chosen = files.filter((f) => selected.has(f.index));
  return (
    <div className="ft">
      <div className="ft-head">
        <Check state={stateOf(root.all, selected)} label={t('fileTree.all')} disabled={!onChange} onChange={(on) => (on ? set(root.all, true) : undefined)} />
        <span>{t('fileTree.chosen', { chosen: chosen.length, count: files.length, size: fmt.bytes(chosen.reduce((n, f) => n + f.size, 0)) })}</span>
      </div>
      <div className="ft-list">{rows}</div>
    </div>
  );
}
