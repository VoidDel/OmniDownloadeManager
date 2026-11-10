// Settings › BT (docs/development-plan.md §4.4, §6.3): seeding policy, upload limit, the
// network the BitTorrent engine uses, and whether this app opens magnet links.

import { isTauri } from '@tauri-apps/api/core';
import { useEffect, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';

import { Select, Switch } from '../../components/ui';
import { api } from '../../lib/api';
import * as fmt from '../../lib/format';
import type { MagnetHandler, Settings } from '../../lib/types';
import { useApp } from '../../store/app';

function Row({ label, hint, children }: { label: ReactNode; hint?: ReactNode; children?: ReactNode }) {
  return (
    <div className="set-row">
      <div className="lab">
        <span>{label}</span>
        {hint && <small>{hint}</small>}
      </div>
      <div className="ctl">{children}</div>
    </div>
  );
}

const RATIOS = [0, 0.5, 1, 1.5, 2, 3, 5];
/** Seeding time presets in minutes (0 = no limit), labelled where rendered. */
const MINUTES = [0, 60, 360, 720, 1440, 4320, 10080];
const UPLOAD = [0, 64, 256, 512, 1024, 4096, 10240].map((k) => k * 1024);

/** Port typed in, saved on blur or Enter when valid (0 lets the system choose). */
function PortSetting({ value, onSave }: { value: number; onSave: (v: number) => void }) {
  const { t } = useTranslation();
  const [draft, setDraft] = useState<string | null>(null);
  const n = Number(draft ?? value);
  const valid = Number.isInteger(n) && n >= 0 && n <= 65535;
  const commit = () => {
    if (draft != null && valid && n !== value) onSave(n);
    setDraft(null);
  };
  return (
    <input
      className="inp set-port"
      aria-label={t('settings.torrent.port')}
      inputMode="numeric"
      value={draft ?? String(value)}
      aria-invalid={!valid}
      onChange={(e) => setDraft(e.target.value.trim())}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === 'Enter') commit();
        if (e.key === 'Escape') setDraft(null);
      }}
    />
  );
}

function MagnetRow() {
  const run = useApp((s) => s.run);
  const toast = useApp((s) => s.toast);
  const { t } = useTranslation();
  const [state, setState] = useState<MagnetHandler | null>(null);
  useEffect(() => {
    if (isTauri()) void api.magnetStatus().then(setState, () => undefined);
  }, []);
  const register = async () => {
    const next = await run(() => api.magnetRegister());
    if (next) {
      setState(next);
      toast('success', t('settings.torrent.magnetSet'));
    }
  };
  return (
    <Row label={t('settings.torrent.magnet')} hint={state?.canRegister === false ? t('settings.torrent.magnetHintInstalled') : t('settings.torrent.magnetHint')}>
      {state?.registered && <span className="bstate ok">{t('settings.torrent.magnetRegistered')}</span>}
      {state?.canRegister && (
        <button type="button" className="btn outline" onClick={() => void register()}>
          {state.registered ? t('settings.torrent.magnetReregister') : t('settings.torrent.magnetRegister')}
        </button>
      )}
    </Row>
  );
}

export function TorrentSettings({ s, set }: { s: Settings; set: (f: (s: Settings) => void) => void }) {
  const { t } = useTranslation();
  const e = s.engine;
  const unlimited = t('settings.torrent.unlimited');
  const minutes = (m: number) => {
    if (m === 0) return unlimited;
    if (m % 10080 === 0) return t('settings.torrent.weeks', { n: m / 10080 });
    if (m % 1440 === 0) return t('settings.torrent.days', { n: m / 1440 });
    return t('settings.torrent.hours', { n: m / 60 });
  };
  return (
    <>
      <div className="set-group">
        <Row label={t('settings.torrent.openTorrents')} hint={t('settings.torrent.openTorrentsHint')}>
          <Switch label={t('settings.torrent.openTorrents')} checked={s.general.openTorrents} onChange={(v) => set((x) => (x.general.openTorrents = v))} />
        </Row>
      </div>
      <div className="set-group">
        <Row label={t('settings.torrent.seeding')} hint={t('settings.torrent.seedingHint')}>
          <Switch label={t('settings.torrent.seeding')} checked={e.seeding} onChange={(v) => set((x) => (x.engine.seeding = v))} />
        </Row>
        <Row label={t('settings.torrent.seedRatio')} hint={t('settings.torrent.seedRatioHint')}>
          <Select
            label={t('settings.torrent.seedRatio')}
            value={String(e.seedRatio)}
            onChange={(v) => set((x) => (x.engine.seedRatio = Number(v)))}
            options={(RATIOS.includes(e.seedRatio) ? RATIOS : [...RATIOS, e.seedRatio].sort((a, b) => a - b)).map((r): [string, string] => [String(r), r === 0 ? unlimited : `${r}`])}
          />
        </Row>
        <Row label={t('settings.torrent.seedTime')} hint={t('settings.torrent.seedTimeHint')}>
          <Select
            label={t('settings.torrent.seedTime')}
            value={String(e.seedMinutes)}
            onChange={(v) => set((x) => (x.engine.seedMinutes = Number(v)))}
            options={(MINUTES.includes(e.seedMinutes) ? MINUTES : [...MINUTES, e.seedMinutes]).map((m): [string, string] => [String(m), MINUTES.includes(m) ? minutes(m) : fmt.duration(m * 60)])}
          />
        </Row>
        <Row label={t('settings.torrent.uploadLimit')} hint={t('settings.torrent.uploadLimitHint')}>
          <Select
            label={t('settings.torrent.uploadLimit')}
            value={String(e.uploadLimit ?? 0)}
            onChange={(v) => set((x) => (x.engine.uploadLimit = Number(v) || null))}
            options={(UPLOAD.includes(e.uploadLimit ?? 0) ? UPLOAD : [...UPLOAD, e.uploadLimit ?? 0].sort((a, b) => a - b)).map((b): [string, string] => [String(b), b === 0 ? unlimited : fmt.speed(b)])}
          />
        </Row>
      </div>
      <div className="set-group">
        <Row label={t('settings.torrent.port')} hint={t('settings.torrent.portHint')}>
          <PortSetting value={e.btPort} onSave={(v) => set((x) => (x.engine.btPort = v))} />
        </Row>
        <Row label={t('settings.torrent.dht')} hint={t('settings.torrent.dhtHint')}>
          <Switch label={t('settings.torrent.dht')} checked={e.btDht} onChange={(v) => set((x) => (x.engine.btDht = v))} />
        </Row>
        <Row label={t('settings.torrent.upnp')} hint={t('settings.torrent.upnpHint')}>
          <Switch label={t('settings.torrent.upnp')} checked={e.btUpnp} onChange={(v) => set((x) => (x.engine.btUpnp = v))} />
        </Row>
      </div>
      <div className="set-group">
        <MagnetRow />
      </div>
    </>
  );
}
