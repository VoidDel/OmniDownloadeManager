// The extension's pages (popup, options) look like the app: omni-ui's tokens, components and
// default theme pair, following the system's light / dark setting.

import { DEFAULT_DARK_THEME, DEFAULT_LIGHT_THEME, builtinTheme, onSystemThemeChange, resolveTheme, systemPrefersDark, writeTheme, type Platform } from 'omni-ui';

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

export function themePage(): void {
  apply();
  onSystemThemeChange(apply);
}

export const STATUS_LABEL: Record<string, string> = {
  queued: '等待中',
  probing: '连接中',
  downloading: '下载中',
  paused: '已暂停',
  seeding: '做种中',
  completed: '已完成',
  failed: '失败',
};

export function speed(bps: number): string {
  const units = ['B/s', 'KB/s', 'MB/s', 'GB/s'];
  let v = bps;
  let u = 0;
  while (v >= 1024 && u < units.length - 1) {
    v /= 1024;
    u++;
  }
  return `${v >= 100 || u === 0 ? Math.round(v) : v.toFixed(1)} ${units[u]}`;
}
