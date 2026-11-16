// The floating ball as drawn: the window (FloatApp) and the previews in the settings share it.
//
// Four looks (General settings). The classic one is a ring in the theme's colours. The others are
// a dark glass ball lit in the theme's accent, whatever the theme (the ball floats over the
// desktop, not in the app), and move all the time, gently while nothing downloads:
//
//   liquid   the ball fills as the downloads progress, its surface waving
//   comet    a comet runs round the progress ring
//   aurora   lights drift inside the ball
//
// Their loops go faster with the download speed (`tempo`). Everything that loops is a transform
// or an opacity of a layer of its own, which the compositor moves without painting the page
// again, and is drawn `FPS` times a second, not at the screen's rate.

import { useEffect, useRef, type CSSProperties, type HTMLAttributes } from 'react';

import { Icon } from '../../components/Icon';
import * as fmt from '../../lib/format';
import type { FloatState, FloatStyle } from '../../lib/types';

export const FLOAT_STYLES: FloatStyle[] = ['classic', 'liquid', 'comet', 'aurora'];

const R = 25; // ring radius in the 56 × 56 ball
const RING = 2 * Math.PI * R;

/** How often the lit looks' loops are drawn, a second. Left to themselves they are drawn at the
 *  screen's rate, and a frame costs the same however small the ball: on a 160 Hz screen that was
 *  a tenth of a processor core for a ball at rest (measured, 2026-11). Slow drifting needs far
 *  less; more while something downloads and they go faster. */
export const FPS = { idle: 30, busy: 60 };

/** How fast the loops run next to their own pace: slow while idle, from 0.6 at a trickle
 *  (100 KB/s and less) to 2 at 20 MB/s and more. */
export function tempo(state: FloatState): number {
  if (!state.busy) return 0.35;
  if (state.speed <= 0) return 0.6;
  const k = Math.log(state.speed / (100 * 1024)) / Math.log(200);
  return 0.6 + 1.4 * Math.min(1, Math.max(0, k));
}

/** How full the liquid ball is (0 to 1): a little at rest, half while the sizes are unknown,
 *  to the brim for a link held over it. */
export function level(state: FloatState, over: boolean): number {
  if (over) return 1;
  if (!state.busy) return 0.16;
  if (state.percent == null) return 0.5;
  return 0.08 + (0.92 * Math.min(100, Math.max(0, state.percent))) / 100;
}

interface Props extends HTMLAttributes<HTMLDivElement> {
  look: FloatStyle;
  state: FloatState;
  /** A link is held over it. */
  over?: boolean;
}

export function Ball({ look, state, over = false, ...rest }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const rate = tempo(state);
  const pct = state.percent;
  const lit = look !== 'classic';
  // The lit looks' loops are held and moved from here, all on one clock: `FPS` times a second
  // (see there), at the pace of the moment, keeping their place when the pace changes (a new
  // duration would make them jump). A loop that starts later (a download begins, another look)
  // is taken along at the next tick. The classic look's one loop runs by itself, as it always did.
  const clock = useRef(0);
  const busy = state.busy;
  useEffect(() => {
    const el = ref.current;
    if (!lit || !el?.getAnimations) return;
    let last = performance.now();
    const tick = () => {
      const now = performance.now();
      clock.current += (now - last) * rate;
      last = now;
      for (const a of el.getAnimations({ subtree: true })) {
        if (!(a instanceof CSSAnimation)) continue;
        if (a.playState !== 'paused') a.pause();
        a.currentTime = clock.current;
      }
    };
    tick();
    const id = setInterval(tick, 1000 / (busy ? FPS.busy : FPS.idle));
    return () => clearInterval(id);
  }, [lit, look, rate, busy]);

  const [num, unit] = fmt.speed(state.speed).split(' ');
  return (
    <div
      {...rest}
      ref={ref}
      className={lit ? 'ball lit' : 'ball'}
      data-look={look}
      data-busy={state.busy}
      data-over={over}
      style={look === 'liquid' ? ({ '--level': level(state, over) } as CSSProperties) : undefined}
    >
      {lit && (
        <span className="body" aria-hidden="true">
          {look === 'liquid' && (
            <span className="lq">
              <i className="wave back" />
              <i className="wave front" />
              <i className="bub" />
              <i className="bub" />
              <i className="bub" />
            </span>
          )}
          {look === 'aurora' && (
            <span className="au">
              <i className="blob a" />
              <i className="blob b" />
              <i className="blob c" />
            </span>
          )}
        </span>
      )}
      {look !== 'liquid' && (
        <svg className={`ring${state.busy && pct == null ? ' spin' : ''}`} viewBox="0 0 56 56" aria-hidden="true">
          <circle className="track" cx="28" cy="28" r={R} />
          {state.busy && (
            <circle className="arc" cx="28" cy="28" r={R} strokeDasharray={RING} strokeDashoffset={pct == null ? RING * 0.75 : RING * (1 - pct / 100)} />
          )}
        </svg>
      )}
      {look === 'comet' && (
        <span className="comet" aria-hidden="true">
          <i className="tail" />
          <i className="head" />
        </span>
      )}
      {over || !state.busy ? (
        <Icon name="download" className="ico" />
      ) : (
        <span className="spd">
          <b>{num}</b>
          <small>{unit}</small>
        </span>
      )}
    </div>
  );
}
