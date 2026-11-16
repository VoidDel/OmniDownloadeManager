// The toolbar popup (docs/development-plan.md §5.4): whether Omni Download is reachable, two tabs
// (the media the page plays, the latest downloads), and ways to open the app and the options
// (where the takeover switch is). It opens on the page's media when there is some, else on the
// downloads.

import { useEffect, useState } from 'react';

import { BridgeError, send, type Unreachable } from '../../lib/bridge';
import { t, tn } from '../../lib/i18n';
import { offers } from '../../lib/media';
import { speed, statusLabel } from '../../lib/page';
import type { Status } from '../../lib/protocol';
import { Bar, Icon, ViewTabs } from '../../lib/ui';
import { MediaList, usePageMedia } from './MediaList';

type View = 'media' | 'downloads';

type State = { kind: 'loading' } | { kind: 'ok'; status: Status } | { kind: 'down'; reason: Unreachable; detail: string };

/** Why the app cannot be reached: the text is `<key>`, what to do about it `<key>Hint`. */
const DOWN: Record<Unreachable, string> = {
  not_running: 'downNotRunning',
  no_host: 'downNoHost',
  forbidden: 'downForbidden',
  unpaired: 'downUnpaired',
  error: 'downError',
};

/** How a task's bar is coloured: as the app colours its list. */
function tone(status: string): string {
  return status === 'completed' ? 'done' : status === 'failed' ? 'failed' : status === 'paused' ? 'paused' : status === 'cancelled' ? 'cancelled' : 'active';
}

/** A task's state, with how far it has got while it downloads. */
function progressText(status: string, progress: number | null): string {
  const label = statusLabel(status);
  return status === 'downloading' && progress != null ? `${label} ${Math.floor(progress * 100)}%` : label;
}

export function Popup() {
  const [state, setState] = useState<State>({ kind: 'loading' });
  const media = usePageMedia();
  /** The tab the user chose; until then, the media when the page has some. */
  const [view, setView] = useState<View | null>(null);

  useEffect(() => {
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
    const timer = setInterval(() => void poll(), 1500);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, []);

  const openApp = () => void send({ type: 'open_app' }).then(() => window.close(), () => undefined);
  const status = state.kind === 'ok' ? state.status : null;
  const recent = status?.recent ?? [];
  const mediaCount = offers(media).length;
  const shown: View = view ?? (mediaCount > 0 || !recent.length ? 'media' : 'downloads');
  const count = (n: number) => (n > 0 ? <span className="pcount">{n}</span> : null);

  return (
    <div className="popup">
      <header>
        <img src="/icon/32.png" alt="" />
        <div className="ptitle">
          <b>Omni Download</b>
          {status && (
            <span className="pline">
              {status.running > 0 ? (
                <>
                  <span>{tn('running', status.running)}</span>
                  <span className="spd">{speed(status.download_speed)}</span>
                </>
              ) : (
                <span>{t('idle')}</span>
              )}
            </span>
          )}
        </div>
        {status && (
          <span className="pstate ok" title={`Omni Download ${status.app_version}`}>
            <i className="dot" aria-hidden="true" />
            {t('connected')}
          </span>
        )}
        {state.kind === 'down' && (
          <span className="pstate warn">
            <i className="dot" aria-hidden="true" />
            {t('notConnected')}
          </span>
        )}
      </header>

      {state.kind === 'down' && (
        <div className="pbanner" title={state.detail}>
          <Icon name="alert" />
          <div>
            <b>{t(DOWN[state.reason])}</b>
            <span>{t(`${DOWN[state.reason]}Hint`)}</span>
          </div>
        </div>
      )}

      <ViewTabs<View>
        label="Omni Download"
        value={shown}
        onChange={setView}
        options={[
          ['media', <>{t('mediaTitle')}{count(mediaCount)}</>, 'panel-media'],
          ['downloads', <>{t('recent')}{count(recent.length)}</>, 'panel-downloads'],
        ]}
      />

      {shown === 'media' ? (
        <MediaList items={media} id="panel-media" />
      ) : (
        <section className="ppanel" id="panel-downloads" role="tabpanel" aria-labelledby="tab-panel-downloads">
          {recent.length ? (
            <ul className="precent">
              {recent.map((task) => (
                <li key={task.id} className={`st-${tone(task.status)}`}>
                  <span className="pname" title={task.name}>
                    {task.name}
                  </span>
                  <span className="pst">{progressText(task.status, task.progress)}</span>
                  {/* A finished one says so; a bar would only be full. */}
                  {task.status !== 'completed' && <Bar value={task.progress ?? 0} busy={task.progress == null && task.status === 'downloading'} />}
                </li>
              ))}
            </ul>
          ) : (
            <p className="pnone">{t(status ? 'recentNone' : 'recentUnreachable')}</p>
          )}
        </section>
      )}

      <footer className="pfoot">
        <div className="pact">
          <button type="button" className="btn primary" onClick={openApp}>
            {t('openApp')}
          </button>
          <button type="button" className="icon-btn" title={t('options')} aria-label={t('options')} onClick={() => void browser.runtime.openOptionsPage()}>
            <Icon name="settings" />
          </button>
        </div>
      </footer>
    </div>
  );
}
