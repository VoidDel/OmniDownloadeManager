// Settings › BT (docs/development-plan.md §4.4, §6.3): seeding policy, upload limit, the
// network the BitTorrent engine uses, IP filtering, and whether this app opens magnet links.

import { isTauri } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
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
      <IpFilter s={s} set={set} />
      <div className="set-group">
        <MagnetRow />
      </div>
    </>
  );
}

/** A text saved when it loses the focus. */
function useDraft(value: string, save: (v: string) => void) {
  const [draft, setDraft] = useState<string | null>(null);
  return {
    value: draft ?? value,
    onChange: (v: string) => setDraft(v),
    onBlur: () => {
      if (draft != null && draft !== value) save(draft);
      setDraft(null);
    },
  };
}

/** IP filtering: on/off, the list (web address or file), the addresses blocked by hand, and how
 *  many ranges the list in use has. Read when BitTorrent starts (the next start of the app). */
function IpFilter({ s, set }: { s: Settings; set: (f: (s: Settings) => void) => void }) {
  const { t } = useTranslation();
  const run = useApp((st) => st.run);
  const toast = useApp((st) => st.toast);
  const e = s.engine;
  const [status, setStatus] = useState<{ rules: number; updatedAt: number | null } | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (isTauri()) void api.ipFilterStatus().then(setStatus);
  }, []);
  const list = useDraft(e.btIpFilterList, (v) => set((x) => (x.engine.btIpFilterList = v.trim())));
  const banned = useDraft(e.btBanned.join('\n'), (v) =>
    set(
      (x) =>
        (x.engine.btBanned = v
          .split(/\r?\n/)
          .map((l) => l.trim())
          .filter(Boolean)),
    ),
  );
  const update = async () => {
    setBusy(true);
    const got = await run(() => api.ipFilterUpdate());
    setBusy(false);
    if (got) {
      setStatus(got);
      toast('success', t('settings.torrent.ipFilterUpdated', { count: got.rules }));
    }
  };
  const pick = async () => {
    const f = await open({ title: t('settings.torrent.ipFilterPick'), filters: [{ name: 'IP filter', extensions: ['p2p', 'dat', 'txt', 'gz', 'cidr'] }] }).catch(() => null);
    if (typeof f === 'string') set((x) => (x.engine.btIpFilterList = f));
  };
  return (
    <div className="set-group">
      <Row label={t('settings.torrent.ipFilter')} hint={t('settings.torrent.ipFilterHint')}>
        <Switch label={t('settings.torrent.ipFilter')} checked={e.btIpFilter} onChange={(v) => set((x) => (x.engine.btIpFilter = v))} />
      </Row>
      <div className="set-row col">
        <div className="lab">
          <span>{t('settings.torrent.ipFilterList')}</span>
          <small>{t('settings.torrent.ipFilterListHint')}</small>
        </div>
        <div className="ctl">
          <div className="inline">
            <input
              className="inp mono grow"
              aria-label={t('settings.torrent.ipFilterList')}
              placeholder="https://… / C:\…\ipfilter.dat"
              spellCheck={false}
              disabled={!e.btIpFilter}
              value={list.value}
              onChange={(ev) => list.onChange(ev.target.value)}
              onBlur={list.onBlur}
            />
            <button type="button" className="btn outline" disabled={!e.btIpFilter || !isTauri()} onClick={() => void pick()}>
              {t('settings.torrent.ipFilterBrowse')}
            </button>
          </div>
        </div>
      </div>
      <div className="set-row col">
        <div className="lab">
          <span>{t('settings.torrent.ipBanned')}</span>
          <small>{t('settings.torrent.ipBannedHint')}</small>
        </div>
        <div className="ctl">
          <textarea
            className="inp mono ip-banned"
            rows={3}
            aria-label={t('settings.torrent.ipBanned')}
            placeholder={'203.0.113.7\n198.51.100.0/24\n192.0.2.10-192.0.2.20'}
            spellCheck={false}
            disabled={!e.btIpFilter}
            value={banned.value}
            onChange={(ev) => banned.onChange(ev.target.value)}
            onBlur={banned.onBlur}
          />
        </div>
      </div>
      <Row
        label={t('settings.torrent.ipFilterRules')}
        hint={status?.updatedAt ? t('settings.torrent.ipFilterRulesHint', { when: fmt.when(status.updatedAt) }) : t('settings.torrent.ipFilterRestart')}
      >
        <span className="muted num">{t('settings.torrent.ipFilterCount', { count: status?.rules ?? 0 })}</span>
        <button type="button" className="btn outline" disabled={!e.btIpFilter || busy || !isTauri()} onClick={() => void update()}>
          {busy ? t('settings.torrent.ipFilterUpdating') : t('settings.torrent.ipFilterUpdate')}
        </button>
      </Row>
    </div>
  );
}
