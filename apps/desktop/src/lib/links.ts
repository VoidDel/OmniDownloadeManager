// Links in pasted text: one per line, or found inside a line of other text ("下载：https://…").
// A link may stand for a numbered series (batch download): `img[1-20].jpg`, `part[01-12].rar`,
// `[a-f]`, `[0-100:5]` (a step); several ranges in one link combine.

export type LinkKind = 'http' | 'ftp' | 'magnet' | 'torrent';

export interface Link {
  url: string;
  kind: LinkKind;
}

const FIND = /(?:https?|ftps?):\/\/[^\s"'<>]+|magnet:\?[^\s"'<>]+/gi;

export function kindOf(url: string): LinkKind | null {
  const u = url.trim().toLowerCase();
  if (u.startsWith('http://') || u.startsWith('https://')) return 'http';
  if (u.startsWith('ftp://') || u.startsWith('ftps://')) return 'ftp';
  if (u.startsWith('magnet:')) return 'magnet';
  if (u.endsWith('.torrent')) return 'torrent';
  return null;
}

/** At most this many links come out of the ranges of one text. */
export const BATCH_MAX = 1000;

/** `[1-20]`, `[020-001]`, `[0-100:5]`; `[a-z]`, `[Z-A]`. */
const RANGE = /\[(?:(\d+)-(\d+)(?::(\d+))?|([a-zA-Z])-([a-zA-Z]))\]/;

/** The values of one range: numbers keep the width of a start written with a leading zero
 *  (`[01-12]` → 01 … 12); a range may count down. */
function rangeValues(m: RegExpMatchArray): string[] {
  const out: string[] = [];
  if (m[1] !== undefined) {
    const [a, b] = [Number(m[1]), Number(m[2])];
    const step = Math.max(1, Number(m[3] ?? 1));
    const width = m[1].length > 1 && m[1].startsWith('0') ? m[1].length : 0;
    const dir = a <= b ? 1 : -1;
    for (let n = a; dir > 0 ? n <= b : n >= b; n += dir * step) {
      out.push(String(n).padStart(width, '0'));
      if (out.length > BATCH_MAX) break;
    }
  } else {
    const [a, b] = [m[4].charCodeAt(0), m[5].charCodeAt(0)];
    // Both letters of one case: [a-Z] is not a range.
    if ((m[4] === m[4].toLowerCase()) !== (m[5] === m[5].toLowerCase())) return [];
    const dir = a <= b ? 1 : -1;
    for (let c = a; dir > 0 ? c <= b : c >= b; c += dir) out.push(String.fromCharCode(c));
  }
  return out;
}

/** The links a link with ranges stands for (itself when it has none), at most `limit`. */
export function expandBatch(url: string, limit = BATCH_MAX): string[] {
  const m = url.match(RANGE);
  if (!m || m.index === undefined) return [url];
  const values = rangeValues(m);
  if (!values.length) return [url];
  const head = url.slice(0, m.index);
  const tail = url.slice(m.index + m[0].length);
  const out: string[] = [];
  for (const v of values) {
    for (const rest of expandBatch(tail, limit - out.length)) {
      out.push(head + v + rest);
      if (out.length >= limit) return out;
    }
  }
  return out;
}

/** Every link in `text`, in order, without duplicates, ranges expanded (at most `BATCH_MAX` from
 *  ranges). Trailing punctuation is not part of a link. */
export function parseLinks(text: string): Link[] {
  const seen = new Set<string>();
  const out: Link[] = [];
  let expanded = 0;
  for (const line of text.split(/\r?\n/)) {
    const t = line.trim();
    if (!t) continue;
    const found = t.match(FIND) ?? (kindOf(t) === 'torrent' ? [t] : []);
    for (const raw of found) {
      const one = raw.replace(/[),.;:!?，。；：！？）」』】]+$/u, '');
      const kind = kindOf(one);
      if (!kind) continue;
      // Magnet links carry no series.
      const urls = kind === 'magnet' || !RANGE.test(one) ? [one] : expandBatch(one, Math.max(1, BATCH_MAX - expanded));
      if (urls.length > 1) expanded += urls.length;
      for (const url of urls) {
        if (seen.has(url)) continue;
        seen.add(url);
        out.push({ url, kind });
      }
    }
  }
  return out;
}

/** Whether `text` has a link with a range in it (the dialog says how many links it made). */
export function hasBatch(text: string): boolean {
  return (text.match(FIND) ?? []).some((u) => !u.toLowerCase().startsWith('magnet:') && RANGE.test(u));
}

/** Whether pasted text holds at least one link (the title-bar box opens the dialog then). */
export function hasLink(text: string): boolean {
  return parseLinks(text).length > 0;
}

/** The host of a URL, for the details panel. */
export function hostOf(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return '';
  }
}

/** The last path segment of a URL, decoded (`https://e.com/a%20b.zip?x` → `a b.zip`). */
export function nameFromUrl(url: string): string | null {
  try {
    const last = new URL(url).pathname.split('/').filter(Boolean).pop();
    return last ? decodeURIComponent(last) : null;
  } catch {
    return null;
  }
}
