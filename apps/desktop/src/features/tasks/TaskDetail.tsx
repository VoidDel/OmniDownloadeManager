// The details panel (docs/development-plan.md §6.2): the selected task's numbers, its segment map
// and the speed of the last two minutes; actions at the bottom follow its state.

import { useEffect, useState } from 'react';

import { Icon } from '../../components/Icon';
import { Tabs } from '../../components/ui';
import { api } from '../../lib/api';
import * as fmt from '../../lib/format';
import { hostOf } from '../../lib/links';
import type { SegmentView, Task } from '../../lib/types';
import { STATUS_COLOR, STATUS_LABEL, errorText } from '../../lib/words';
import { speedHistory, useApp } from '../../store/app';

type Tab = 'overview' | 'segments';
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

/** The segments of a task, refreshed twice a second while it runs. */
function useSegments(id: string, running: boolean, status: Task['status']): SegmentView[] {
  const [segments, setSegments] = useState<SegmentView[]>([]);
  useEffect(() => {
    let live = true;
    const load = () =>
      void api
        .detail(id)
        .then((d) => live && setSegments(d.segments))
        .catch(() => undefined);
    load();
    const t = running ? setInterval(load, 500) : undefined;
    return () => {
      live = false;
      clearInterval(t);
    };
    // `status`: reload once when it settles (paused, failed).
  }, [id, running, status]);
  return segments;
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

function Overview({ t, segments }: { t: Task; segments: SegmentView[] }) {
  const running = t.status === 'downloading' || t.status === 'probing';
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
            <dt>连接</dt>
            <dd>{t.connections ? `${t.connections} 个连接` : '—'}</dd>
          </>
        )}
        <dt>断点续传</dt>
        <dd>{t.startedAt == null ? '开始后确定' : t.resumable ? '支持' : '不支持（暂停后需要重新下载）'}</dd>
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
      {t.totalBytes != null && t.totalBytes > 0 && (
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
      case 'completed':
      case 'seeding':
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
  const running = t?.status === 'downloading' || t?.status === 'probing';
  const segments = useSegments(id, running, t?.status ?? 'queued');
  if (!t) return null;
  const color = STATUS_COLOR[t.status];
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
          {STATUS_LABEL[t.status]}
        </span>
        {hostOf(t.finalUrl ?? t.source) && (
          <>
            <span className="dot" />
            <span className="ellipsis">{hostOf(t.finalUrl ?? t.source)}</span>
          </>
        )}
      </div>
      <Tabs
        id="det"
        label="详情"
        value={tab}
        onChange={setTab}
        options={[
          ['overview', '概览'],
          ['segments', '分片'],
        ]}
      />
      <div className="det-body">{tab === 'overview' ? <Overview t={t} segments={segments} /> : <Segments t={t} segments={segments} />}</div>
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
