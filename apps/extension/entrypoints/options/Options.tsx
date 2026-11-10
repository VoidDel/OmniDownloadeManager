// Options (docs/development-plan.md §5.2): which downloads are taken over, and how the extension
// reaches the app (Native Messaging, or the paired HTTP channel).

import { Segmented, Switch } from 'omni-ui';
import { useEffect, useState, type ReactNode } from 'react';

import { BridgeError, send } from '../../lib/bridge';
import { t } from '../../lib/i18n';
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
    document.title = t('optionsTitle');
  }, []);

  const allowSites = () =>
    void browser.permissions.request({ origins: ['<all_urls>'] }).then(setSitesAllowed, () => undefined);

  const set = (patch: Partial<Rules>) => {
    const next = { ...rules, ...patch };
    setRules(next);
    void save({ rules: next });
  };

  const test = async () => {
    setCheck(t('checking'));
    try {
      const r = await send<Status>({ type: 'get_status' });
      setCheck(r.ok && r.result ? t('checkOk', r.result.app_version) : t('checkAppError', r.error?.message ?? ''));
    } catch (e) {
      const kind = e instanceof BridgeError ? e.kind : 'error';
      setCheck(
        {
          no_host: t('checkNoHost'),
          forbidden: t('checkForbidden'),
          not_running: t('checkNotRunning'),
          unpaired: t('checkUnpaired'),
          error: t('checkError', String((e as Error)?.message ?? e)),
        }[kind],
      );
    }
  };

  const applyCode = () => {
    const p = parsePairing(code);
    if (!p) {
      setCheck(t('pairBadFormat'));
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
        {t('optionsHeading')}
      </h1>

      {!sitesAllowed && (
        <div className="set-group warn-group">
          <Row label={t('sitesNeeded')} hint={t('sitesNeededHint')}>
            <button type="button" className="btn primary" onClick={allowSites}>
              {t('sitesGrant')}
            </button>
          </Row>
        </div>
      )}

      <h2>{t('sectionTakeover')}</h2>
      <div className="set-group">
        <Row label={t('takeover')} hint={t('takeoverHint')}>
          <Switch label={t('takeover')} checked={rules.enabled} onChange={(v) => set({ enabled: v })} />
        </Row>
        <Row label={t('minSize')} hint={t('minSizeHint')}>
          <span className="inline">
            <input
              className="inp opt-num"
              type="number"
              min={0}
              step={0.5}
              aria-label={t('minSizeLabel')}
              value={rules.minSizeMB}
              onChange={(e) => set({ minSizeMB: Math.max(0, Number(e.target.value) || 0) })}
            />
            MB
          </span>
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
      <div className="set-group">
        <Row label={t('connState')} hint={check ?? t('connStateHint')}>
          <button type="button" className="btn outline" onClick={() => void test()}>
            {t('connCheck')}
          </button>
        </Row>
        <Row label={t('transport')} hint={t('transportHint')}>
          <Segmented<Transport>
            value={transport}
            label={t('transport')}
            onChange={(v) => {
              setTransport(v);
              void save({ transport: v });
            }}
            options={[
              ['auto', t('transportAuto')],
              ['native', t('transportNative')],
              ['http', t('transportHttp')],
            ]}
          />
        </Row>
        <Row col label={t('pairCode')} hint={pairing ? t('paired', pairing.port) : t('pairHint')}>
          <span className="inline">
            <input className="inp opt-text mono" aria-label={t('pairLabel')} value={code} placeholder={t('pairPlaceholder')} onChange={(e) => setCode(e.target.value)} onKeyDown={(e) => e.key === 'Enter' && applyCode()} />
            <button type="button" className="btn outline" disabled={!code.trim()} onClick={applyCode}>
              {t('pair')}
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
                {t('unpair')}
              </button>
            )}
          </span>
        </Row>
      </div>
    </div>
  );
}
