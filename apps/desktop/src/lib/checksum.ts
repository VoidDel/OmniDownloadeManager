// Checksums as people paste them, read as `omnidl_core::Checksum::parse` reads them (the app
// checks again): bare hex (the algorithm from its length), `sha256:…`, `SHA-256 …`, `md5=…`, or a
// line of a `*sum` file (`<hex>  <name>`).

import type { Checksum, HashAlgo } from './types';

const LENGTH: Record<HashAlgo, number> = { md5: 32, sha1: 40, sha256: 64, sha512: 128 };
export const ALGO_NAME: Record<HashAlgo, string> = { md5: 'MD5', sha1: 'SHA-1', sha256: 'SHA-256', sha512: 'SHA-512' };

function algoOf(name: string): HashAlgo | null {
  const n = name.replace(/[^a-z0-9]/gi, '').toLowerCase();
  return n in LENGTH ? (n as HashAlgo) : null;
}

function of(algo: HashAlgo | null, hex: string): Checksum | null {
  if (!hex || !/^[0-9a-f]+$/i.test(hex)) return null;
  const a = algo ?? (Object.keys(LENGTH) as HashAlgo[]).find((k) => LENGTH[k] === hex.length) ?? null;
  return a && LENGTH[a] === hex.length ? { algo: a, hex: hex.toLowerCase() } : null;
}

export function parseChecksum(text: string): Checksum | null {
  const s = text.trim();
  if (!s) return null;
  const i = s.search(/[:= \t]/);
  if (i > 0) {
    const algo = algoOf(s.slice(0, i));
    if (algo) return of(algo, s.slice(i + 1).trim().split(/\s+/)[0] ?? '');
  }
  return of(null, (s.split(/\s+/)[0] ?? '').replace(/^\*/, ''));
}
