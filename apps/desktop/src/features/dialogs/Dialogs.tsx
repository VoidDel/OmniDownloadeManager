// The dialog host: renders the open dialog (one at a time).

import { useState } from 'react';

import { Modal } from '../../components/ui';
import { api } from '../../lib/api';
import * as fmt from '../../lib/format';
import { useApp } from '../../store/app';
import { NewTaskDialog } from './NewTaskDialog';

function PurgeDialog({ ids }: { ids: string[] }) {
  const { closeDialog, run, select } = useApp.getState();
  const [files, setFiles] = useState(false);
  const confirm = async () => {
    closeDialog();
    select([], null);
    await run(() => api.purge(ids, files));
  };
  return (
    <Modal
      title={ids.length > 1 ? `彻底删除 ${ids.length} 个任务？` : '彻底删除这个任务？'}
      onClose={closeDialog}
      closeLabel="关闭"
      footer={
        <>
          <button type="button" className="btn outline" onClick={closeDialog}>
            取消
          </button>
          <button type="button" className="btn primary danger-fill" onClick={() => void confirm()}>
            彻底删除
          </button>
        </>
      }
    >
      <p className="dlg-text">任务记录和未完成的临时文件会被删除，无法还原。</p>
      <label className="chk">
        <input type="checkbox" checked={files} onChange={(e) => setFiles(e.target.checked)} />
        同时删除已下载完成的文件
      </label>
    </Modal>
  );
}

function SpeedDialog() {
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
      title="全局限速"
      onClose={closeDialog}
      width={400}
      closeLabel="关闭"
      footer={
        <>
          <button type="button" className="btn outline" onClick={closeDialog}>
            取消
          </button>
          <button type="button" className="btn primary" disabled={parsed === undefined} onClick={() => void confirm()}>
            确定
          </button>
        </>
      }
    >
      <div className="field">
        <label htmlFor="speed-limit">所有任务合计的最高下载速度</label>
        <input
          id="speed-limit"
          className="inp"
          value={value}
          placeholder="不限速"
          aria-invalid={parsed === undefined}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && void confirm()}
        />
        <small className="hint">{parsed === undefined ? '无法识别，例如 500K、2M、1.5MB/s' : parsed == null ? '留空表示不限速' : `每秒 ${fmt.bytes(parsed)}`}</small>
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
