import { describe, expect, it } from 'vitest';

import { hostOf, kindOf, nameFromUrl, parseLinks } from './links';

describe('links', () => {
  it('recognises kinds', () => {
    expect(kindOf('HTTPS://a/b')).toBe('http');
    expect(kindOf('ftp://a/b')).toBe('ftp');
    expect(kindOf('magnet:?xt=urn:btih:abc')).toBe('magnet');
    expect(kindOf('C:\\x\\a.torrent')).toBe('torrent');
    expect(kindOf('hello')).toBeNull();
  });

  it('finds links in pasted text', () => {
    const text = [
      'https://releases.ubuntu.com/24.04/ubuntu.iso',
      '下载地址：https://example.com/a.zip。',
      '  ',
      '(see https://example.com/b.pdf), and magnet:?xt=urn:btih:dd82&dn=Big+Buck',
      'https://releases.ubuntu.com/24.04/ubuntu.iso',
      'not a link',
    ].join('\n');
    expect(parseLinks(text)).toEqual([
      { url: 'https://releases.ubuntu.com/24.04/ubuntu.iso', kind: 'http' },
      { url: 'https://example.com/a.zip', kind: 'http' },
      { url: 'https://example.com/b.pdf', kind: 'http' },
      { url: 'magnet:?xt=urn:btih:dd82&dn=Big+Buck', kind: 'magnet' },
    ]);
  });

  it('reads hosts', () => {
    expect(hostOf('https://cdn.example.com:8443/a')).toBe('cdn.example.com:8443');
    expect(hostOf('nope')).toBe('');
  });
});

describe('nameFromUrl', () => {
  it('decodes the last segment', () => {
    expect(nameFromUrl('https://e.com/files/a%20b.zip?x=1')).toBe('a b.zip');
    expect(nameFromUrl('https://e.com/')).toBeNull();
    expect(nameFromUrl('nope')).toBeNull();
  });
});
