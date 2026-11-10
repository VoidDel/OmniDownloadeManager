import { describe, expect, it } from 'vitest';

import { BATCH_MAX, expandBatch, hasBatch, hostOf, kindOf, nameFromUrl, parseLinks } from './links';

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

describe('batch links', () => {
  it('expands numbers, with or without a fixed width, and steps', () => {
    expect(expandBatch('https://e.com/img[1-3].jpg')).toEqual(['https://e.com/img1.jpg', 'https://e.com/img2.jpg', 'https://e.com/img3.jpg']);
    expect(expandBatch('https://e.com/p[08-10].rar')).toEqual(['https://e.com/p08.rar', 'https://e.com/p09.rar', 'https://e.com/p10.rar']);
    expect(expandBatch('https://e.com/[0-10:5]')).toEqual(['https://e.com/0', 'https://e.com/5', 'https://e.com/10']);
    expect(expandBatch('https://e.com/[3-1]')).toEqual(['https://e.com/3', 'https://e.com/2', 'https://e.com/1']);
  });

  it('expands letters and combines ranges', () => {
    expect(expandBatch('https://e.com/[a-c].zip')).toHaveLength(3);
    expect(expandBatch('https://e.com/[a-b]/[1-2]')).toEqual(['https://e.com/a/1', 'https://e.com/a/2', 'https://e.com/b/1', 'https://e.com/b/2']);
    // Mixed case is not a range; neither is an IPv6 host.
    expect(expandBatch('https://e.com/[a-Z]')).toEqual(['https://e.com/[a-Z]']);
    expect(expandBatch('http://[::1]:8080/a')).toEqual(['http://[::1]:8080/a']);
  });

  it('caps the series and finds them in pasted text', () => {
    expect(expandBatch('https://e.com/[1-5000]')).toHaveLength(BATCH_MAX);
    expect(parseLinks('下载：https://e.com/v[1-2].mp4\nhttps://e.com/v1.mp4').map((l) => l.url)).toEqual(['https://e.com/v1.mp4', 'https://e.com/v2.mp4']);
    expect(hasBatch('https://e.com/v[1-2].mp4')).toBe(true);
    expect(hasBatch('https://e.com/v1.mp4')).toBe(false);
  });
});
