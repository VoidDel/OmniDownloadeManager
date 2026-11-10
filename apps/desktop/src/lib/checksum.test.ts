import { describe, expect, it } from 'vitest';

import { parseChecksum } from './checksum';

const MD5 = '900150983cd24fb0d6963f7d28e17f72';
const SHA256 = 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad';

describe('parseChecksum', () => {
  it('reads values as the app does', () => {
    expect(parseChecksum(SHA256)).toEqual({ algo: 'sha256', hex: SHA256 });
    expect(parseChecksum(MD5.toUpperCase())).toEqual({ algo: 'md5', hex: MD5 });
    expect(parseChecksum(`sha256:${SHA256}`)?.algo).toBe('sha256');
    expect(parseChecksum(`SHA-256 ${SHA256}`)?.algo).toBe('sha256');
    expect(parseChecksum(`${SHA256}  file.iso`)?.hex).toBe(SHA256);
    expect(parseChecksum(`${SHA256} *file.iso`)?.hex).toBe(SHA256);
    expect(parseChecksum(`md5=${MD5}`)?.algo).toBe('md5');
  });

  it('rejects what is not one', () => {
    expect(parseChecksum(`sha1:${MD5}`)).toBeNull();
    expect(parseChecksum('not a hash')).toBeNull();
    expect(parseChecksum('abc123')).toBeNull();
    expect(parseChecksum('')).toBeNull();
  });
});
