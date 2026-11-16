import { describe, expect, it } from 'vitest';

import rust from '../../../src-tauri/src/settings.rs?raw';
import i18n from '../../i18n';
import type { FloatState } from '../../lib/types';
import { FLOAT_STYLES, level, tempo } from './Ball';
import { demoAt } from './FloatStylePicker';

const MB = 1024 * 1024;
const idle: FloatState = { busy: false, percent: null, speed: 0 };
const at = (speed: number, percent: number | null = 50): FloatState => ({ busy: true, percent, speed });

describe('the floating ball', () => {
  it('moves slowly at rest and faster with the speed, within bounds', () => {
    expect(tempo(idle)).toBe(0.35);
    expect(tempo(at(0))).toBe(0.6);
    expect(tempo(at(50 * 1024))).toBe(0.6);
    expect(tempo(at(100 * MB))).toBe(2);
    const speeds = [100 * 1024, 500 * 1024, MB, 5 * MB, 20 * MB].map((s) => tempo(at(s)));
    expect(speeds).toEqual([...speeds].sort((a, b) => a - b));
    expect(speeds[0]).toBeCloseTo(0.6);
    expect(speeds[4]).toBeCloseTo(2);
  });

  it('fills as the downloads progress', () => {
    expect(level(idle, false)).toBe(0.16);
    // Sizes unknown: half.
    expect(level(at(MB, null), false)).toBe(0.5);
    expect(level(at(MB, 0), false)).toBeCloseTo(0.08);
    expect(level(at(MB, 100), false)).toBeCloseTo(1);
    expect(level(at(MB, 250), false)).toBeCloseTo(1);
    expect(level(at(MB, 40), false)).toBeLessThan(level(at(MB, 60), false));
    // A link held over it: to the brim, whatever it shows.
    expect(level(idle, true)).toBe(1);
  });

  it('shows a whole download, then rests, in the settings', () => {
    expect(demoAt(0)).toMatchObject({ busy: true, percent: 0 });
    expect(demoAt(4.5)).toMatchObject({ busy: true, percent: 50 });
    expect(demoAt(4.5).speed).toBeGreaterThan(demoAt(0.5).speed);
    expect(demoAt(10)).toEqual(idle);
    // And again.
    expect(demoAt(12 + 4.5)).toEqual(demoAt(4.5));
  });

  it('has the looks the settings file may name, each with its text', () => {
    // settings.rs: `FloatStyle`, written in snake case.
    const body = /pub enum FloatStyle \{([^}]*)\}/.exec(rust)?.[1] ?? '';
    const variants = [...body.matchAll(/^\s*([A-Z]\w*),/gm)].map((m) => m[1].toLowerCase());
    expect(variants).toEqual(FLOAT_STYLES);
    // What reads them back: every name but these is the classic look.
    const read = [...rust.matchAll(/^\s*"(\w+)" => Self::\w+,/gm)].map((m) => m[1]);
    expect(['classic', ...read]).toEqual(FLOAT_STYLES);
    for (const look of FLOAT_STYLES) expect(i18n.exists(`float.style.${look}`)).toBe(true);
  });
});
