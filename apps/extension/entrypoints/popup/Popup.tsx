// The toolbar popup (docs/development-plan.md §5.4): whether Omni Download is reachable, the
// takeover switch, the latest downloads, the media the page plays, and a way to open the app.

import { Icon, ProgressBar, Switch } from 'omni-ui';
import { useEffect, useState } from 'react';

import { BridgeError, send, type Unreachable } from '../../lib/bridge';
import { t, tn } from '../../lib/i18n';
import { speed, statusLabel } from '../../lib/page';
import type { Status } from '../../lib/protocol';
import { DEFAULT_RULES, type Rules } from '../../lib/rules';
import { load, save } from '../../lib/settings';
import { MediaList } from './MediaList';

type State = { kind: 'loading' } | { kind: 'ok'; status: Status } | { kind: 'down'; reason: Unreachable; detail: string };

/** Why the app cannot be reached: the text is `<key>`, what to do about it `<key>Hint`. */
const DOWN: Record<Unreachable, string> = {
  not_running: 'downNotRunning',
  no_host: 'downNoHost',
  forbidden: 'downForbidden',
  unpaired: 'downUnpaired',
  error: 'downError',
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
          <span className="pstate ok" title={t('connected')}>
            <i aria-hidden="true" />
            {state.status.app_version}
          </span>
        )}
        {state.kind === 'down' && (
          <span className="pstate warn">
            <i aria-hidden="true" />
            {t('notConnected')}
          </span>
        )}
      </header>

      {state.kind === 'down' && (
        <div className="pmsg" title={state.detail}>
          <b>{t(DOWN[state.reason])}</b>
          <span>{t(`${DOWN[state.reason]}Hint`)}</span>
        </div>
      )}
      {state.kind === 'ok' && (
        <div className="pline">
          {state.status.running > 0 ? (
            <>
              <Icon name="download" />
              {tn('running', state.status.running)} · {speed(state.status.download_speed)}
            </>
          ) : (
            t('idle')
          )}
        </div>
      )}

      <div className="prow">
        <div className="plab">
          <span>{t('takeover')}</span>
          <small>{rules.enabled ? t('takeoverOn', rules.minSizeMB) : t('takeoverOff')}</small>
        </div>
        <Switch label={t('takeover')} checked={rules.enabled} onChange={setEnabled} />
      </div>

      {state.kind === 'ok' && state.status.recent.length > 0 && (
        <ul className="precent" aria-label={t('recent')}>
          {state.status.recent.map((task) => (
            <li key={task.id}>
              <span className="pname" title={task.name}>
                {task.name}
              </span>
              <span className="pst">{statusLabel(task.status)}</span>
              <ProgressBar value={task.progress ?? 0} indeterminate={task.progress == null && task.status === 'downloading'} />
            </li>
          ))}
        </ul>
      )}

      <MediaList />

      <footer>
        <button type="button" className="btn primary grow" onClick={openApp}>
          {t('openApp')}
        </button>
        <button type="button" className="iconbtn" title={t('options')} aria-label={t('options')} onClick={() => void browser.runtime.openOptionsPage()}>
          <Icon name="gear" />
        </button>
      </footer>
    </div>
  );
}
