// Shared Omni design system (tokens, base, window shell, components), then this app's own styles.
import 'omni-ui/styles.css';
import './styles/app.css';

import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { StrictMode, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
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
    createRoot(document.getElementById('root')!).render(
      <StrictMode>
        <Language>{quick ? <QuickApp /> : <App platform={platform} />}</Language>
      </StrictMode>,
    );
    // The main window is created hidden; it shows once the first frame is painted (no white
    // flash), unless the browser bridge started the app in the background. Quick-download
    // windows show themselves once they have their request.
    // A window coming back from the notification area was asked for: it takes the focus too.
    if (isTauri() && !quick && !background) {
      nextFrame(() =>
        nextFrame(() => {
          const w = getCurrentWindow();
          void w.show().then(() => w.setFocus());
        }),
      );
    }
  });

/** Draws everything again when the language changes: texts made outside components (status,
 *  errors, sizes) follow too. The app's state lives in the store, so nothing is lost. */
function Language({ children }: { children: ReactNode }) {
  const { i18n } = useTranslation();
  return <div key={i18n.language} style={{ display: 'contents' }}>{children}</div>;
}
