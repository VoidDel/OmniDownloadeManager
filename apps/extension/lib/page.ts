// The extension's pages (popup, options) look like the app: omni-ui's tokens, components and
// default theme pair, following the system's light / dark setting.

import { DEFAULT_DARK_THEME, DEFAULT_LIGHT_THEME, builtinTheme, onSystemThemeChange, resolveTheme, systemPrefersDark, writeTheme, type Platform } from 'omni-ui';

import { t } from './i18n';

function platform(): Platform {
  const ua = navigator.userAgent;
  if (/Windows/.test(ua)) return 'win';
  if (/Macintosh|Mac OS X/.test(ua)) return 'mac';
  return 'linux';
}

function apply() {
  const theme = builtinTheme(systemPrefersDark() ? DEFAULT_DARK_THEME : DEFAULT_LIGHT_THEME);
  if (!theme) return;
  writeTheme(resolveTheme(theme, { accent: 'theme', material: false }), {
    platform: platform(),
    density: 'comfortable',
    iconStyle: 'duotone',
    material: false,
    fontSize: 14,
  });
}

/** Sets a page up: the theme, and the language (the font stacks follow it). */
export function themePage(): void {
  // As omni-ui's font stacks name the languages: `zh-TW` for traditional Chinese, `ja-JP`.
  const ui = browser.i18n.getUILanguage();
  document.documentElement.lang = /^zh-(TW|HK|MO|Hant)/i.test(ui) ? 'zh-TW' : /^ja/i.test(ui) ? 'ja-JP' : ui;
  apply();
  onSystemThemeChange(apply);
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
