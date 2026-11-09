import { describe, expect, it } from 'vitest';

import { detectPlatform } from './platform';

describe('detectPlatform', () => {
  it('recognises the three web views', () => {
    expect(detectPlatform('Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36 Edg/142.0.0.0')).toBe('win');
    expect(detectPlatform('Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)')).toBe('mac');
    expect(detectPlatform('Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15')).toBe('linux');
  });

  it('falls back to Linux for anything else', () => {
    expect(detectPlatform('')).toBe('linux');
  });
});
