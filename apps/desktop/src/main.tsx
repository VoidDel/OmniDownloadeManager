// Shared Omni design system (tokens, base, window shell, components), then this app's own styles.
import 'omni-ui/styles.css';
import './styles/app.css';

import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { App } from './App';
import { nextFrame } from './lib/frame';
import { detectPlatform } from './lib/platform';
import { applyTheme } from './lib/theme';
import { DEFAULT_SETTINGS, useApp } from './store/app';

const platform = detectPlatform(navigator.userAgent);
// Something to paint with while the settings load (the window is still hidden).
applyTheme(DEFAULT_SETTINGS.appearance, platform);

// The WebView's own context menu is replaced by the app's menus (text fields keep theirs).
document.addEventListener('contextmenu', (e) => {
  if (!(e.target as HTMLElement).closest('input, textarea')) e.preventDefault();
});

// Settings, theme and tasks first, so the first frame is the real one.
void useApp
  .getState()
  .boot(platform)
  .finally(() => {
    createRoot(document.getElementById('root')!).render(
      <StrictMode>
        <App platform={platform} />
      </StrictMode>,
    );
    // The window is created hidden; show it once the first frame is painted (no white flash).
    if (isTauri()) nextFrame(() => nextFrame(() => void getCurrentWindow().show()));
  });
