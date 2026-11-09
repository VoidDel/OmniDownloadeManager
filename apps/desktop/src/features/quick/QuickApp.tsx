// The quick-download window (docs/development-plan.md §6.3): a download taken over from the
// browser, waiting for the user. Enter starts it, Esc adds it paused, "let the browser download
// it" hands it back. The window shows itself once it has something to show.

import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { open } from '@tauri-apps/plugin-dialog';
import { useEffect, useRef, useState } from 'react';

import { FileIcon, Icon, Logo } from '../../components/Icon';
import { api, asError } from '../../lib/api';
import * as fmt from '../../lib/format';
import { nextFrame } from '../../lib/frame';
import { nameFromUrl } from '../../lib/links';
import type { AppError, ProbeInfo, QuickDecision, QuickRequest } from '../../lib/types';
import { errorText } from '../../lib/words';

const BROWSER: Record<string, string> = { chrome: 'Chrome', edge: 'Edge', firefox: 'Firefox', brave: 'Brave', chromium: 'Chromium' };

export function QuickApp() {
  const [req, setReq] = useState<QuickRequest | null>(null);
  const [name, setName] = useState('');
  const [dir, setDir] = useState('');
  const [connections, setConnections] = useState('8');
  const [limit, setLimit] = useState('');
  const [remember, setRemember] = useState(false);
  const [probe, setProbe] = useState<ProbeInfo | null>(null);
  const [probeError, setProbeError] = useState<AppError | null>(null);
  const busy = useRef(false);
  const nameEdited = useRef(false);

  // What to ask about; then show the window.
  useEffect(() => {
    if (!isTauri()) return;
    void api.quickRequest().then((r) => {
      if (!r) {
        void getCurrentWindow().close();
        return;
      }
      setReq(r);
      setName(r.fileName ?? nameFromUrl(r.url) ?? '');
      setDir(r.saveDir);
      setConnections(String(r.connections));
      nextFrame(() =>
        nextFrame(() => {
          const w = getCurrentWindow();
          void w.show().then(() => w.setFocus());
        }),
      );
    });
  }, []);

  // Ask the server as the browser would (cookies, referer, user agent).
  useEffect(() => {
    if (!req) return;
    let live = true;
    api.probe(req.url, req.options).then(
      (p) => {
        if (!live) return;
        setProbe(p);
        if (!nameEdited.current && !req.fileName) setName(p.fileName);
      },
      (e) => live && setProbeError(asError(e)),
    );
    return () => {
      live = false;
    };
  }, [req]);

  const speed = fmt.parseSpeed(limit);
  const decide = (action: QuickDecision['action']) => {
    if (busy.current) return;
    if ((action === 'start' || action === 'later') && speed === undefined) return;
    busy.current = true;
    void api
      .quickResolve({
        action,
        fileName: name.trim() || null,
        saveDir: dir.trim() || null,
        connections: Math.max(1, Math.min(64, Number.parseInt(connections, 10) || req?.connections || 8)),
        speedLimit: speed ?? null,
        remember,
        probe,
      })
      .catch(() => {
        busy.current = false;
      });
  };
  const decideRef = useRef(decide);
  useEffect(() => {
    decideRef.current = decide;
  });

  // Enter starts, Esc adds it for later (buttons keep their own Enter).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        decideRef.current('later');
      } else if (e.key === 'Enter' && !(e.target instanceof HTMLButtonElement)) {
        e.preventDefault();
        decideRef.current('start');
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  const browse = async () => {
    const picked = await open({ directory: true, defaultPath: dir || undefined, title: '选择保存位置' }).catch(() => null);
    if (typeof picked === 'string') setDir(picked);
  };

  if (!req) return <div className="quick" />;
  const size = probe?.total ?? req.size;
  const conn = Math.max(1, Math.min(64, Number.parseInt(connections, 10) || req.connections));
  const parts: string[] = [];
  if (probe) {
    parts.push(probe.resumable ? '支持断点续传' : '不支持断点续传');
    if (probe.resumable && probe.total != null) parts.push(`将使用 ${Math.max(1, Math.min(conn, Math.floor(probe.total / 1048576) || 1))} 个连接`);
  }
  const extras = [req.hasCookies && 'Cookie', req.hasReferer && 'Referer'].filter(Boolean).join(' 与 ');
  return (
    <div className="quick">
      <header data-tauri-drag-region>
        <Logo size={18} />
        <h1 data-tauri-drag-region>
          来自 {BROWSER[req.browser ?? ''] ?? '浏览器'} 的下载
          <span>{req.site}</span>
        </h1>
        <button type="button" className="q-close" aria-label="关闭" title="关闭（不下载）" onClick={() => decide('cancel')}>
          <Icon name="x" />
        </button>
      </header>
      <div className="q-body">
        <label htmlFor="qa-name">文件名</label>
        <div className="q-name">
          <FileIcon name={name || 'file'} isDir={false} />
          <input
            id="qa-name"
            className="inp"
            value={name}
            spellCheck={false}
            onChange={(e) => {
              nameEdited.current = true;
              setName(e.target.value);
            }}
          />
        </div>
        <span />
        <div className="q-info">
          <b>{size != null ? fmt.bytes(size) : '大小未知'}</b>
          <span className="badge proto-http">HTTP</span>
          {!probe && !probeError && (
            <span className="q-probing">
              <span className="pulse" aria-hidden="true" />
              正在获取文件信息…
            </span>
          )}
          {probeError && (
            <span className="err-text" title={probeError.message}>
              {errorText(probeError)}
            </span>
          )}
          {parts.map((p) => (
            <span key={p} className="q-part">
              {p}
            </span>
          ))}
          {extras && (
            <span className="q-ok">
              <Icon name="check" />
              已附带 {extras}
            </span>
          )}
        </div>
        <label htmlFor="qa-dir">保存到</label>
        <div className="inline">
          <input id="qa-dir" className="inp mono" value={dir} onChange={(e) => setDir(e.target.value)} />
          <button type="button" className="btn outline" onClick={() => void browse()}>
            浏览
          </button>
        </div>
        <label htmlFor="qa-conn">连接数</label>
        <div className="inline q-nums">
          <input id="qa-conn" className="inp" type="number" min={1} max={64} value={connections} onChange={(e) => setConnections(e.target.value)} />
          <label htmlFor="qa-limit">限速</label>
          <input id="qa-limit" className="inp" value={limit} placeholder="不限" aria-invalid={speed === undefined} title="例如 500K、2M；留空不限速" onChange={(e) => setLimit(e.target.value)} />
        </div>
        <span />
        <label className="chk q-remember">
          <input type="checkbox" checked={remember} onChange={(e) => setRemember(e.target.checked)} />
          {req.site || '这个网站'} 的下载以后不再询问，直接按这里的设置开始
        </label>
      </div>
      <footer>
        <span className="q-keys">
          <kbd>Enter</kbd> 开始 · <kbd>Esc</kbd> 稍后
        </span>
        <button type="button" className="btn subtle" onClick={() => decide('browser')}>
          改用浏览器下载
        </button>
        <button type="button" className="btn outline" onClick={() => decide('later')}>
          稍后下载
        </button>
        <button type="button" className="btn primary" data-autofocus onClick={() => decide('start')}>
          <Icon name="download" />
          开始下载
        </button>
      </footer>
    </div>
  );
}
