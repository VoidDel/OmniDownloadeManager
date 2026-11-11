// Settings › About, as in OmniZip: the app, its version (with the update check), licence, source
// code, what it downloads, what it is built with, and the family's other app (OmniZip).

import omnizipIcon from '@icons/family/omnizip.svg';
import { isTauri } from '@tauri-apps/api/core';
import { useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';

import { GitHubMark, Icon, Logo } from '../../components/Icon';
import { api } from '../../lib/api';
import { useApp } from '../../store/app';

/** What it downloads: protocol names, the same in every language (but the magnet link's). */
const PROTOCOLS = ['HTTP · HTTPS', 'FTP · FTPS', 'BitTorrent', 'magnet', 'HLS (m3u8)'];

/** The main parts it is built from (Cargo.toml, package.json). */
const COMPONENTS = ['Tauri 2', 'React 19', 'tokio', 'reqwest', 'rustls', 'librqbit', 'suppaftp', 'SQLite · sqlx', 'axum', 'i18next', 'omni-ui'];

/** The family's other apps: name, icon, a line about it, and the page `help_open` opens. One
 *  entry per app as more join. */
const FAMILY: { name: string; icon: string; hint: string; page: 'omnizip' }[] = [{ name: 'OmniZip', icon: omnizipIcon, hint: 'settings.about.omnizipHint', page: 'omnizip' }];

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

export function AboutSettings() {
  const { t } = useTranslation();
  const version = useApp((st) => st.version);
  const run = useApp((st) => st.run);
  const { checkUpdate } = useApp.getState();
  const [checking, setChecking] = useState(false);
  const check = async () => {
    setChecking(true);
    await checkUpdate(true);
    setChecking(false);
  };
  return (
    <>
      <div className="about-logo">
        <Logo size={56} />
        <div>
          <b>Omni Download</b>
          <br />
          <span>{t('settings.about.tagline')}</span>
        </div>
      </div>
      <div className="set-group">
        <Row label={t('settings.about.version')}>
          <span className="mono">{version}</span>
          <button type="button" className="btn outline" disabled={checking || !isTauri()} onClick={() => void check()}>
            <Icon name="refresh" />
            {checking ? t('update.checking') : t('update.check')}
          </button>
        </Row>
        <Row label={t('settings.about.license')}>
          <span>MIT</span>
        </Row>
        <Row label={t('settings.about.source')}>
          <button type="button" className="gh-btn" title="github.com/VoidDel/OmniDownloadeManager" aria-label="GitHub" disabled={!isTauri()} onClick={() => void run(() => api.helpOpen('source'))}>
            <GitHubMark />
          </button>
        </Row>
        <Row col label={t('settings.about.protocols')}>
          <div className="chips">
            {PROTOCOLS.map((p) => (
              <span key={p} className="chip">
                {p === 'magnet' ? t('settings.about.magnet') : p}
              </span>
            ))}
          </div>
        </Row>
        <Row col label={t('settings.about.components')}>
          <div className="chips">
            {COMPONENTS.map((c) => (
              <span key={c} className="chip">
                {c}
              </span>
            ))}
          </div>
        </Row>
        {/* The other apps of the Omni family (the design canvas's "同家族应用"): a card each. */}
        <Row col label={t('settings.about.family')} hint={t('settings.about.familyHint')}>
          <div className="family-apps">
            {FAMILY.map((a) => (
              <button
                key={a.page}
                type="button"
                className="family-app"
                title={t(a.hint)}
                aria-label={t('settings.about.learnAbout', { app: a.name })}
                disabled={!isTauri()}
                onClick={() => void run(() => api.helpOpen(a.page))}
              >
                <img src={a.icon} width={40} height={40} alt="" draggable={false} />
                <span>{a.name}</span>
              </button>
            ))}
          </div>
        </Row>
      </div>
    </>
  );
}
