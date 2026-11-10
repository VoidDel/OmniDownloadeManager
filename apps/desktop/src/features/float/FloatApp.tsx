// The floating ball (docs/development-plan.md §6.3, "悬浮窗"): the overall progress of the running
// downloads as a ring, the speed inside it, in the theme's colours (it follows the settings as they
// change). No window material: the system cannot draw one in a round window (see float.rs). Links (or a `.torrent` file) dragged onto it are
// downloaded; it moves when dragged; a double click opens the main window, the right button a
// menu (native: the window is only as big as the ball).

import { isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { onSystemThemeChange } from 'omni-ui';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useEffect, useRef, useState, type DragEvent, type PointerEvent } from 'react';
import { useTranslation } from 'react-i18next';

import { Icon } from '../../components/Icon';
import { resolveLang, setLanguage } from '../../i18n';
import { api } from '../../lib/api';
import * as fmt from '../../lib/format';
import { nextFrame } from '../../lib/frame';
import { applyTheme } from '../../lib/theme';
import type { FloatState } from '../../lib/types';
import { useApp } from '../../store/app';

const R = 25; // ring radius in the 56 × 56 ball
const RING = 2 * Math.PI * R;

/** The links of a drop: `text/uri-list` (its comment lines left out), else the plain text. */
function droppedText(d: DataTransfer): string {
  const list = d
    .getData('text/uri-list')
    .split(/\r?\n/)
    .filter((l) => l.trim() && !l.startsWith('#'))
    .join('\n');
  return list || d.getData('text/plain');
}

export function FloatApp() {
  const { t } = useTranslation();
  const [state, setState] = useState<FloatState>({ busy: false, percent: null, speed: 0 });
  const [over, setOver] = useState(false);
  const press = useRef<{ x: number; y: number } | null>(null);
  // Settings changed in the main window (or the tray) arrive as `settings:changed`.
  const appearance = useApp((s) => s.settings.appearance);
  const language = useApp((s) => s.settings.language);
  const platform = useApp((s) => s.platform);
  const first = useRef(true);
  useEffect(() => {
    if (first.current) {
      first.current = false; // already applied by the start-up
      return;
    }
    applyTheme(appearance, platform, true);
  }, [appearance, platform]);
  useEffect(() => {
    if (appearance.mode !== 'system') return;
    return onSystemThemeChange(() => applyTheme(appearance, platform, true));
  }, [appearance, platform]);
  useEffect(() => {
    void setLanguage(resolveLang(language));
  }, [language]);

  useEffect(() => {
    document.documentElement.classList.add('float-root');
    if (!isTauri()) return;
    void api.floatState().then(setState);
    const un = listen<FloatState>('float:state', (e) => setState(e.payload));
    // Shown once drawn, without taking the focus from the app in use.
    nextFrame(() => nextFrame(() => void getCurrentWindow().show()));
    return () => {
      void un.then((f) => f());
    };
  }, []);

  // Moving: the window follows once the pointer has gone a few pixels with the button down (a
  // click or double click stays one).
  const onPointerDown = (e: PointerEvent) => {
    if (e.button === 0) press.current = { x: e.screenX, y: e.screenY };
  };
  const onPointerMove = (e: PointerEvent) => {
    const p = press.current;
    if (!p || !(e.buttons & 1)) return;
    if (Math.hypot(e.screenX - p.x, e.screenY - p.y) > 3) {
      press.current = null;
      if (isTauri()) void getCurrentWindow().startDragging();
    }
  };

  const onDragOver = (e: DragEvent) => {
    e.preventDefault();
    e.dataTransfer.dropEffect = 'copy';
    setOver(true);
  };
  const onDrop = (e: DragEvent) => {
    e.preventDefault();
    setOver(false);
    const torrents = [...e.dataTransfer.files].filter((f) => f.name.toLowerCase().endsWith('.torrent'));
    if (torrents.length) {
      for (const f of torrents) void f.arrayBuffer().then((b) => api.floatDropTorrent(f.name, Array.from(new Uint8Array(b))));
      return;
    }
    const text = droppedText(e.dataTransfer);
    if (text.trim()) void api.floatDrop(text);
  };

  const [num, unit] = fmt.speed(state.speed).split(' ');
  const pct = state.percent;
  const title = [
    'Omni Download',
    state.busy ? `${fmt.speed(state.speed)}${pct != null ? ` · ${pct}%` : ''}` : t('float.idle'),
    t('float.hint'),
  ].join('\n');
  return (
    <div className="float" onDragOver={onDragOver} onDragEnter={onDragOver} onDragLeave={() => setOver(false)} onDrop={onDrop}>
      <div
        className="ball"
        data-busy={state.busy}
        data-over={over}
        title={title}
        role="img"
        aria-label={title}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={() => (press.current = null)}
        onDoubleClick={() => void api.floatOpen()}
        onContextMenu={(e) => {
          e.preventDefault();
          void api.floatMenu();
        }}
      >
        <svg className="ring" viewBox="0 0 56 56" aria-hidden="true">
          <circle className="track" cx="28" cy="28" r={R} />
          {state.busy && (
            <circle
              className={`arc${pct == null ? ' spin' : ''}`}
              cx="28"
              cy="28"
              r={R}
              strokeDasharray={RING}
              strokeDashoffset={pct == null ? RING * 0.75 : RING * (1 - pct / 100)}
            />
          )}
        </svg>
        {over || !state.busy ? (
          <Icon name="download" className="ico" />
        ) : (
          <span className="spd">
            <b>{num}</b>
            <small>{unit}</small>
          </span>
        )}
      </div>
    </div>
  );
}
