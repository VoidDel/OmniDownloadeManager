// The extension's pages (popup, options): their language, and how they show states, speeds and
// sizes. Their look is their own (`pages.css`), light or dark as the system is.

import { t } from './i18n';

/** Sets a page up: the language, which the font stacks follow (`zh-TW` for traditional
 *  Chinese, `ja-JP`). */
export function themePage(): void {
  const ui = browser.i18n.getUILanguage();
  document.documentElement.lang = /^zh-(TW|HK|MO|Hant)/i.test(ui) ? 'zh-TW' : /^ja/i.test(ui) ? 'ja-JP' : /^zh/i.test(ui) ? 'zh-CN' : ui;
}

const STATUS_KEY: Record<string, string> = {
  queued: 'statusQueued',
  probing: 'statusProbing',
  downloading: 'statusDownloading',
  paused: 'statusPaused',
  seeding: 'statusSeeding',
  completed: 'statusCompleted',
  failed: 'statusFailed',
};

/** A task state as the app names it; one this version does not know is shown as it is. */
export function statusLabel(status: string): string {
  return STATUS_KEY[status] ? t(STATUS_KEY[status]) : status;
}

function scaled(n: number, units: string[]): string {
  let v = n;
  let u = 0;
  while (v >= 1024 && u < units.length - 1) {
    v /= 1024;
    u++;
  }
  return `${v >= 100 || u === 0 ? Math.round(v) : v.toFixed(1)} ${units[u]}`;
}

export function speed(bps: number): string {
  return scaled(bps, ['B/s', 'KB/s', 'MB/s', 'GB/s']);
}

export function size(bytes: number): string {
  return scaled(bytes, ['B', 'KB', 'MB', 'GB', 'TB']);
}
