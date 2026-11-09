import { useTranslation } from 'react-i18next';

import { Icon } from '../../components/Icon';
import type { Filter } from '../../store/app';
import { useApp } from '../../store/app';

/** Filters with their own texts (`empty.<filter>.title` / `.hint`); the categories share
 *  `empty.category`. */
const OWN: readonly Filter[] = ['all', 'active', 'done', 'torrent', 'failed', 'trash'];

/** The task list when nothing matches the current filter. */
export function EmptyTasks({ filter }: { filter: Filter }) {
  const { t } = useTranslation();
  const openDialog = useApp((s) => s.openDialog);
  const key = OWN.includes(filter) ? filter : 'category';
  const title = t(`empty.${key}.title`);
  const hint = t(`empty.${key}.hint`);
  return (
    <div className="emptyst">
      <Icon name={filter === 'trash' ? 'trash' : 'download'} />
      <div>
        <b className="empty-title">{title}</b>
        <p>{hint}</p>
      </div>
      {filter === 'all' && (
        <button type="button" className="btn primary" onClick={() => openDialog({ kind: 'new', text: '' })}>
          <Icon name="plus" />
          {t('shell.cmd.newDownload')}
        </button>
      )}
    </div>
  );
}
