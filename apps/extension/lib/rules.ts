// Which downloads Omni Download takes over (docs/development-plan.md §5.3). Pure functions; the
// rules themselves are edited on the options page.

export interface Rules {
  /** The master switch. */
  enabled: boolean;
  /** Smaller files stay with the browser (unknown sizes are taken). */
  minSizeMB: number;
  /** When not empty, only these extensions are taken. */
  onlyExtensions: string[];
  /** These extensions stay with the browser. */
  skipExtensions: string[];
  /** Sites (and their subdomains) whose downloads stay with the browser. */
  skipSites: string[];
  /** Start in Omni Download without its confirmation window. */
  silent: boolean;
}

export const DEFAULT_RULES: Rules = {
  enabled: true,
  minSizeMB: 1,
  onlyExtensions: [],
  skipExtensions: [],
  skipSites: [],
  silent: false,
};

/** A download the browser is about to make. */
export interface Candidate {
  url: string;
  filename?: string | null;
  size?: number | null;
  /** The page it started from. */
  pageUrl?: string | null;
}

/** `zip`, `gz`, `` — lower case, from the name or the URL's path. */
export function extensionOf(c: Pick<Candidate, 'url' | 'filename'>): string {
  let name = c.filename ?? '';
  if (!name) {
    try {
      name = decodeURIComponent(new URL(c.url).pathname.split('/').pop() ?? '');
    } catch {
      name = '';
    }
  }
  name = name.split(/[\\/]/).pop() ?? '';
  const dot = name.lastIndexOf('.');
  return dot > 0 && dot < name.length - 1 ? name.slice(dot + 1).toLowerCase() : '';
}

function hostOf(url: string | null | undefined): string {
  if (!url) return '';
  try {
    return new URL(url).hostname.toLowerCase();
  } catch {
    return '';
  }
}

/** `example.com` matches `example.com` and `dl.example.com`. */
export function siteMatches(host: string, site: string): boolean {
  const s = site.trim().toLowerCase().replace(/^\*\./, '').replace(/^https?:\/\//, '').replace(/\/.*$/, '');
  return !!s && (host === s || host.endsWith(`.${s}`));
}

/** Splits a typed list (`zip, 7z .iso`) into clean entries. */
export function parseList(text: string): string[] {
  return text
    .split(/[\s,，;；]+/)
    .map((s) => s.trim().replace(/^\./, '').toLowerCase())
    .filter(Boolean);
}

/** Whether to take `c` over, and why not when not. */
export function decide(c: Candidate, r: Rules): { take: true } | { take: false; reason: string } {
  if (!r.enabled) return { take: false, reason: 'disabled' };
  if (!/^https?:\/\//i.test(c.url)) return { take: false, reason: 'scheme' };
  if (c.size != null && c.size >= 0 && c.size < r.minSizeMB * 1024 * 1024) return { take: false, reason: 'small' };
  const ext = extensionOf(c);
  if (r.onlyExtensions.length && !r.onlyExtensions.includes(ext)) return { take: false, reason: 'extension' };
  if (ext && r.skipExtensions.includes(ext)) return { take: false, reason: 'extension' };
  const hosts = [hostOf(c.url), hostOf(c.pageUrl)].filter(Boolean);
  if (r.skipSites.some((s) => hosts.some((h) => siteMatches(h, s)))) return { take: false, reason: 'site' };
  return { take: true };
}

/** `Cookie` header value from cookies. */
export function cookieHeader(cookies: { name: string; value: string }[]): string {
  return cookies.map((c) => `${c.name}=${c.value}`).join('; ');
}

export interface StoredCookie {
  name: string;
  value: string;
  /** Set for a cookie with the `Partitioned` attribute (CHIPS): it goes only to requests whose
   *  top-level page is on `topLevelSite` (`https://example.com`). */
  partitionKey?: { topLevelSite?: string } | null;
}

/** `host` (and its scheme) is on `site`, a scheme and registrable domain. */
function onSite(url: string | null | undefined, site: string): boolean {
  try {
    const u = new URL(url ?? '');
    const s = new URL(site);
    const host = u.hostname.toLowerCase();
    const top = s.hostname.toLowerCase();
    return u.protocol === s.protocol && (host === top || host.endsWith(`.${top}`));
  } catch {
    return false;
  }
}

/**
 * The cookies, of all partitions, that a download of `url` started from `pageUrl` carries:
 * those without a partition, and the partitioned ones of the site of its top-level page — the
 * download's own when it is a navigation, the page's when the page started it. Cloudflare's
 * clearance (`cf_clearance`) is one of the partitioned ones.
 */
export function sendableCookies<T extends StoredCookie>(cookies: T[], url: string, pageUrl?: string | null): T[] {
  return cookies.filter((c) => {
    const site = c.partitionKey?.topLevelSite;
    return !site || onSite(url, site) || onSite(pageUrl, site);
  });
}
