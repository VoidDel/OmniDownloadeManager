// The details panel (docs/development-plan.md §6.2): the selected task's numbers, its segment map
// and the speed of the last two minutes; a torrent shows its files (to choose from), peers and
// trackers instead of segments. Actions at the bottom follow its state.

import { useEffect, useState } from 'react';

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
  const h = speedHistory(id);
  if (h.length < 2) return null;
  const max = Math.max(...h, 1);
  const n = h.length;
  const pts = h.map((v, i) => `${((i / (n - 1)) * 300).toFixed(1)},${(54 - (v / max) * 50).toFixed(1)}`);
  return (
    <div>
      <div className="det-head">
        <span>速度 · 近 2 分钟</span>
        <span className="num">{fmt.speed(speed)}</span>
      </div>
      <svg className="spark" viewBox="0 0 300 56" preserveAspectRatio="none" role="img" aria-label="最近两分钟的下载速度">
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
  return (
    <>
      <dl className="det-dl">
        <dt>大小</dt>
        <dd className="num">{fmt.bytes(t.totalBytes)}</dd>
        <dt>已下载</dt>
        <dd className="num">
          {fmt.bytes(t.status === 'completed' ? t.totalBytes : t.downloadedBytes)}
          {t.totalBytes ? ` · ${t.status === 'completed' ? '100%' : fmt.percent(t.downloadedBytes, t.totalBytes)}` : ''}
        </dd>
        {running && (
          <>
            <dt>速度</dt>
            <dd className="num">{fmt.speed(t.speedBps)}</dd>
            <dt>剩余时间</dt>
            <dd className="num">{fmt.duration(t.etaSecs)}</dd>
            <dt>{bt ? '节点' : '连接'}</dt>
            <dd>{t.connections ? `${t.connections} 个${bt ? '节点' : '连接'}` : '—'}</dd>
          </>
        )}
        {bt && (
          <>
            {t.status === 'seeding' && (
              <>
                <dt>上传速度</dt>
                <dd className="num">{fmt.speed(t.uploadSpeedBps)}</dd>
                <dt>节点</dt>
                <dd>{t.connections ? `${t.connections} 个节点` : '—'}</dd>
              </>
            )}
            <dt>已上传</dt>
            <dd className="num">
              {fmt.bytes(t.uploadedBytes)} · 分享率 {ratio(t)}
            </dd>
            {torrent && (
              <>
                <dt>文件</dt>
                <dd>
                  {torrent.files.filter((f) => f.selected).length} / {torrent.files.length} 个 · {torrent.pieces} 块 × {fmt.bytes(torrent.pieceSize)}
                </dd>
                <dt>特征码</dt>
                <dd className="mono path">{torrent.infoHash}</dd>
              </>
            )}
          </>
        )}
        {!bt && (
          <>
            <dt>断点续传</dt>
            <dd>{t.startedAt == null ? '开始后确定' : t.resumable ? '支持' : '不支持（暂停后需要重新下载）'}</dd>
          </>
        )}
        <dt>添加时间</dt>
        <dd>{fmt.when(t.createdAt)}</dd>
        {t.completedAt != null && (
          <>
            <dt>完成时间</dt>
            <dd>{fmt.when(t.completedAt)}</dd>
          </>
        )}
        <dt>保存到</dt>
        <dd className="mono path">{t.saveDir}</dd>
        <dt>链接</dt>
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
            <span>分片进度</span>
            <span className="muted">{segments.length ? `${segments.length} 个分片` : ''}</span>
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
  const fill = blockFill(segments, t.totalBytes, t.status === 'completed');
  if (t.status === 'completed') return <p className="muted det-note">下载已完成，分片已合并为完整文件。</p>;
  if (!segments.length) return <p className="muted det-note">{t.startedAt == null ? '开始下载后显示分片。' : '没有分片信息。'}</p>;
  return (
    <>
      <div>
        <div className="det-head">
          <span>分片地图</span>
          <span className="muted">{segments.filter((s) => s.active).length} 个连接工作中</span>
        </div>
        <div className="blockmap" aria-hidden="true">
          {fill.map((f, i) => (
            <span key={i} style={{ background: blockColor(f) }} />
          ))}
        </div>
        <div className="legend">
          <span>
            <i style={{ background: 'var(--accent)' }} />
            已完成
          </span>
          <span>
            <i style={{ background: 'color-mix(in srgb, var(--accent) 45%, transparent)' }} />
            部分完成
          </span>
          <span>
            <i style={{ background: 'var(--bg-pressed)' }} />
            未开始
          </span>
        </div>
      </div>
      <div>
        <div className="det-head">
          <span>分片</span>
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
  if (!torrent) return <p className="muted det-note">{t.startedAt == null ? '获取到种子信息后显示文件。' : '没有文件信息。'}</p>;
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
  return (
    <>
      <div>
        <div className="det-head">
          <span>节点</span>
          <span className="muted">{live && torrent ? `已连接 ${torrent.peers.length} · 已发现 ${torrent.peersSeen}` : ''}</span>
        </div>
        {!live || !torrent?.peers.length ? (
          <p className="muted det-note">{live ? '正在寻找节点…' : '任务运行时显示连接的节点。'}</p>
        ) : (
          <div className="peers" role="table" aria-label="节点">
            <div role="row" className="peer head">
              <span role="columnheader">地址</span>
              <span role="columnheader">客户端</span>
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
          <span>Tracker</span>
          <span className="muted">{torrent ? `${torrent.trackers.length} 个` : ''}</span>
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
          <p className="muted det-note">没有 Tracker，靠 DHT 与节点交换寻找节点。</p>
        )}
      </div>
    </>
  );
}

/** The primary action for a task's state. */
function Primary({ t }: { t: Task }) {
  const run = useApp((s) => s.run);
  const [label, icon, act]: [string, 'pause' | 'play' | 'refresh' | 'external', () => Promise<unknown>] = (() => {
    switch (t.status) {
      case 'queued':
      case 'probing':
      case 'downloading':
        return ['暂停', 'pause', () => api.pause([t.id])];
      case 'paused':
        return ['继续', 'play', () => api.resume([t.id])];
      case 'failed':
        return t.error?.code === 'remote_changed' ? ['重新下载', 'refresh', () => api.restart([t.id])] : ['重试', 'refresh', () => api.resume([t.id])];
      case 'seeding':
        return ['停止做种', 'pause', () => api.pause([t.id])];
      case 'completed':
        return ['打开文件', 'external', () => api.open(t.id)];
      default:
        return ['还原', 'refresh', () => api.restore([t.id])];
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
  const setDetailOpen = useApp((s) => s.setDetailOpen);
  const run = useApp((s) => s.run);
  const toast = useApp((s) => s.toast);
  const [tab, setTab] = useState<Tab>('overview');
  const bt = t?.kind === 'torrent';
  const running = t?.status === 'downloading' || t?.status === 'probing' || t?.status === 'seeding';
  const detail = useDetail(id, running, t?.status ?? 'queued', bt ? 1000 : 500);
  if (!t) return null;
  const segments = detail?.segments ?? [];
  const torrent = detail?.torrent;
  const color = STATUS_COLOR[t.status];
  const tabs: [Tab, string][] = bt
    ? [
        ['overview', '概览'],
        ['files', '文件'],
        ['peers', '节点'],
      ]
    : [
        ['overview', '概览'],
        ['segments', '分片'],
      ];
  const shown = tabs.some(([k]) => k === tab) ? tab : 'overview';
  return (
    <aside className="detail" aria-label="任务详情">
      <div className="det-top">
        <h2 title={t.fileName}>{t.fileName}</h2>
        <button type="button" className="iconbtn sm" title="隐藏详情" aria-label="隐藏详情" onClick={() => setDetailOpen(false)}>
          <Icon name="chevronsRight" />
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
      <Tabs id="det" label="详情" value={shown} onChange={setTab} options={tabs} />
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
          打开文件夹
        </button>
        <button
          type="button"
          className="iconbtn outline"
          title="复制链接"
          aria-label="复制链接"
          onClick={() => void navigator.clipboard.writeText(t.source).then(() => toast('success', '已复制链接'))}
        >
          <Icon name="copy" />
        </button>
      </div>
    </aside>
  );
}
