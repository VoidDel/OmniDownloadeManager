// Settings › Browser (docs/development-plan.md §6.3): which browsers can reach Omni Download, the
// quick-download window, sites that never ask, and the HTTP fallback channel's pairing code.

import { isTauri } from '@tauri-apps/api/core';
import { useEffect, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';

import { Icon } from '../../components/Icon';
import { Switch } from '../../components/ui';
import { api } from '../../lib/api';
import * as fmt from '../../lib/format';
import type { BrowserState, Integration, Settings } from '../../lib/types';
import i18n from '../../i18n';
import { useApp } from '../../store/app';

function Row({ label, hint, children, col }: { label: ReactNode; hint?: ReactNode; children?: ReactNode; col?: boolean }) {
  return (
    <div className={`set-row${col ? ' col' : ''}`}>
      <div className="lab">
        <span>{label}</span>
        {hint && <small>{hint}</small>}
      </div>
      <div className="ctl">{children}</div>
    </div>
  );
}

/** A browser's state for people: [class, text]. Also in the first-run guide. */
/** A browser's row: whether it is installed, registered for this installation, and whether its
 *  extension has been in touch (`seen` in this run, `before` in any). The app registers every
 *  browser, installed or not, so that one installed later works at once: registration alone
 *  says nothing about the browser or the extension. */
export function browserState(b: BrowserState, seen?: number, before?: number): [string, string] {
  if (b.ours && seen) return ['ok', i18n.t('settings.browser.state.connected', { when: fmt.when(seen) })];
  if (!b.detected && !before) return ['off', i18n.t('settings.browser.state.notInstalled')];
  if (b.registered && !b.ours) return ['warn', i18n.t('settings.browser.state.otherInstall')];
  if (!b.registered) return ['warn', i18n.t('settings.browser.state.unregistered')];
  if (before) return ['ok', i18n.t('settings.browser.state.lastConnected', { when: fmt.when(before) })];
  return ['off', i18n.t('settings.browser.state.waiting')];
}

function BrowserRow({ b, seen, before }: { b: BrowserState; seen?: number; before?: number }) {
  const state = browserState(b, seen, before);
  return (
    <Row label={b.name}>
      <span className={`bstate ${state[0]}`}>
        <i aria-hidden="true" />
        {state[1]}
      </span>
    </Row>
  );
}

export function BrowserSettings({ s, set }: { s: Settings; set: (f: (s: Settings) => void) => void }) {
  const run = useApp((st) => st.run);
  const toast = useApp((st) => st.toast);
  const { t } = useTranslation();
  const [info, setInfo] = useState<Integration | null>(null);
  // Keeps the page's settings in step with what the backend changed (port, token).
  const take = (i: Integration | undefined) => {
    if (!i) return;
    setInfo(i);
    useApp.setState({ settings: i.settings });
  };
  useEffect(() => {
    if (!isTauri()) return;
    void api.browserStatus().then(setInfo, () => undefined);
  }, []);

  const seen = new Map(info?.seen ?? []);
  const before = new Map(info?.seenBefore ?? []);
  const b = s.browser;
  return (
    <>
      <div className="set-group">
        <Row
          col
          label={t('settings.browser.extension')}
          hint={info?.hostPath ? t('settings.browser.extensionHint') : t('settings.browser.hostMissing')}
        >
          <button type="button" className="btn outline" disabled={!info?.hostPath} onClick={() => void run(api.browserRepair).then((i) => i && (take(i), toast('success', t('settings.browser.repaired'))))}>
            <Icon name="refresh" />
            {t('settings.browser.repair')}
          </button>
        </Row>
        {info?.browsers.map((br) => <BrowserRow key={br.id} b={br} seen={seen.get(br.id)} before={before.get(br.id)} />)}
        {info && !info.bridgeOk && (
          <Row label={t('settings.browser.bridge')} hint={t('settings.browser.bridgeBusy')}>
            <span className="bstate warn">
              <i aria-hidden="true" />
              {t('settings.browser.unavailable')}
            </span>
          </Row>
        )}
      </div>
      <div className="set-group">
        <Row label={t('settings.browser.quickDialog')} hint={t('settings.browser.quickDialogHint')}>
          <Switch label={t('settings.browser.quickDialog')} checked={b.quickDialog} onChange={(v) => set((x) => (x.browser.quickDialog = v))} />
        </Row>
        <Row col label={t('settings.browser.silentSites')} hint={b.silentSites.length ? t('settings.browser.silentSitesHint') : t('settings.browser.silentSitesEmpty')}>
          {b.silentSites.length > 0 && (
            <div className="chips">
              {b.silentSites.map((site) => (
                <span key={site} className="chip site-chip">
                  {site}
                  <button type="button" aria-label={t('settings.browser.silentSiteRemove', { site })} onClick={() => set((x) => (x.browser.silentSites = x.browser.silentSites.filter((y) => y !== site)))}>
                    <Icon name="x" />
                  </button>
                </span>
              ))}
            </div>
          )}
        </Row>
      </div>
      <div className="set-group">
        <Row label={t('settings.browser.httpBridge')} hint={t('settings.browser.httpBridgeHint')}>
          <Switch label={t('settings.browser.httpBridge')} checked={b.httpBridge} onChange={(v) => void run(() => api.httpBridgeSet(v)).then(take)} />
        </Row>
        {b.httpBridge && info?.pairingCode && (
          <Row col label={t('settings.browser.pairingCode')} hint={t('settings.browser.pairingCodeHint')}>
            <div className="inline">
              <input className="inp mono set-text" readOnly value={info.pairingCode} aria-label={t('settings.browser.pairingCode')} onFocus={(e) => e.target.select()} />
              <button type="button" className="btn outline" onClick={() => void navigator.clipboard.writeText(info.pairingCode!).then(() => toast('success', t('settings.browser.copied')))}>
                <Icon name="copy" />
                {t('settings.browser.copy')}
              </button>
              <button type="button" className="btn outline" onClick={() => void run(api.httpBridgeNewToken).then(take)}>
                {t('settings.browser.regenerate')}
              </button>
            </div>
          </Row>
        )}
      </div>
    </>
  );
}
