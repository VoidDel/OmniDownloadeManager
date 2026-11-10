// The details panel (docs/development-plan.md §6.2): the selected task's numbers, its segment map
// and the speed of the last two minutes; a torrent shows its files (to choose from), peers and
// trackers instead of segments. Actions at the bottom follow its state.

import { Fragment, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { Icon } from '../../components/Icon';
import { Tabs } from '../../components/ui';
import { api } from '../../lib/api';
import { ALGO_NAME, parseChecksum } from '../../lib/checksum';
import * as fmt from '../../lib/format';
import { hostOf } from '../../lib/links';
import type { TaskDetail as Detail, SegmentView, Task, TorrentDetail } from '../../lib/types';
import { STATUS_COLOR, checksumFailed, errorHint, errorText, nameParts, statusText, textColor } from '../../lib/words';
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
  const avg = fmt.averageSpeed(t);
  // The stored peak arrives with status changes; while running, the recent samples too.
  const peak = Math.max(t.peakBps ?? 0, ...speedHistory(t.id));
  const gone = useApp((s) => !!s.missing[t.id]) && t.status === 'completed';
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
        {avg != null && (
          <>
            <dt>{tr('detail.overview.avgSpeed')}</dt>
            <dd>{fmt.speed(avg)}</dd>
          </>
        )}
        {peak > 0 && (
          <>
            <dt>{tr('detail.overview.peakSpeed')}</dt>
            <dd>{fmt.speed(peak)}</dd>
          </>
        )}
        <dt>{tr('detail.overview.saveTo')}</dt>
        <dd className="mono path">{t.saveDir}</dd>
        <dt>{tr('detail.overview.link')}</dt>
        <dd>
          <LongText text={t.source} />
        </dd>
        {!!t.options.mirrors?.length && (
          <>
            <dt>{tr('detail.overview.mirrors')}</dt>
            <dd>
              <LongText text={t.options.mirrors.join('\n')} />
            </dd>
          </>
        )}
      </dl>
      {gone && (
        <div className="det-err warn" role="status">
          <Icon name="alert" />
          <div>
            <b>{tr('status.fileMissing')}</b>
            <small>{tr('detail.fileMissing')}</small>
          </div>
        </div>
      )}
      {t.error && (
        <div className="det-err" role="status">
          <Icon name="alert" />
          <div>
            <b>{errorText(t.error)}</b>
            {errorHint(t.error) && <span className="hint">{errorHint(t.error)}</span>}
            {/* A checksum mismatch's detail (path, value) is in the checksum block below. */}
            {t.error.code !== 'checksum_mismatch' && <small>{t.error.message}</small>}
          </div>
        </div>
      )}
      {!bt && <ChecksumBlock t={t} />}
      {/* Pushed to the bottom of the panel; the facts above stay at the top. */}
      <div className="det-bottom" aria-hidden="true" />
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

/** File checksums: the value the download should have (checked when it finishes, or now for a
 *  finished one), and the values of a finished file, to compare with the site's by eye. */
function ChecksumBlock({ t }: { t: Task }) {
  const { t: tr } = useTranslation();
  const run = useApp((s) => s.run);
  const toast = useApp((s) => s.toast);
  const gone = useApp((s) => !!s.missing[t.id]);
  const [value, setValue] = useState('');
  const [busy, setBusy] = useState<'set' | 'hash' | null>(null);
  const [digests, setDigests] = useState<[string, string][] | null>(null);
  const finished = t.status === 'completed' && !gone;
  const sum = t.options.checksum ?? null;
  const bad = checksumFailed(t);
  const parsed = parseChecksum(value);
  const apply = async () => {
    if (!parsed || busy) return;
    setBusy('set');
    const done = await run(() => api.setChecksum(t.id, value));
    setBusy(null);
    if (done) setValue('');
  };
  const compute = async () => {
    setBusy('hash');
    const d = await run(() => api.digests(t.id));
    setBusy(null);
    if (d) setDigests(d);
  };
  const copy = (hex: string) => void navigator.clipboard.writeText(hex).then(() => toast('success', tr('detail.checksum.copied')));
  return (
    <div className="det-sum">
      <div className="det-head">
        <span>{tr('detail.checksum.title')}</span>
        {sum && (
          <button type="button" className="lnk" onClick={() => void run(() => api.setChecksum(t.id, ''))}>
            {tr('detail.checksum.clear')}
          </button>
        )}
      </div>
      {sum && (
        <div className={`sum-state ${!finished ? 'wait' : bad ? 'bad' : 'ok'}`}>
          <Icon name={!finished ? 'clock' : bad ? 'alert' : 'checkCircle'} />
          <span className="grow">{tr(!finished ? 'detail.checksum.pending' : bad ? 'detail.checksum.bad' : 'detail.checksum.ok')}</span>
          <code className="mono" title={sum.hex}>
            {ALGO_NAME[sum.algo]}
          </code>
        </div>
      )}
      {sum && <code className="mono path sum-hex">{sum.hex}</code>}
      <form
        className="inline sum-form"
        onSubmit={(e) => {
          e.preventDefault();
          void apply();
        }}
      >
        <input
          className="inp mono"
          value={value}
          spellCheck={false}
          aria-label={tr('detail.checksum.placeholder')}
          aria-invalid={value.trim() !== '' && !parsed}
          placeholder={tr(sum ? 'detail.checksum.replace' : 'detail.checksum.placeholder')}
          onChange={(e) => setValue(e.target.value)}
        />
        <button type="submit" className="btn outline" disabled={!parsed || busy !== null}>
          {tr(finished ? 'detail.checksum.check' : 'detail.checksum.set')}
        </button>
      </form>
      {finished &&
        (digests ? (
          <dl className="det-dl sum-list">
            {digests.map(([algo, hex]) => (
              <Fragment key={algo}>
                <dt>{algo}</dt>
                <dd className="sum-row">
                  <span className="mono path">{hex}</span>
                  <button type="button" className="iconbtn" title={tr('detail.checksum.copy')} aria-label={tr('detail.checksum.copyAlgo', { algo })} onClick={() => copy(hex)}>
                    <Icon name="copy" />
                  </button>
                </dd>
              </Fragment>
            ))}
          </dl>
        ) : (
          <button type="button" className="lnk sum-compute" disabled={busy !== null} onClick={() => void compute()}>
            {tr(busy === 'hash' ? 'detail.checksum.computing' : 'detail.checksum.compute')}
          </button>
        ))}
    </div>
  );
}

/** A long text (a link with a long query, say) cut to three lines, with a button to show it all. */
function LongText({ text }: { text: string }) {
  const { t: tr } = useTranslation();
  const [open, setOpen] = useState(false);
  const [long, setLong] = useState(false);
  const ref = useRef<HTMLSpanElement>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const check = () => setLong(el.scrollHeight > el.clientHeight + 1);
    check();
    const sizes = new ResizeObserver(check);
    sizes.observe(el);
    return () => sizes.disconnect();
  }, [text]);
  return (
    <>
      <span ref={ref} className={`mono path${open ? '' : ' clamp'}`} title={open ? undefined : text}>
        {text}
      </span>
      {(long || open) && (
        <button type="button" className="lnk more" aria-expanded={open} onClick={() => setOpen(!open)}>
          {tr(open ? 'detail.showLess' : 'detail.showMore')}
        </button>
      )}
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
  const gone = useApp((s) => !!s.missing[t.id]);
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
        return gone
          ? [tr('detail.action.redownload'), 'refresh', () => api.restart([t.id])]
          : [tr('detail.action.openFile'), 'external', () => useApp.getState().openTask(t.id)];
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
        <h2 title={t.fileName}>
          <span className="nm-base">{nameParts(t.fileName)[0]}</span>
          <span className="nm-ext">{nameParts(t.fileName)[1]}</span>
        </h2>
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
        <span className="st" style={{ color: textColor(color) }}>
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
