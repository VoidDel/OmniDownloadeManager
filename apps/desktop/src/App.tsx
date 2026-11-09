// The main window (docs/development-plan.md §6.2): sidebar, title bar, command bar, the task list
// with its details panel (or the settings page), and the status bar, in omni-ui's window shell.

import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { onSystemThemeChange, type Platform } from 'omni-ui';
import { useEffect } from 'react';

import { Toasts } from './components/ui';
import { Dialogs } from './features/dialogs/Dialogs';
import { Settings } from './features/settings/Settings';
import { CommandBar, ResizeEdges, Sidebar, StatusBar, TitleBar } from './features/shell/Chrome';
import { TaskDetail } from './features/tasks/TaskDetail';
import { TaskTable } from './features/tasks/TaskTable';
import { applyTheme } from './lib/theme';
import { useApp } from './store/app';

/** Window-wide shortcuts: Ctrl/⌘+N new download, Ctrl/⌘+, settings, Ctrl/⌘+L the paste box. */
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
      } else if (key === 'l') {
        e.preventDefault();
        document.getElementById('pastebox')?.focus();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
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
  const view = useApp((s) => s.view);
  const appearance = useApp((s) => s.settings.appearance);
  const detailOpen = useApp((s) => s.detailOpen);
  const single = useApp((s) => (s.selection.length === 1 ? s.selection[0] : null));
  const toasts = useApp((s) => s.toasts);
  const dismiss = useApp((s) => s.dismiss);
  useShortcuts();
  useTorrentDrop();

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
        {tasks && <CommandBar />}
        <div className="workarea">
          <main className="content">{tasks ? <TaskTable /> : <Settings />}</main>
          {tasks && detailOpen && single && <TaskDetail key={single} id={single} />}
        </div>
        <StatusBar />
      </div>
      <Dialogs />
      <Toasts toasts={toasts} onDismiss={dismiss} closeLabel="关闭" />
      <ResizeEdges platform={platform} />
    </div>
  );
}
