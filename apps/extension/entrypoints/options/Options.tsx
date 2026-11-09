// Options (docs/development-plan.md §5.2): which downloads are taken over, and how the extension
// reaches the app (Native Messaging, or the paired HTTP channel).

import { Segmented, Switch } from 'omni-ui';
import { useEffect, useState, type ReactNode } from 'react';

import { BridgeError, send } from '../../lib/bridge';
import type { Status } from '../../lib/protocol';
import { DEFAULT_RULES, parseList, type Rules } from '../../lib/rules';
import { load, parsePairing, save, type Pairing, type Transport } from '../../lib/settings';

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

/** A text field saved when it loses focus. */
function Text({ value, onSave, label, placeholder, area }: { value: string; onSave: (v: string) => void; label: string; placeholder?: string; area?: boolean }) {
  const [draft, setDraft] = useState<string | null>(null);
  const props = {
    className: 'inp opt-text',
    'aria-label': label,
    value: draft ?? value,
    placeholder,
    spellCheck: false,
    onChange: (e: { target: { value: string } }) => setDraft(e.target.value),
    onBlur: () => {
      if (draft != null && draft !== value) onSave(draft);
      setDraft(null);
    },
  };
  return area ? <textarea rows={3} {...props} /> : <input {...props} />;
}

export function Options() {
  const [rules, setRules] = useState<Rules>(DEFAULT_RULES);
  const [pairing, setPairing] = useState<Pairing | null>(null);
  const [transport, setTransport] = useState<Transport>('auto');
  const [code, setCode] = useState('');
  const [check, setCheck] = useState<string | null>(null);
  // Firefox (MV3) grants access to sites only when the user agrees; cookies and takeover need it.
  const [sitesAllowed, setSitesAllowed] = useState(true);

  useEffect(() => {
    void load().then((s) => {
      setRules(s.rules);
      setPairing(s.pairing);
      setTransport(s.transport);
    });
    void browser.permissions.contains({ origins: ['<all_urls>'] }).then(setSitesAllowed, () => undefined);
  }, []);

  const allowSites = () =>
    void browser.permissions.request({ origins: ['<all_urls>'] }).then(setSitesAllowed, () => undefined);

  const set = (patch: Partial<Rules>) => {
    const next = { ...rules, ...patch };
    setRules(next);
    void save({ rules: next });
  };

  const test = async () => {
    setCheck('正在连接…');
    try {
      const r = await send<Status>({ type: 'get_status' });
      setCheck(r.ok && r.result ? `已连接 Omni Download ${r.result.app_version}` : `Omni Download 回复了错误：${r.error?.message ?? ''}`);
    } catch (e) {
      const kind = e instanceof BridgeError ? e.kind : 'error';
      setCheck(
        {
          no_host: '没有找到 Omni Download 的浏览器通信程序。请安装并运行一次 Omni Download。',
          not_running: 'Omni Download 没有运行。',
          unpaired: '配对码无效或已重新生成，请重新填写。',
          error: `无法连接：${String((e as Error)?.message ?? e)}`,
        }[kind],
      );
    }
  };

  const applyCode = () => {
    const p = parsePairing(code);
    if (!p) {
      setCheck('配对码的格式不对，应为 omnidl:端口:令牌。');
      return;
    }
    setPairing(p);
    setCode('');
    void save({ pairing: p }).then(test);
  };

  return (
    <div className="options">
      <h1>
        <img src="/icon/48.png" width={28} height={28} alt="" />
        Omni Download 扩展
      </h1>

      {!sitesAllowed && (
        <div className="set-group warn-group">
          <Row label="需要访问网站的权限" hint="读取下载所在网站的 Cookie、在点击下载时接管，都需要这个权限。">
            <button type="button" className="btn primary" onClick={allowSites}>
              授予权限
            </button>
          </Row>
        </div>
      )}

      <h2>接管下载</h2>
      <div className="set-group">
        <Row label="接管浏览器下载" hint="点击下载时交给 Omni Download。按住 Alt 点击链接时仍由浏览器下载。">
          <Switch label="接管浏览器下载" checked={rules.enabled} onChange={(v) => set({ enabled: v })} />
        </Row>
        <Row label="最小文件大小" hint="更小的文件留在浏览器；大小未知的文件会接管">
          <span className="inline">
            <input
              className="inp opt-num"
              type="number"
              min={0}
              step={0.5}
              aria-label="最小文件大小（MB）"
              value={rules.minSizeMB}
              onChange={(e) => set({ minSizeMB: Math.max(0, Number(e.target.value) || 0) })}
            />
            MB
          </span>
        </Row>
        <Row col label="只接管这些扩展名" hint="留空表示所有文件；用空格或逗号分隔，例如 zip 7z iso">
          <Text label="只接管这些扩展名" value={rules.onlyExtensions.join(' ')} onSave={(v) => set({ onlyExtensions: parseList(v) })} />
        </Row>
        <Row col label="不接管这些扩展名">
          <Text label="不接管这些扩展名" value={rules.skipExtensions.join(' ')} placeholder="例如 pdf jpg" onSave={(v) => set({ skipExtensions: parseList(v) })} />
        </Row>
        <Row col label="不接管的网站" hint="每行一个，包括它的子域名">
          <Text area label="不接管的网站" value={rules.skipSites.join('\n')} placeholder="example.com" onSave={(v) => set({ skipSites: v.split(/\s+/).map((s) => s.trim()).filter(Boolean) })} />
        </Row>
        <Row label="直接开始" hint="不弹出 Omni Download 的确认窗口，按它的默认设置开始下载">
          <Switch label="直接开始" checked={rules.silent} onChange={(v) => set({ silent: v })} />
        </Row>
      </div>

      <h2>连接 Omni Download</h2>
      <div className="set-group">
        <Row label="连接状态" hint={check ?? '检查扩展能否连接到 Omni Download'}>
          <button type="button" className="btn outline" onClick={() => void test()}>
            检查连接
          </button>
        </Row>
        <Row label="连接方式" hint="自动：先用本机通信，不可用时用备用通道">
          <Segmented<Transport>
            value={transport}
            label="连接方式"
            onChange={(v) => {
              setTransport(v);
              void save({ transport: v });
            }}
            options={[
              ['auto', '自动'],
              ['native', '本机通信'],
              ['http', '备用通道'],
            ]}
          />
        </Row>
        <Row col label="备用通道配对码" hint={pairing ? `已配对：127.0.0.1:${pairing.port}` : 'Snap、Flatpak 版浏览器用不了本机通信时，在 Omni Download“设置 › 浏览器”中打开备用通道，把配对码粘贴到这里'}>
          <span className="inline">
            <input className="inp opt-text mono" aria-label="配对码" value={code} placeholder="omnidl:端口:令牌" onChange={(e) => setCode(e.target.value)} onKeyDown={(e) => e.key === 'Enter' && applyCode()} />
            <button type="button" className="btn outline" disabled={!code.trim()} onClick={applyCode}>
              配对
            </button>
            {pairing && (
              <button
                type="button"
                className="btn outline"
                onClick={() => {
                  setPairing(null);
                  void save({ pairing: null });
                }}
              >
                取消配对
              </button>
            )}
          </span>
        </Row>
      </div>
    </div>
  );
}
