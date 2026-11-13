import { describe, expect, it } from 'vitest';

import { dropTarget, edgeTarget, moved, stepTarget } from './reorder';

const list = ['a', 'b', 'c', 'd', 'e'];
const after = (moving: string[], index: number) => {
  const place = dropTarget(list, moving, index);
  return place ? moved(list, moving, place).join('') : null;
};

describe('dropTarget', () => {
  it('puts one row in the gap it was dropped in', () => {
    expect(after(['d'], 0)).toBe('dabce');
    expect(after(['d'], 1)).toBe('adbce');
    expect(after(['a'], 3)).toBe('bcade');
    expect(after(['a'], 5)).toBe('bcdea');
    expect(after(['a'], 99)).toBe('bcdea');
  });

  it('changes nothing when dropped where it is', () => {
    expect(dropTarget(list, ['c'], 2)).toBeNull();
    expect(dropTarget(list, ['c'], 3)).toBeNull();
    expect(dropTarget(list, ['a'], 0)).toBeNull();
    expect(dropTarget(list, ['e'], 5)).toBeNull();
    expect(dropTarget(list, ['b', 'c'], 1)).toBeNull();
    expect(dropTarget(list, ['b', 'c'], 2)).toBeNull();
    expect(dropTarget(list, ['b', 'c'], 3)).toBeNull();
  });

  it('moves several rows together, in the order they have', () => {
    expect(after(['d', 'b'], 0)).toBe('bdace');
    expect(after(['a', 'e'], 2)).toBe('baecd');
    // Rows that are apart come together even when dropped between themselves.
    expect(after(['b', 'd'], 2)).toBe('abdce');
    expect(after(['b', 'd'], 3)).toBe('acbde');
  });

  it('has nowhere to go without other rows, or without rows to move', () => {
    expect(dropTarget(['a', 'b'], ['a', 'b'], 0)).toBeNull();
    expect(dropTarget(list, [], 2)).toBeNull();
    expect(dropTarget(list, ['x'], 2)).toBeNull();
  });
});

describe('stepTarget and edgeTarget', () => {
  const step = (moving: string[], down: boolean) => {
    const place = stepTarget(list, moving, down);
    return place ? moved(list, moving, place).join('') : null;
  };

  it('steps over the neighbour', () => {
    expect(step(['c'], false)).toBe('acbde');
    expect(step(['c'], true)).toBe('abdce');
    expect(step(['b', 'c'], true)).toBe('adbce');
    expect(step(['b', 'c'], false)).toBe('bcade');
  });

  it('stops at the ends', () => {
    expect(step(['a'], false)).toBeNull();
    expect(step(['e'], true)).toBeNull();
    expect(step(['a', 'b'], false)).toBeNull();
  });

  it('brings rows that are apart together first', () => {
    expect(step(['a', 'c'], false)).toBe('acbde');
    expect(step(['a', 'c'], true)).toBe('bacde');
  });

  it('goes to the top and the bottom', () => {
    const edge = (moving: string[], bottom: boolean) => {
      const place = edgeTarget(list, moving, bottom);
      return place ? moved(list, moving, place).join('') : null;
    };
    expect(edge(['c', 'd'], false)).toBe('cdabe');
    expect(edge(['b'], true)).toBe('acdeb');
    expect(edge(['a'], false)).toBeNull();
    expect(edge(['e'], true)).toBeNull();
  });
});
