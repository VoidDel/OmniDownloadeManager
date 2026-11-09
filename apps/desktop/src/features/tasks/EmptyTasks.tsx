import { Icon } from '../../components/Icon';

/** The task list before there is anything in it. */
export function EmptyTasks() {
  return (
    <div className="emptyst">
      <Icon name="download" />
      <div>
        <b className="empty-title">还没有下载任务</b>
        <p>这是开发预览版，只包含窗口框架；下载功能会在后续版本加入。</p>
      </div>
    </div>
  );
}
