// The quick-download window (docs/development-plan.md §6.3): a download taken over from the
// browser, waiting for the user. Enter starts it, Esc adds it paused, "let the browser download
// it" hands it back. The window shows itself once it has something to show.

import { isTauri } from '@tauri-apps/api/core';
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
    const picked = await open({ directory: true, defaultPath: dir || undefined, title: t('quick.pickDir') }).catch(() => null);
    if (typeof picked === 'string') setDir(picked);
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
  return (
    <div className="quick">
      <header data-tauri-drag-region>
        <Logo size={18} />
        <h1 data-tauri-drag-region>
          {browser ? t('quick.fromBrowser', { browser }) : t('quick.fromAnyBrowser')}
          <span>{req.site}</span>
        </h1>
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
            }}
          />
        </div>
        <span />
        <div className="q-info">
          <b>{size != null ? fmt.bytes(size) : t('quick.sizeUnknown')}</b>
          <span className="badge proto-http">HTTP</span>
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
          <input id="qa-dir" className="inp mono" value={dir} onChange={(e) => setDir(e.target.value)} />
          <button type="button" className="btn outline" onClick={() => void browse()}>
            {t('quick.browse')}
          </button>
        </div>
        <label htmlFor="qa-conn">{t('quick.connections')}</label>
        <div className="inline q-nums">
          <input id="qa-conn" className="inp" type="number" min={1} max={64} value={connections} onChange={(e) => setConnections(e.target.value)} />
          <label htmlFor="qa-limit">{t('quick.limit')}</label>
          <input
            id="qa-limit"
            className="inp"
            value={limit}
            placeholder={t('quick.limitPlaceholder')}
            aria-invalid={speed === undefined}
            title={t('quick.limitHint')}
            onChange={(e) => setLimit(e.target.value)}
          />
        </div>
        <span />
        <label className="chk q-remember">
          <input type="checkbox" checked={remember} onChange={(e) => setRemember(e.target.checked)} />
          {req.site ? t('quick.remember', { site: req.site }) : t('quick.rememberAnySite')}
        </label>
      </div>
      <footer>
        <span className="q-keys">
          <kbd>Enter</kbd> {t('quick.keyStart')} · <kbd>Esc</kbd> {t('quick.keyLater')}
        </span>
        <button type="button" className="btn subtle" onClick={() => decide('browser')}>
          {t('quick.useBrowser')}
        </button>
        <button type="button" className="btn outline" onClick={() => decide('later')}>
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
