// The details panel (docs/development-plan.md §6.2): the selected task's numbers, its segment map
// and the speed of the last two minutes; a torrent shows its files (to choose from), peers and
// trackers instead of segments. Actions at the bottom follow its state.

import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { Icon } from '../../components/Icon';
import { Tabs } from '../../components/ui';
import { api } from '../../lib/api';
import * as fmt from '../../lib/format';
import { hostOf } from '../../lib/links';
import type { TaskDetail as Detail, SegmentView, Task, TorrentDetail } from '../../lib/types';
import { STATUS_COLOR, errorText, statusText } from '../../lib/words';
import { speedHistory, useApp } from '../../store/app';
import { FileTree } from './FileTree';

type Tab = 'overview' | 'segments' | 'files' | 'peers';
const BLOCKS = 64;
const PROTO: Record<Task['kind'], string> = { http: 'HTTP', ftp: 'FTP', torrent: 'BT' };

/** How much of each of `BLOCKS` equal parts of the file is downloaded: 0, partial or 1. */
export function blockFill(segments: SegmentView[], total: number | null, complete: boolean): number[] {
  if (complete) return Array(BLOCKS).fill(1);
  if (!total || !segments.length) return Array(BLOCKS).fill(0);
  const size = total / BLOCKS;
  const done = new Float64Array(BLOCKS);
  for (const s of segments) {
    let a = s.start;
    const b = s.start + s.downloaded; // exclusive
    while (a < b) {
      const i = Math.min(BLOCKS - 1, Math.floor(a / size));
      const end = Math.min(b, (i + 1) * size);
      done[i] += end - a;
      a = end;
    }
  }
  return Array.from(done, (d) => Math.min(1, d / size));
}

function blockColor(f: number): string {
  if (f >= 0.999) return 'var(--accent)';
  if (f > 0) return 'color-mix(in srgb, var(--accent) 45%, transparent)';
  return 'var(--bg-pressed)';
}

/** Segments (and a torrent's files and peers) of a task, refreshed while it runs. */
function useDetail(id: string, running: boolean, status: Task['status'], every: number): Detail | null {
  const [detail, setDetail] = useState<Detail | null>(null);
  useEffect(() => {
    let live = true;
    const load = () =>
      void api
        .detail(id)
        .then((d) => live && setDetail(d))
        .catch(() => undefined);
    load();
    const t = running ? setInterval(load, every) : undefined;
    return () => {
      live = false;
      clearInterval(t);
    };
    // `status`: reload once when it settles (paused, failed).
  }, [id, running, status, every]);
  return detail?.task.id === id ? detail : null;
}

/** Speed over the last two minutes. */
function SpeedChart({ id, speed }: { id: string; speed: number }) {
  const { t } = useTranslation();
  const h = speedHistory(id);
  if (h.length < 2) return null;
  const max = Math.max(...h, 1);
  const n = h.length;
  const pts = h.map((v, i) => `${((i / (n - 1)) * 300).toFixed(1)},${(54 - (v / max) * 50).toFixed(1)}`);
  return (
    <div>
      <div className="det-head">
        <span>{t('detail.chart.title')}</span>
        <span className="num">{fmt.speed(speed)}</span>
      </div>
      <svg className="spark" viewBox="0 0 300 56" preserveAspectRatio="none" role="img" aria-label={t('detail.chart.label')}>
        <polygon points={`0,56 ${pts.join(' ')} 300,56`} fill="var(--accent)" opacity=".14" />
        <polyline points={pts.join(' ')} fill="none" stroke="var(--accent)" strokeWidth="1.5" strokeLinejoin="round" vectorEffect="non-scaling-stroke" />
      </svg>
    </div>
  );
}

/** Uploaded over size. */
const ratio = (t: Task) => (t.totalBytes ? (t.uploadedBytes / t.totalBytes).toFixed(2) : '—');

function Overview({ t, segments, torrent }: { t: Task; segments: SegmentView[]; torrent?: TorrentDetail }) {
  const running = t.status === 'downloading' || t.status === 'probing';
  const bt = t.kind === 'torrent';
  const fill = blockFill(segments, t.totalBytes, t.status === 'completed');
  const { t: tr } = useTranslation();
  return (
    <>
      <dl className="det-dl">
        <dt>{tr('detail.overview.size')}</dt>
        <dd className="num">{fmt.bytes(t.totalBytes)}</dd>
        <dt>{tr('detail.overview.downloaded')}</dt>
        <dd className="num">
          {fmt.bytes(t.status === 'completed' ? t.totalBytes : t.downloadedBytes)}
          {t.totalBytes ? ` · ${t.status === 'completed' ? '100%' : fmt.percent(t.downloadedBytes, t.totalBytes)}` : ''}
        </dd>
        {running && (
          <>
            <dt>{tr('detail.overview.speed')}</dt>
            <dd className="num">{fmt.speed(t.speedBps)}</dd>
            <dt>{tr('detail.overview.eta')}</dt>
            <dd className="num">{fmt.duration(t.etaSecs)}</dd>
            <dt>{tr(bt ? 'detail.overview.peers' : 'detail.overview.connections')}</dt>
            <dd>{t.connections ? tr(bt ? 'detail.overview.peerCount' : 'detail.overview.connectionCount', { count: t.connections }) : '—'}</dd>
          </>
        )}
        {bt && (
          <>
            {t.status === 'seeding' && (
              <>
                <dt>{tr('detail.overview.uploadSpeed')}</dt>
                <dd className="num">{fmt.speed(t.uploadSpeedBps)}</dd>
                <dt>{tr('detail.overview.peers')}</dt>
                <dd>{t.connections ? tr('detail.overview.peerCount', { count: t.connections }) : '—'}</dd>
              </>
            )}
            <dt>{tr('detail.overview.uploaded')}</dt>
            <dd className="num">{tr('detail.overview.uploadedValue', { size: fmt.bytes(t.uploadedBytes), ratio: ratio(t) })}</dd>
            {torrent && (
              <>
                <dt>{tr('detail.overview.files')}</dt>
                <dd>
                  {tr('detail.overview.filesValue', {
                    selected: torrent.files.filter((f) => f.selected).length,
                    count: torrent.files.length,
                    pieces: torrent.pieces,
                    pieceSize: fmt.bytes(torrent.pieceSize),
                  })}
                </dd>
                <dt>{tr('detail.overview.infoHash')}</dt>
                <dd className="mono path">{torrent.infoHash}</dd>
              </>
            )}
          </>
        )}
        {!bt && (
          <>
            <dt>{tr('detail.overview.resume')}</dt>
            <dd>{tr(t.startedAt == null ? 'detail.overview.resumeUnknown' : t.resumable ? 'detail.overview.resumeYes' : 'detail.overview.resumeNo')}</dd>
          </>
        )}
        <dt>{tr('detail.overview.added')}</dt>
        <dd>{fmt.when(t.createdAt)}</dd>
        {t.completedAt != null && (
          <>
            <dt>{tr('detail.overview.completed')}</dt>
            <dd>{fmt.when(t.completedAt)}</dd>
          </>
        )}
        <dt>{tr('detail.overview.saveTo')}</dt>
        <dd className="mono path">{t.saveDir}</dd>
        <dt>{tr('detail.overview.link')}</dt>
        <dd className="mono path">{t.source}</dd>
      </dl>
      {t.error && (
        <div className="det-err" role="status">
          <Icon name="alert" />
          <div>
            <b>{errorText(t.error)}</b>
            <small>{t.error.message}</small>
          </div>
        </div>
      )}
      {!bt && t.totalBytes != null && t.totalBytes > 0 && (
        <div>
          <div className="det-head">
            <span>{tr('detail.overview.segmentProgress')}</span>
            <span className="muted">{segments.length ? tr('detail.overview.segmentCount', { count: segments.length }) : ''}</span>
          </div>
          <div className="blocks" aria-hidden="true">
            {fill.map((f, i) => (
              <span key={i} style={{ background: blockColor(f) }} />
            ))}
          </div>
        </div>
      )}
      {running && <SpeedChart id={t.id} speed={t.speedBps} />}
    </>
  );
}

function Segments({ t, segments }: { t: Task; segments: SegmentView[] }) {
  const { t: tr } = useTranslation();
  const fill = blockFill(segments, t.totalBytes, t.status === 'completed');
  if (t.status === 'completed') return <p className="muted det-note">{tr('detail.segments.merged')}</p>;
  if (!segments.length) return <p className="muted det-note">{tr(t.startedAt == null ? 'detail.segments.notStarted' : 'detail.segments.none')}</p>;
  return (
    <>
      <div>
        <div className="det-head">
          <span>{tr('detail.segments.map')}</span>
          <span className="muted">{tr('detail.segments.active', { count: segments.filter((s) => s.active).length })}</span>
        </div>
        <div className="blockmap" aria-hidden="true">
          {fill.map((f, i) => (
            <span key={i} style={{ background: blockColor(f) }} />
          ))}
        </div>
        <div className="legend">
          <span>
            <i style={{ background: 'var(--accent)' }} />
            {tr('detail.segments.done')}
          </span>
          <span>
            <i style={{ background: 'color-mix(in srgb, var(--accent) 45%, transparent)' }} />
            {tr('detail.segments.partial')}
          </span>
          <span>
            <i style={{ background: 'var(--bg-pressed)' }} />
            {tr('detail.segments.pending')}
          </span>
        </div>
      </div>
      <div>
        <div className="det-head">
          <span>{tr('detail.segments.title')}</span>
        </div>
        <div className="seglist">
          {segments.map((s, i) => {
            const len = s.end != null ? s.end - s.start + 1 : null;
            return (
              <div key={s.start} className="segrow" data-active={s.active}>
                <span className="mono muted">#{i + 1}</span>
                <span className="segmid">
                  <span className="num">
                    {fmt.bytes(s.start)} – {s.end != null ? fmt.bytes(s.end + 1) : '?'}
                  </span>
                  <span className="minibar">
                    <i style={{ width: `${len ? (s.downloaded / len) * 100 : 0}%` }} />
                  </span>
                </span>
                <span className="num">{len ? fmt.percent(s.downloaded, len) : fmt.bytes(s.downloaded)}</span>
              </div>
            );
          })}
        </div>
      </div>
    </>
  );
}

/** A torrent's files: which to download (until it is complete), and how far each is. */
function Files({ t, torrent }: { t: Task; torrent?: TorrentDetail }) {
  const run = useApp((s) => s.run);
  const [pending, setPending] = useState<Set<number> | null>(null);
  const { t: tr } = useTranslation();
  if (!torrent) return <p className="muted det-note">{tr(t.startedAt == null ? 'detail.files.pending' : 'detail.files.none')}</p>;
  const editable = ['queued', 'probing', 'downloading', 'paused', 'failed'].includes(t.status);
  const selected = pending ?? new Set(torrent.files.filter((f) => f.selected).map((f) => f.index));
  const choose = (next: Set<number>) => {
    setPending(next);
    void run(() => api.setTorrentFiles(t.id, [...next].sort((a, b) => a - b))).finally(() => setPending(null));
  };
  return <FileTree files={torrent.files} selected={selected} onChange={editable ? choose : undefined} />;
}

/** Connected peers and the trackers. */
function Peers({ t, torrent }: { t: Task; torrent?: TorrentDetail }) {
  const live = t.status === 'downloading' || t.status === 'seeding';
  const { t: tr } = useTranslation();
  return (
    <>
      <div>
        <div className="det-head">
          <span>{tr('detail.peers.title')}</span>
          <span className="muted">{live && torrent ? tr('detail.peers.stats', { connected: torrent.peers.length, seen: torrent.peersSeen }) : ''}</span>
        </div>
        {!live || !torrent?.peers.length ? (
          <p className="muted det-note">{tr(live ? 'detail.peers.searching' : 'detail.peers.idle')}</p>
        ) : (
          <div className="peers" role="table" aria-label={tr('detail.peers.title')}>
            <div role="row" className="peer head">
              <span role="columnheader">{tr('detail.peers.address')}</span>
              <span role="columnheader">{tr('detail.peers.client')}</span>
              <span role="columnheader" className="num">
                ↓
              </span>
              <span role="columnheader" className="num">
                ↑
              </span>
            </div>
            {torrent.peers.map((p) => (
              <div role="row" key={p.addr} className="peer">
                <span role="cell" className="mono" title={`${p.addr} · ${p.transport.toUpperCase()}`}>
                  {p.addr}
                </span>
                <span role="cell" className="dim" title={p.client ?? undefined}>
                  {p.client ?? '—'}
                </span>
                <span role="cell" className="num">
                  {fmt.bytes(p.downloaded)}
                </span>
                <span role="cell" className="num">
                  {fmt.bytes(p.uploaded)}
                </span>
              </div>
            ))}
          </div>
        )}
      </div>
      <div>
        <div className="det-head">
          <span>{tr('detail.trackers.title')}</span>
          <span className="muted">{torrent ? tr('detail.trackers.count', { count: torrent.trackers.length }) : ''}</span>
        </div>
        {torrent?.trackers.length ? (
          <ul className="trackers">
            {torrent.trackers.map((u) => (
              <li key={u} className="mono">
                {u}
              </li>
            ))}
          </ul>
        ) : (
          <p className="muted det-note">{tr('detail.trackers.none')}</p>
        )}
      </div>
    </>
  );
}

/** The primary action for a task's state. */
function Primary({ t }: { t: Task }) {
  const run = useApp((s) => s.run);
  const { t: tr } = useTranslation();
  const [label, icon, act]: [string, 'pause' | 'play' | 'refresh' | 'external', () => Promise<unknown>] = (() => {
    switch (t.status) {
      case 'queued':
      case 'probing':
      case 'downloading':
        return [tr('detail.action.pause'), 'pause', () => api.pause([t.id])];
      case 'paused':
        return [tr('detail.action.resume'), 'play', () => api.resume([t.id])];
      case 'failed':
        return t.error?.code === 'remote_changed'
          ? [tr('detail.action.redownload'), 'refresh', () => api.restart([t.id])]
          : [tr('detail.action.retry'), 'refresh', () => api.resume([t.id])];
      case 'seeding':
        return [tr('detail.action.stopSeeding'), 'pause', () => api.pause([t.id])];
      case 'completed':
        return [tr('detail.action.openFile'), 'external', () => api.open(t.id)];
      default:
        return [tr('detail.action.restore'), 'refresh', () => api.restore([t.id])];
    }
  })();
  return (
    <button type="button" className="btn primary grow" onClick={() => void run(act)}>
      <Icon name={icon} />
      {label}
    </button>
  );
}

export function TaskDetail({ id }: { id: string }) {
  const t = useApp((s) => s.tasks[id]);
  const pinned = useApp((s) => s.detailPinned);
  const { closeDetail, setDetailPinned } = useApp.getState();
  const run = useApp((s) => s.run);
  const toast = useApp((s) => s.toast);
  const [tab, setTab] = useState<Tab>('overview');
  const { t: tr } = useTranslation();
  const bt = t?.kind === 'torrent';
  const running = t?.status === 'downloading' || t?.status === 'probing' || t?.status === 'seeding';
  const detail = useDetail(id, running, t?.status ?? 'queued', bt ? 1000 : 500);
  if (!t) return null;
  const segments = detail?.segments ?? [];
  const torrent = detail?.torrent;
  const color = STATUS_COLOR[t.status];
  const tabs: [Tab, string][] = bt
    ? [
        ['overview', tr('detail.tab.overview')],
        ['files', tr('detail.tab.files')],
        ['peers', tr('detail.tab.peers')],
      ]
    : [
        ['overview', tr('detail.tab.overview')],
        ['segments', tr('detail.tab.segments')],
      ];
  const shown = tabs.some(([k]) => k === tab) ? tab : 'overview';
  return (
    <aside className="detail" aria-label={tr('detail.label')}>
      <div className="det-top">
        <h2 title={t.fileName}>{t.fileName}</h2>
        <button type="button" className="iconbtn" title={tr('detail.close')} aria-label={tr('detail.close')} onClick={closeDetail}>
          <Icon name="x" />
        </button>
        {/* The same switch as the command bar's, in the same place: highlighted, the panel opens
            for every task selected. */}
        <button
          type="button"
          className="iconbtn"
          aria-pressed={pinned}
          title={tr(pinned ? 'detail.unpinTip' : 'detail.pinTip')}
          aria-label={tr('detail.pin')}
          onClick={() => setDetailPinned(!pinned)}
        >
          <Icon name="sidebar" />
        </button>
      </div>
      <div className="det-meta">
        <span className={`badge proto-${t.kind}`}>{PROTO[t.kind]}</span>
        <span className="st" style={{ color }}>
          <i className="sdot" style={{ background: color }} />
          {statusText(t)}
        </span>
        {hostOf(t.finalUrl ?? t.source) && (
          <>
            <span className="dot" />
            <span className="ellipsis">{hostOf(t.finalUrl ?? t.source)}</span>
          </>
        )}
      </div>
      <Tabs id="det" label={tr('detail.tabs')} value={shown} onChange={setTab} options={tabs} />
      <div className="det-body">
        {shown === 'overview' && <Overview t={t} segments={segments} torrent={torrent} />}
        {shown === 'segments' && <Segments t={t} segments={segments} />}
        {shown === 'files' && <Files t={t} torrent={torrent} />}
        {shown === 'peers' && <Peers t={t} torrent={torrent} />}
      </div>
      <div className="det-foot">
        <Primary t={t} />
        <button type="button" className="btn outline" onClick={() => void run(() => api.reveal(t.id))}>
          <Icon name="folderOpen" />
          {tr('detail.openFolder')}
        </button>
        <button
          type="button"
          className="iconbtn outline"
          title={tr('detail.copyLink')}
          aria-label={tr('detail.copyLink')}
          onClick={() => void navigator.clipboard.writeText(t.source).then(() => toast('success', tr('detail.copied')))}
        >
          <Icon name="copy" />
        </button>
      </div>
    </aside>
  );
}
