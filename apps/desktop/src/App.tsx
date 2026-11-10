// The main window (docs/development-plan.md §6.2): sidebar, title bar, command bar, the task list
// with its details panel (or the settings page), and the status bar, in omni-ui's window shell.

import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { onSystemThemeChange, type Platform } from 'omni-ui';
import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';

import { Toasts } from './components/ui';
import { Dialogs } from './features/dialogs/Dialogs';
import { Settings } from './features/settings/Settings';
import { CommandBar, ResizeEdges, Sidebar, StatusBar, TitleBar } from './features/shell/Chrome';
import { TaskDetail } from './features/tasks/TaskDetail';
import { TaskTable } from './features/tasks/TaskTable';
import { hasLink } from './lib/links';
import { applyTheme } from './lib/theme';
import { detailTask, useApp } from './store/app';

/** Window-wide shortcuts: Ctrl/⌘+N new download, Ctrl/⌘+, settings, Ctrl/⌘+F the search box. */
function useShortcuts() {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || e.altKey) return;
      const s = useApp.getState();
      if (s.dialog) return;
      const key = e.key.toLowerCase();
      if (key === 'n') {
        e.preventDefault();
        s.openDialog({ kind: 'new', text: '' });
      } else if (key === ',') {
        e.preventDefault();
        s.setView(s.view === 'settings' ? 'tasks' : 'settings');
      } else if (key === 'f') {
        e.preventDefault();
        const box = document.getElementById('search') as HTMLInputElement | null;
        box?.focus();
        box?.select();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);
}

/** Links pasted anywhere but in a text field (Ctrl/⌘+V on the list) open the new-download
 *  dialog with them. */
function usePasteLinks() {
  useEffect(() => {
    const onPaste = (e: ClipboardEvent) => {
      const el = e.target as HTMLElement | null;
      if (el?.closest('input, textarea, [contenteditable="true"]')) return;
      const s = useApp.getState();
      if (s.dialog) return;
      const text = e.clipboardData?.getData('text') ?? '';
      if (!hasLink(text)) return;
      e.preventDefault();
      s.setView('tasks');
      s.openDialog({ kind: 'new', text });
    };
    window.addEventListener('paste', onPaste);
    return () => window.removeEventListener('paste', onPaste);
  }, []);
}

/** `.torrent` files dropped on the window open the new-download dialog with them. */
function useTorrentDrop() {
  useEffect(() => {
    if (!isTauri()) return;
    let off: (() => void) | undefined;
    let gone = false;
    void getCurrentWebview()
      .onDragDropEvent((e) => {
        if (e.payload.type !== 'drop') return;
        const files = e.payload.paths.filter((p) => p.toLowerCase().endsWith('.torrent'));
        if (!files.length) return;
        const s = useApp.getState();
        s.setView('tasks');
        s.openDialog({ kind: 'new', text: files.join('\n') });
      })
      .then((f) => {
        if (gone) f();
        else off = f;
      });
    return () => {
      gone = true;
      off?.();
    };
  }, []);
}

export function App({ platform }: { platform: Platform }) {
  const { t } = useTranslation();
  const view = useApp((s) => s.view);
  const appearance = useApp((s) => s.settings.appearance);
  const shown = useApp(detailTask);
  const toasts = useApp((s) => s.toasts);
  const dismiss = useApp((s) => s.dismiss);
  useShortcuts();
  usePasteLinks();
  useTorrentDrop();

  // Files deleted or moved while the window was in the background.
  useEffect(() => {
    const check = () => void useApp.getState().checkFiles();
    window.addEventListener('focus', check);
    return () => window.removeEventListener('focus', check);
  }, []);

  // Following the system: switch with it.
  useEffect(() => {
    if (appearance.mode !== 'system') return;
    return onSystemThemeChange(() => applyTheme(appearance, platform, true));
  }, [appearance, platform]);

  const tasks = view === 'tasks';
  return (
    <div className="window">
      <Sidebar platform={platform} />
      <div className="maincol">
        <TitleBar platform={platform} />
        {/* The details panel runs from the title bar down; the command bar is over the list only. */}
        <div className="workarea">
          <div className="listcol">
            {tasks && <CommandBar />}
            <main className="content">{tasks ? <TaskTable /> : <Settings />}</main>
          </div>
          {tasks && shown && <TaskDetail key={shown} id={shown} />}
        </div>
        <StatusBar />
      </div>
      <Dialogs />
      <Toasts toasts={toasts} onDismiss={dismiss} closeLabel={t('shell.window.close')} />
      <ResizeEdges platform={platform} />
    </div>
  );
}
