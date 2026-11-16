// A master HLS playlist's qualities: what the popup offers to choose from, and the playlists it
// names (each quality's, its sound's, subtitles', I-frames'), which a player loads too and the
// list folds into the master.

/** A quality of a master playlist. */
export interface HlsVariant {
  /** Its playlist's address. */
  url: string;
  /** Bits per second (`BANDWIDTH`). */
  bandwidth: number;
  width?: number;
  height?: number;
}

export interface HlsMaster {
  /** Highest bandwidth first. */
  variants: HlsVariant[];
  /** Every playlist it names, the variants' among them. */
  parts: string[];
}

/** `KEY=value,KEY="quoted, value"` → pairs. */
function attributes(s: string): Record<string, string> {
  const out: Record<string, string> = {};
  const re = /([A-Z0-9-]+)=("[^"]*"|[^,]*)/g;
  for (let m = re.exec(s); m; m = re.exec(s)) out[m[1]] = m[2].replace(/^"|"$/g, '');
  return out;
}

function resolve(uri: string, base: string): string | null {
  try {
    const u = new URL(uri, base);
    return /^https?:$/.test(u.protocol) ? u.toString() : null;
  } catch {
    return null;
  }
}

/** A master playlist's qualities and the playlists it names; `null` for a media playlist (or
 *  text that is no playlist). */
export function parseMaster(text: string, base: string): HlsMaster | null {
  if (!text.trimStart().startsWith('#EXTM3U')) return null;
  const variants: HlsVariant[] = [];
  const parts = new Set<string>();
  let pending: Record<string, string> | null = null;
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line) continue;
    if (line.startsWith('#EXT-X-STREAM-INF:')) {
      pending = attributes(line.slice('#EXT-X-STREAM-INF:'.length));
    } else if (line.startsWith('#EXT-X-MEDIA:') || line.startsWith('#EXT-X-I-FRAME-STREAM-INF:')) {
      const uri = attributes(line.slice(line.indexOf(':') + 1)).URI;
      const url = uri ? resolve(uri, base) : null;
      if (url) parts.add(url);
    } else if (!line.startsWith('#') && pending) {
      const url = resolve(line, base);
      if (url) {
        const [w, h] = (pending.RESOLUTION ?? '').split('x').map(Number);
        variants.push({ url, bandwidth: Number(pending.BANDWIDTH) || 0, ...(w > 0 && h > 0 ? { width: w, height: h } : {}) });
        parts.add(url);
      }
      pending = null;
    }
  }
  if (!variants.length) return null;
  variants.sort((a, b) => b.bandwidth - a.bandwidth);
  return { variants, parts: [...parts] };
}

/** How the popup names a quality: `1080p`, else its bit rate (`800 kbps`, `2.8 Mbps`). `alone`:
 *  the only one of that height (else the bit rate goes with it). */
export function qualityLabel(v: HlsVariant, alone = true): string {
  const rate = v.bandwidth >= 1_000_000 ? `${(v.bandwidth / 1_000_000).toFixed(1)} Mbps` : `${Math.round(v.bandwidth / 1000)} kbps`;
  if (!v.height) return rate;
  return alone ? `${v.height}p` : `${v.height}p · ${rate}`;
}

/** Labels for a master's qualities, in its order. */
export function qualityLabels(variants: HlsVariant[]): string[] {
  return variants.map((v) => qualityLabel(v, !v.height || variants.filter((x) => x.height === v.height).length === 1));
}
