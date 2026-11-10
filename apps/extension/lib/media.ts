// Media sniffing: the videos, audio and HLS streams a page loads (seen in its network
// requests), kept per tab for the popup to offer for download. Pieces of a stream (HLS
// segments, fMP4 fragments), tiny files and DASH manifests (the app cannot join DASH) are not
// listed.

import type { Header } from './headers';
import { contentLength, header } from './headers';

export type MediaKind = 'video' | 'audio' | 'hls';

export interface Media {
  url: string;
  kind: MediaKind;
  mime: string | null;
  size: number | null;
  /** The page's title when it was seen (names an HLS stream's file). */
  title: string | null;
  /** The page it plays on. */
  pageUrl: string | null;
  at: number;
}

/** At most this many per tab (the newest kept). */
export const MEDIA_MAX = 50;
/** Smaller files are page sounds and previews. */
const MIN_SIZE = 512 * 1024;

const VIDEO_EXT = new Set(['mp4', 'webm', 'mkv', 'mov', 'flv', 'm4v', 'avi', 'wmv', '3gp', 'ogv']);
const AUDIO_EXT = new Set(['mp3', 'm4a', 'aac', 'flac', 'ogg', 'oga', 'opus', 'wav', 'wma']);
/** Pieces of streams. */
const PIECE_EXT = new Set(['ts', 'm4s', 'mpd', 'f4f', 'key']);
const HLS_TYPES = ['application/vnd.apple.mpegurl', 'application/x-mpegurl', 'audio/mpegurl', 'audio/x-mpegurl'];

function extOf(url: string): string {
  try {
    const last = new URL(url).pathname.split('/').pop() ?? '';
    const dot = last.lastIndexOf('.');
    return dot > 0 ? last.slice(dot + 1).toLowerCase() : '';
  } catch {
    return '';
  }
}

/** What a response is, as media worth listing (`null`: not media, or a piece of a stream). */
export function classify(url: string, headers: Header[] | undefined): { kind: MediaKind; mime: string | null; size: number | null } | null {
  if (!/^https?:/i.test(url)) return null;
  const mime = header(headers, 'content-type')?.split(';')[0].trim().toLowerCase() ?? null;
  const size = contentLength(headers);
  const ext = extOf(url);
  if ((mime && HLS_TYPES.includes(mime)) || ext === 'm3u8') return { kind: 'hls', mime, size };
  if (PIECE_EXT.has(ext) || mime === 'video/mp2t' || mime === 'video/iso.segment' || mime === 'application/dash+xml') return null;
  let kind: MediaKind | null = null;
  if (mime?.startsWith('video/')) kind = 'video';
  else if (mime?.startsWith('audio/')) kind = 'audio';
  else if (VIDEO_EXT.has(ext)) kind = 'video';
  else if (AUDIO_EXT.has(ext)) kind = 'audio';
  if (!kind) return null;
  // A range of a file: Content-Range says the whole size.
  const whole = Number(header(headers, 'content-range')?.split('/')[1]);
  const total = Number.isFinite(whole) && whole > 0 ? whole : size;
  if (total != null && total < MIN_SIZE) return null;
  return { kind, mime, size: total };
}

/** A media item's address without what changes between requests of the same file (a range
 *  query some players add), to list it once. */
export function mediaKey(url: string): string {
  try {
    const u = new URL(url);
    for (const p of ['range', 'bytes', 'rn', 'rbuf', 'start', 'end']) u.searchParams.delete(p);
    u.hash = '';
    return u.toString();
  } catch {
    return url;
  }
}

/** `list` with `m` in it: an item seen before keeps its place and gains what it lacked. */
export function withMedia(list: Media[], m: Media): Media[] {
  const key = mediaKey(m.url);
  const i = list.findIndex((x) => mediaKey(x.url) === key);
  if (i >= 0) {
    const old = list[i];
    const merged = { ...old, size: old.size ?? m.size, mime: old.mime ?? m.mime, title: old.title ?? m.title };
    return list.map((x, j) => (j === i ? merged : x));
  }
  return [...list, m].slice(-MEDIA_MAX);
}

/** The name the file is offered under: the page title for a stream (its playlist is often
 *  `index.m3u8`), the URL's last segment otherwise. */
export function mediaName(m: Media): string {
  if (m.kind === 'hls' && m.title?.trim()) return m.title.trim();
  try {
    const last = decodeURIComponent(new URL(m.url).pathname.split('/').filter(Boolean).pop() ?? '');
    return last || new URL(m.url).host;
  } catch {
    return m.url;
  }
}

// ---------------------------------------------------------------------------------- per tab

const KEY = (tabId: number) => `media:${tabId}`;

/** What a tab has played so far (kept in session storage: the background may sleep). */
export async function mediaOf(tabId: number): Promise<Media[]> {
  const k = KEY(tabId);
  const got = await browser.storage.session.get(k);
  return (got[k] as Media[] | undefined) ?? [];
}

export async function saveMedia(tabId: number, list: Media[]): Promise<void> {
  if (list.length) await browser.storage.session.set({ [KEY(tabId)]: list });
  else await browser.storage.session.remove(KEY(tabId));
}
