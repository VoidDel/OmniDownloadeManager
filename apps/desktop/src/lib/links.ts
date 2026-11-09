// Links in pasted text: one per line, or found inside a line of other text ("下载：https://…").

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

/** Every link in `text`, in order, without duplicates. Trailing punctuation is not part of it. */
export function parseLinks(text: string): Link[] {
  const seen = new Set<string>();
  const out: Link[] = [];
  for (const line of text.split(/\r?\n/)) {
    const t = line.trim();
    if (!t) continue;
    const found = t.match(FIND) ?? (kindOf(t) === 'torrent' ? [t] : []);
    for (const raw of found) {
      const url = raw.replace(/[),.;:!?，。；：！？）」』】]+$/u, '');
      const kind = kindOf(url);
      if (!kind || seen.has(url)) continue;
      seen.add(url);
      out.push({ url, kind });
    }
  }
  return out;
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
