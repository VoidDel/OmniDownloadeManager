import { Icon } from '../../components/Icon';
import type { Filter } from '../../store/app';
import { useApp } from '../../store/app';

const TEXT: Partial<Record<Filter, [string, string]>> = {
  all: ['还没有下载任务', '把下载链接粘贴到上方的框里，或者点“新建下载”。'],
  active: ['没有进行中的任务', '正在下载、等待和暂停的任务会显示在这里。'],
  done: ['还没有完成的任务', '下载完成的文件会显示在这里，双击即可打开。'],
  torrent: ['还没有种子任务', '粘贴磁力链接、打开种子文件，或把 .torrent 文件拖进窗口。'],
  failed: ['没有失败的任务', '下载出错的任务会显示在这里，可以重试。'],
  trash: ['回收站是空的', '删除的任务会先放在这里，可以还原。'],
};

/** The task list when nothing matches the current filter. */
export function EmptyTasks({ filter }: { filter: Filter }) {
  const openDialog = useApp((s) => s.openDialog);
  const [title, hint] = TEXT[filter] ?? ['这个分类里还没有任务', '下载的文件按扩展名自动归类。'];
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
          新建下载
        </button>
      )}
    </div>
  );
}
