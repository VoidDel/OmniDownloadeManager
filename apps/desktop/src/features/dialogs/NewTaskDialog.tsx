// The new-download dialog (docs/development-plan.md §6.3): links one per line (or .torrent
// files), each probed for name, size and range support (a torrent's files) while the user looks;
// folder, connections, limit and request details apply to all of them. A single torrent shows
// its files to choose from.

import { isTauri } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';

import { FileIcon, Icon } from '../../components/Icon';
import { Modal, PasswordInput, Segmented, Select } from '../../components/ui';
import { api, asError } from '../../lib/api';
import * as fmt from '../../lib/format';
import { parseLinks, type Link } from '../../lib/links';
import type { AppError, NewTask, Prefill, ProbeInfo, ProxyConfig, TaskOptions } from '../../lib/types';
import { errorText } from '../../lib/words';
import { useApp } from '../../store/app';
import { FileTree } from '../tasks/FileTree';

type Probe = { state: 'probing' } | { state: 'ok'; info: ProbeInfo } | { state: 'error'; error: AppError };

/** Links probed at once; more are added without a preview (the engine probes them anyway). */
const PROBE_MAX = 20;

type ProxyMode = 'inherit' | 'system' | 'none' | 'custom';
type FtpMode = 'inherit' | 'passive' | 'active';

const BADGE: Record<Link['kind'], [string, string]> = {
  http: ['HTTP', 'proto-http'],
  ftp: ['FTP', 'proto-ftp'],
  magnet: ['磁力', 'proto-torrent'],
  torrent: ['种子', 'proto-torrent'],
};

/** `previewed`: within the links probed here (the rest are probed when they start). */
/** Kinds this version adds. */
const SUPPORTED: ReadonlySet<Link['kind']> = new Set(['http', 'ftp', 'magnet', 'torrent']);

function LinkRow({ link, probe, previewed, connections }: { link: Link; probe?: Probe; previewed: boolean; connections: number }) {
  const supported = SUPPORTED.has(link.kind);
  const torrent = link.kind === 'magnet' || link.kind === 'torrent';
  const name = probe?.state === 'ok' ? probe.info.fileName : link.url;
  let sub: ReactNode;
  if (!supported) sub = <span className="warn-text">这类链接暂不支持，这一行不会添加</span>;
  else if (!probe && !previewed) sub = '开始下载时获取文件信息';
  else if (!probe || probe.state === 'probing')
    sub = (
      <>
        <span className="pulse" aria-hidden="true" />
        {link.kind === 'magnet' ? '正在向节点获取种子信息…' : '正在获取文件信息…'}
      </>
    );
  else if (probe.state === 'error') sub = <span className="err-text" title={probe.error.message}>{errorText(probe.error)}，仍可添加，开始时会重试</span>;
  else if (torrent) {
    const i = probe.info;
    const files = i.files?.length ?? 1;
    sub = [i.total != null ? fmt.bytes(i.total) : '大小未知', files > 1 ? `${files} 个文件` : '单个文件'].join(' · ');
  } else {
    const i = probe.info;
    const parts = [i.total != null ? fmt.bytes(i.total) : '大小未知', i.resumable ? '支持断点续传' : '不支持断点续传'];
    const most = link.kind === 'ftp' ? Math.min(connections, 8) : connections;
    const unit = link.kind === 'ftp' ? '会话' : '连接';
    if (i.resumable && i.total != null) parts.push(`将使用 ${Math.max(1, Math.min(most, Math.floor(i.total / 1048576) || 1))} 个${unit}`);
    sub = parts.join(' · ');
  }
  const many = probe?.state === 'ok' && (probe.info.files?.length ?? 0) > 1;
  return (
    <div className="nt-row" data-off={!supported}>
      {probe?.state === 'ok' ? <FileIcon name={name} isDir={many} /> : <Icon name={torrent ? 'magnet' : 'link'} className="fi" />}
      <span className="nt-txt">
        <span className="nt-name" title={link.url}>
          {name}
        </span>
        <span className="nt-sub">{sub}</span>
      </span>
      <span className={`badge ${BADGE[link.kind][1]}`}>{BADGE[link.kind][0]}</span>
    </div>
  );
}

/** `initial`: the text pasted so far, and request details when the links came from a browser. */
export function NewTaskDialog({ initial }: { initial: Prefill }) {
  const settings = useApp((s) => s.settings);
  const defaultDir = useApp((s) => s.settings.downloadDir || s.defaultDir);
  const { closeDialog, run, toast, select, setFilter } = useApp.getState();

  const [text, setText] = useState(initial.text);
  const links = useMemo(() => parseLinks(text), [text]);
  const [probes, setProbes] = useState<Record<string, Probe>>({});
  const [dir, setDir] = useState(defaultDir);
  const [name, setName] = useState<string | null>(null);
  // Empty: the setting (HTTP and FTP have their own).
  const [connections, setConnections] = useState('');
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [ftpMode, setFtpMode] = useState<FtpMode>('inherit');
  const [insecureTls, setInsecureTls] = useState(false);
  const [limit, setLimit] = useState('');
  const [referer, setReferer] = useState(initial.referer ?? '');
  const [cookies, setCookies] = useState(initial.cookies ?? '');
  const [userAgent, setUserAgent] = useState(initial.userAgent ?? '');
  // From a browser the request details are filled in: show them.
  const [advancedOpen] = useState(Boolean(initial.referer || initial.cookies || initial.userAgent));
  const [proxyMode, setProxyMode] = useState<ProxyMode>('inherit');
  const [proxyUrl, setProxyUrl] = useState('');
  const [start, setStart] = useState(true);
  const [busy, setBusy] = useState(false);
  const textRef = useRef<HTMLTextAreaElement>(null);

  const addable = useMemo(() => links.filter((l) => SUPPORTED.has(l.kind)), [links]);
  const hasHttp = addable.some((l) => l.kind === 'http');
  const hasFtp = addable.some((l) => l.kind === 'ftp');
  // FTP links are probed again when the account or TLS choice changes.
  const ftpKey = JSON.stringify([username, password, ftpMode, insecureTls]);
  const keyOf = useCallback((l: Link) => (l.kind === 'ftp' ? `${l.url}\u0000${ftpKey}` : l.url), [ftpKey]);
  const single = addable.length === 1 ? addable[0] : null;
  const singleProbe = single ? probes[keyOf(single)] : undefined;
  // The files of a single torrent: all chosen until the user unticks some.
  const torrentFiles = useMemo(
    () => (single && single.kind !== 'http' && singleProbe?.state === 'ok' ? (singleProbe.info.files ?? []).map((f, index) => ({ ...f, index })) : []),
    [single, singleProbe],
  );
  const [picked, setPicked] = useState<{ url: string; files: Set<number> } | null>(null);
  const chosen = picked && picked.url === single?.url ? picked.files : new Set(torrentFiles.map((f) => f.index));
  const defaultConnections = hasHttp || !hasFtp ? settings.engine.connections : settings.engine.ftpConnections;
  const conn = Math.max(1, Math.min(64, Number.parseInt(connections, 10) || defaultConnections));
  const speed = fmt.parseSpeed(limit);

  const options = useMemo((): TaskOptions => {
    const proxy: ProxyConfig | null =
      proxyMode === 'inherit' ? null : proxyMode === 'custom' ? { mode: 'custom', url: proxyUrl.trim() } : { mode: proxyMode };
    return {
      fileName: null,
      connections: connections.trim() ? conn : null,
      speedLimit: speed ?? null,
      userAgent: userAgent.trim() || null,
      referer: referer.trim() || null,
      cookies: cookies.trim() || null,
      headers: {},
      proxy,
      onlyFiles: null,
      username: username.trim() || null,
      password: username.trim() && password ? password : null,
      ftpActive: ftpMode === 'inherit' ? null : ftpMode === 'active',
      insecureTls: insecureTls || null,
    };
  }, [conn, connections, speed, userAgent, referer, cookies, proxyMode, proxyUrl, username, password, ftpMode, insecureTls]);

  // Probe new links shortly after typing stops (with the request details set so far).
  useEffect(() => {
    if (!isTauri()) return;
    const todo = addable.slice(0, PROBE_MAX).filter((l) => !probes[keyOf(l)]);
    if (!todo.length) return;
    const t = setTimeout(() => {
      setProbes((p) => ({ ...p, ...Object.fromEntries(todo.map((l) => [keyOf(l), { state: 'probing' } as Probe])) }));
      for (const l of todo) {
        const key = keyOf(l);
        api.probe(l.url, options).then(
          (info) => setProbes((p) => ({ ...p, [key]: { state: 'ok', info } })),
          (e) => setProbes((p) => ({ ...p, [key]: { state: 'error', error: asError(e) } })),
        );
      }
    }, 350);
    return () => clearTimeout(t);
  }, [addable, probes, options, keyOf]);

  // The name field starts with the server's name for a single link.
  const shownName = name ?? (singleProbe?.state === 'ok' ? singleProbe.info.fileName : '');

  const known = addable.map((l) => probes[keyOf(l)]).filter((p): p is Extract<Probe, { state: 'ok' }> => p?.state === 'ok');
  const choosing = torrentFiles.length > 1 && chosen.size < torrentFiles.length;
  const totalKnown = choosing
    ? torrentFiles.filter((f) => chosen.has(f.index)).reduce((n, f) => n + f.size, 0)
    : known.reduce((n, p) => n + (p.info.total ?? 0), 0);
  const invalid = speed === undefined || (proxyMode === 'custom' && !proxyUrl.trim());

  const addTorrentFiles = async () => {
    const files = await open({ multiple: true, title: '选择种子文件', filters: [{ name: 'BitTorrent', extensions: ['torrent'] }] }).catch(() => null);
    const list = Array.isArray(files) ? files : typeof files === 'string' ? [files] : [];
    if (list.length) setText((t) => [t.trim(), ...list].filter(Boolean).join('\n'));
  };

  const browse = async () => {
    const picked = await open({ directory: true, defaultPath: dir || undefined, title: '选择保存位置' }).catch(() => null);
    if (typeof picked === 'string') setDir(picked);
  };

  const submit = async () => {
    if (!addable.length || invalid || busy) return;
    setBusy(true);
    const opts = options;
    const items: NewTask[] = addable.map((l) => {
      const p = probes[keyOf(l)];
      return {
        source: l.url,
        saveDir: dir.trim() || defaultDir,
        options: {
          ...opts,
          fileName: single?.kind === 'http' && name?.trim() ? name.trim() : null,
          onlyFiles: l === single && choosing ? [...chosen].sort((a, b) => a - b) : null,
        },
        start,
        probe: p?.state === 'ok' ? p.info : null,
      };
    });
    const added = await run(() => api.add(items));
    setBusy(false);
    if (!added) return;
    closeDialog();
    if (useApp.getState().filter === 'trash' || useApp.getState().view !== 'tasks') setFilter('all');
    select(
      added.map((t) => t.id),
      added[0]?.id ?? null,
    );
    toast('success', added.length > 1 ? `已添加 ${added.length} 个任务` : `已添加 ${added[0].fileName}`);
  };

  const footer = (
    <>
      <span className="grow nt-sum">
        {addable.length > 0 && (
          <>
            <b>{addable.length} 个任务</b>
            {totalKnown > 0 && ` · ${fmt.bytes(totalKnown)}`}
          </>
        )}
      </span>
      <button type="button" className="btn outline" onClick={closeDialog}>
        取消
      </button>
      <button type="button" className="btn primary" disabled={!addable.length || invalid || busy} onClick={() => void submit()}>
        {start ? '添加并开始' : '添加'}
      </button>
    </>
  );

  return (
    <Modal title="新建下载" onClose={closeDialog} footer={footer} width={640} closeLabel="关闭">
      <div className="field">
        <label htmlFor="nt-urls">链接</label>
        <textarea
          id="nt-urls"
          ref={textRef}
          data-autofocus
          className="inp mono nt-urls"
          rows={3}
          spellCheck={false}
          value={text}
          placeholder="https://example.com/file.zip 或 magnet:?xt=…"
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
              e.preventDefault();
              void submit();
            }
          }}
        />
        <div className="nt-hintrow">
          <small className="hint">每行一个：HTTP、FTP 链接，磁力链接或种子文件的路径。</small>
          <button type="button" className="btn subtle sm" onClick={() => void addTorrentFiles()}>
            <Icon name="folderOpen" />
            打开种子文件
          </button>
        </div>
      </div>
      {links.length > 0 && (
        <div className="nt-list">
          {links.map((l) => (
            <LinkRow key={l.url} link={l} probe={probes[keyOf(l)]} previewed={isTauri() && addable.indexOf(l) < PROBE_MAX} connections={conn} />
          ))}
        </div>
      )}
      {single && torrentFiles.length > 1 && <FileTree files={torrentFiles} selected={chosen} onChange={(files) => setPicked({ url: single.url, files })} />}
      <div className="frow">
        {single?.kind === 'http' && (
          <>
            <label htmlFor="nt-name">文件名</label>
            <input id="nt-name" className="inp" value={shownName} placeholder="按服务器提供的名称" onChange={(e) => setName(e.target.value)} />
          </>
        )}
        <label htmlFor="nt-dir">保存到</label>
        <div className="inline">
          <input id="nt-dir" className="inp mono" value={dir} onChange={(e) => setDir(e.target.value)} />
          <button type="button" className="btn outline" onClick={() => void browse()}>
            浏览
          </button>
        </div>
        <label htmlFor={hasHttp || hasFtp ? 'nt-conn' : 'nt-limit'}>{hasHttp || hasFtp ? '连接数' : '限速'}</label>
        <div className="inline nt-nums">
          {(hasHttp || hasFtp) && (
            <>
              <input
                id="nt-conn"
                className="inp"
                type="number"
                min={1}
                max={hasHttp ? 64 : 8}
                value={connections}
                placeholder={String(defaultConnections)}
                onChange={(e) => setConnections(e.target.value)}
              />
              <label htmlFor="nt-limit">限速</label>
            </>
          )}
          <input
            id="nt-limit"
            className="inp"
            value={limit}
            placeholder="不限"
            aria-invalid={speed === undefined}
            title="例如 500K、2M；留空不限速"
            onChange={(e) => setLimit(e.target.value)}
          />
        </div>
      </div>
      {hasFtp && (
        <div className="frow">
          <label htmlFor="nt-user">FTP 账号</label>
          <div className="inline">
            <input id="nt-user" className="inp" value={username} placeholder="留空为匿名登录" autoComplete="off" spellCheck={false} onChange={(e) => setUsername(e.target.value)} />
            {username.trim() && <PasswordInput id="nt-pass" label="密码" toggleLabel="显示密码" value={password} onChange={setPassword} />}
          </div>
          <label>传输模式</label>
          <div className="inline">
            <Segmented<FtpMode>
              value={ftpMode}
              label="传输模式"
              onChange={setFtpMode}
              options={[
                ['inherit', '跟随设置'],
                ['passive', '被动'],
                ['active', '主动'],
              ]}
            />
            {addable.some((l) => l.kind === 'ftp' && l.url.toLowerCase().startsWith('ftps:')) && (
              <label className="chk">
                <input type="checkbox" checked={insecureTls} onChange={(e) => setInsecureTls(e.target.checked)} />
                接受未验证的证书
              </label>
            )}
          </div>
        </div>
      )}
      <details className="nt-adv" open={advancedOpen || undefined}>
        <summary>
          <Icon name="chevronRight" />
          高级
          <small>Referer · Cookie · User-Agent · 代理</small>
        </summary>
        <div className="frow">
          <label htmlFor="nt-ref">Referer</label>
          <input id="nt-ref" className="inp mono" value={referer} placeholder="https://" onChange={(e) => setReferer(e.target.value)} />
          <label htmlFor="nt-cookie">Cookie</label>
          <input id="nt-cookie" className="inp mono" value={cookies} placeholder="name=value; other=value" onChange={(e) => setCookies(e.target.value)} />
          <label htmlFor="nt-ua">User-Agent</label>
          <input id="nt-ua" className="inp mono" value={userAgent} placeholder="跟随设置" onChange={(e) => setUserAgent(e.target.value)} />
          <label htmlFor="nt-proxy">代理</label>
          <div className="inline">
            <Select<ProxyMode>
              id="nt-proxy"
              value={proxyMode}
              onChange={setProxyMode}
              options={[
                ['inherit', '跟随设置'],
                ['system', '系统代理'],
                ['none', '不使用代理'],
                ['custom', '自定义'],
              ]}
            />
            {proxyMode === 'custom' && <input className="inp mono" value={proxyUrl} placeholder="socks5h://127.0.0.1:1080" aria-label="代理地址" onChange={(e) => setProxyUrl(e.target.value)} />}
          </div>
        </div>
      </details>
      <label className="chk">
        <input type="checkbox" checked={start} onChange={(e) => setStart(e.target.checked)} />
        添加后立即开始
      </label>
    </Modal>
  );
}
