// Settings › BT (docs/development-plan.md §4.4, §6.3): seeding policy, upload limit, the
// network the BitTorrent engine uses, extra trackers, IP filtering, and whether this app opens
// magnet links.

import { isTauri } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { useEffect, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';

import { Select, Switch } from '../../components/ui';
import { api } from '../../lib/api';
import * as fmt from '../../lib/format';
import type { MagnetHandler, Settings, TrackersStatus } from '../../lib/types';
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
      <Trackers s={s} />
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

/** Lines of a text box, trimmed, without empty ones. */
const lines = (v: string) =>
  v
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter(Boolean);

/** Subscribed lists: web addresses or files, one per line; “Choose file…” adds a line. */
function Sources({
  label,
  hint,
  placeholder,
  value,
  disabled,
  filter,
  onSave,
}: {
  label: string;
  hint: string;
  placeholder: string;
  value: string[];
  disabled: boolean;
  filter: { name: string; extensions: string[] };
  onSave: (v: string[]) => void;
}) {
  const { t } = useTranslation();
  const text = useDraft(value.join('\n'), (v) => onSave(lines(v)));
  const pick = async () => {
    const f = await open({ title: label, multiple: true, filters: [filter] }).catch(() => null);
    const picked = (Array.isArray(f) ? f : f ? [f] : []).filter((p) => !value.includes(p));
    if (picked.length) onSave([...value, ...picked]);
  };
  return (
    <div className="set-row col">
      <div className="lab">
        <span>{label}</span>
        <small>{hint}</small>
      </div>
      <div className="ctl">
        <div className="inline top">
          <textarea
            className="inp mono set-lines grow"
            rows={Math.min(Math.max(value.length, 2), 6)}
            aria-label={label}
            placeholder={placeholder}
            spellCheck={false}
            wrap="off"
            disabled={disabled}
            value={text.value}
            onChange={(ev) => text.onChange(ev.target.value)}
            onBlur={text.onBlur}
          />
          <button type="button" className="btn outline" disabled={disabled || !isTauri()} onClick={() => void pick()}>
            {t('settings.torrent.ipFilterBrowse')}
          </button>
        </div>
      </div>
    </div>
  );
}

/** IP filtering: on/off, the lists (web addresses or files), the addresses blocked by hand, and
 *  how many ranges the list in use has. Read when BitTorrent starts (the next start of the app). */
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
  const banned = useDraft(e.btBanned.join('\n'), (v) => set((x) => (x.engine.btBanned = lines(v))));
  const update = async () => {
    setBusy(true);
    const got = await run(() => api.ipFilterUpdate());
    setBusy(false);
    if (got) {
      setStatus(got);
      toast('success', t('settings.torrent.ipFilterUpdated', { count: got.rules }));
    }
  };
  return (
    <div className="set-group">
      <Row label={t('settings.torrent.ipFilter')} hint={t('settings.torrent.ipFilterHint')}>
        <Switch label={t('settings.torrent.ipFilter')} checked={e.btIpFilter} onChange={(v) => set((x) => (x.engine.btIpFilter = v))} />
      </Row>
      <Sources
        label={t('settings.torrent.ipFilterList')}
        hint={t('settings.torrent.ipFilterListHint')}
        placeholder={'https://…/level1.gz\nC:\\…\\ipfilter.dat'}
        value={e.btIpFilterLists}
        disabled={!e.btIpFilter}
        filter={{ name: 'IP filter', extensions: ['p2p', 'dat', 'txt', 'gz', 'cidr'] }}
        onSave={(v) => set((x) => (x.engine.btIpFilterLists = v))}
      />
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

/** Extra trackers: on/off, the user's list, subscribed lists (web addresses or files, fetched
 *  daily), and how many are added. Tasks that start after a change use them. */
function Trackers({ s }: { s: Settings }) {
  const { t } = useTranslation();
  const run = useApp((st) => st.run);
  const toast = useApp((st) => st.toast);
  const save = useApp((st) => st.saveSettings);
  const e = s.engine;
  const [status, setStatus] = useState<TrackersStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const refresh = () => {
    if (isTauri()) void api.trackersStatus().then(setStatus, () => undefined);
  };
  useEffect(refresh, []);
  /** Saves, then reads the count again (it follows the saved settings). */
  const change = (f: (x: Settings) => void) => void save(f).then(refresh);
  const list = useDraft(e.btTrackers.join('\n'), (v) => change((x) => (x.engine.btTrackers = lines(v))));
  const update = async () => {
    setBusy(true);
    const got = await run(() => api.trackersUpdate());
    setBusy(false);
    if (got) {
      setStatus(got);
      toast('success', t('settings.torrent.trackersUpdated', { count: got.fetched }));
    } else refresh(); // Lists that could be fetched were updated all the same.
  };
  const off = !e.btExtraTrackers;
  return (
    <div className="set-group">
      <Row label={t('settings.torrent.trackers')} hint={t('settings.torrent.trackersHint')}>
        <Switch label={t('settings.torrent.trackers')} checked={e.btExtraTrackers} onChange={(v) => change((x) => (x.engine.btExtraTrackers = v))} />
      </Row>
      <div className="set-row col">
        <div className="lab">
          <span>{t('settings.torrent.trackersList')}</span>
          <small>{t('settings.torrent.trackersListHint')}</small>
        </div>
        <div className="ctl">
          <textarea
            className="inp mono set-lines"
            rows={4}
            aria-label={t('settings.torrent.trackersList')}
            placeholder={'udp://tracker.example.org:1337/announce\nhttps://tracker.example.net/announce'}
            spellCheck={false}
            wrap="off"
            disabled={off}
            value={list.value}
            onChange={(ev) => list.onChange(ev.target.value)}
            onBlur={list.onBlur}
          />
        </div>
      </div>
      <Sources
        label={t('settings.torrent.trackersUrl')}
        hint={t('settings.torrent.trackersUrlHint')}
        placeholder={'https://…/trackers_best.txt\nhttps://…/trackers_all_udp.txt'}
        value={e.btTrackersUrls}
        disabled={off}
        filter={{ name: 'Trackers', extensions: ['txt', 'list'] }}
        onSave={(v) => change((x) => (x.engine.btTrackersUrls = v))}
      />
      <Row
        label={t('settings.torrent.trackersInUse')}
        hint={
          !e.btTrackersUrls.length
            ? undefined
            : status?.updatedAt
              ? t('settings.torrent.trackersInUseHint', { when: fmt.when(status.updatedAt), ready: status.ready, sources: status.sources, fetched: status.fetched })
              : t('settings.torrent.trackersNever')
        }
      >
        <span className="muted num">{t('settings.torrent.trackersCount', { count: off ? 0 : (status?.count ?? 0) })}</span>
        <button type="button" className="btn outline" disabled={off || busy || !e.btTrackersUrls.length || !isTauri()} onClick={() => void update()}>
          {busy ? t('settings.torrent.ipFilterUpdating') : t('settings.torrent.ipFilterUpdate')}
        </button>
      </Row>
    </div>
  );
}
