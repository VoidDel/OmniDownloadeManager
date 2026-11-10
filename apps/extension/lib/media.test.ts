import { describe, expect, it } from 'vitest';

import { classify, mediaKey, mediaName, withMedia, MEDIA_MAX, type Media } from './media';

const h = (pairs: Record<string, string>) => Object.entries(pairs).map(([name, value]) => ({ name, value }));
const MB = 1024 * 1024;

describe('classify', () => {
  it('lists videos, audio and HLS playlists', () => {
    expect(classify('https://v.example.com/a/movie.mp4', h({ 'content-type': 'video/mp4', 'content-length': String(80 * MB) }))?.kind).toBe('video');
    expect(classify('https://cdn.example.com/x?id=1', h({ 'content-type': 'audio/mpeg', 'content-length': String(5 * MB) }))?.kind).toBe('audio');
    expect(classify('https://cdn.example.com/hls/index.m3u8?t=1', h({}))?.kind).toBe('hls');
    expect(classify('https://cdn.example.com/play', h({ 'content-type': 'application/vnd.apple.mpegurl' }))?.kind).toBe('hls');
    // A range of a big file: its whole size.
    expect(classify('https://v.example.com/b.webm', h({ 'content-type': 'video/webm', 'content-length': '65536', 'content-range': `bytes 0-65535/${300 * MB}` }))?.size).toBe(300 * MB);
  });

  it('leaves out pieces of streams, small files, DASH and pages', () => {
    expect(classify('https://cdn.example.com/hls/seg12.ts', h({ 'content-type': 'video/mp2t' }))).toBeNull();
    expect(classify('https://cdn.example.com/dash/chunk-3.m4s', h({}))).toBeNull();
    expect(classify('https://cdn.example.com/manifest.mpd', h({ 'content-type': 'application/dash+xml' }))).toBeNull();
    expect(classify('https://example.com/click.mp3', h({ 'content-type': 'audio/mpeg', 'content-length': '20000' }))).toBeNull();
    expect(classify('https://example.com/', h({ 'content-type': 'text/html' }))).toBeNull();
    expect(classify('blob:https://example.com/123', h({ 'content-type': 'video/mp4' }))).toBeNull();
  });
});

describe('the list of a tab', () => {
  const item = (url: string, extra: Partial<Media> = {}): Media => ({ url, kind: 'video', mime: null, size: null, title: null, pageUrl: null, at: 0, ...extra });

  it('lists a file once, whatever range it was asked for', () => {
    expect(mediaKey('https://v.example.com/a.mp4?range=0-100&sig=x')).toBe(mediaKey('https://v.example.com/a.mp4?range=200-300&sig=x'));
    let list = withMedia([], item('https://v.example.com/a.mp4?range=0-100'));
    list = withMedia(list, item('https://v.example.com/a.mp4?range=101-200', { size: 9 }));
    expect(list).toHaveLength(1);
    expect(list[0].size).toBe(9);
  });

  it('keeps the newest', () => {
    let list: Media[] = [];
    for (let i = 0; i < MEDIA_MAX + 5; i++) list = withMedia(list, item(`https://v.example.com/${i}.mp4`));
    expect(list).toHaveLength(MEDIA_MAX);
    expect(list.at(-1)?.url).toBe(`https://v.example.com/${MEDIA_MAX + 4}.mp4`);
  });

  it('names streams by the page', () => {
    expect(mediaName(item('https://cdn.example.com/hls/index.m3u8', { kind: 'hls', title: ' Big Buck Bunny ' }))).toBe('Big Buck Bunny');
    expect(mediaName(item('https://v.example.com/films/My%20Film.mp4'))).toBe('My Film.mp4');
  });
});
