import { describe, expect, it } from 'vitest';

import rust from '../../../../crates/omnidl-core/src/filename.rs?raw';
import { categoryOf } from './category';

const RUST_NAMES: Record<string, string> = {
  Video: 'video',
  Audio: 'audio',
  Document: 'document',
  Archive: 'archive',
  Program: 'program',
  DiskImage: 'disk_image',
  Image: 'image',
};

describe('categoryOf', () => {
  it('matches the cases of the Rust tests', () => {
    expect(categoryOf('a.MKV')).toBe('video');
    expect(categoryOf('a.tar.gz')).toBe('archive');
    expect(categoryOf('setup.exe')).toBe('program');
    expect(categoryOf('ubuntu.iso')).toBe('disk_image');
    expect(categoryOf('paper.pdf')).toBe('document');
    expect(categoryOf('README')).toBe('other');
    expect(categoryOf('.bashrc')).toBe('other');
  });

  it('knows every extension the Rust side does', () => {
    // Each `"ext" | "ext" => Category::X` arm of crates/omnidl-core/src/filename.rs.
    const body = rust.slice(rust.indexOf('pub fn category'), rust.indexOf('#[cfg(test)]'));
    let checked = 0;
    for (const arm of body.matchAll(/((?:"[a-z0-9]+"\s*\|?\s*)+)=>\s*\{?\s*Category::(\w+)/g)) {
      for (const ext of arm[1].matchAll(/"([a-z0-9]+)"/g)) {
        expect(categoryOf(`file.${ext[1]}`), `.${ext[1]}`).toBe(RUST_NAMES[arm[2]]);
        checked++;
      }
    }
    expect(checked).toBeGreaterThan(70);
  });
});
