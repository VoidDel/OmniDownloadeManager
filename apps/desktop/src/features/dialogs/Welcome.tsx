// The first-run guide (docs/development-plan.md §6.5): language and download folder, the browser
// extension for each browser found, then running in the background. Skipping (or closing) it
// counts as done; everything in it is in the settings too.

import { isTauri } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { Icon } from '../../components/Icon';
import { Modal, Select, Switch } from '../../components/ui';
import { LANGUAGE_NAMES, SUPPORTED, systemLang } from '../../i18n';
import { api } from '../../lib/api';
import type { Integration } from '../../lib/types';
import { useApp } from '../../store/app';
import { browserState } from '../settings/BrowserSettings';

const STEPS = 3;

export function Welcome() {
  const { t } = useTranslation();
  const settings = useApp((s) => s.settings);
  const autostart = useApp((s) => s.autostart);
  const dir = useApp((s) => s.downloadDir());
  const { saveSettings, setAutostart, closeDialog, run } = useApp.getState();
  const [step, setStep] = useState(0);
  const [info, setInfo] = useState<Integration | null>(null);
  useEffect(() => {
    if (isTauri()) void api.browserStatus().then(setInfo, () => undefined);
  }, []);

  const finish = () => {
    closeDialog();
    void saveSettings((s) => (s.general.onboarded = true));
  };
  const browse = async () => {
    const picked = await open({ directory: true, defaultPath: dir || undefined, title: t('settings.download.folderPick') }).catch(() => null);
    if (typeof picked === 'string') void saveSettings((s) => (s.downloadDir = picked));
  };
  const seen = new Map(info?.seen ?? []);
  const browsers = (info?.browsers ?? []).filter((b) => b.detected || b.registered);

  return (
    <Modal
      title={t('welcome.title')}
      onClose={finish}
      width={520}
      closeLabel={t('dialogs.close')}
      footer={
        <>
          <span className="welcome-dots" aria-hidden="true">
            {Array.from({ length: STEPS }, (_, i) => (
              <i key={i} className={i === step ? 'on' : undefined} />
            ))}
          </span>
          <span className="grow" />
          {step > 0 ? (
            <button type="button" className="btn outline" onClick={() => setStep(step - 1)}>
              {t('welcome.back')}
            </button>
          ) : (
            <button type="button" className="btn subtle" onClick={finish}>
              {t('welcome.skip')}
            </button>
          )}
          <button type="button" className="btn primary" onClick={() => (step < STEPS - 1 ? setStep(step + 1) : finish())}>
            {step < STEPS - 1 ? t('welcome.next') : t('welcome.done')}
          </button>
        </>
      }
    >
      {step === 0 && (
        <div className="welcome">
          <p className="lead">{t('welcome.intro')}</p>
          <div className="field">
            <label>{t('settings.language.ui')}</label>
            <Select
              label={t('settings.language.ui')}
              value={settings.language || 'auto'}
              onChange={(v) => void saveSettings((x) => (x.language = v))}
              options={[['auto', t('settings.language.auto', { lang: LANGUAGE_NAMES[systemLang()] })], ...SUPPORTED.map((l): [string, string] => [l, LANGUAGE_NAMES[l]])]}
            />
          </div>
          <div className="field">
            <label>{t('settings.download.folder')}</label>
            <div className="inline">
              <span className="field-look welcome-path" title={dir}>
                {dir}
              </span>
              <button type="button" className="btn outline" onClick={() => void browse()}>
                {t('settings.download.browse')}
              </button>
            </div>
          </div>
        </div>
      )}
      {step === 1 && (
        <div className="welcome">
          <p className="lead">{t('welcome.browsersIntro')}</p>
          {browsers.length === 0 ? (
            <p className="muted">{info ? t('welcome.noBrowsers') : t('welcome.lookingForBrowsers')}</p>
          ) : (
            <ul className="welcome-browsers">
              {browsers.map((b) => {
                const [cls, text] = browserState(b, seen.get(b.id));
                return (
                  <li key={b.id}>
                    <span className="nm">{b.name}</span>
                    <span className={`bstate ${cls}`}>
                      <i aria-hidden="true" />
                      {text}
                    </span>
                    <button type="button" className="btn outline" onClick={() => void run(() => api.extensionOpen(b.id))}>
                      <Icon name="external" />
                      {t('welcome.getExtension')}
                    </button>
                  </li>
                );
              })}
            </ul>
          )}
          <small className="muted">{t('welcome.browsersLater')}</small>
        </div>
      )}
      {step === 2 && (
        <div className="welcome">
          <p className="lead">{t('welcome.backgroundIntro')}</p>
          <label className="welcome-switch">
            <span>
              <b>{t('settings.general.autostart')}</b>
              <small>{t('settings.general.autostartHint')}</small>
            </span>
            <Switch label={t('settings.general.autostart')} checked={autostart} disabled={!isTauri()} onChange={(v) => void setAutostart(v)} />
          </label>
          <label className="welcome-switch">
            <span>
              <b>{t('settings.general.closeToTray')}</b>
              <small>{t('settings.general.closeToTrayHint')}</small>
            </span>
            <Switch label={t('settings.general.closeToTray')} checked={settings.general.closeToTray} onChange={(v) => void saveSettings((s) => (s.general.closeToTray = v))} />
          </label>
        </div>
      )}
    </Modal>
  );
}
