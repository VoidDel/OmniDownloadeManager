import { describe, expect, it } from 'vitest';

import rust from '../../src-tauri/src/settings.rs?raw';
import { folderFor, joinPath } from './folders';
import type { Category } from './types';

const zh: Record<Category, string> = { video: '视频', audio: '音乐', document: '文档', archive: '压缩包', program: '程序', disk_image: '镜像', image: '图片', other: '其他' };
const label = (c: Category) => zh[c];

describe('folderFor', () => {
  const dirs = { video: 'E:\\影片', audio: '  ' };

  it('sorts as the settings say', () => {
    expect(folderFor('none', dirs, 'D:\\下载', 'video', label)).toBe('D:\\下载');
    expect(folderFor(undefined, dirs, 'D:\\下载', 'video', label)).toBe('D:\\下载');
    expect(folderFor('auto', dirs, 'D:\\下载', 'video', label)).toBe('D:\\下载\\视频');
    expect(folderFor('auto', dirs, '/home/u/Downloads', 'disk_image', () => 'Disk images')).toBe('/home/u/Downloads/Disk images');
    expect(folderFor('manual', dirs, 'D:\\下载', 'video', label)).toBe('E:\\影片');
    expect(folderFor('manual', dirs, 'D:\\下载', 'audio', label)).toBe('D:\\下载');
  });

  it('keeps a file of no category in the download folder', () => {
    for (const mode of ['none', 'auto', 'manual'] as const) expect(folderFor(mode, dirs, 'D:\\下载', 'other', label)).toBe('D:\\下载');
  });

  it('joins paths with their own separator', () => {
    expect(joinPath('C:\\', 'x')).toBe('C:\\x');
    expect(joinPath('/home/u/', 'x')).toBe('/home/u/x');
  });

  it('knows the ways the Rust side knows', () => {
    const block = /pub enum CategoryFolders \{([\s\S]*?)\n\}/.exec(rust)?.[1] ?? '';
    const variants = [...block.matchAll(/^\s{4}(\w+),$/gm)].map((m) => m[1].toLowerCase()).sort();
    expect(variants).toEqual(['auto', 'manual', 'none']);
  });
});
