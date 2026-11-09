// Settings › Browser (docs/development-plan.md §6.3): which browsers can reach Omni Download, the
// quick-download window, sites that never ask, and the HTTP fallback channel's pairing code.

import { isTauri } from '@tauri-apps/api/core';
import { useEffect, useState, type ReactNode } from 'react';

import { Icon } from '../../components/Icon';
import { Switch } from '../../components/ui';
import { api } from '../../lib/api';
import * as fmt from '../../lib/format';
import type { BrowserState, Integration, Settings } from '../../lib/types';
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

function BrowserRow({ b, seen }: { b: BrowserState; seen?: number }) {
  let state: [string, string];
  if (b.ours) state = ['ok', seen ? `已连接 · ${fmt.when(seen)}` : '已就绪'];
  else if (b.registered) state = ['warn', '指向另一个安装'];
  else if (b.detected) state = ['warn', '未注册'];
  else state = ['off', '未安装'];
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
  const b = s.browser;
  return (
    <>
      <div className="set-group">
        <Row
          col
          label="浏览器扩展"
          hint={
            info?.hostPath
              ? '安装 Omni Download 扩展后，在浏览器中点击下载会自动交给 Omni Download。'
              : '没有找到浏览器通信程序 omnidl-nm-host，请重新安装 Omni Download。'
          }
        >
          <button type="button" className="btn outline" disabled={!info?.hostPath} onClick={() => void run(api.browserRepair).then((i) => i && (take(i), toast('success', '已重新写入浏览器通信配置')))}>
            <Icon name="refresh" />
            重新写入通信配置
          </button>
        </Row>
        {info?.browsers.map((br) => <BrowserRow key={br.id} b={br} seen={seen.get(br.id)} />)}
        {info && !info.bridgeOk && (
          <Row label="本机通信" hint="另一个 Omni Download 正在占用浏览器通信，这个窗口收不到浏览器的下载。">
            <span className="bstate warn">
              <i aria-hidden="true" />
              不可用
            </span>
          </Row>
        )}
      </div>
      <div className="set-group">
        <Row label="接管前询问" hint="浏览器交来下载时，先在小窗口里确认文件名和保存位置">
          <Switch label="接管前询问" checked={b.quickDialog} onChange={(v) => set((x) => (x.browser.quickDialog = v))} />
        </Row>
        <Row col label="不再询问的网站" hint={b.silentSites.length ? '这些网站的下载直接开始；点 × 恢复询问' : '在询问窗口中勾选“不再询问”的网站会列在这里'}>
          {b.silentSites.length > 0 && (
            <div className="chips">
              {b.silentSites.map((site) => (
                <span key={site} className="chip site-chip">
                  {site}
                  <button type="button" aria-label={`恢复询问 ${site}`} onClick={() => set((x) => (x.browser.silentSites = x.browser.silentSites.filter((y) => y !== site)))}>
                    <Icon name="x" />
                  </button>
                </span>
              ))}
            </div>
          )}
        </Row>
      </div>
      <div className="set-group">
        <Row label="备用通道" hint="Snap、Flatpak 等无法使用本机通信的浏览器，改用本机 HTTP 接口；需要在扩展选项中填入配对码">
          <Switch label="备用通道" checked={b.httpBridge} onChange={(v) => void run(() => api.httpBridgeSet(v)).then(take)} />
        </Row>
        {b.httpBridge && info?.pairingCode && (
          <Row col label="配对码" hint="只在本机有效；重新生成后，已配对的扩展需要重新填写">
            <div className="inline">
              <input className="inp mono set-text" readOnly value={info.pairingCode} aria-label="配对码" onFocus={(e) => e.target.select()} />
              <button type="button" className="btn outline" onClick={() => void navigator.clipboard.writeText(info.pairingCode!).then(() => toast('success', '已复制配对码'))}>
                <Icon name="copy" />
                复制
              </button>
              <button type="button" className="btn outline" onClick={() => void run(api.httpBridgeNewToken).then(take)}>
                重新生成
              </button>
            </div>
          </Row>
        )}
      </div>
    </>
  );
}
