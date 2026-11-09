// The settings page (docs/development-plan.md §6.3): general, download, network, BT, browser.
// Changes save at once (the backend applies them to downloads that start later; the global limit
// and appearance apply immediately). Layout and classes are OmniZip's (omni-ui settings CSS).

import { isTauri } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { builtinThemes, summarize, type ThemeSummary } from 'omni-ui';
import { useState, type ReactNode } from 'react';

import { Icon } from '../../components/Icon';
import { Segmented, Select, Switch } from '../../components/ui';
import { api } from '../../lib/api';
import type { Density, Settings as S, ThemeMode } from '../../lib/types';
import { useApp } from '../../store/app';
import { SETTINGS_TABS } from '../shell/Chrome';
import { BrowserSettings } from './BrowserSettings';
import { TorrentSettings } from './TorrentSettings';

const THEMES: ThemeSummary[] = builtinThemes.map((t) => summarize(t, true));

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

/** A select over numbers. */
function NumberSelect({ value, options, onChange, label, unit = '' }: { value: number; options: number[]; onChange: (v: number) => void; label: string; unit?: string }) {
  const list = options.includes(value) ? options : [...options, value].sort((a, b) => a - b);
  return (
    <Select
      value={String(value)}
      label={label}
      options={list.map((n): [string, string] => [String(n), `${n}${unit}`])}
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
  const name = th.name['zh-CN'] ?? th.name['en-US'];
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

function General({ s, set }: { s: S; set: (f: (s: S) => void) => void }) {
  const version = useApp((st) => st.version);
  const logDir = useApp((st) => st.logDir);
  const run = useApp((st) => st.run);
  const a = s.appearance;
  return (
    <>
      <div className="set-group">
        <Row label="外观">
          <Segmented<ThemeMode>
            value={a.mode}
            label="外观"
            onChange={(v) => set((x) => (x.appearance.mode = v))}
            options={[
              ['light', '浅色'],
              ['dark', '深色'],
              ['system', '跟随系统'],
            ]}
          />
        </Row>
        <Row col label="浅色主题" hint="外观为浅色，或跟随系统且系统为浅色时使用">
          <div className="themes">
            {THEMES.filter((t) => t.appearance === 'light').map((th) => (
              <ThemeCard key={th.id} th={th} selected={a.lightTheme === th.id} onPick={() => set((x) => (x.appearance.lightTheme = th.id))} />
            ))}
          </div>
        </Row>
        <Row col label="深色主题">
          <div className="themes">
            {THEMES.filter((t) => t.appearance === 'dark').map((th) => (
              <ThemeCard key={th.id} th={th} selected={a.darkTheme === th.id} onPick={() => set((x) => (x.appearance.darkTheme = th.id))} />
            ))}
          </div>
        </Row>
        <Row label="列表密度" hint="任务列表每行的高度">
          <Segmented<Density>
            value={a.density}
            label="列表密度"
            onChange={(v) => set((x) => (x.appearance.density = v))}
            options={[
              ['compact', '紧凑'],
              ['comfortable', '舒适'],
              ['spacious', '宽松'],
            ]}
          />
        </Row>
      </div>
      <div className="set-group">
        <Row label="版本">
          <span className="muted">Omni Download {version}</span>
        </Row>
        <Row label="日志" hint={<span className="mono">{logDir}</span>}>
          <button type="button" className="btn outline" disabled={!logDir || !isTauri()} onClick={() => void run(() => api.openFolder(logDir))}>
            <Icon name="folderOpen" />
            打开文件夹
          </button>
        </Row>
      </div>
    </>
  );
}

function Download({ s, set }: { s: S; set: (f: (s: S) => void) => void }) {
  const defaultDir = useApp((st) => st.defaultDir);
  const run = useApp((st) => st.run);
  const e = s.engine;
  const dir = s.downloadDir || defaultDir;
  const browse = async () => {
    const picked = await open({ directory: true, defaultPath: dir || undefined, title: '选择默认下载文件夹' }).catch(() => null);
    if (typeof picked === 'string') set((x) => (x.downloadDir = picked));
  };
  return (
    <>
      <div className="set-group">
        <Row col label="默认下载文件夹" hint={s.downloadDir ? undefined : '系统的“下载”文件夹'}>
          <div className="inline">
            <TextSetting mono label="默认下载文件夹" value={dir} onSave={(v) => set((x) => (x.downloadDir = v === defaultDir ? '' : v))} />
            <button type="button" className="btn outline" onClick={() => void browse()}>
              浏览
            </button>
            <button type="button" className="btn outline" disabled={!isTauri()} onClick={() => void run(() => api.openFolder(dir))}>
              打开
            </button>
          </div>
        </Row>
        <Row label="同时下载的任务数" hint="其余任务排队等待">
          <NumberSelect label="同时下载的任务数" value={e.maxConcurrent} options={[1, 2, 3, 4, 5, 6, 8, 10]} onChange={(v) => set((x) => (x.engine.maxConcurrent = v))} />
        </Row>
        <Row label="每个任务的连接数" hint="服务器支持分段下载时同时打开的连接，可在新建下载时单独设置">
          <NumberSelect label="每个任务的连接数" value={e.connections} options={[1, 2, 4, 8, 16, 32]} onChange={(v) => set((x) => (x.engine.connections = v))} />
        </Row>
        <Row label="文件已存在时">
          <Segmented<'rename' | 'overwrite'>
            value={e.conflict}
            label="文件已存在时"
            onChange={(v) => set((x) => (x.engine.conflict = v))}
            options={[
              ['rename', '自动重命名'],
              ['overwrite', '覆盖'],
            ]}
          />
        </Row>
      </div>
      <div className="set-group">
        <Row label="启动时继续未完成的下载" hint="退出时正在下载的任务，下次启动后自动继续">
          <Switch label="启动时继续未完成的下载" checked={e.resumeOnStartup} onChange={(v) => set((x) => (x.engine.resumeOnStartup = v))} />
        </Row>
        <Row label="每秒把进度写入磁盘" hint="断电或系统崩溃后也能从准确的位置继续；关闭可以减少磁盘写入">
          <Switch label="每秒把进度写入磁盘" checked={e.syncWrites} onChange={(v) => set((x) => (x.engine.syncWrites = v))} />
        </Row>
      </div>
    </>
  );
}

type ProxyMode = 'system' | 'none' | 'custom';

function Network({ s, set }: { s: S; set: (f: (s: S) => void) => void }) {
  const e = s.engine;
  // "Custom" is saved once it has an address; until then it is only shown here.
  const [mode, setMode] = useState<ProxyMode>(e.proxy.mode);
  const [customUrl, setCustomUrl] = useState(e.proxy.mode === 'custom' ? e.proxy.url : '');
  return (
    <div className="set-group">
      <Row col label="代理" hint="HTTP、HTTPS 或 SOCKS5 代理；socks5h:// 由代理解析域名">
        <div className="inline">
          <Segmented<ProxyMode>
            value={mode}
            label="代理"
            onChange={(v) => {
              setMode(v);
              if (v !== 'custom') set((x) => (x.engine.proxy = { mode: v }));
              else if (customUrl.trim()) set((x) => (x.engine.proxy = { mode: 'custom', url: customUrl.trim() }));
            }}
            options={[
              ['system', '系统代理'],
              ['none', '不使用'],
              ['custom', '自定义'],
            ]}
          />
          {mode === 'custom' && (
            <TextSetting
              mono
              label="代理地址"
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
      <Row col label="User-Agent" hint="默认使用桌面版 Chrome 的标识；有的网站会拒绝陌生的客户端">
        <div className="inline">
          <TextSetting mono label="User-Agent" value={e.userAgent} onSave={(v) => set((x) => (x.engine.userAgent = v))} />
          <button type="button" className="btn outline" onClick={() => set((x) => (x.engine.userAgent = ''))}>
            恢复默认
          </button>
        </div>
      </Row>
      <Row label="连接超时">
        <NumberSelect label="连接超时" unit=" 秒" value={e.connectTimeoutSecs} options={[5, 10, 15, 30, 60]} onChange={(v) => set((x) => (x.engine.connectTimeoutSecs = v))} />
      </Row>
      <Row label="无响应超时" hint="连接这么久收不到数据就断开重连">
        <NumberSelect label="无响应超时" unit=" 秒" value={e.readTimeoutSecs} options={[10, 20, 30, 60, 120]} onChange={(v) => set((x) => (x.engine.readTimeoutSecs = v))} />
      </Row>
      <Row label="失败重试次数" hint="网络错误或服务器 5xx 时，每个连接按 1、2、4… 秒的间隔重试">
        <NumberSelect label="失败重试次数" unit=" 次" value={e.maxRetries} options={[0, 3, 5, 10, 20]} onChange={(v) => set((x) => (x.engine.maxRetries = v))} />
      </Row>
    </div>
  );
}

export function Settings() {
  const s = useApp((st) => st.settings);
  const tab = useApp((st) => st.settingsTab);
  const setTab = useApp((st) => st.setSettingsTab);
  const save = useApp((st) => st.saveSettings);
  const set = (f: (s: S) => void) => void save(f);
  const label = SETTINGS_TABS.find(([id]) => id === tab)?.[1] ?? '';
  return (
    <div className="settings">
      <nav className="set-nav">
        {SETTINGS_TABS.map(([id, text, icon]) => (
          <button key={id} type="button" className="nav-item" aria-current={tab === id} onClick={() => setTab(id)}>
            <Icon name={icon} />
            <span className="lbl">{text}</span>
          </button>
        ))}
      </nav>
      <div className="set-body">
        <h2>{label}</h2>
        {tab === 'general' && <General s={s} set={set} />}
        {tab === 'download' && <Download s={s} set={set} />}
        {tab === 'network' && <Network s={s} set={set} />}
        {tab === 'torrent' && <TorrentSettings s={s} set={set} />}
        {tab === 'browser' && <BrowserSettings s={s} set={set} />}
      </div>
    </div>
  );
}
