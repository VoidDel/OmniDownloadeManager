// The floating ball's looks to choose from (General settings): each one drawn as the ball draws
// it, going through a download again and again.

import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';

import type { FloatState, FloatStyle } from '../../lib/types';
import { Ball, FLOAT_STYLES } from './Ball';

const MB = 1024 * 1024;
/** A download of nine seconds, then three of rest. */
const RUN = 9;
const REST = 3;

/** What the previews show `seconds` in: a download from start to end, its speed rising and
 *  falling once (0.4 to 6 MB/s), then nothing for a while. */
export function demoAt(seconds: number): FloatState {
  const t = seconds % (RUN + REST);
  if (t >= RUN) return { busy: false, percent: null, speed: 0 };
  const k = t / RUN;
  const speed = (0.4 + 5.6 * (0.5 - 0.5 * Math.cos(k * 2 * Math.PI))) * MB;
  return { busy: true, percent: Math.round(k * 100), speed: Math.round(speed / (0.1 * MB)) * 0.1 * MB };
}

function useDemo(): FloatState {
  const [ticks, setTicks] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setTicks((n) => n + 1), 250);
    return () => clearInterval(id);
  }, []);
  return demoAt(ticks / 4);
}

export function FloatStylePicker({ value, onChange, label }: { value: FloatStyle; onChange: (v: FloatStyle) => void; label: string }) {
  const { t } = useTranslation();
  const demo = useDemo();
  return (
    <div className="fstyles" role="radiogroup" aria-label={label}>
      {FLOAT_STYLES.map((look) => (
        <button key={look} type="button" className="fstyle" role="radio" aria-checked={look === value} onClick={() => onChange(look)}>
          <Ball look={look} state={demo} aria-hidden="true" />
          {t(`float.style.${look}`)}
        </button>
      ))}
    </div>
  );
}
