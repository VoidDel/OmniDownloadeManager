import { describe, expect, it } from 'vitest';

import { sentWith } from './headers';
import { MAX_POST, postBody } from './post';

const URLENCODED = 'application/x-www-form-urlencoded';
const text = (b64: string) => atob(b64);
const bytes = (s: string) => new TextEncoder().encode(s).buffer as ArrayBuffer;

describe('sentWith', () => {
  it('keeps the cookies and origin the form went out with', () => {
    const h = (pairs: [string, string][]) => pairs.map(([name, value]) => ({ name, value }));
    expect(sentWith(h([['Cookie', 'captk=1; cf_clearance=x'], ['Origin', 'https://e.com'], ['Referer', 'https://e.com/p'], ['Accept', '*/*']]))).toEqual({
      cookies: 'captk=1; cf_clearance=x',
      referer: 'https://e.com/p',
      headers: { Origin: 'https://e.com' },
    });
    // Lower case, an opaque origin, none at all.
    expect(sentWith(h([['cookie', 'a=1'], ['origin', 'null']]))).toEqual({ cookies: 'a=1', referer: null, headers: {} });
    expect(sentWith(undefined)).toEqual({ cookies: null, referer: null, headers: {} });
  });
});

describe('postBody', () => {
  it('writes a form again as the browser sent it', () => {
    const p = postBody({ formData: { innum: ['26554'], tmpinfo1aa: ['01__W55'] } }, URLENCODED);
    expect(p).toEqual({ content_type: URLENCODED, body: btoa('innum=26554&tmpinfo1aa=01__W55') });
    // Spaces, reserved and non-ASCII characters, a name given twice.
    const q = postBody({ formData: { q: ['a b&c=d', '数据'], 'x y': [''] } }, `${URLENCODED}; charset=UTF-8`);
    expect(q?.content_type).toBe(`${URLENCODED}; charset=UTF-8`);
    expect(text(q!.body)).toBe('q=a+b%26c%3Dd&q=%E6%95%B0%E6%8D%AE&x+y=');
  });

  it('sends bytes as they are', () => {
    const p = postBody({ raw: [{ bytes: bytes('{"id":') }, { bytes: bytes('7}') }] }, 'application/json');
    expect(p).toEqual({ content_type: 'application/json', body: btoa('{"id":7}') });
    const binary = postBody({ raw: [{ bytes: new Uint8Array([0, 255, 128]).buffer }] }, 'application/octet-stream');
    expect(text(binary!.body)).toBe('\u0000\u00ff\u0080');
  });

  it('sends a POST without a body', () => {
    expect(postBody(undefined, URLENCODED)).toEqual({ content_type: URLENCODED, body: '' });
  });

  it('leaves what it cannot send again', () => {
    // No type, a body the browser could not read.
    expect(postBody({ formData: { a: ['1'] } }, undefined)).toBeNull();
    expect(postBody({ error: 'Unknown error.' }, URLENCODED)).toBeNull();
    // Files: a multipart form (parsed, files by name only), an upload.
    expect(postBody({ formData: { a: ['1'], f: ['report.pdf'] } }, 'multipart/form-data; boundary=x')).toBeNull();
    expect(postBody({ raw: [{ file: 'C:\\x.bin' }] }, 'application/octet-stream')).toBeNull();
    // Text that was not UTF-8.
    expect(postBody({ formData: { a: ['\uFFFD\uFFFD'] } }, URLENCODED)).toBeNull();
    // More than a form's fields.
    expect(postBody({ raw: [{ bytes: new ArrayBuffer(MAX_POST + 1) }] }, 'application/octet-stream')).toBeNull();
    expect(postBody({ formData: { a: ['x'.repeat(MAX_POST)] } }, URLENCODED)).toBeNull();
    // A body of another shape.
    expect(postBody({}, URLENCODED)).toBeNull();
  });
});
