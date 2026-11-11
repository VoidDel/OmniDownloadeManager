// The dialog host: renders the open dialog (one at a time).

import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { Modal } from '../../components/ui';
import { UpdateDialog } from './UpdateDialog';
import { Welcome } from './Welcome';
import { api } from '../../lib/api';
import * as fmt from '../../lib/format';
import type { WhenDone } from '../../lib/types';
import { useApp } from '../../store/app';
import { NewTaskDialog } from './NewTaskDialog';

/** Deleting to the recycle bin: with the local files or not, and whether to keep that answer
 *  (then deleting asks no more; Settings › Download changes it). */
function RemoveDialog({ ids }: { ids: string[] }) {
  const { t } = useTranslation();
  const { closeDialog, removeTasks, saveSettings } = useApp.getState();
  const [files, setFiles] = useState(false);
  const [remember, setRemember] = useState(false);
  const confirm = async () => {
    closeDialog();
    if (remember) void saveSettings((s) => (s.general.removeFiles = files ? 'delete' : 'keep'));
    await removeTasks(ids, files);
  };
  return (
    <Modal
      title={ids.length > 1 ? t('dialogs.remove.titleMany', { count: ids.length }) : t('dialogs.remove.titleOne')}
      onClose={closeDialog}
      closeLabel={t('dialogs.close')}
      footer={
        <>
          <button type="button" className="btn outline" onClick={closeDialog}>
            {t('dialogs.cancel')}
          </button>
          <button type="button" className={`btn primary${files ? ' danger-fill' : ''}`} data-autofocus onClick={() => void confirm()}>
            {t('dialogs.remove.confirm')}
          </button>
        </>
      }
    >
      <p className="dlg-text">{files ? t('dialogs.remove.textFiles') : t('dialogs.remove.text')}</p>
      <label className="chk">
        <input type="checkbox" checked={files} onChange={(e) => setFiles(e.target.checked)} />
        {t('dialogs.remove.deleteFiles')}
      </label>
      <label className="chk">
        <input type="checkbox" checked={remember} onChange={(e) => setRemember(e.target.checked)} />
        <span>
          {t('dialogs.remove.remember')}
          <small className="muted chk-hint">{t('dialogs.remove.rememberHint')}</small>
        </span>
      </label>
    </Modal>
  );
}

/** Deleting for good: the tasks go at once, not to the recycle bin, and by default their local
 *  files with them. */
function PurgeDialog({ ids }: { ids: string[] }) {
  const { t } = useTranslation();
  const { closeDialog, run, select } = useApp.getState();
  const [files, setFiles] = useState(true);
  const confirm = async () => {
    closeDialog();
    select([], null);
    await run(() => api.purge(ids, files));
  };
  return (
    <Modal
      title={ids.length > 1 ? t('dialogs.purge.titleMany', { count: ids.length }) : t('dialogs.purge.titleOne')}
      onClose={closeDialog}
      closeLabel={t('dialogs.close')}
      footer={
        <>
          {/* It cannot be undone: Enter is "cancel". */}
          <button type="button" className="btn outline" data-autofocus onClick={closeDialog}>
            {t('dialogs.cancel')}
          </button>
          <button type="button" className="btn primary danger-fill" onClick={() => void confirm()}>
            {t('dialogs.purge.confirm')}
          </button>
        </>
      }
    >
      <p className="dlg-text">{t('dialogs.purge.text')}</p>
      <label className="chk">
        <input type="checkbox" checked={files} onChange={(e) => setFiles(e.target.checked)} />
        {t('dialogs.purge.deleteFiles')}
      </label>
    </Modal>
  );
}

function SpeedDialog() {
  const { t } = useTranslation();
  const current = useApp((s) => s.settings.engine.speedLimit);
  const { closeDialog, run } = useApp.getState();
  const [value, setValue] = useState(current != null ? String(Math.round(current / 1024)) : '');
  const parsed = fmt.parseSpeed(value);
  const confirm = async () => {
    if (parsed === undefined) return;
    const settings = await run(() => api.setSpeedLimit(parsed));
    if (settings) useApp.setState({ settings });
    closeDialog();
  };
  return (
    <Modal
      title={t('dialogs.speed.title')}
      onClose={closeDialog}
      width={400}
      closeLabel={t('dialogs.close')}
      footer={
        <>
          <button type="button" className="btn outline" onClick={closeDialog}>
            {t('dialogs.cancel')}
          </button>
          <button type="button" className="btn primary" disabled={parsed === undefined} onClick={() => void confirm()}>
            {t('dialogs.ok')}
          </button>
        </>
      }
    >
      <div className="field">
        <label htmlFor="speed-limit">{t('dialogs.speed.label')}</label>
        <input
          id="speed-limit"
          className="inp"
          value={value}
          placeholder={t('dialogs.speed.placeholder')}
          aria-invalid={parsed === undefined}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && void confirm()}
        />
        <small className="hint">
          {parsed === undefined ? t('dialogs.speed.invalid') : parsed == null ? t('dialogs.speed.empty') : t('dialogs.speed.perSecond', { size: fmt.bytes(parsed) })}
        </small>
      </div>
    </Modal>
  );
}

/** Everything downloaded: the computer sleeps or shuts down when the count reaches zero (the app
 *  does it; this only shows the count and can stop it). */
function PowerDialog({ action, seconds }: { action: WhenDone; seconds: number }) {
  const { t } = useTranslation();
  const { closeDialog, run } = useApp.getState();
  const [deadline] = useState(() => Date.now() + seconds * 1000);
  const [left, setLeft] = useState(seconds);
  useEffect(() => {
    const timer = setInterval(() => setLeft(Math.max(0, Math.ceil((deadline - Date.now()) / 1000))), 250);
    return () => clearInterval(timer);
  }, [deadline]);
  const cancel = () => {
    closeDialog();
    void run(() => api.powerCancel());
  };
  const now = () => {
    closeDialog();
    void run(() => api.powerNow());
  };
  return (
    <Modal
      title={t(`dialogs.power.title.${action}`)}
      onClose={cancel}
      width={420}
      closeLabel={t('dialogs.cancel')}
      footer={
        <>
          <button type="button" className="btn outline" data-autofocus onClick={cancel}>
            {t('dialogs.cancel')}
          </button>
          <button type="button" className="btn primary danger-fill" onClick={now}>
            {t(`dialogs.power.now.${action}`)}
          </button>
        </>
      }
    >
      <p className="dlg-text" role="timer" aria-live="polite">
        {t(`dialogs.power.text.${action}`, { count: left })}
      </p>
    </Modal>
  );
}

export function Dialogs() {
  const dialog = useApp((s) => s.dialog);
  if (!dialog) return null;
  switch (dialog.kind) {
    case 'new':
      return <NewTaskDialog initial={dialog} />;
    case 'purge':
      return <PurgeDialog ids={dialog.ids} />;
    case 'remove':
      return <RemoveDialog ids={dialog.ids} />;
    case 'speed':
      return <SpeedDialog />;
    case 'update':
      return <UpdateDialog />;
    case 'welcome':
      return <Welcome />;
    case 'power':
      return <PowerDialog action={dialog.action} seconds={dialog.seconds} />;
  }
}
