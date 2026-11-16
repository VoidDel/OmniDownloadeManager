import { describe, expect, it } from 'vitest';

import { parseMaster, qualityLabel, qualityLabels } from './hls';
import { offers, type Media } from './media';

const BASE = 'https://cdn.e.com/v/master.m3u8?token=1';
const MASTER = [
  '#EXTM3U',
  '#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="aac",NAME="English",DEFAULT=YES,URI="audio/en.m3u8"',
  '#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID="subs",NAME="中文",URI="subs/zh.m3u8"',
  '#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360,CODECS="avc1.4d401e,mp4a.40.2",AUDIO="aac"',
  '360p/index.m3u8',
  '#EXT-X-STREAM-INF:BANDWIDTH=5000000,RESOLUTION=1920x1080,AUDIO="aac"',
  'https://cdn2.e.com/1080p/index.m3u8?sig=x',
  '#EXT-X-STREAM-INF:BANDWIDTH=2800000,RESOLUTION=1280x720,AUDIO="aac"',
  '720p/index.m3u8',
  '#EXT-X-I-FRAME-STREAM-INF:BANDWIDTH=90000,URI="iframes.m3u8"',
  '',
].join('\r\n');

describe('master playlists', () => {
  it('reads the qualities, best first, and the playlists named', () => {
    const m = parseMaster(MASTER, BASE)!;
    expect(m.variants).toEqual([
      { url: 'https://cdn2.e.com/1080p/index.m3u8?sig=x', bandwidth: 5_000_000, width: 1920, height: 1080 },
      { url: 'https://cdn.e.com/v/720p/index.m3u8', bandwidth: 2_800_000, width: 1280, height: 720 },
      { url: 'https://cdn.e.com/v/360p/index.m3u8', bandwidth: 800_000, width: 640, height: 360 },
    ]);
    expect(m.parts.sort()).toEqual(
      ['audio/en.m3u8', 'subs/zh.m3u8', '360p/index.m3u8', '720p/index.m3u8', 'iframes.m3u8'].map((p) => new URL(p, BASE).toString()).concat('https://cdn2.e.com/1080p/index.m3u8?sig=x').sort(),
    );
  });

  it('is nothing for a media playlist or other text', () => {
    expect(parseMaster('#EXTM3U\n#EXTINF:2,\nseg0.ts\n#EXT-X-ENDLIST\n', BASE)).toBeNull();
    expect(parseMaster('<html>', BASE)).toBeNull();
    // A variant without a usable address.
    expect(parseMaster('#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\njavascript:alert(1)\n', BASE)).toBeNull();
  });

  it('names qualities by height, else bit rate', () => {
    expect(qualityLabel({ url: 'x', bandwidth: 5_000_000, height: 1080 })).toBe('1080p');
    expect(qualityLabel({ url: 'x', bandwidth: 800_000 })).toBe('800 kbps');
    expect(qualityLabel({ url: 'x', bandwidth: 2_800_000 })).toBe('2.8 Mbps');
    // Two of one height: their bit rates tell them apart.
    expect(qualityLabels([
      { url: 'a', bandwidth: 6_000_000, height: 1080 },
      { url: 'b', bandwidth: 3_000_000, height: 1080 },
      { url: 'c', bandwidth: 1_000_000, height: 480 },
    ])).toEqual(['1080p · 6.0 Mbps', '1080p · 3.0 Mbps', '480p']);
  });

  it('folds the playlists a master names into it', () => {
    const m = parseMaster(MASTER, BASE)!;
    const hls = (url: string, extra: Partial<Media> = {}): Media => ({ url, kind: 'hls', mime: null, size: null, title: 'Clip', pageUrl: 'https://e.com/v', at: 0, ...extra });
    const list = [
      hls(BASE, { variants: m.variants, parts: m.parts }),
      hls('https://cdn2.e.com/1080p/index.m3u8?sig=x'),
      hls('https://cdn.e.com/v/audio/en.m3u8'),
      hls('https://other.e.com/live.m3u8'),
    ];
    expect(offers(list).map((o) => o.media.url)).toEqual(['https://other.e.com/live.m3u8', BASE]);
    // Until the master is read, each is offered.
    expect(offers(list.map((x) => ({ ...x, variants: undefined, parts: undefined }))).length).toBe(4);
  });
});
