// Options (docs/development-plan.md §5.2): which downloads are taken over, and how the extension
// reaches the app (Native Messaging, or the paired HTTP channel). Whether it does is the first
// thing the page says, checked when it opens.

import { useEffect, useState, type ReactNode } from 'react';

import { BridgeError, send } from '../../lib/bridge';
import { t } from '../../lib/i18n';
import type { Status } from '../../lib/protocol';
import { DEFAULT_RULES, parseList, type Rules } from '../../lib/rules';
import { load, parsePairing, save, type Pairing, type Transport } from '../../lib/settings';
import { Icon, Switch } from '../../lib/ui';

const REPO = 'https://github.com/VoidDel/OmniDownloadeManager';
const GUIDE = `${REPO}/blob/main/docs/user-guide.md#接管浏览器的下载`;
const PRIVACY = `${REPO}/blob/main/docs/privacy.md`;
const RELEASES = `${REPO}/releases/latest`;

function Row({ label, hint, children, col }: { label: ReactNode; hint?: ReactNode; children?: ReactNode; col?: boolean }) {
  return (
    <div className={`orow${col ? ' col' : ''}`}>
      <div className="olab">
        <span>{label}</span>
        {hint && <small>{hint}</small>}
      </div>
      <div className="octl">{children}</div>
    </div>
  );
}

/** A text field saved when it loses focus. */
function Text({ value, onSave, label, placeholder, area }: { value: string; onSave: (v: string) => void; label: string; placeholder?: string; area?: boolean }) {
  const [draft, setDraft] = useState<string | null>(null);
  const props = {
    className: 'field wide',
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

/** One value out of a few, as a segmented choice (a radio group). */
function Choice<T extends string>({ value, options, onChange, label }: { value: T; options: [T, string][]; onChange: (v: T) => void; label: string }) {
  return (
    <div className="seg" role="radiogroup" aria-label={label}>
      {options.map(([v, text]) => (
        <button key={v} type="button" role="radio" aria-checked={v === value} onClick={() => onChange(v)}>
          {text}
        </button>
      ))}
    </div>
  );
}

type Check = { tone: 'busy' | 'ok' | 'warn'; title: string; text?: string };

/** Whether the app is reached, and if not, why. */
async function ask(): Promise<Check> {
  try {
    const r = await send<Status>({ type: 'get_status' });
    return r.ok && r.result ? { tone: 'ok', title: t('checkOk', r.result.app_version) } : { tone: 'warn', title: t('notConnected'), text: t('checkAppError', r.error?.message ?? '') };
  } catch (e) {
    const kind = e instanceof BridgeError ? e.kind : 'error';
    return {
      tone: 'warn',
      title: t('notConnected'),
      text: {
        no_host: t('checkNoHost'),
        forbidden: t('checkForbidden'),
        not_running: t('checkNotRunning'),
        unpaired: t('checkUnpaired'),
        error: t('checkError', String((e as Error)?.message ?? e)),
      }[kind],
    };
  }
}

export function Options() {
  const [rules, setRules] = useState<Rules>(DEFAULT_RULES);
  const [pairing, setPairing] = useState<Pairing | null>(null);
  const [transport, setTransport] = useState<Transport>('auto');
  const [code, setCode] = useState('');
  /** Whether the app is reached: being checked, yes (`ok`), or why not (`warn`). */
  const [check, setCheck] = useState<Check>({ tone: 'busy', title: t('checking') });
  // Firefox (MV3) grants access to sites only when the user agrees; cookies and takeover need it.
  const [sitesAllowed, setSitesAllowed] = useState(true);

  useEffect(() => {
    void load().then((s) => {
      setRules(s.rules);
      setPairing(s.pairing);
      setTransport(s.transport);
    });
    void browser.permissions.contains({ origins: ['<all_urls>'] }).then(setSitesAllowed, () => undefined);
    document.title = t('optionsTitle');
    // The page says "checking" already.
    void ask().then(setCheck);
  }, []);

  /** Asks again, saying so meanwhile. */
  const test = () => {
    setCheck({ tone: 'busy', title: t('checking') });
    void ask().then(setCheck);
  };

  const allowSites = () => void browser.permissions.request({ origins: ['<all_urls>'] }).then(setSitesAllowed, () => undefined);

  const set = (patch: Partial<Rules>) => {
    const next = { ...rules, ...patch };
    setRules(next);
    void save({ rules: next });
  };

  const applyCode = () => {
    const p = parsePairing(code);
    if (!p) {
      setCheck({ tone: 'warn', title: t('notConnected'), text: t('pairBadFormat') });
      return;
    }
    setPairing(p);
    setCode('');
    void save({ pairing: p }).then(() => test());
  };

  return (
    <div className="options">
      <header className="ohead">
        <img src="/icon/48.png" alt="" />
        <h1>{t('optionsHeading')}</h1>
      </header>

      {!sitesAllowed && (
        <div className="ogroup owarn">
          <Row label={t('sitesNeeded')} hint={t('sitesNeededHint')}>
            <button type="button" className="btn primary" onClick={allowSites}>
              {t('sitesGrant')}
            </button>
          </Row>
        </div>
      )}

      <div className={`ostatus ${check.tone}`} role="status">
        <i className="dot" aria-hidden="true" />
        <div>
          <b>{check.title}</b>
          {check.tone !== 'ok' && <span>{check.text ?? t('connStateHint')}</span>}
        </div>
        <button type="button" className="btn quiet" disabled={check.tone === 'busy'} onClick={test}>
          {t('connCheck')}
        </button>
      </div>

      <h2>{t('sectionTakeover')}</h2>
      <div className="ogroup">
        <Row label={t('takeover')} hint={t('takeoverHint')}>
          <Switch label={t('takeover')} checked={rules.enabled} onChange={(v) => set({ enabled: v })} />
        </Row>
        <Row label={t('minSize')} hint={t('minSizeHint')}>
          <input
            className="field num"
            type="number"
            min={0}
            step={0.5}
            aria-label={t('minSizeLabel')}
            value={rules.minSizeMB}
            onChange={(e) => set({ minSizeMB: Math.max(0, Number(e.target.value) || 0) })}
          />
          <span className="ounit">MB</span>
        </Row>
        <Row col label={t('onlyExt')} hint={t('onlyExtHint')}>
          <Text label={t('onlyExt')} value={rules.onlyExtensions.join(' ')} onSave={(v) => set({ onlyExtensions: parseList(v) })} />
        </Row>
        <Row col label={t('skipExt')}>
          <Text label={t('skipExt')} value={rules.skipExtensions.join(' ')} placeholder={t('skipExtPlaceholder')} onSave={(v) => set({ skipExtensions: parseList(v) })} />
        </Row>
        <Row col label={t('skipSites')} hint={t('skipSitesHint')}>
          <Text area label={t('skipSites')} value={rules.skipSites.join('\n')} placeholder="example.com" onSave={(v) => set({ skipSites: v.split(/\s+/).map((s) => s.trim()).filter(Boolean) })} />
        </Row>
        <Row label={t('silent')} hint={t('silentHint')}>
          <Switch label={t('silent')} checked={rules.silent} onChange={(v) => set({ silent: v })} />
        </Row>
      </div>

      <h2>{t('sectionConnect')}</h2>
      <div className="ogroup">
        <Row label={t('transport')} hint={t('transportHint')}>
          <Choice<Transport>
            value={transport}
            label={t('transport')}
            onChange={(v) => {
              setTransport(v);
              void save({ transport: v }).then(() => test());
            }}
            options={[
              ['auto', t('transportAuto')],
              ['native', t('transportNative')],
              ['http', t('transportHttp')],
            ]}
          />
        </Row>
        <Row col label={t('pairCode')} hint={pairing ? t('paired', pairing.port) : t('pairHint')}>
          <input className="field wide code" aria-label={t('pairLabel')} value={code} placeholder={t('pairPlaceholder')} onChange={(e) => setCode(e.target.value)} onKeyDown={(e) => e.key === 'Enter' && applyCode()} />
          <button type="button" className="btn quiet" disabled={!code.trim()} onClick={applyCode}>
            {t('pair')}
          </button>
          {pairing && (
            <button
              type="button"
              className="btn quiet"
              onClick={() => {
                setPairing(null);
                void save({ pairing: null }).then(() => test());
              }}
            >
              {t('unpair')}
            </button>
          )}
        </Row>
      </div>

      <footer className="ofoot">
        <span>{t('optionsVersion', browser.runtime.getManifest().version)}</span>
        <a href={GUIDE} target="_blank" rel="noreferrer">
          {t('footGuide')}
          <Icon name="external" />
        </a>
        <a href={PRIVACY} target="_blank" rel="noreferrer">
          {t('footPrivacy')}
          <Icon name="external" />
        </a>
        <a href={RELEASES} target="_blank" rel="noreferrer">
          {t('footGetApp')}
          <Icon name="external" />
        </a>
      </footer>
    </div>
  );
}
