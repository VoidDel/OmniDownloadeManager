// The quick-download window (docs/development-plan.md §6.3): a download taken over from the
// browser, waiting for the user. Enter starts it, Esc adds it paused, "let the browser download
// it" hands it back. A link copied to the clipboard or dropped on the floating ball asks the same
// way, without the browser's parts; for a copied link Esc drops it (copying is not asking). The
// window shows itself once it has something to show.

import { isTauri } from '@tauri-apps/api/core';
import { LogicalSize } from '@tauri-apps/api/dpi';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { open } from '@tauri-apps/plugin-dialog';
import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { FileIcon, Icon, Logo } from '../../components/Icon';
import { api, asError } from '../../lib/api';
import * as fmt from '../../lib/format';
import { nextFrame } from '../../lib/frame';
import { nameFromUrl } from '../../lib/links';
import type { AppError, ProbeInfo, QuickDecision, QuickRequest } from '../../lib/types';
import { errorText } from '../../lib/words';
import { useApp } from '../../store/app';

const BROWSER: Record<string, string> = { chrome: 'Chrome', edge: 'Edge', firefox: 'Firefox', brave: 'Brave', chromium: 'Chromium' };

export function QuickApp() {
  const { t } = useTranslation();
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
  const dirEdited = useRef(false);
  const dirFor = useApp((s) => s.dirFor);

  // What to ask about; then show the window.
  useEffect(() => {
    if (!isTauri()) return;
    void api.quickRequest().then((r) => {
      if (!r) {
        void getCurrentWindow().close();
        return;
      }
      setReq(r);
      if (r.origin === 'clipboard') escape.current = 'cancel';
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
        if (!nameEdited.current && !req.fileName) {
          setName(p.fileName);
          if (!dirEdited.current) setDir(useApp.getState().dirFor(p.fileName));
        }
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

  // Enter starts, Esc adds it for later, or drops a copied link (buttons keep their own Enter).
  const escape = useRef<QuickDecision['action']>('later');
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        decideRef.current(escape.current);
      } else if (e.key === 'Enter' && !(e.target instanceof HTMLButtonElement) && !(e.target instanceof Element && e.target.closest('summary'))) {
        e.preventDefault();
        decideRef.current('start');
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  // The window is as tall as what it shows: opening "Advanced" (or a long error) makes it taller.
  const rootRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = rootRef.current;
    if (!req || !el || !isTauri()) return;
    const fit = () => {
      // Header, body and footer at their own heights, and the window's 1 px border.
      const h = Math.ceil([...el.children].reduce((n, c) => n + c.getBoundingClientRect().height, 0) + 2);
      if (Math.abs(h - window.innerHeight) > 1) void getCurrentWindow().setSize(new LogicalSize(window.innerWidth, h)).catch(() => undefined);
    };
    const ro = new ResizeObserver(fit);
    for (const c of el.children) ro.observe(c);
    return () => ro.disconnect();
  }, [req]);

  const browse = async () => {
    const picked = await open({ directory: true, defaultPath: dir || undefined, title: t('quick.pickDir') }).catch(() => null);
    if (typeof picked === 'string') {
      dirEdited.current = true;
      setDir(picked);
    }
  };

  if (!req) return <div className="quick" />;
  const size = probe?.total ?? req.size;
  const conn = Math.max(1, Math.min(64, Number.parseInt(connections, 10) || req.connections));
  const parts: string[] = [];
  if (probe) {
    parts.push(probe.resumable ? t('quick.resumable') : t('quick.notResumable'));
    if (probe.resumable && probe.total != null) parts.push(t('quick.connectionsUsed', { count: Math.max(1, Math.min(conn, Math.floor(probe.total / 1048576) || 1)) }));
  }
  const extras =
    req.hasCookies && req.hasReferer
      ? t('quick.attached.both')
      : req.hasCookies
        ? t('quick.attached.cookie')
        : req.hasReferer
          ? t('quick.attached.referer')
          : '';
  const browser = BROWSER[req.browser ?? ''];
  const fromBrowser = req.origin === 'browser';
  const heading = fromBrowser ? (browser ? t('quick.fromBrowser', { browser }) : t('quick.fromAnyBrowser')) : req.origin === 'clipboard' ? t('quick.fromClipboard') : t('quick.fromFloat');
  const proto = /^ftps?:/i.test(req.url) ? 'FTP' : 'HTTP';
  return (
    <div className="quick" ref={rootRef}>
      <header data-tauri-drag-region>
        <Logo size={18} />
        <h1 data-tauri-drag-region>
          {heading}
          <span>{req.site}</span>
        </h1>
        {/* Handing it back, beside closing: the footer keeps its room for the two ways to keep it. */}
        {fromBrowser && (
          <button type="button" className="btn subtle sm q-browser" onClick={() => decide('browser')}>
            <Icon name="globe" />
            {t('quick.useBrowser')}
          </button>
        )}
        <button type="button" className="q-close" aria-label={t('quick.close')} title={t('quick.closeTitle')} onClick={() => decide('cancel')}>
          <Icon name="x" />
        </button>
      </header>
      <div className="q-body">
        <label htmlFor="qa-name">{t('quick.fileName')}</label>
        <div className="q-name">
          <FileIcon name={name} />
          <input
            id="qa-name"
            className="inp"
            value={name}
            spellCheck={false}
            onChange={(e) => {
              nameEdited.current = true;
              setName(e.target.value);
              if (!dirEdited.current && e.target.value.trim()) setDir(dirFor(e.target.value));
            }}
          />
        </div>
        <span />
        <div className="q-info">
          <b>{size != null ? fmt.bytes(size) : t('quick.sizeUnknown')}</b>
          <span className={`badge proto-${proto.toLowerCase()}`}>{proto}</span>
          {!probe && !probeError && (
            <span className="q-probing">
              <span className="pulse" aria-hidden="true" />
              {t('quick.probing')}
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
              {extras}
            </span>
          )}
        </div>
        <label htmlFor="qa-dir">{t('quick.saveTo')}</label>
        <div className="inline">
          <input
            id="qa-dir"
            className="inp mono"
            value={dir}
            onChange={(e) => {
              dirEdited.current = true;
              setDir(e.target.value);
            }}
          />
          <button type="button" className="btn outline" onClick={() => void browse()}>
            {t('quick.browse')}
          </button>
        </div>
        {/* As in the new-download dialog: connections and speed limit under "Advanced", and from
            a browser the choice not to ask again for this site. */}
        <details className="nt-adv q-adv">
          <summary>
            <Icon name="chevronRight" />
            {t('newTask.advanced')}
            <small>{fromBrowser ? `${t('newTask.advSpeed')} · ${t('quick.rememberShort')}` : t('newTask.advSpeed')}</small>
          </summary>
          <div className="frow">
            <label htmlFor="qa-conn" className="top">
              {t('quick.connections')}
            </label>
            <div className="nt-sum-field">
              <input
                id="qa-conn"
                className="inp nt-num"
                type="number"
                min={1}
                max={64}
                value={connections}
                aria-describedby="qa-conn-hint"
                onChange={(e) => setConnections(e.target.value)}
              />
              <small id="qa-conn-hint" className="muted">
                {t('newTask.connectionsHint')}
              </small>
            </div>
            <label htmlFor="qa-limit" className="top">
              {t('quick.limit')}
            </label>
            <div className="nt-sum-field">
              <input
                id="qa-limit"
                className="inp nt-num"
                value={limit}
                placeholder={t('quick.limitPlaceholder')}
                aria-invalid={speed === undefined}
                aria-describedby="qa-limit-hint"
                onChange={(e) => setLimit(e.target.value)}
              />
              <small id="qa-limit-hint" className={speed === undefined ? 'err-text' : 'muted'}>
                {t('quick.limitHint')}
              </small>
            </div>
            {fromBrowser && (
              <>
                <span />
                <label className="chk q-remember">
                  <input type="checkbox" checked={remember} onChange={(e) => setRemember(e.target.checked)} />
                  {req.site ? t('quick.remember', { site: req.site }) : t('quick.rememberAnySite')}
                </label>
              </>
            )}
          </div>
        </details>
      </div>
      <footer>
        <span className="q-keys">
          <kbd>Enter</kbd> {t('quick.keyStart')} · <kbd>Esc</kbd> {req.origin === 'clipboard' ? t('quick.keyCancel') : t('quick.keyLater')}
        </span>
        <button type="button" className="btn outline" onClick={() => decide('later')}>
          <Icon name="clock" />
          {t('quick.later')}
        </button>
        <button type="button" className="btn primary" data-autofocus onClick={() => decide('start')}>
          <Icon name="download" />
          {t('quick.start')}
        </button>
      </footer>
    </div>
  );
}
