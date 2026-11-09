// Theme: one of omni-ui's twelve built-in themes for light and one for dark, resolved in the page
// with omni-ui (the same resolver as OmniZip's Rust theme service) and written to the document,
// with the window material where the system has one (src-tauri/src/material.rs).

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

import type { Density, Settings, ThemeMode } from './types';

/** Row height of each density (omni-ui's `--row-h`). */
export const ROW_HEIGHT: Record<Density, number> = { compact: 28, comfortable: 32, spacious: 40 };

/** What the system offers (from the app; a browser has neither). */
let materialSupported = false;
let systemAccent: string | null = null;

export function setSystemLook(o: { materialSupported: boolean; systemAccent: string | null }): void {
  materialSupported = o.materialSupported;
  systemAccent = o.systemAccent;
}

export function isDark(mode: ThemeMode): boolean {
  return mode === 'dark' || (mode === 'system' && systemPrefersDark());
}

/** Resolves the theme of `a` and writes it to the document, cross-fading when `animate`. */
export function applyTheme(a: Settings['appearance'], platform: Platform, animate = false): void {
  const dark = isDark(a.mode);
  const theme = builtinTheme(dark ? a.darkTheme : a.lightTheme) ?? builtinTheme(dark ? DEFAULT_DARK_THEME : DEFAULT_LIGHT_THEME);
  if (!theme) return;
  const material = a.material && materialSupported;
  const resolved = resolveTheme(theme, { accent: a.accent || 'theme', systemAccent, material });
  const opts: ApplyOptions = { platform, density: a.density, iconStyle: 'duotone', material, fontSize: 14 };
  if (animate) writeThemeAnimated(resolved, opts);
  else writeTheme(resolved, opts);
  // The native window follows too (title bar colours, web view scheme); "system" hands it back
  // to the OS so prefers-color-scheme keeps tracking it.
  if (isTauri()) {
    void getCurrentWindow()
      .setTheme(a.mode === 'system' ? null : a.mode)
      .catch(() => undefined);
  }
}
