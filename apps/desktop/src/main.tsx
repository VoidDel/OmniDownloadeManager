// Shared Omni design system (tokens, base, window shell, components), then this app's own styles.
import 'omni-ui/styles.css';
import './styles/app.css';

import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { App } from './App';
import { QuickApp } from './features/quick/QuickApp';
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

// The same page serves the main window and the quick-download windows (`quick-<n>`).
const quick = isTauri() && getCurrentWindow().label.startsWith('quick-');

// Settings, theme and tasks first, so the first frame is the real one.
void useApp
  .getState()
  .boot(platform)
  .then((background) => {
    createRoot(document.getElementById('root')!).render(<StrictMode>{quick ? <QuickApp /> : <App platform={platform} />}</StrictMode>);
    // The main window is created hidden; it shows once the first frame is painted (no white
    // flash), unless the browser bridge started the app in the background. Quick-download
    // windows show themselves once they have their request.
    if (isTauri() && !quick && !background) nextFrame(() => nextFrame(() => void getCurrentWindow().show()));
  });
