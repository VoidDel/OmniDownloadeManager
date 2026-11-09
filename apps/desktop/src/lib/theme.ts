// Theme for Phase 0: the default pair (Aurora light / Midnight dark) resolved in the page with
// omni-ui. From Phase 1 the Rust theme service resolves it (user themes, accent, material) and
// the page only writes the result, as OmniZip does.

import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import {
  DEFAULT_DARK_THEME,
  DEFAULT_LIGHT_THEME,
  builtinTheme,
  resolveTheme,
  systemPrefersDark,
  writeTheme,
  writeThemeAnimated,
  type ApplyOptions,
  type Platform,
} from 'omni-ui';

export type ThemeMode = 'system' | 'light' | 'dark';

const KEY = 'omnidl.themeMode';

/** The mode picked last time; a convenience kept in the web view until settings exist. */
export function loadThemeMode(): ThemeMode {
  try {
    const v = localStorage.getItem(KEY);
    if (v === 'system' || v === 'light' || v === 'dark') return v;
  } catch {
    // Storage unavailable: follow the system.
  }
  return 'system';
}

export function saveThemeMode(mode: ThemeMode): void {
  try {
    localStorage.setItem(KEY, mode);
  } catch {
    // Storage unavailable: the choice lasts until the window closes.
  }
}

export function isDark(mode: ThemeMode): boolean {
  return mode === 'dark' || (mode === 'system' && systemPrefersDark());
}

/** Resolves the theme for `mode` and writes it to the document, cross-fading when `animate`. */
export function applyTheme(mode: ThemeMode, platform: Platform, animate = false): void {
  const theme = builtinTheme(isDark(mode) ? DEFAULT_DARK_THEME : DEFAULT_LIGHT_THEME);
  if (!theme) return;
  const resolved = resolveTheme(theme, { accent: 'theme', material: false });
  const opts: ApplyOptions = { platform, density: 'comfortable', iconStyle: 'duotone', material: false, fontSize: 14 };
  if (animate) writeThemeAnimated(resolved, opts);
  else writeTheme(resolved, opts);
  // The native window follows too (title bar colours, web view scheme); "system" hands it back
  // to the OS so prefers-color-scheme keeps tracking it.
  if (isTauri()) {
    void getCurrentWindow()
      .setTheme(mode === 'system' ? null : mode)
      .catch(() => undefined);
  }
}
