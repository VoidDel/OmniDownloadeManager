import { describe, expect, it } from 'vitest';

import { classify, cleanTitle, mediaKey, mediaName, offers, withMedia, MEDIA_MAX, type Media } from './media';

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
    expect(mediaName(item('https://cdn.example.com/hls/index.m3u8', { kind: 'hls', title: ' Big Buck Bunny ' }))).toBe('Big Buck Bunny.mp4');
    expect(mediaName(item('https://v.example.com/films/My%20Film.mp4'))).toBe('My Film.mp4');
  });

  it('names files whose address says nothing by the page, with an extension', () => {
    const page = 'https://www.youtube.com/watch?v=x';
    expect(mediaName(item('https://cdn.example.com/videoplayback?id=1', { mime: 'video/webm', title: '(3) Cat video - YouTube', pageUrl: page }))).toBe('Cat video.webm');
    expect(mediaName(item('https://cdn.example.com/a/9f86d081884c7d659a2feaa0c55ad015.mp4', { title: 'Trailer', pageUrl: 'https://films.example.com/t' }))).toBe('Trailer.mp4');
    // Characters a file name cannot hold become dashes.
    expect(mediaName(item('https://cdn.example.com/stream', { kind: 'audio', mime: 'audio/mpeg', title: 'Song | Radio One', pageUrl: 'https://radio.example.com/r' }))).toBe('Song - Radio One.mp3');
    // Nothing to go by: the address's words.
    expect(mediaName(item('https://cdn.example.com/1234567890.mp4', { pageUrl: 'https://films.example.com/' }))).toBe('films.example.com.mp4');
  });
});

describe('page titles', () => {
  it('drop the site at their end', () => {
    expect(cleanTitle('【4K】海边日落_哔哩哔哩_bilibili', 'https://www.bilibili.com/video/BV1x')).toBe('【4K】海边日落');
    expect(cleanTitle('Cat video - YouTube', 'https://www.youtube.com/watch?v=1')).toBe('Cat video');
    expect(cleanTitle('第1集_高清完整版在线观看_腾讯视频', 'https://v.qq.com/x/cover/a.html')).toBe('第1集');
    expect(cleanTitle('Show: Episode 2 | Example', 'https://www.example.com/s')).toBe('Show - Episode 2');
    // A title that is only the site's name stays.
    expect(cleanTitle('YouTube', 'https://www.youtube.com/')).toBe('YouTube');
  });
});

describe('DASH streams', () => {
  const MBs = (n: number) => h({ 'content-type': 'video/mp4', 'content-range': `bytes 0-1023/${n * MB}`, 'content-length': '1024' });
  const bili = (id: number) => `https://upos-sz.bilivideo.com/upgcxcode/12/34/5678/5678-1-${id}.m4s?e=x&deadline=1`;

  it('lists whole files played in ranges, picture or sound', () => {
    expect(classify(bili(30080), MBs(40))).toEqual({ kind: 'dash', mime: 'video/mp4', size: 40 * MB, track: 'video' });
    expect(classify(bili(30280), MBs(3))?.track).toBe('audio');
    expect(classify('https://cdn.example.com/a/audio_eng.m4s', h({ 'content-type': 'audio/mp4', 'content-range': `bytes 0-9/${2 * MB}` }))?.track).toBe('audio');
    // A segment (fetched whole) or a small one: not.
    expect(classify(bili(30080), h({ 'content-type': 'video/mp4', 'content-length': String(40 * MB) }))).toBeNull();
    expect(classify(bili(30080), MBs(0.1))).toBeNull();
  });

  it('leaves out YouTube player streams', () => {
    expect(classify('https://rr3---sn-a.googlevideo.com/videoplayback?itag=137&range=0-100', h({ 'content-type': 'video/mp4', 'content-length': String(5 * MB) }))).toBeNull();
  });

  it('offers the picture with the latest sound', () => {
    const at = (url: string, track: 'video' | 'audio', size: number, n: number): Media => ({ url, kind: 'dash', track, mime: 'video/mp4', size, title: '海边_哔哩哔哩_bilibili', pageUrl: 'https://www.bilibili.com/video/BV1', at: n });
    const file: Media = { url: 'https://v.example.com/x.mp4', kind: 'video', mime: null, size: null, title: null, pageUrl: null, at: 4 };
    const list = [at(bili(30280), 'audio', 3, 1), at(bili(30080), 'video', 40, 2), at(bili(30232), 'audio', 2, 3), file];
    const o = offers(list);
    expect(o.map((x) => x.kind)).toEqual(['video', 'dash']);
    expect(o[1]).toMatchObject({ name: '海边.mp4', size: 42, audio: { url: bili(30232) } });
    // Sound alone: offered as audio.
    expect(offers([at(bili(30280), 'audio', 3, 1)])).toMatchObject([{ kind: 'audio', name: '海边.m4a', audio: null }]);
  });
});
