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
import { applyTheme, loadThemeMode } from './lib/theme';

const platform = detectPlatform(navigator.userAgent);
// Before the first render, so the first frame already has the right colours.
applyTheme(loadThemeMode(), platform);

// The WebView's own context menu is replaced by the app's menus (text fields keep theirs).
document.addEventListener('contextmenu', (e) => {
  if (!(e.target as HTMLElement).closest('input, textarea')) e.preventDefault();
});

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App platform={platform} />
  </StrictMode>,
);

// The window is created hidden; show it once the first frame is painted (no white flash).
if (isTauri()) nextFrame(() => nextFrame(() => void getCurrentWindow().show()));
