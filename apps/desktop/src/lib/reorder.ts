// Moving tasks in the list (drag and drop, Alt+arrows, "move to top"): where they land.

export interface Place {
  /** The task they go next to. */
  anchor: string;
  /** After it, or before it. */
  after: boolean;
}

/**
 * Where dragged tasks land. `visible` is the list as shown, `moving` the dragged ids and `index`
 * the gap they were dropped in (0: above the first row; `visible.length`: below the last). Null
 * when dropping there changes nothing.
 */
export function dropTarget(visible: string[], moving: string[], index: number): Place | null {
  const dragged = new Set(moving);
  const shown = visible.filter((id) => dragged.has(id));
  // The rows that stay, and the gap among them the drop falls in.
  const rest = visible.filter((id) => !dragged.has(id));
  if (!rest.length || !shown.length) return null;
  const at = Math.max(0, Math.min(visible.length, index));
  const gap = visible.slice(0, at).filter((id) => !dragged.has(id)).length;
  // The dragged rows are together already, right at that gap.
  const first = visible.indexOf(shown[0]);
  const together = visible.slice(first, first + shown.length).every((id) => dragged.has(id));
  if (together && first === gap) return null;
  return gap < rest.length ? { anchor: rest[gap], after: false } : { anchor: rest[rest.length - 1], after: true };
}

/** One step up or down for the selected rows (Alt+arrows): past the row next to them. */
export function stepTarget(visible: string[], moving: string[], down: boolean): Place | null {
  const dragged = new Set(moving);
  const at = visible.map((id, i) => (dragged.has(id) ? i : -1)).filter((i) => i >= 0);
  if (!at.length) return null;
  // Together, they step over their neighbour; apart, they first come together at the leading one.
  const together = at[at.length - 1] - at[0] === at.length - 1;
  if (down) return dropTarget(visible, moving, together ? at[at.length - 1] + 2 : at[at.length - 1] + 1);
  return dropTarget(visible, moving, together ? at[0] - 1 : at[0]);
}

/** The top or the bottom of the list as shown. */
export function edgeTarget(visible: string[], moving: string[], bottom: boolean): Place | null {
  return dropTarget(visible, moving, bottom ? visible.length : 0);
}

/** `visible` as it will be once `moving` has gone to `place` (for tests and for showing the move
 *  before the app confirms it). */
export function moved(visible: string[], moving: string[], place: Place): string[] {
  const dragged = new Set(moving);
  const shown = visible.filter((id) => dragged.has(id));
  const rest = visible.filter((id) => !dragged.has(id));
  const i = rest.indexOf(place.anchor);
  if (i < 0) return visible;
  rest.splice(i + (place.after ? 1 : 0), 0, ...shown);
  return rest;
}
