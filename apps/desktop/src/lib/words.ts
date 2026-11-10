// Labels and messages for codes from the Rust side, in the UI language (locales/*/app.json:
// `status`, `category`, `errors`).

import i18n from '../i18n';
import type { AppError, Category, Task, TaskStatus } from './types';

export function statusLabel(status: TaskStatus): string {
  return i18n.t(`status.${status}`);
}

/** The status of a task in words (a torrent looks for its metadata while "connecting"). */
export function statusText(t: Pick<Task, 'status' | 'kind'>): string {
  if (t.kind === 'torrent' && t.status === 'probing') return i18n.t('status.fetchingTorrent');
  return statusLabel(t.status);
}

/** Finished, but the file does not have the checksum the user gave. */
export function checksumFailed(t: Pick<Task, 'status' | 'error'>): boolean {
  return t.status === 'completed' && t.error?.code === 'checksum_mismatch';
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

/** A status colour for text: its `-text` variant, which reads at 4.5:1 (the colour itself is for
 *  dots and bars). omni-ui before those tokens falls back to the colour. */
export function textColor(color: string): string {
  return color.replace(/^var\(--(accent|success|warning|danger|info|bt)\)$/, 'var(--$1-text, var(--$1))');
}

/** A file name as base and extension, so a name cut short keeps its extension on screen
 *  (`ubuntu-24.04….iso`). Only a short last part counts as one. */
export function nameParts(fileName: string): [string, string] {
  const dot = fileName.lastIndexOf('.');
  const at = dot > 0 && fileName.length - dot <= 8 ? dot : fileName.length;
  return [fileName.slice(0, at), fileName.slice(at)];
}

export function categoryLabel(c: Category): string {
  return i18n.t(`category.${c}`);
}

/** What to do about an error (`errors.hints.<code>`), or '' when there is nothing to add. */
export function errorHint(e: AppError): string {
  return i18n.t(`errors.hints.${e.code}`, { defaultValue: '' });
}

/** What went wrong, in the user's words; the technical detail goes in a tooltip. */
export function errorText(e: AppError): string {
  switch (e.code) {
    case 'invalid_request':
      return e.message;
    case 'http_status':
    case 'ftp_reply':
      return e.status ? i18n.t(`errors.${e.code}`, { status: e.status }) : i18n.t(`errors.${e.code}NoStatus`);
    default:
      return i18n.t(`errors.${e.code}`, { defaultValue: i18n.t('errors.internal') });
  }
}
