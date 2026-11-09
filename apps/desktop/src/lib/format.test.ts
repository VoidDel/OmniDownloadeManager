import { describe, expect, it } from 'vitest';

import { bytes, duration, parseSpeed, percent, speed, when } from './format';

describe('format', () => {
  it('sizes with three significant digits', () => {
    expect(bytes(0)).toBe('0 B');
    expect(bytes(512)).toBe('512 B');
    expect(bytes(1536)).toBe('1.5 KB');
    expect(bytes(643 * 1024 ** 2)).toBe('643 MB');
    expect(bytes(1.24 * 1024 ** 3)).toBe('1.24 GB');
    expect(bytes(5.9 * 1024 ** 3)).toBe('5.9 GB');
    expect(bytes(1023.9 * 1024)).toBe('1 MB');
    expect(bytes(null)).toBe('—');
    expect(speed(48.2 * 1024 ** 2)).toBe('48.2 MB/s');
  });

  it('durations', () => {
    expect(duration(47)).toBe('47 秒');
    expect(duration(65)).toBe('1 分 5 秒');
    expect(duration(120)).toBe('2 分');
    expect(duration(2 * 3600 + 180)).toBe('2 小时 3 分');
    expect(duration(4 * 86400 + 7200)).toBe('4 天 2 小时');
    expect(duration(null)).toBe('—');
  });

  it('percent never shows 100 early', () => {
    expect(percent(999, 1000)).toBe('99%');
    expect(percent(1000, 1000)).toBe('100%');
    expect(percent(5, null)).toBe('—');
  });

  it('times relative to today', () => {
    const now = new Date(2026, 10, 9, 15, 0).getTime();
    expect(when(new Date(2026, 10, 9, 10, 42).getTime(), now)).toBe('10:42');
    expect(when(new Date(2026, 10, 8, 9, 5).getTime(), now)).toBe('昨天 09:05');
    expect(when(new Date(2026, 9, 3).getTime(), now)).toBe('10月3日');
    expect(when(new Date(2025, 9, 3).getTime(), now)).toBe('2025年10月3日');
  });

  it('reads typed speeds', () => {
    expect(parseSpeed('')).toBeNull();
    expect(parseSpeed('不限')).toBeNull();
    expect(parseSpeed('500')).toBe(500 * 1024);
    expect(parseSpeed('500K')).toBe(500 * 1024);
    expect(parseSpeed('2M')).toBe(2 * 1024 ** 2);
    expect(parseSpeed('1.5 MB/s')).toBe(1.5 * 1024 ** 2);
    expect(parseSpeed('1g')).toBe(1024 ** 3);
    expect(parseSpeed('fast')).toBeUndefined();
  });
});
