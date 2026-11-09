// The main window (docs/development-plan.md §6.2): sidebar, title bar, command bar, task list and
// status bar in omni-ui's window shell. Phase 0 has no engine yet, so the list is always empty.

import { onSystemThemeChange, type Platform } from 'omni-ui';
import { useEffect, useState } from 'react';

import { CommandBar, FILTERS, ResizeEdges, Sidebar, StatusBar, TitleBar, type Filter } from './features/shell/Chrome';
import { EmptyTasks } from './features/tasks/EmptyTasks';
import { applyTheme, loadThemeMode, saveThemeMode, type ThemeMode } from './lib/theme';

export function App({ platform }: { platform: Platform }) {
  const [filter, setFilter] = useState<Filter>('all');
  const [mode, setMode] = useState<ThemeMode>(loadThemeMode);

  // Following the system: switch with it.
  useEffect(() => {
    if (mode !== 'system') return;
    return onSystemThemeChange(() => applyTheme('system', platform, true));
  }, [mode, platform]);

  const changeMode = (m: ThemeMode) => {
    setMode(m);
    saveThemeMode(m);
    applyTheme(m, platform, true);
  };

  const title = FILTERS.find((f) => f.id === filter)?.label ?? '';
  return (
    <div className="window">
      <Sidebar platform={platform} filter={filter} onFilter={setFilter} />
      <div className="maincol">
        <TitleBar platform={platform} title={title} mode={mode} onMode={changeMode} />
        <CommandBar />
        <main className="content">
          <EmptyTasks />
        </main>
        <StatusBar />
      </div>
      <ResizeEdges platform={platform} />
    </div>
  );
}
