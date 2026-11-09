import { describe, expect, it } from 'vitest';

import { contentLength, dispositionName, isDownload } from './headers';
import { DEFAULT_RULES, cookieHeader, decide, extensionOf, parseList, siteMatches } from './rules';
import { parsePairing } from './settings';

const MB = 1024 * 1024;

describe('decide', () => {
  const page = 'https://www.example.com/downloads';
  it('takes ordinary downloads', () => {
    expect(decide({ url: 'https://dl.example.com/a.zip', size: 5 * MB, pageUrl: page }, DEFAULT_RULES)).toEqual({ take: true });
    expect(decide({ url: 'https://dl.example.com/a.zip', size: null }, DEFAULT_RULES).take).toBe(true);
  });

  it('leaves the rest to the browser', () => {
    const no = (r: ReturnType<typeof decide>) => (r.take ? '' : r.reason);
    expect(no(decide({ url: 'https://e.com/a.zip', size: 100 }, DEFAULT_RULES))).toBe('small');
    expect(no(decide({ url: 'blob:https://e.com/x', size: 5 * MB }, DEFAULT_RULES))).toBe('scheme');
    expect(no(decide({ url: 'data:application/zip;base64,AA' }, DEFAULT_RULES))).toBe('scheme');
    expect(no(decide({ url: 'https://e.com/a.zip' }, { ...DEFAULT_RULES, enabled: false }))).toBe('disabled');
    expect(no(decide({ url: 'https://e.com/a.pdf' }, { ...DEFAULT_RULES, skipExtensions: ['pdf'] }))).toBe('extension');
    expect(no(decide({ url: 'https://e.com/a.pdf' }, { ...DEFAULT_RULES, onlyExtensions: ['zip'] }))).toBe('extension');
    expect(no(decide({ url: 'https://cdn.net/a.zip', pageUrl: page }, { ...DEFAULT_RULES, skipSites: ['example.com'] }))).toBe('site');
  });

  it('reads extensions and sites', () => {
    expect(extensionOf({ url: 'https://e.com/get?id=1', filename: 'Report.PDF' })).toBe('pdf');
    expect(extensionOf({ url: 'https://e.com/files/a%20b.tar.gz?x=1' })).toBe('gz');
    expect(extensionOf({ url: 'https://e.com/files/' })).toBe('');
    expect(siteMatches('dl.example.com', 'example.com')).toBe(true);
    expect(siteMatches('example.com', 'https://example.com/')).toBe(true);
    expect(siteMatches('notexample.com', 'example.com')).toBe(false);
    expect(parseList(' .ZIP, 7z；iso ')).toEqual(['zip', '7z', 'iso']);
    expect(cookieHeader([{ name: 's', value: '1' }, { name: 't', value: 'x=y' }])).toBe('s=1; t=x=y');
  });
});

describe('headers', () => {
  const h = (pairs: Record<string, string>) => Object.entries(pairs).map(([name, value]) => ({ name, value }));
  it('recognises downloads', () => {
    expect(isDownload(h({ 'Content-Disposition': 'attachment; filename="a.zip"' }))).toBe(true);
    expect(isDownload(h({ 'content-type': 'application/octet-stream' }))).toBe(true);
    expect(isDownload(h({ 'Content-Type': 'text/html; charset=utf-8' }))).toBe(false);
    expect(isDownload(h({ 'Content-Type': 'application/pdf', 'Content-Disposition': 'inline' }))).toBe(false);
    expect(contentLength(h({ 'Content-Length': '1234' }))).toBe(1234);
    expect(contentLength(h({}))).toBeNull();
  });

  it('reads file names', () => {
    expect(dispositionName('attachment; filename="a b.zip"')).toBe('a b.zip');
    expect(dispositionName("attachment; filename=\"x.bin\"; filename*=UTF-8''%E6%B5%8B%E8%AF%95.bin")).toBe('测试.bin');
    expect(dispositionName("attachment; filename*=iso-8859-1''%A3%20rates.txt")).toBe('£ rates.txt');
    expect(dispositionName('attachment; filename="../../evil.exe"')).toBe('evil.exe');
    // A backslash in a quoted name escapes the next character (RFC 6266).
    expect(dispositionName('attachment; filename="a\\"b.zip"')).toBe('a"b.zip');
    expect(dispositionName('attachment')).toBeNull();
  });
});

describe('pairing', () => {
  it('reads the code from the app', () => {
    const token = 'a'.repeat(64);
    expect(parsePairing(` omnidl:21640:${token} `)).toEqual({ port: 21640, token });
    expect(parsePairing('omnidl:0:' + token)).toBeNull();
    expect(parsePairing('omnidl:21640:short')).toBeNull();
    expect(parsePairing('nonsense')).toBeNull();
  });
});
