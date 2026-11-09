// The dialog host: renders the open dialog (one at a time).

import { useState } from 'react';
import { useTranslation } from 'react-i18next';

import { Modal } from '../../components/ui';
import { api } from '../../lib/api';
import * as fmt from '../../lib/format';
import { useApp } from '../../store/app';
import { NewTaskDialog } from './NewTaskDialog';

function PurgeDialog({ ids }: { ids: string[] }) {
  const { t } = useTranslation();
  const { closeDialog, run, select } = useApp.getState();
  const [files, setFiles] = useState(false);
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
          <button type="button" className="btn outline" onClick={closeDialog}>
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

export function Dialogs() {
  const dialog = useApp((s) => s.dialog);
  if (!dialog) return null;
  switch (dialog.kind) {
    case 'new':
      return <NewTaskDialog initial={dialog} />;
    case 'purge':
      return <PurgeDialog ids={dialog.ids} />;
    case 'speed':
      return <SpeedDialog />;
  }
}
