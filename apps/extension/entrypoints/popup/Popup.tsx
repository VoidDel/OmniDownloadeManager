// The toolbar popup (docs/development-plan.md §5.4): whether Omni Download is reachable, the
// takeover switch, the latest downloads, and a way to open the app.

import { Icon, ProgressBar, Switch } from 'omni-ui';
import { useEffect, useState } from 'react';

import { BridgeError, send, type Unreachable } from '../../lib/bridge';
import { STATUS_LABEL, speed } from '../../lib/page';
import type { Status } from '../../lib/protocol';
import { DEFAULT_RULES, type Rules } from '../../lib/rules';
import { load, save } from '../../lib/settings';

type State = { kind: 'loading' } | { kind: 'ok'; status: Status } | { kind: 'down'; reason: Unreachable; detail: string };

const DOWN: Record<Unreachable, [string, string]> = {
  not_running: ['Omni Download 未运行', '点“打开 Omni Download”启动它；下载时也会自动启动。'],
  no_host: ['没有找到 Omni Download', '请安装并运行一次 Omni Download；已安装时，在它的“设置 › 浏览器”中点“重新写入通信配置”。'],
  unpaired: ['备用通道未配对', '在扩展选项中填写 Omni Download“设置 › 浏览器”里的配对码。'],
  error: ['无法连接 Omni Download', '请确认它已安装并在运行。'],
};

export function Popup() {
  const [state, setState] = useState<State>({ kind: 'loading' });
  const [rules, setRules] = useState<Rules>(DEFAULT_RULES);

  useEffect(() => {
    void load().then((s) => setRules(s.rules));
    let live = true;
    const poll = async () => {
      try {
        const r = await send<Status>({ type: 'get_status' });
        if (!live) return;
        if (r.ok && r.result) setState({ kind: 'ok', status: r.result });
        else setState({ kind: 'down', reason: 'error', detail: r.error?.message ?? '' });
      } catch (e) {
        if (!live) return;
        const err = e instanceof BridgeError ? e : new BridgeError('error', String(e));
        setState({ kind: 'down', reason: err.kind, detail: err.message });
      }
    };
    void poll();
    const t = setInterval(() => void poll(), 1500);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, []);

  const setEnabled = (enabled: boolean) => {
    const next = { ...rules, enabled };
    setRules(next);
    void save({ rules: next });
  };

  const openApp = () => void send({ type: 'open_app' }).then(() => window.close(), () => undefined);

  return (
    <div className="popup">
      <header>
        <img src="/icon/32.png" width={20} height={20} alt="" />
        <b>Omni Download</b>
        {state.kind === 'ok' && (
          <span className="pstate ok" title="已连接">
            <i aria-hidden="true" />
            {state.status.app_version}
          </span>
        )}
        {state.kind === 'down' && (
          <span className="pstate warn">
            <i aria-hidden="true" />
            未连接
          </span>
        )}
      </header>

      {state.kind === 'down' && (
        <div className="pmsg" title={state.detail}>
          <b>{DOWN[state.reason][0]}</b>
          <span>{DOWN[state.reason][1]}</span>
        </div>
      )}
      {state.kind === 'ok' && (
        <div className="pline">
          {state.status.running > 0 ? (
            <>
              <Icon name="download" />
              {state.status.running} 个任务下载中 · {speed(state.status.download_speed)}
            </>
          ) : (
            '没有正在下载的任务'
          )}
        </div>
      )}

      <div className="prow">
        <div className="plab">
          <span>接管浏览器下载</span>
          <small>{rules.enabled ? `${rules.minSizeMB} MB 以上的文件交给 Omni Download` : '下载留在浏览器中'}</small>
        </div>
        <Switch label="接管浏览器下载" checked={rules.enabled} onChange={setEnabled} />
      </div>

      {state.kind === 'ok' && state.status.recent.length > 0 && (
        <ul className="precent" aria-label="最近的下载">
          {state.status.recent.map((t) => (
            <li key={t.id}>
              <span className="pname" title={t.name}>
                {t.name}
              </span>
              <span className="pst">{STATUS_LABEL[t.status] ?? t.status}</span>
              <ProgressBar value={t.progress ?? 0} indeterminate={t.progress == null && t.status === 'downloading'} />
            </li>
          ))}
        </ul>
      )}

      <footer>
        <button type="button" className="btn primary grow" onClick={openApp}>
          打开 Omni Download
        </button>
        <button type="button" className="iconbtn" title="选项" aria-label="选项" onClick={() => void browser.runtime.openOptionsPage()}>
          <Icon name="gear" />
        </button>
      </footer>
    </div>
  );
}
