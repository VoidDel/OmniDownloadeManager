// Media sniffing: the videos, audio and streams a page loads (seen in its network requests),
// kept per tab for the popup to offer for download.
//
// Listed: whole video and audio files, HLS playlists, and the whole-file DASH streams sites such
// as Bilibili play (`.m4s` files fetched in ranges, the picture and the sound apart): the popup
// offers a picture together with the sound, which the app joins into one MP4. Not listed: pieces
// of streams (HLS segments, small DASH segments), tiny files, DASH manifests, and YouTube's
// player streams (expiring pieces, or its own protocol: not files to download).

import type { Header } from './headers';
import { contentLength, header } from './headers';

/** `dash`: a whole-file DASH stream, the picture or the sound (`track`). */
export type MediaKind = 'video' | 'audio' | 'hls' | 'dash';

export interface Media {
  url: string;
  kind: MediaKind;
  mime: string | null;
  size: number | null;
  /** A DASH stream: whether it holds the sound or the picture. */
  track?: 'video' | 'audio';
  /** The page's title when it was seen (names streams and badly named files). */
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
const PIECE_EXT = new Set(['ts', 'mpd', 'f4f', 'key']);
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

function hostOf(url: string | null | undefined): string {
  try {
    return url ? new URL(url).hostname.toLowerCase() : '';
  } catch {
    return '';
  }
}

/** A DASH stream's track: by its type, Bilibili's stream ID (302xx are sound), or its path. */
function dashTrack(url: string, mime: string | null): 'video' | 'audio' {
  if (mime?.startsWith('audio/')) return 'audio';
  const id = /-(\d{5,6})\.m4s$/i.exec(new URL(url).pathname)?.[1];
  if (id) return Number(id) >= 30200 && Number(id) < 30300 ? 'audio' : 'video';
  return /audio|[_/-]a\d*[_/.-]/i.test(new URL(url).pathname) ? 'audio' : 'video';
}

/** What a response is, as media worth listing (`null`: not media, or a piece of a stream). */
export function classify(url: string, headers: Header[] | undefined): Pick<Media, 'kind' | 'mime' | 'size' | 'track'> | null {
  if (!/^https?:/i.test(url)) return null;
  if (/(^|\.)googlevideo\.com$/.test(hostOf(url))) return null;
  const mime = header(headers, 'content-type')?.split(';')[0].trim().toLowerCase() ?? null;
  const size = contentLength(headers);
  const ext = extOf(url);
  // A range of a file: Content-Range says the whole size.
  const range = header(headers, 'content-range');
  const whole = Number(range?.split('/')[1]);
  const total = Number.isFinite(whole) && whole > 0 ? whole : size;
  if ((mime && HLS_TYPES.includes(mime)) || ext === 'm3u8') return { kind: 'hls', mime, size };
  // A DASH file played in ranges is a whole stream; one fetched whole is a segment of one.
  if (ext === 'm4s') {
    if (!range || total == null || total < MIN_SIZE) return null;
    return { kind: 'dash', mime, size: total, track: dashTrack(url, mime) };
  }
  if (PIECE_EXT.has(ext) || mime === 'video/mp2t' || mime === 'video/iso.segment' || mime === 'application/dash+xml') return null;
  let kind: MediaKind | null = null;
  if (mime?.startsWith('video/')) kind = 'video';
  else if (mime?.startsWith('audio/')) kind = 'audio';
  else if (VIDEO_EXT.has(ext)) kind = 'video';
  else if (AUDIO_EXT.has(ext)) kind = 'audio';
  if (!kind) return null;
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
    const merged = { ...old, size: old.size ?? m.size, mime: old.mime ?? m.mime, title: old.title ?? m.title, at: m.at };
    return list.map((x, j) => (j === i ? merged : x));
  }
  return [...list, m].slice(-MEDIA_MAX);
}

// ---------------------------------------------------------------------------------- offers

/** What the popup offers: a media item, a DASH picture with its sound (`audio`). */
export interface Offer {
  media: Media;
  kind: Exclude<MediaKind, 'dash'> | 'dash';
  audio: Media | null;
  size: number | null;
  name: string;
}

/** The list as offered: each DASH picture with the latest DASH sound (joined by the app); a
 *  DASH sound on its own when there is no picture to go with. Newest first. */
export function offers(list: Media[]): Offer[] {
  const sounds = list.filter((m) => m.kind === 'dash' && m.track === 'audio');
  const sound = sounds.reduce<Media | null>((a, m) => (!a || m.at >= a.at ? m : a), null);
  const pictures = list.some((m) => m.kind === 'dash' && m.track !== 'audio');
  const out: Offer[] = [];
  for (const m of list) {
    if (m.kind === 'dash' && m.track === 'audio') {
      if (!pictures) out.push({ media: m, kind: 'audio', audio: null, size: m.size, name: mediaName(m, 'audio') });
    } else if (m.kind === 'dash') {
      const size = m.size != null && sound?.size != null ? m.size + sound.size : m.size;
      out.push({ media: m, kind: 'dash', audio: sound, size, name: mediaName(m, 'dash') });
    } else {
      out.push({ media: m, kind: m.kind, audio: null, size: m.size, name: mediaName(m) });
    }
  }
  return out.reverse();
}

// ---------------------------------------------------------------------------------- names

/** Names that say nothing about the video: players' and CDNs' words, and parts of streams. */
const GENERIC = new Set([
  'videoplayback', 'video', 'audio', 'index', 'playlist', 'master', 'media', 'stream', 'play', 'player', 'file',
  'download', 'manifest', 'chunklist', 'prog_index', 'main', 'source', 'output', 'default', 'hls', 'dash',
]);

/** Whether a file name's stem is no name: a player's word, a hash, a number, a stream ID. */
export function meaningless(stem: string): boolean {
  const s = stem.trim();
  return (
    s.length < 2 ||
    GENERIC.has(s.toLowerCase()) ||
    /^[\d_-]+$/.test(s) ||
    /^[0-9a-f]{16,}$/i.test(s) ||
    /^[A-Za-z0-9_-]{24,}$/.test(s) && !/[aeiou]{2}|\s/i.test(s) ||
    /^\d+-\d+-\d+$/.test(s)
  );
}

/** The sites' own names, as their pages end their titles (besides the address's words). */
const BRANDS: Record<string, string[]> = {
  bilibili: ['哔哩哔哩', 'bilibili'],
  youtube: ['YouTube'],
  youku: ['优酷', '优酷视频', 'Youku'],
  iqiyi: ['爱奇艺', 'iQIYI'],
  qq: ['腾讯视频'],
  douyin: ['抖音'],
  ixigua: ['西瓜视频'],
  weibo: ['微博'],
  acfun: ['AcFun'],
  mgtv: ['芒果TV'],
  vimeo: ['Vimeo'],
  twitter: ['X', 'Twitter'],
};
/** Address words that are not a site's name. */
const NOT_BRANDS = new Set(['www', 'com', 'net', 'org', 'cdn', 'cn', 'tv', 'm', 'video', 'v', 'play', 'player', 'static', 'media', 'img', 'app']);

const escape = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

/** A page's title without the site's name at its end (`…_哔哩哔哩_bilibili`, `… - YouTube`),
 *  the "watch online" tails of video sites, and a leading notification count (`(3) …`). */
export function cleanTitle(title: string | null | undefined, pageUrl?: string | null): string {
  let t = (title ?? '').trim().replace(/^\(\d+\)\s+/, '');
  const words = hostOf(pageUrl)
    .split('.')
    .filter((w) => w.length > 1 && !NOT_BRANDS.has(w));
  const brands = new Set(words.flatMap((w) => [w, ...(BRANDS[w] ?? [])]));
  const sep = '[\\s_\\-|–—·:：]';
  for (let changed = true; changed; ) {
    changed = false;
    for (const b of brands) {
      const re = new RegExp(`${sep}+${escape(b)}${sep}*$`, 'i');
      const cut = t.replace(re, '').trim();
      if (cut !== t && cut) {
        t = cut;
        changed = true;
      }
    }
    const tail = t.replace(/[_\s-]*(高清完整版|高清|完整版)?(在线观看|在线播放|免费观看).*$/, '').trim();
    if (tail !== t && tail) {
      t = tail;
      changed = true;
    }
  }
  // What a file name cannot hold: `a | b`, `a: b` read as `a - b`.
  return t
    .replace(/\s*[|]\s*|\s*:\s+/g, ' - ')
    .replace(/[\\/:*?"<>|]+/g, ' ')
    .replace(/\s+/g, ' ')
    .trim();
}

/** File extensions by type, for files whose address has none. */
const EXT_BY_TYPE: Record<string, string> = {
  'video/mp4': 'mp4',
  'video/webm': 'webm',
  'video/x-flv': 'flv',
  'video/quicktime': 'mov',
  'video/x-matroska': 'mkv',
  'video/ogg': 'ogv',
  'audio/mpeg': 'mp3',
  'audio/mp4': 'm4a',
  'audio/aac': 'aac',
  'audio/ogg': 'ogg',
  'audio/webm': 'weba',
  'audio/flac': 'flac',
  'audio/wav': 'wav',
  'audio/x-wav': 'wav',
};

/** The address's last segment, decoded: `[stem, extension]`. */
function lastSegment(url: string): [string, string] {
  try {
    const last = decodeURIComponent(new URL(url).pathname.split('/').filter(Boolean).pop() ?? '');
    const dot = last.lastIndexOf('.');
    return dot > 0 ? [last.slice(0, dot), last.slice(dot + 1).toLowerCase()] : [last, ''];
  } catch {
    return ['', ''];
  }
}

/** The file name a media item is offered and downloaded under, with its extension: the
 *  address's name when it says something, else the page's title. Streams (HLS, DASH) are named
 *  after the page (their files are `index.m3u8`, `1234-1-30080.m4s`); the app sets the real
 *  extension of an HLS stream (`.ts` or `.mp4`). `as`: how it is offered (a DASH sound alone is
 *  audio). */
export function mediaName(m: Media, as: Offer['kind'] = m.kind === 'dash' ? 'dash' : m.kind): string {
  const [stem, ext] = lastSegment(m.url);
  const title = cleanTitle(m.title, m.pageUrl);
  const fallback = () => title || (meaningless(stem) ? '' : stem) || hostOf(m.pageUrl ?? m.url) || 'video';
  if (as === 'hls' || as === 'dash') return `${fallback()}.mp4`;
  if (m.kind === 'dash') return `${fallback()}.m4a`;
  const known = as === 'audio' ? AUDIO_EXT.has(ext) : VIDEO_EXT.has(ext) || AUDIO_EXT.has(ext);
  if (stem && known && !meaningless(stem)) return `${stem}.${ext}`;
  const typed = (m.mime && EXT_BY_TYPE[m.mime]) || (known ? ext : '') || (as === 'audio' ? 'mp3' : 'mp4');
  return `${fallback()}.${typed}`;
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
