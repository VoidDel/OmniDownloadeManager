// The few controls the extension's pages use (their look is the extension's own, `pages.css`):
// line icons, a switch, text tabs, a progress bar.

import { useRef, type KeyboardEvent, type ReactNode } from 'react';

/** 24-unit line icons: paths, `circle:cx,cy,r` and `rect:x,y,w,h,rx`. */
const ICONS = {
  download: ['M12 4v11', 'm7 10.5 5 5 5-5', 'M5 20h14'],
  copy: ['rect:9,9,11,11,2', 'M5 15V6a2 2 0 0 1 2-2h8'],
  check: ['m5 12.5 4.5 4.5L19 7.5'],
  settings: ['M4 7h9', 'M17 7h3', 'M4 17h3', 'M11 17h9', 'circle:15,7,2', 'circle:9,17,2'],
  alert: ['circle:12,12,9', 'M12 7.5v5.5', 'M12 16.4v.1'],
  external: ['M14 4h6v6', 'M20 4l-9 9', 'M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5'],
  arrowDown: ['M12 5v14', 'm6 13 6 6 6-6'],
} as const;

export type IconName = keyof typeof ICONS;

export function Icon({ name, className }: { name: IconName; className?: string }) {
  return (
    <svg className={`ic${className ? ` ${className}` : ''}`} viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round">
      {ICONS[name].map((d) => {
        if (d.startsWith('circle:')) {
          const [cx, cy, r] = d.slice(7).split(',').map(Number);
          return <circle key={d} cx={cx} cy={cy} r={r} />;
        }
        if (d.startsWith('rect:')) {
          const [x, y, w, h, rx] = d.slice(5).split(',').map(Number);
          return <rect key={d} x={x} y={y} width={w} height={h} rx={rx} />;
        }
        return <path key={d} d={d} />;
      })}
    </svg>
  );
}

export function Switch({ checked, onChange, label }: { checked: boolean; onChange: (v: boolean) => void; label: string }) {
  return <button type="button" role="switch" className="switch" aria-checked={checked} aria-label={label} onClick={() => onChange(!checked)} />;
}

/** One value out of a few, as text tabs (a radio group: the arrow keys move the choice). */
export function Tabs<T extends string>({ value, options, onChange, label, className }: { value: T; options: [T, ReactNode][]; onChange: (v: T) => void; label: string; className?: string }) {
  const ref = useRef<HTMLDivElement>(null);
  const onKeyDown = (e: KeyboardEvent) => {
    const at = options.findIndex(([v]) => v === value);
    const to = ({ ArrowLeft: at - 1, ArrowUp: at - 1, ArrowRight: at + 1, ArrowDown: at + 1, Home: 0, End: options.length - 1 } as Record<string, number>)[e.key];
    if (to === undefined) return;
    e.preventDefault();
    const i = (to + options.length) % options.length;
    onChange(options[i][0]);
    ref.current?.querySelectorAll<HTMLElement>('[role="radio"]')[i]?.focus();
  };
  return (
    <div ref={ref} className={`tabs${className ? ` ${className}` : ''}`} role="radiogroup" aria-label={label} onKeyDown={onKeyDown}>
      {options.map(([v, text]) => (
        <button key={v} type="button" role="radio" aria-checked={v === value} tabIndex={v === value ? 0 : -1} onClick={() => onChange(v)}>
          {text}
        </button>
      ))}
    </div>
  );
}

/** Tabs that switch what a page shows (a tab list: the arrow keys move between them). Each
 *  option: its value, its label, and an id of the panel it shows. */
export function ViewTabs<T extends string>({ value, options, onChange, label }: { value: T; options: [T, ReactNode, string][]; onChange: (v: T) => void; label: string }) {
  const ref = useRef<HTMLDivElement>(null);
  const onKeyDown = (e: KeyboardEvent) => {
    const at = options.findIndex(([v]) => v === value);
    const to = ({ ArrowLeft: at - 1, ArrowRight: at + 1, Home: 0, End: options.length - 1 } as Record<string, number>)[e.key];
    if (to === undefined) return;
    e.preventDefault();
    const i = (to + options.length) % options.length;
    onChange(options[i][0]);
    ref.current?.querySelectorAll<HTMLElement>('[role="tab"]')[i]?.focus();
  };
  return (
    <div ref={ref} className="views" role="tablist" aria-label={label} onKeyDown={onKeyDown}>
      {options.map(([v, text, panel]) => (
        <button key={v} type="button" role="tab" id={`tab-${panel}`} aria-selected={v === value} aria-controls={panel} tabIndex={v === value ? 0 : -1} onClick={() => onChange(v)}>
          {text}
        </button>
      ))}
    </div>
  );
}

/** A thin bar; its colour follows the row's state class. */
export function Bar({ value, busy }: { value: number; busy?: boolean }) {
  return (
    <div className={`bar${busy ? ' busy' : ''}`} role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(value * 100)}>
      <i style={{ width: `${Math.max(0, Math.min(1, value)) * 100}%` }} />
    </div>
  );
}
