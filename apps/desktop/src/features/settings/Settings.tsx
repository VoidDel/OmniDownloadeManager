// The settings page (docs/development-plan.md §6.3): general, appearance (language too), download,
// network, BT, browser.
// Changes save at once (the backend applies them to downloads that start later; the global limit
// and appearance apply immediately). Layout and classes are OmniZip's (omni-ui settings CSS).

import { isTauri } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { builtinThemes, summarize, type ThemeSummary } from 'omni-ui';
import { useState, type CSSProperties, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';

import { Icon } from '../../components/Icon';
import { ShortcutInput } from '../../components/ShortcutInput';
import { Segmented, Select, Switch } from '../../components/ui';
import { api } from '../../lib/api';
import type { Category, Density, MaterialKind, Settings as S, ThemeMode } from '../../lib/types';
import { categoryLabel } from '../../lib/words';
import { useApp } from '../../store/app';
import { LANGUAGE_NAMES, SUPPORTED, systemLang } from '../../i18n';
import { SETTINGS_TABS } from '../shell/Chrome';
import { BrowserSettings } from './BrowserSettings';
import { TorrentSettings } from './TorrentSettings';

const THEMES: ThemeSummary[] = builtinThemes.map((t) => summarize(t, true));

/** Accent presets, as in OmniZip. */
const ACCENTS = ['#2F6FED', '#5B8CFF', '#7A5AF8', '#C2410C', '#B8641B', '#D4587A', '#DB2777', '#0E9F8E', '#1E9E63', '#4D7C0F', '#0891B2', '#64748B'];

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

/** A select over numbers; `format` turns a number into its option text (with a unit, say). */
function NumberSelect({ value, options, onChange, label, format = String }: { value: number; options: number[]; onChange: (v: number) => void; label: string; format?: (n: number) => string }) {
  const list = options.includes(value) ? options : [...options, value].sort((a, b) => a - b);
  return (
    <Select
      value={String(value)}
      label={label}
      options={list.map((n): [string, string] => [String(n), format(n)])}
      onChange={(v) => onChange(Number(v))}
    />
  );
}

/** A text field saved when it loses focus or on Enter. */
function TextSetting({ value, onSave, label, placeholder, mono }: { value: string; onSave: (v: string) => void; label: string; placeholder?: string; mono?: boolean }) {
  const [draft, setDraft] = useState<string | null>(null);
  const commit = () => {
    if (draft != null && draft !== value) onSave(draft.trim());
    setDraft(null);
  };
  return (
    <input
      className={`inp set-text${mono ? ' mono' : ''}`}
      aria-label={label}
      value={draft ?? value}
      placeholder={placeholder}
      spellCheck={false}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === 'Enter') (e.target as HTMLInputElement).blur();
        if (e.key === 'Escape') setDraft(null);
      }}
    />
  );
}

function ThemeCard({ th, selected, onPick }: { th: ThemeSummary; selected: boolean; onPick: () => void }) {
  const c = th.colors;
  const { i18n } = useTranslation();
  const name = th.name[i18n.language] ?? th.name['en-US'];
  return (
    <button type="button" className="tcard" aria-pressed={selected} title={name} onClick={onPick}>
      <div className="prev" style={{ background: c['bg.canvas'] }}>
        <div className="ps" style={{ background: c['bg.sidebar'] }}>
          <i style={{ background: c.accent, width: '70%' }} />
          <i style={{ background: c['fg.secondary'], width: '55%' }} />
          <i style={{ background: c['fg.secondary'], width: '62%' }} />
        </div>
        <div className="pm">
          <i style={{ background: c['bg.surface'], border: `1px solid ${c['border.default']}` }} />
          <i style={{ background: c['fg.primary'], width: '46%', opacity: 0.85 }} />
          <i style={{ background: c['fg.secondary'], width: '70%', opacity: 0.6 }} />
          <i style={{ background: c.accent, width: '28%' }} />
        </div>
      </div>
      <div className="tn">
        <span>{name}</span>
        <small>{th.contrast.toFixed(1)}:1</small>
      </div>
    </button>
  );
}

function Appearance({ s, set }: { s: S; set: (f: (s: S) => void) => void }) {
  const platform = useApp((st) => st.platform);
  const materialSupported = useApp((st) => st.materialSupported);
  const systemAccent = useApp((st) => st.systemAccent);
  const { t } = useTranslation();
  const a = s.appearance;
  const accentIsCustom = a.accent.startsWith('#');
  const accentIsPicked = accentIsCustom && !ACCENTS.includes(a.accent.toUpperCase());
  return (
    <>
      <div className="set-group">
        <Row label={t('settings.language.ui')}>
          <Select
            label={t('settings.language.ui')}
            value={s.language || 'auto'}
            onChange={(v) => set((x) => (x.language = v))}
            options={[['auto', t('settings.language.auto', { lang: LANGUAGE_NAMES[systemLang()] })], ...SUPPORTED.map((l): [string, string] => [l, LANGUAGE_NAMES[l]])]}
          />
        </Row>
      </div>
      <div className="set-group">
        <Row label={t('settings.appearance.mode')}>
          <Segmented<ThemeMode>
            value={a.mode}
            label={t('settings.appearance.mode')}
            onChange={(v) => set((x) => (x.appearance.mode = v))}
            options={[
              ['light', t('settings.appearance.modes.light')],
              ['dark', t('settings.appearance.modes.dark')],
              ['system', t('settings.appearance.modes.system')],
            ]}
          />
        </Row>
        <Row col label={t('settings.appearance.lightTheme')} hint={t('settings.appearance.lightThemeHint')}>
          <div className="themes">
            {THEMES.filter((t) => t.appearance === 'light').map((th) => (
              <ThemeCard key={th.id} th={th} selected={a.lightTheme === th.id} onPick={() => set((x) => (x.appearance.lightTheme = th.id))} />
            ))}
          </div>
        </Row>
        <Row col label={t('settings.appearance.darkTheme')}>
          <div className="themes">
            {THEMES.filter((t) => t.appearance === 'dark').map((th) => (
              <ThemeCard key={th.id} th={th} selected={a.darkTheme === th.id} onPick={() => set((x) => (x.appearance.darkTheme = th.id))} />
            ))}
          </div>
        </Row>
        <Row col label={t('settings.appearance.accent')}>
          <div className="swatches">
            <Segmented<string>
              value={accentIsCustom ? 'custom' : a.accent}
              label={t('settings.appearance.accent')}
              onChange={(v) => v !== 'custom' && set((x) => (x.appearance.accent = v))}
              options={[
                ['theme', t('settings.appearance.accentTheme')],
                ['system', t('settings.appearance.accentSystem'), !systemAccent],
              ]}
            />
            <span style={{ width: 8 }} />
            {ACCENTS.map((c) => (
              <button key={c} type="button" className="sw" style={{ background: c }} aria-pressed={a.accent.toUpperCase() === c} aria-label={c} onClick={() => set((x) => (x.appearance.accent = c))} />
            ))}
            {/* A native colour input, invisible over a rainbow swatch; a colour not among the presets shows in its middle. */}
            <label className={'sw sw-custom' + (accentIsPicked ? ' on' : '')} title={t('settings.appearance.accentCustom')} style={accentIsPicked ? ({ '--c': a.accent } as CSSProperties) : undefined}>
              <input type="color" aria-label={t('settings.appearance.accentCustom')} value={accentIsCustom ? a.accent : '#2F6FED'} onChange={(e) => set((x) => (x.appearance.accent = e.target.value.toUpperCase()))} />
            </label>
          </div>
        </Row>
        {/* As in OmniZip: on Windows 11 one choice for both "whether" and "which". */}
        <Row label={t('settings.appearance.material')} hint={materialSupported ? t('settings.appearance.materialHint') : t('settings.appearance.materialUnavailable')}>
          {platform === 'win' && materialSupported ? (
            <Segmented<'off' | MaterialKind>
              value={a.material ? a.materialKind : 'off'}
              label={t('settings.appearance.material')}
              onChange={(v) =>
                set((x) => {
                  x.appearance.material = v !== 'off';
                  if (v !== 'off') x.appearance.materialKind = v;
                })
              }
              options={[
                ['off', t('settings.appearance.materialKinds.off')],
                ['mica', t('settings.appearance.materialKinds.mica')],
                ['mica-alt', t('settings.appearance.materialKinds.micaAlt')],
                ['acrylic', t('settings.appearance.materialKinds.acrylic')],
              ]}
            />
          ) : (
            <Switch label={t('settings.appearance.material')} disabled={!materialSupported} checked={a.material && materialSupported} onChange={(v) => set((x) => (x.appearance.material = v))} />
          )}
        </Row>
        <Row label={t('settings.appearance.density')} hint={t('settings.appearance.densityHint')}>
          <Segmented<Density>
            value={a.density}
            label={t('settings.appearance.density')}
            onChange={(v) => set((x) => (x.appearance.density = v))}
            options={[
              ['compact', t('settings.appearance.densities.compact')],
              ['comfortable', t('settings.appearance.densities.comfortable')],
              ['spacious', t('settings.appearance.densities.spacious')],
            ]}
          />
        </Row>
      </div>
    </>
  );
}

function General({ s, set }: { s: S; set: (f: (s: S) => void) => void }) {
  const version = useApp((st) => st.version);
  const logDir = useApp((st) => st.logDir);
  const run = useApp((st) => st.run);
  const platform = useApp((st) => st.platform);
  const autostart = useApp((st) => st.autostart);
  const { setAutostart, checkUpdate, toast } = useApp.getState();
  const [checking, setChecking] = useState(false);
  const { t } = useTranslation();
  const g = s.general;
  /** Sets one shortcut, unless the other one already has those keys. */
  const shortcut = (which: 'shortcutShow' | 'shortcutNew', other: string) => (v: string) => {
    if (v && v === other) toast('error', t('settings.general.shortcutDuplicate'));
    else set((x) => (x.general[which] = v));
  };
  const pickSound = async () => {
    const picked = await open({ title: t('settings.general.soundPick'), filters: [{ name: 'WAV', extensions: ['wav'] }], defaultPath: g.soundFile || undefined }).catch(() => null);
    if (typeof picked === 'string') {
      set((x) => (x.general.soundFile = picked));
      void run(() => api.soundTest(picked));
    }
  };
  const check = async () => {
    setChecking(true);
    await checkUpdate(true);
    setChecking(false);
  };
  return (
    <>
      <div className="set-group">
        <Row label={t('settings.general.autostart')} hint={t('settings.general.autostartHint')}>
          <Switch label={t('settings.general.autostart')} checked={autostart} disabled={!isTauri()} onChange={(v) => void setAutostart(v)} />
        </Row>
        <Row label={t('settings.general.closeToTray')} hint={t('settings.general.closeToTrayHint')}>
          <Switch label={t('settings.general.closeToTray')} checked={g.closeToTray} onChange={(v) => set((x) => (x.general.closeToTray = v))} />
        </Row>
        <Row label={t('settings.general.notifyDone')}>
          <Switch label={t('settings.general.notifyDone')} checked={g.notifyDone} onChange={(v) => set((x) => (x.general.notifyDone = v))} />
        </Row>
        <Row label={t('settings.general.notifyFailed')} hint={t('settings.general.notifyHint')}>
          <Switch label={t('settings.general.notifyFailed')} checked={g.notifyFailed} onChange={(v) => set((x) => (x.general.notifyFailed = v))} />
        </Row>
      </div>
      <div className="set-group">
        <Row label={t('settings.general.soundDone')} hint={t('settings.general.soundDoneHint')}>
          <Switch label={t('settings.general.soundDone')} checked={g.soundDone} onChange={(v) => set((x) => (x.general.soundDone = v))} />
        </Row>
        <Row label={t('settings.general.sound')} hint={g.soundFile ? <span className="mono">{g.soundFile}</span> : t('settings.general.soundBuiltIn')}>
          <button type="button" className="btn outline" disabled={!isTauri()} onClick={() => void run(() => api.soundTest(g.soundFile))}>
            <Icon name="play" />
            {t('settings.general.soundTest')}
          </button>
          <button type="button" className="btn outline" disabled={!isTauri()} onClick={() => void pickSound()}>
            {t('settings.general.soundPick')}
          </button>
          {g.soundFile && (
            <button type="button" className="btn outline" onClick={() => set((x) => (x.general.soundFile = ''))}>
              {t('settings.general.soundReset')}
            </button>
          )}
        </Row>
      </div>
      <div className="set-group">
        <Row label={t('settings.general.shortcutShow')} hint={t('settings.general.shortcutHint')}>
          <ShortcutInput label={t('settings.general.shortcutShow')} platform={platform} value={g.shortcutShow} onChange={shortcut('shortcutShow', g.shortcutNew)} />
        </Row>
        <Row label={t('settings.general.shortcutNew')}>
          <ShortcutInput label={t('settings.general.shortcutNew')} platform={platform} value={g.shortcutNew} onChange={shortcut('shortcutNew', g.shortcutShow)} />
        </Row>
      </div>
      <div className="set-group">
        <Row label={t('settings.general.version')}>
          <span className="muted">Omni Download {version}</span>
          <button type="button" className="btn outline" disabled={checking || !isTauri()} onClick={() => void check()}>
            <Icon name="refresh" />
            {checking ? t('update.checking') : t('update.check')}
          </button>
        </Row>
        <Row label={t('settings.general.checkUpdates')} hint={t('settings.general.checkUpdatesHint')}>
          <Switch label={t('settings.general.checkUpdates')} checked={g.checkUpdates} onChange={(v) => set((x) => (x.general.checkUpdates = v))} />
        </Row>
        <Row label={t('settings.general.logs')} hint={<span className="mono">{logDir}</span>}>
          <button type="button" className="btn outline" disabled={!logDir || !isTauri()} onClick={() => void run(() => api.openFolder(logDir))}>
            <Icon name="folderOpen" />
            {t('settings.general.openFolder')}
          </button>
        </Row>
        <Row label={t('settings.general.help')} hint={t('settings.general.helpHint')}>
          <button type="button" className="btn outline" disabled={!isTauri()} onClick={() => void run(() => api.helpOpen('guide'))}>
            <Icon name="external" />
            {t('settings.general.guide')}
          </button>
          <button type="button" className="btn outline" disabled={!isTauri()} onClick={() => void run(() => api.helpOpen('feedback'))}>
            <Icon name="external" />
            {t('settings.general.feedback')}
          </button>
        </Row>
      </div>
    </>
  );
}

/** The categories that can have a folder of their own ("other" always goes to the download folder). */
const CATEGORY_FOLDERS: Category[] = ['video', 'audio', 'document', 'archive', 'program', 'disk_image', 'image'];

/** A category's folder: empty means the download folder (shown greyed). */
function CategoryFolder({ category, value, fallback, set }: { category: Category; value: string; fallback: string; set: (f: (s: S) => void) => void }) {
  const { t } = useTranslation();
  const save = (v: string) =>
    set((x) => {
      const dirs = { ...(x.categoryDirs ?? {}) };
      if (v.trim()) dirs[category] = v.trim();
      else delete dirs[category];
      x.categoryDirs = dirs;
    });
  const browse = async () => {
    const picked = await open({ directory: true, defaultPath: value || fallback || undefined, title: t('settings.download.byCategoryPick', { category: categoryLabel(category) }) }).catch(() => null);
    if (typeof picked === 'string') save(picked);
  };
  return (
    <Row label={categoryLabel(category)}>
      <div className="inline cat-dir">
        <TextSetting mono label={categoryLabel(category)} value={value} placeholder={fallback} onSave={save} />
        <button type="button" className="btn outline" onClick={() => void browse()}>
          {t('settings.download.browse')}
        </button>
        {/* Hidden rather than gone, so the fields line up. */}
        <button type="button" className="iconbtn" title={t('settings.download.byCategoryClear')} aria-label={t('settings.download.byCategoryClear')} style={value ? undefined : { visibility: 'hidden' }} onClick={() => save('')}>
          <Icon name="x" />
        </button>
      </div>
    </Row>
  );
}

function Download({ s, set }: { s: S; set: (f: (s: S) => void) => void }) {
  const defaultDir = useApp((st) => st.defaultDir);
  const run = useApp((st) => st.run);
  const { t } = useTranslation();
  const e = s.engine;
  const dir = s.downloadDir || defaultDir;
  const browse = async () => {
    const picked = await open({ directory: true, defaultPath: dir || undefined, title: t('settings.download.folderPick') }).catch(() => null);
    if (typeof picked === 'string') set((x) => (x.downloadDir = picked));
  };
  return (
    <>
      <div className="set-group">
        <Row col label={t('settings.download.folder')} hint={s.downloadDir ? undefined : t('settings.download.folderDefault')}>
          <div className="inline">
            <TextSetting mono label={t('settings.download.folder')} value={dir} onSave={(v) => set((x) => (x.downloadDir = v === defaultDir ? '' : v))} />
            <button type="button" className="btn outline" onClick={() => void browse()}>
              {t('settings.download.browse')}
            </button>
            <button type="button" className="btn outline" disabled={!isTauri()} onClick={() => void run(() => api.openFolder(dir))}>
              {t('settings.download.open')}
            </button>
          </div>
        </Row>
      </div>
      <div className="set-group">
        <Row col label={t('settings.download.byCategory')} hint={t('settings.download.byCategoryHint')} />
        {CATEGORY_FOLDERS.map((c) => (
          <CategoryFolder key={c} category={c} value={s.categoryDirs?.[c] ?? ''} fallback={dir} set={set} />
        ))}
      </div>
      <div className="set-group">
        <Row label={t('settings.download.maxConcurrent')} hint={t('settings.download.maxConcurrentHint')}>
          <NumberSelect label={t('settings.download.maxConcurrent')} value={e.maxConcurrent} options={[1, 2, 3, 4, 5, 6, 8, 10]} onChange={(v) => set((x) => (x.engine.maxConcurrent = v))} />
        </Row>
        <Row label={t('settings.download.connections')} hint={t('settings.download.connectionsHint')}>
          <NumberSelect label={t('settings.download.connections')} value={e.connections} options={[1, 2, 4, 8, 16, 32]} onChange={(v) => set((x) => (x.engine.connections = v))} />
        </Row>
        <Row label={t('settings.download.conflict')}>
          <Segmented<'rename' | 'overwrite'>
            value={e.conflict}
            label={t('settings.download.conflict')}
            onChange={(v) => set((x) => (x.engine.conflict = v))}
            options={[
              ['rename', t('settings.download.conflictRename')],
              ['overwrite', t('settings.download.conflictOverwrite')],
            ]}
          />
        </Row>
      </div>
      <div className="set-group">
        <Row label={t('settings.download.resumeOnStartup')} hint={t('settings.download.resumeOnStartupHint')}>
          <Switch label={t('settings.download.resumeOnStartup')} checked={e.resumeOnStartup} onChange={(v) => set((x) => (x.engine.resumeOnStartup = v))} />
        </Row>
        <Row label={t('settings.download.syncWrites')} hint={t('settings.download.syncWritesHint')}>
          <Switch label={t('settings.download.syncWrites')} checked={e.syncWrites} onChange={(v) => set((x) => (x.engine.syncWrites = v))} />
        </Row>
      </div>
    </>
  );
}

type ProxyMode = 'system' | 'none' | 'custom';

function Network({ s, set }: { s: S; set: (f: (s: S) => void) => void }) {
  const { t } = useTranslation();
  const e = s.engine;
  // "Custom" is saved once it has an address; until then it is only shown here.
  const [mode, setMode] = useState<ProxyMode>(e.proxy.mode);
  const [customUrl, setCustomUrl] = useState(e.proxy.mode === 'custom' ? e.proxy.url : '');
  const seconds = (n: number) => t('settings.network.seconds', { n });
  return (
    <>
      <div className="set-group">
        <Row col label={t('settings.network.proxy')} hint={t('settings.network.proxyHint')}>
          <div className="inline">
            <Segmented<ProxyMode>
              value={mode}
              label={t('settings.network.proxy')}
              onChange={(v) => {
                setMode(v);
                if (v !== 'custom') set((x) => (x.engine.proxy = { mode: v }));
                else if (customUrl.trim()) set((x) => (x.engine.proxy = { mode: 'custom', url: customUrl.trim() }));
              }}
              options={[
                ['system', t('settings.network.proxyModes.system')],
                ['none', t('settings.network.proxyModes.none')],
                ['custom', t('settings.network.proxyModes.custom')],
              ]}
            />
            {mode === 'custom' && (
              <TextSetting
                mono
                label={t('settings.network.proxyUrl')}
                value={customUrl}
                placeholder="http://127.0.0.1:7890"
                onSave={(v) => {
                  setCustomUrl(v);
                  if (v) set((x) => (x.engine.proxy = { mode: 'custom', url: v }));
                }}
              />
            )}
          </div>
        </Row>
        <Row col label="User-Agent" hint={t('settings.network.userAgentHint')}>
          <div className="inline">
            <TextSetting mono label="User-Agent" value={e.userAgent} onSave={(v) => set((x) => (x.engine.userAgent = v))} />
            <button type="button" className="btn outline" onClick={() => set((x) => (x.engine.userAgent = ''))}>
              {t('settings.network.resetDefault')}
            </button>
          </div>
        </Row>
        <Row label={t('settings.network.connectTimeout')}>
          <NumberSelect label={t('settings.network.connectTimeout')} format={seconds} value={e.connectTimeoutSecs} options={[5, 10, 15, 30, 60]} onChange={(v) => set((x) => (x.engine.connectTimeoutSecs = v))} />
        </Row>
        <Row label={t('settings.network.readTimeout')} hint={t('settings.network.readTimeoutHint')}>
          <NumberSelect label={t('settings.network.readTimeout')} format={seconds} value={e.readTimeoutSecs} options={[10, 20, 30, 60, 120]} onChange={(v) => set((x) => (x.engine.readTimeoutSecs = v))} />
        </Row>
        <Row label={t('settings.network.retries')} hint={t('settings.network.retriesHint')}>
          <NumberSelect label={t('settings.network.retries')} format={(n) => t('settings.network.times', { n })} value={e.maxRetries} options={[0, 3, 5, 10, 20]} onChange={(v) => set((x) => (x.engine.maxRetries = v))} />
        </Row>
      </div>
      <div className="set-group">
        <Row label={t('settings.network.ftpSessions')} hint={t('settings.network.ftpSessionsHint')}>
          <NumberSelect label={t('settings.network.ftpSessions')} value={e.ftpConnections} options={[1, 2, 3, 4, 6, 8]} onChange={(v) => set((x) => (x.engine.ftpConnections = v))} />
        </Row>
        <Row label={t('settings.network.ftpMode')} hint={t('settings.network.ftpModeHint')}>
          <Segmented<'passive' | 'active'>
            value={e.ftpActive ? 'active' : 'passive'}
            label={t('settings.network.ftpMode')}
            onChange={(v) => set((x) => (x.engine.ftpActive = v === 'active'))}
            options={[
              ['passive', t('settings.network.ftpPassive')],
              ['active', t('settings.network.ftpActive')],
            ]}
          />
        </Row>
      </div>
    </>
  );
}

export function Settings() {
  const s = useApp((st) => st.settings);
  const tab = useApp((st) => st.settingsTab);
  const setTab = useApp((st) => st.setSettingsTab);
  const save = useApp((st) => st.saveSettings);
  const set = (f: (s: S) => void) => void save(f);
  const { t } = useTranslation();
  const current = SETTINGS_TABS.find(([id]) => id === tab)?.[1];
  const label = current ? t(current) : '';
  return (
    <div className="settings">
      <nav className="set-nav">
        {SETTINGS_TABS.map(([id, key, icon]) => (
          <button key={id} type="button" className="nav-item" aria-current={tab === id} onClick={() => setTab(id)}>
            <Icon name={icon} />
            <span className="lbl">{t(key)}</span>
          </button>
        ))}
      </nav>
      <div className="set-body">
        <h2>{label}</h2>
        {tab === 'general' && <General s={s} set={set} />}
        {tab === 'appearance' && <Appearance s={s} set={set} />}
        {tab === 'download' && <Download s={s} set={set} />}
        {tab === 'network' && <Network s={s} set={set} />}
        {tab === 'torrent' && <TorrentSettings s={s} set={set} />}
        {tab === 'browser' && <BrowserSettings s={s} set={set} />}
      </div>
    </div>
  );
}
