// A newer version: what is new, then downloading and installing it. Downloads in progress stop
// and save their place first; the new version resumes them.

import { useState } from 'react';
import { useTranslation } from 'react-i18next';

import { Modal, ProgressBar } from '../../components/ui';
import { api, asError } from '../../lib/api';
import * as fmt from '../../lib/format';
import { useApp } from '../../store/app';

export function UpdateDialog() {
  const { t } = useTranslation();
  const update = useApp((s) => s.update);
  const version = useApp((s) => s.version);
  const progress = useApp((s) => s.updateProgress);
  const { closeDialog, toast } = useApp.getState();
  const [installing, setInstalling] = useState(false);
  if (!update) return null;

  const install = async () => {
    setInstalling(true);
    try {
      // Resolves only if nothing happened: the app restarts into the new version.
      await api.updateInstall();
    } catch (e) {
      setInstalling(false);
      useApp.setState({ updateProgress: null });
      toast('error', t('update.failed', { reason: asError(e).message }));
    }
  };
  const share = progress?.total ? progress.downloaded / progress.total : 0;
  return (
    <Modal
      title={t('update.title', { version: update.version })}
      onClose={installing ? () => undefined : closeDialog}
      width={480}
      closeLabel={t('dialogs.close')}
      footer={
        <>
          <button type="button" className="btn outline" disabled={installing} onClick={closeDialog}>
            {t('update.later')}
          </button>
          <button type="button" className="btn primary" disabled={installing} onClick={() => void install()}>
            {t('update.install')}
          </button>
        </>
      }
    >
      <div className="update">
        <p className="muted">{t('update.current', { version })}</p>
        {update.notes.trim() && <div className="update-notes">{update.notes.trim()}</div>}
        {installing ? (
          <div className="update-progress">
            <ProgressBar value={share} indeterminate={!progress?.total} />
            <small className="muted">
              {progress
                ? t('update.downloading', { done: fmt.bytes(progress.downloaded), total: fmt.bytes(progress.total) })
                : t('update.starting')}
            </small>
          </div>
        ) : (
          <small className="muted">{t('update.hint')}</small>
        )}
      </div>
    </Modal>
  );
}
