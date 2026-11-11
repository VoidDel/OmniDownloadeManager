// The new-download dialog (docs/development-plan.md §6.3): links one per line (or .torrent
// files), each probed for name, size and range support (a torrent's files) while the user looks;
// folder, connections, limit and request details apply to all of them. A single torrent shows
// its files to choose from.

import { isTauri } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';

import { FileIcon, Icon } from '../../components/Icon';
import { Modal, PasswordInput, Segmented, Select, Tabs } from '../../components/ui';
import { api, asError } from '../../lib/api';
import { ALGO_NAME, parseChecksum } from '../../lib/checksum';
import * as fmt from '../../lib/format';
import { BATCH_MAX, hasBatch, kindOf, nameFromUrl, parseLinks, type Link } from '../../lib/links';
import type { AppError, NewTask, Prefill, ProbeInfo, ProxyConfig, TaskOptions } from '../../lib/types';
import { errorText } from '../../lib/words';
import { useApp } from '../../store/app';
import { FileTree } from '../tasks/FileTree';

type Probe = { state: 'probing' } | { state: 'ok'; info: ProbeInfo } | { state: 'error'; error: AppError };

/** Links probed at once; more are added without a preview (the engine probes them anyway). */
const PROBE_MAX = 20;

type ProxyMode = 'inherit' | 'system' | 'none' | 'custom';
type FtpMode = 'inherit' | 'passive' | 'active';

/** Badge class per kind; HTTP and FTP are shown as is, the others translated where rendered. */
const BADGE: Record<Link['kind'], string> = {
  http: 'proto-http',
  ftp: 'proto-ftp',
  magnet: 'proto-torrent',
  torrent: 'proto-torrent',
};

/** `previewed`: within the links probed here (the rest are probed when they start). */
/** Kinds this version adds. */
const SUPPORTED: ReadonlySet<Link['kind']> = new Set(['http', 'ftp', 'magnet', 'torrent']);

/** Shown in the empty link box under the hint: a link, a magnet link, a numbered series. */
const LINK_EXAMPLES = ['https://example.com/file.zip', 'magnet:?xt=urn:btih:…', 'https://example.com/photo[01-20].jpg'].join('\n');

/** Where the dialog's downloads come from: links typed or pasted, or .torrent files picked. */
type Source = 'links' | 'torrents';
type AdvTab = 'speed' | 'request' | 'mirrors' | 'checksum' | 'proxy';

/** `onRemove`: a button to take the row out (the torrent files picked). */
function LinkRow({ link, probe, previewed, connections, onRemove }: { link: Link; probe?: Probe; previewed: boolean; connections: number; onRemove?: () => void }) {
  const { t } = useTranslation();
  const supported = SUPPORTED.has(link.kind);
  const torrent = link.kind === 'magnet' || link.kind === 'torrent';
  // An FTP folder: its files, a task each.
  const folder = link.kind === 'ftp' && link.url.endsWith('/');
  const name = probe?.state === 'ok' ? probe.info.fileName : link.url;
  let sub: ReactNode;
  if (!supported) sub = <span className="warn-text">{t('newTask.row.unsupported')}</span>;
  else if (!probe && !previewed) sub = t('newTask.row.later');
  else if (!probe || probe.state === 'probing')
    sub = (
      <>
        <span className="pulse" aria-hidden="true" />
        {link.kind === 'magnet' ? t('newTask.row.probingMagnet') : t('newTask.row.probing')}
      </>
    );
  else if (probe.state === 'error')
    sub = (
      <span className="err-text" title={probe.error.message}>
        {t('newTask.row.probeFailed', { error: errorText(probe.error) })}
      </span>
    );
  else if (torrent || folder) {
    const i = probe.info;
    const files = i.files?.length ?? 1;
    sub = [i.total != null ? fmt.bytes(i.total) : t('newTask.row.sizeUnknown'), files > 1 || folder ? t('newTask.row.files', { count: files }) : t('newTask.row.singleFile')].join(' · ');
  } else if (/mpegurl/i.test(probe.info.contentType ?? '')) {
    // An HLS stream: its size shows while it downloads.
    sub = t('newTask.row.hls');
  } else {
    const i = probe.info;
    const parts = [i.total != null ? fmt.bytes(i.total) : t('newTask.row.sizeUnknown'), i.resumable ? t('newTask.row.resumable') : t('newTask.row.notResumable')];
    const most = link.kind === 'ftp' ? Math.min(connections, 8) : connections;
    if (i.resumable && i.total != null) {
      const count = Math.max(1, Math.min(most, Math.floor(i.total / 1048576) || 1));
      parts.push(link.kind === 'ftp' ? t('newTask.row.sessions', { count }) : t('newTask.row.connections', { count }));
    }
    sub = parts.join(' · ');
  }
  const many = folder || (probe?.state === 'ok' && (probe.info.files?.length ?? 0) > 1);
  return (
    <div className="nt-row" data-off={!supported} data-x={onRemove ? true : undefined}>
      {probe?.state === 'ok' ? <FileIcon name={name} isDir={many} /> : <Icon name={torrent ? 'magnet' : 'link'} className="fi" />}
      <span className="nt-txt">
        <span className="nt-name" title={link.url}>
          {name}
        </span>
        <span className="nt-sub">{sub}</span>
      </span>
      <span className={`badge ${BADGE[link.kind]}`}>{link.kind === 'http' ? 'HTTP' : link.kind === 'ftp' ? 'FTP' : t(`newTask.badge.${link.kind}`)}</span>
      {onRemove && (
        <button type="button" className="nt-x" aria-label={t('newTask.removeTorrent', { name })} title={t('newTask.removeTorrent', { name })} onClick={onRemove}>
          <Icon name="x" />
        </button>
      )}
    </div>
  );
}

/** `initial`: the text pasted so far, and request details when the links came from a browser. */
export function NewTaskDialog({ initial }: { initial: Prefill }) {
  const { t } = useTranslation();
  const settings = useApp((s) => s.settings);
  const defaultDir = useApp((s) => s.settings.downloadDir || s.defaultDir);
  const { closeDialog, run, toast, select, setFilter } = useApp.getState();

  // Only .torrent files handed in (dropped on the window, opened with the app): the torrent tab.
  const [init] = useState(() => {
    const given = parseLinks(initial.text);
    return given.length > 0 && given.every((l) => l.kind === 'torrent')
      ? { source: 'torrents' as Source, text: '', torrents: given.map((l) => l.url) }
      : { source: 'links' as Source, text: initial.text, torrents: [] as string[] };
  });
  const [source, setSource] = useState<Source>(init.source);
  const [text, setText] = useState(init.text);
  const [torrents, setTorrents] = useState<string[]>(init.torrents);
  // What is added: the current tab's (the other keeps its own until the dialog closes).
  const links = useMemo(
    () => (source === 'links' ? parseLinks(text) : torrents.map((url): Link => ({ url, kind: 'torrent' }))),
    [source, text, torrents],
  );
  // From a browser the request details are what is worth seeing first.
  const [advTab, setAdvTab] = useState<AdvTab>(initial.referer || initial.cookies || initial.userAgent ? 'request' : 'speed');
  const [probes, setProbes] = useState<Record<string, Probe>>({});
  // Until a folder is typed or picked, each download goes to its category's.
  const [dir, setDir] = useState<string | null>(null);
  const dirFor = useApp((s) => s.dirFor);
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
  const [checksum, setChecksum] = useState('');
  const [mirrorText, setMirrorText] = useState('');
  // From a browser the request details are filled in: show them.
  const [advancedOpen] = useState(Boolean(initial.referer || initial.cookies || initial.userAgent));
  const [proxyMode, setProxyMode] = useState<ProxyMode>('inherit');
  const [proxyUrl, setProxyUrl] = useState('');
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
  // Torrents and magnet links only: a connection count means nothing to them.
  const noConnections = source === 'torrents' || (addable.length > 0 && !hasHttp && !hasFtp);
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
    const timer = setTimeout(() => {
      setProbes((p) => ({ ...p, ...Object.fromEntries(todo.map((l) => [keyOf(l), { state: 'probing' } as Probe])) }));
      for (const l of todo) {
        const key = keyOf(l);
        api.probe(l.url, options).then(
          (info) => setProbes((p) => ({ ...p, [key]: { state: 'ok', info } })),
          (e) => setProbes((p) => ({ ...p, [key]: { state: 'error', error: asError(e) } })),
        );
      }
    }, 350);
    return () => clearTimeout(timer);
  }, [addable, probes, options, keyOf]);

  // The name field starts with the server's name for a single link.
  const shownName = name ?? (singleProbe?.state === 'ok' ? singleProbe.info.fileName : '');

  const known = addable.map((l) => probes[keyOf(l)]).filter((p): p is Extract<Probe, { state: 'ok' }> => p?.state === 'ok');
  const choosing = torrentFiles.length > 1 && chosen.size < torrentFiles.length;
  const totalKnown = choosing
    ? torrentFiles.filter((f) => chosen.has(f.index)).reduce((n, f) => n + f.size, 0)
    : known.reduce((n, p) => n + (p.info.total ?? 0), 0);
  // One file to check: a single HTTP or FTP link (a torrent checks its own pieces).
  const sum = parseChecksum(checksum);
  const sumApplies = addable.length === 1 && addable[0].kind !== 'torrent';
  const sumInvalid = checksum.trim() !== '' && (!sum || !sumApplies);
  // Mirrors: other HTTP addresses of a single HTTP link's file.
  const mirrors = mirrorText
    .split(/\s+/)
    .map((m) => m.trim())
    .filter(Boolean);
  const mirrorsApply = addable.length === 1 && addable[0].kind === 'http';
  const mirrorsInvalid = mirrors.length > 0 && (!mirrorsApply || mirrors.some((m) => kindOf(m) !== 'http'));
  // An FTP folder adds a task per file (the chosen ones when it is the only link).
  const taskCount = addable.reduce((n, l) => {
    if (l.kind !== 'ftp' || !l.url.endsWith('/')) return n + 1;
    if (l === single) return n + (torrentFiles.length ? chosen.size : 0);
    const p = probes[keyOf(l)];
    return n + (p?.state === 'ok' ? (p.info.files?.length ?? 0) : 1);
  }, 0);
  const invalid = speed === undefined || (proxyMode === 'custom' && !proxyUrl.trim()) || sumInvalid || mirrorsInvalid;

  const addTorrentFiles = async () => {
    const files = await open({ multiple: true, title: t('newTask.pickTorrents'), filters: [{ name: 'BitTorrent', extensions: ['torrent'] }] }).catch(() => null);
    const list = Array.isArray(files) ? files : typeof files === 'string' ? [files] : [];
    if (list.length) setTorrents((prev) => [...new Set([...prev, ...list])]);
  };
  /** Choosing the torrent tab with none picked yet opens the file picker at once. */
  const pickSource = (v: Source) => {
    setSource(v);
    if (v === 'torrents' && !torrents.length) void addTorrentFiles();
  };

  const browse = async () => {
    const picked = await open({ directory: true, defaultPath: shownDir || defaultDir || undefined, title: t('newTask.pickDir') }).catch(() => null);
    if (typeof picked === 'string') setDir(picked);
  };
  /** A link's file name as far as it is known: the server's, the one typed, or the link's. */
  const nameOf = (l: Link) => {
    const p = probes[keyOf(l)];
    if (l === single && single?.kind === 'http' && name?.trim()) return name.trim();
    return p?.state === 'ok' ? p.info.fileName : (nameFromUrl(l.url) ?? '');
  };
  const dirs = [...new Set(addable.map((l) => dirFor(nameOf(l))))];
  // One folder for all: shown; several (by category): the field says so.
  const shownDir = dir ?? (dirs.length === 1 ? dirs[0] : '');

  /** `start`: download now, or only add the tasks (paused) to start later. */
  const submit = async (start: boolean) => {
    if (!addable.length || invalid || busy) return;
    setBusy(true);
    const opts = options;
    const items: NewTask[] = addable.map((l) => {
      const p = probes[keyOf(l)];
      return {
        source: l.url,
        saveDir: dir?.trim() || dirFor(nameOf(l)),
        options: {
          ...opts,
          fileName: single?.kind === 'http' && name?.trim() ? name.trim() : null,
          onlyFiles: l === single && choosing ? [...chosen].sort((a, b) => a - b) : null,
          checksum: sumApplies ? sum : null,
          mirrors: mirrorsApply ? mirrors : [],
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
      added.map((task) => task.id),
      added[0]?.id ?? null,
    );
    // Where new tasks land depends on the sort order: bring the first one into view.
    if (added[0]) useApp.getState().revealTask(added[0].id);
    toast('success', added.length > 1 ? t('newTask.addedMany', { count: added.length }) : t('newTask.addedOne', { name: added[0].fileName }));
  };

  const footer = (
    <>
      <span className="grow nt-sum">
        {addable.length > 0 && (
          <>
            {/* A series in a link: how many it made, in place of the plain count. */}
            <b>
              {source === 'links' && hasBatch(text)
                ? t(links.length >= BATCH_MAX ? 'newTask.batchCapped' : 'newTask.batchMade', { count: links.length })
                : t('newTask.summary', { count: taskCount })}
            </b>
            {totalKnown > 0 && ` · ${fmt.bytes(totalKnown)}`}
          </>
        )}
      </span>
      <button type="button" className="btn outline" disabled={!addable.length || invalid || busy} onClick={() => void submit(false)}>
        <Icon name="clock" />
        {t('newTask.later')}
      </button>
      <button type="button" className="btn primary" disabled={!addable.length || invalid || busy} onClick={() => void submit(true)}>
        <Icon name="download" />
        {t('newTask.now')}
      </button>
    </>
  );

  return (
    <Modal title={t('newTask.title')} onClose={closeDialog} footer={footer} width={640} closeLabel={t('newTask.close')}>
      <Tabs<Source>
        id="nt-src"
        label={t('newTask.source')}
        value={source}
        onChange={pickSource}
        options={[
          ['links', t('newTask.links')],
          ['torrents', t('newTask.tabTorrents')],
        ]}
      />
      <div className="nt-src" role="tabpanel" aria-labelledby={`nt-src-${source}`}>
        {source === 'links' ? (
          <div className="field">
            <textarea
              id="nt-urls"
              ref={textRef}
              data-autofocus
              className="inp mono nt-urls"
              rows={3}
              spellCheck={false}
              value={text}
              aria-label={t('newTask.links')}
              placeholder={`${t('newTask.linksHint')}\n\n${LINK_EXAMPLES}`}
              onChange={(e) => setText(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
                  e.preventDefault();
                  void submit(true);
                }
              }}
            />
          </div>
        ) : torrents.length === 0 ? (
          <div className="nt-pick">
            <Icon name="magnet" />
            <b>{t('newTask.torrentsEmpty')}</b>
            <small>{t('newTask.torrentsEmptyHint')}</small>
            <button type="button" className="btn outline" data-autofocus onClick={() => void addTorrentFiles()}>
              <Icon name="folderOpen" />
              {t('newTask.pickTorrents')}
            </button>
          </div>
        ) : null}
        {links.length > 0 && (
          <div className="nt-list">
            {links.map((l) => (
              <LinkRow
                key={l.url}
                link={l}
                probe={probes[keyOf(l)]}
                previewed={isTauri() && addable.indexOf(l) < PROBE_MAX}
                connections={conn}
                onRemove={source === 'torrents' ? () => setTorrents((list) => list.filter((x) => x !== l.url)) : undefined}
              />
            ))}
          </div>
        )}
        {source === 'torrents' && torrents.length > 0 && (
          <button type="button" className="btn subtle sm nt-more" onClick={() => void addTorrentFiles()}>
            <Icon name="plus" />
            {t('newTask.addTorrents')}
          </button>
        )}
      </div>
      {single && torrentFiles.length > 1 && <FileTree files={torrentFiles} selected={chosen} onChange={(files) => setPicked({ url: single.url, files })} />}
      <div className="frow">
        {single?.kind === 'http' && (
          <>
            <label htmlFor="nt-name">{t('newTask.fileName')}</label>
            <input id="nt-name" className="inp" value={shownName} placeholder={t('newTask.fileNamePlaceholder')} onChange={(e) => setName(e.target.value)} />
          </>
        )}
        <label htmlFor="nt-dir">{t('newTask.saveTo')}</label>
        <div className="inline">
          <input
            id="nt-dir"
            className="inp mono"
            value={shownDir}
            placeholder={dirs.length > 1 ? t('newTask.dirByCategory') : defaultDir}
            onChange={(e) => setDir(e.target.value)}
          />
          <button type="button" className="btn outline" onClick={() => void browse()}>
            {t('newTask.browse')}
          </button>
        </div>
      </div>
      {hasFtp && (
        <div className="frow">
          <label htmlFor="nt-user">{t('newTask.ftpAccount')}</label>
          <div className="inline">
            <input id="nt-user" className="inp" value={username} placeholder={t('newTask.ftpAnonymous')} autoComplete="off" spellCheck={false} onChange={(e) => setUsername(e.target.value)} />
            {username.trim() && <PasswordInput id="nt-pass" label={t('newTask.password')} toggleLabel={t('newTask.showPassword')} value={password} onChange={setPassword} />}
          </div>
          <label>{t('newTask.transferMode')}</label>
          <div className="inline">
            <Segmented<FtpMode>
              value={ftpMode}
              label={t('newTask.transferMode')}
              onChange={setFtpMode}
              options={[
                ['inherit', t('newTask.followSettings')],
                ['passive', t('newTask.ftpPassive')],
                ['active', t('newTask.ftpActive')],
              ]}
            />
            {addable.some((l) => l.kind === 'ftp' && l.url.toLowerCase().startsWith('ftps:')) && (
              <label className="chk">
                <input type="checkbox" checked={insecureTls} onChange={(e) => setInsecureTls(e.target.checked)} />
                {t('newTask.insecureTls')}
              </label>
            )}
          </div>
        </div>
      )}
      <details className="nt-adv" open={advancedOpen || undefined}>
        <summary>
          <Icon name="chevronRight" />
          {t('newTask.advanced')}
          <small>{t('newTask.advancedSummary')}</small>
        </summary>
        <Tabs<AdvTab>
          id="nt-adv"
          label={t('newTask.advanced')}
          value={advTab}
          onChange={setAdvTab}
          options={[
            ['speed', t('newTask.advSpeed'), Boolean(connections.trim() || limit.trim())],
            ['request', t('newTask.advRequest'), Boolean(referer.trim() || cookies.trim() || userAgent.trim())],
            ['mirrors', t('newTask.mirrors'), mirrors.length > 0],
            ['checksum', t('newTask.checksum'), checksum.trim() !== ''],
            ['proxy', t('newTask.proxy'), proxyMode !== 'inherit'],
          ]}
        />
        {/* Every panel in one grid cell, the hidden ones invisible: the section is as tall as the
            tallest, so the dialog does not jump when the tab changes. */}
        <div className="nt-adv-panels">
          <div className="frow" role="tabpanel" aria-labelledby="nt-adv-speed" data-on={advTab === 'speed'} inert={advTab !== 'speed'}>
            <label htmlFor="nt-conn" className="top">
              {t('newTask.connections')}
            </label>
            <div className="nt-sum-field">
              <input
                id="nt-conn"
                className="inp nt-num"
                type="number"
                min={1}
                max={hasHttp || !hasFtp ? 64 : 8}
                value={connections}
                disabled={noConnections}
                placeholder={String(defaultConnections)}
                aria-describedby="nt-conn-hint"
                onChange={(e) => setConnections(e.target.value)}
              />
              <small id="nt-conn-hint" className="muted">
                {noConnections ? t('newTask.connectionsBt') : t('newTask.connectionsHint')}
              </small>
            </div>
            <label htmlFor="nt-limit" className="top">
              {t('newTask.limit')}
            </label>
            <div className="nt-sum-field">
              <input
                id="nt-limit"
                className="inp nt-num"
                value={limit}
                placeholder={t('newTask.limitPlaceholder')}
                aria-invalid={speed === undefined}
                aria-describedby="nt-limit-hint"
                onChange={(e) => setLimit(e.target.value)}
              />
              <small id="nt-limit-hint" className={speed === undefined ? 'err-text' : 'muted'}>
                {t('newTask.limitHint')}
              </small>
            </div>
          </div>
          <div className="frow" role="tabpanel" aria-labelledby="nt-adv-request" data-on={advTab === 'request'} inert={advTab !== 'request'}>
            <label htmlFor="nt-ref">Referer</label>
            <input id="nt-ref" className="inp mono" value={referer} placeholder="https://" onChange={(e) => setReferer(e.target.value)} />
            <label htmlFor="nt-cookie">Cookie</label>
            <input id="nt-cookie" className="inp mono" value={cookies} placeholder="name=value; other=value" onChange={(e) => setCookies(e.target.value)} />
            <label htmlFor="nt-ua">User-Agent</label>
            <input id="nt-ua" className="inp mono" value={userAgent} placeholder={t('newTask.followSettings')} onChange={(e) => setUserAgent(e.target.value)} />
          </div>
          <div className="nt-sum-field" role="tabpanel" aria-labelledby="nt-adv-mirrors" data-on={advTab === 'mirrors'} inert={advTab !== 'mirrors'}>
            <textarea
              id="nt-mirrors"
              className="inp mono nt-mirrors"
              rows={3}
              value={mirrorText}
              aria-label={t('newTask.mirrors')}
              placeholder={t('newTask.mirrorsPlaceholder')}
              aria-invalid={mirrorsInvalid}
              aria-describedby="nt-mirrors-hint"
              spellCheck={false}
              onChange={(e) => setMirrorText(e.target.value)}
            />
            <small id="nt-mirrors-hint" className={mirrorsInvalid ? 'err-text' : 'muted'}>
              {!mirrors.length
                ? t('newTask.mirrorsHint')
                : !mirrorsApply
                  ? t('newTask.mirrorsOneLink')
                  : mirrorsInvalid
                    ? t('newTask.mirrorsInvalid')
                    : t('newTask.mirrorsCount', { count: mirrors.length })}
            </small>
          </div>
          <div className="nt-sum-field" role="tabpanel" aria-labelledby="nt-adv-checksum" data-on={advTab === 'checksum'} inert={advTab !== 'checksum'}>
            <input
              id="nt-sum"
              className="inp mono"
              value={checksum}
              aria-label={t('newTask.checksum')}
              placeholder={t('newTask.checksumPlaceholder')}
              aria-invalid={sumInvalid}
              aria-describedby="nt-sum-hint"
              spellCheck={false}
              onChange={(e) => setChecksum(e.target.value)}
            />
            <small id="nt-sum-hint" className={sumInvalid ? 'err-text' : 'muted'}>
              {checksum.trim() === ''
                ? t('newTask.checksumHint')
                : !sumApplies
                  ? t('newTask.checksumOneFile')
                  : sum
                    ? t('newTask.checksumAlgo', { algo: ALGO_NAME[sum.algo] })
                    : t('checksum.invalid')}
            </small>
          </div>
          <div className="inline nt-proxy" role="tabpanel" aria-labelledby="nt-adv-proxy" data-on={advTab === 'proxy'} inert={advTab !== 'proxy'}>
            <Select<ProxyMode>
              id="nt-proxy"
              label={t('newTask.proxy')}
              value={proxyMode}
              onChange={setProxyMode}
              options={[
                ['inherit', t('newTask.followSettings')],
                ['system', t('newTask.proxySystem')],
                ['none', t('newTask.proxyNone')],
                ['custom', t('newTask.proxyCustom')],
              ]}
            />
            {proxyMode === 'custom' && (
              <input className="inp mono" value={proxyUrl} placeholder="socks5h://127.0.0.1:1080" aria-label={t('newTask.proxyUrl')} onChange={(e) => setProxyUrl(e.target.value)} />
            )}
          </div>
        </div>
      </details>
    </Modal>
  );
}
