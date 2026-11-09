// Labels and messages shown for codes from the Rust side. Chinese until i18n (Phase 5).

import type { AppError, Category, Task, TaskStatus } from './types';

export const STATUS_LABEL: Record<TaskStatus, string> = {
  queued: '等待中',
  probing: '连接中',
  downloading: '下载中',
  paused: '已暂停',
  seeding: '做种中',
  completed: '已完成',
  failed: '失败',
  removed: '已删除',
};

/** The status of a task in words (a torrent looks for its metadata while "connecting"). */
export function statusText(t: Pick<Task, 'status' | 'kind'>): string {
  if (t.kind === 'torrent' && t.status === 'probing') return '获取种子信息';
  return STATUS_LABEL[t.status];
}

/** State colours (docs/development-plan.md §6.1): colour only says state. */
export const STATUS_COLOR: Record<TaskStatus, string> = {
  queued: 'var(--fg-muted)',
  probing: 'var(--accent)',
  downloading: 'var(--accent)',
  paused: 'var(--warning)',
  seeding: 'var(--info)',
  completed: 'var(--success)',
  failed: 'var(--danger)',
  removed: 'var(--fg-muted)',
};

export const CATEGORY_LABEL: Record<Category, string> = {
  video: '视频',
  audio: '音乐',
  document: '文档',
  archive: '压缩包',
  program: '程序',
  disk_image: '镜像',
  image: '图片',
  other: '其他',
};

/** What went wrong, in the user's words; the technical detail goes in a tooltip. */
export function errorText(e: AppError): string {
  switch (e.code) {
    case 'invalid_url':
      return '不是可以下载的链接';
    case 'unsupported':
      return '这类下载将在后续版本支持';
    case 'network':
      return '网络连接失败';
    case 'timeout':
      return '连接超时';
    case 'tls':
      return '安全连接失败，证书无效';
    case 'http_status':
      return e.status ? `服务器返回错误 ${e.status}` : '服务器返回错误';
    case 'not_found':
      return '服务器上没有这个文件';
    case 'auth_required':
      return '需要登录或授权';
    case 'forbidden':
      return '服务器拒绝访问';
    case 'remote_changed':
      return '服务器上的文件已变更，需要重新下载';
    case 'file_missing':
      return '文件丢失或无法读取';
    case 'disk_full':
      return '磁盘空间不足';
    case 'io':
      return '读写文件失败';
    case 'invalid_request':
      return e.message;
    default:
      return '出现内部错误';
  }
}
