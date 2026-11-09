// Settings › BT (docs/development-plan.md §4.4, §6.3): seeding policy, upload limit, the
// network the BitTorrent engine uses, and whether this app opens magnet links.

import { isTauri } from '@tauri-apps/api/core';
import { useEffect, useState, type ReactNode } from 'react';

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
const MINUTES: [number, string][] = [
  [0, '不限'],
  [60, '1 小时'],
  [360, '6 小时'],
  [720, '12 小时'],
  [1440, '1 天'],
  [4320, '3 天'],
  [10080, '1 周'],
];
const UPLOAD = [0, 64, 256, 512, 1024, 4096, 10240].map((k) => k * 1024);

/** Port typed in, saved on blur or Enter when valid (0 lets the system choose). */
function PortSetting({ value, onSave }: { value: number; onSave: (v: number) => void }) {
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
      aria-label="监听端口"
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
  const [state, setState] = useState<MagnetHandler | null>(null);
  useEffect(() => {
    if (isTauri()) void api.magnetStatus().then(setState, () => undefined);
  }, []);
  const register = async () => {
    const next = await run(() => api.magnetRegister());
    if (next) {
      setState(next);
      toast('success', '已设为打开磁力链接的程序');
    }
  };
  return (
    <Row label="磁力链接" hint={state?.canRegister === false ? '安装后的应用会自动关联磁力链接与种子文件' : '在浏览器里点击 magnet: 链接时用 Omni Download 打开'}>
      {state?.registered && <span className="bstate ok">已关联</span>}
      {state?.canRegister && (
        <button type="button" className="btn outline" onClick={() => void register()}>
          {state.registered ? '重新关联' : '设为默认程序'}
        </button>
      )}
    </Row>
  );
}

export function TorrentSettings({ s, set }: { s: Settings; set: (f: (s: Settings) => void) => void }) {
  const e = s.engine;
  const restart = '重启应用后生效';
  return (
    <>
      <div className="set-group">
        <Row label="下载完成后做种" hint="继续把文件上传给其他下载者；暂停做种中的任务即可停止">
          <Switch label="下载完成后做种" checked={e.seeding} onChange={(v) => set((x) => (x.engine.seeding = v))} />
        </Row>
        <Row label="分享率达到时停止" hint="上传量达到文件大小的这个倍数">
          <Select
            label="分享率达到时停止"
            value={String(e.seedRatio)}
            onChange={(v) => set((x) => (x.engine.seedRatio = Number(v)))}
            options={(RATIOS.includes(e.seedRatio) ? RATIOS : [...RATIOS, e.seedRatio].sort((a, b) => a - b)).map((r): [string, string] => [String(r), r === 0 ? '不限' : `${r}`])}
          />
        </Row>
        <Row label="做种时长" hint="从下载完成时算起，与分享率先到者为准">
          <Select
            label="做种时长"
            value={String(e.seedMinutes)}
            onChange={(v) => set((x) => (x.engine.seedMinutes = Number(v)))}
            options={(MINUTES.some(([m]) => m === e.seedMinutes) ? MINUTES : [...MINUTES, [e.seedMinutes, fmt.duration(e.seedMinutes * 60)] as [number, string]]).map(([m, l]): [string, string] => [String(m), l])}
          />
        </Row>
        <Row label="上传限速" hint="所有 BT 任务合计">
          <Select
            label="上传限速"
            value={String(e.uploadLimit ?? 0)}
            onChange={(v) => set((x) => (x.engine.uploadLimit = Number(v) || null))}
            options={(UPLOAD.includes(e.uploadLimit ?? 0) ? UPLOAD : [...UPLOAD, e.uploadLimit ?? 0].sort((a, b) => a - b)).map((b): [string, string] => [String(b), b === 0 ? '不限' : fmt.speed(b)])}
          />
        </Row>
      </div>
      <div className="set-group">
        <Row label="监听端口" hint={`其他节点连接进来的端口，0 由系统选择；${restart}`}>
          <PortSetting value={e.btPort} onSave={(v) => set((x) => (x.engine.btPort = v))} />
        </Row>
        <Row label="DHT 网络" hint={`没有 Tracker 时靠它寻找节点，大多数磁力链接都需要；${restart}`}>
          <Switch label="DHT 网络" checked={e.btDht} onChange={(v) => set((x) => (x.engine.btDht = v))} />
        </Row>
        <Row label="UPnP 端口映射" hint={`请路由器转发监听端口，让更多节点连得进来；${restart}`}>
          <Switch label="UPnP 端口映射" checked={e.btUpnp} onChange={(v) => set((x) => (x.engine.btUpnp = v))} />
        </Row>
      </div>
      <div className="set-group">
        <MagnetRow />
      </div>
    </>
  );
}
