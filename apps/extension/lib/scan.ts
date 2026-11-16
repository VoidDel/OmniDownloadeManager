// A look at the page when the popup opens: the media its elements and its own records name,
// for what the network sniffing did not see (loaded before the extension started, served from
// the cache or by the page's service worker). Read only, in the extension's own world, in every
// frame; nothing stays in the page.

/** A media address a page names, and where it was found. */
export interface Found {
  url: string;
  /** Named by a `<video>` or `<audio>` (or its `<source>`), or by the page's metadata
   *  (`og:video`, `og:audio`): what it is, when its address does not say. */
  tag?: 'video' | 'audio';
  /** Bytes, when the page's records know (same-origin, or allowed by the server). */
  size?: number | null;
  /** The frame's address. */
  frameUrl: string;
  /** The frame is the top one. */
  top: boolean;
}

/**
 * Runs in the page (`scripting.executeScript`): it must not use anything from outside its body.
 * Media elements' sources, the page's media metadata, and the resources the page loaded (its
 * performance records), each address once.
 */
export function scanPage(): Found[] {
  const out = new Map<string, Found>();
  const frameUrl = location.href;
  const top = window === window.top;
  const add = (raw: string | null | undefined, tag?: 'video' | 'audio', size?: number | null) => {
    if (!raw) return;
    let url: string;
    try {
      url = new URL(raw, location.href).toString();
    } catch {
      return;
    }
    if (!/^https?:/i.test(url)) return;
    const had = out.get(url);
    out.set(url, { url, tag: had?.tag ?? tag, size: had?.size || size || null, frameUrl, top });
  };
  for (const el of Array.from(document.querySelectorAll<HTMLMediaElement>('video, audio'))) {
    const tag = el.tagName === 'AUDIO' ? 'audio' : 'video';
    add(el.currentSrc, tag);
    add(el.getAttribute('src'), tag);
    for (const s of Array.from(el.querySelectorAll('source'))) add(s.getAttribute('src'), tag);
  }
  const metas: [string, 'video' | 'audio'][] = [
    ['og:video', 'video'],
    ['og:video:url', 'video'],
    ['og:video:secure_url', 'video'],
    ['og:audio', 'audio'],
    ['og:audio:url', 'audio'],
    ['og:audio:secure_url', 'audio'],
    ['twitter:player:stream', 'video'],
  ];
  for (const [name, tag] of metas) {
    for (const m of Array.from(document.querySelectorAll(`meta[property="${name}"], meta[name="${name}"]`))) add(m.getAttribute('content'), tag);
  }
  for (const e of performance.getEntriesByType('resource') as PerformanceResourceTiming[]) {
    const tag = e.initiatorType === 'video' ? 'video' : e.initiatorType === 'audio' ? 'audio' : undefined;
    // Zero: the server does not let the page know (another origin), or nothing came.
    add(e.name, tag, e.decodedBodySize || e.encodedBodySize || null);
  }
  return [...out.values()].slice(0, 500);
}
