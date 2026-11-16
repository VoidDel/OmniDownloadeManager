// The folder a new download goes to, as the settings sort downloads by category: the Rust side's
// (apps/desktop/src-tauri/src/settings.rs `folder_for`), for names that are not tasks yet (the
// new-download dialog, the quick window). folders.test.ts keeps the two in step.

import type { Category, CategoryFolders } from './types';

/** `name` inside `dir`, with the separator `dir` uses (Windows' when it has one). */
export function joinPath(dir: string, name: string): string {
  const sep = dir.includes('\\') || /^[A-Za-z]:/.test(dir) ? '\\' : '/';
  return dir.endsWith(sep) ? `${dir}${name}` : `${dir}${sep}${name}`;
}

/** A category's name as a folder name: what a file name cannot hold left out. */
function folderName(label: string): string {
  return label.replace(/[\\/:*?"<>|]+/g, ' ').replace(/\s+/g, ' ').trim();
}

/** Where a download of `category` goes: `downloadDir`, a folder for its category in it named
 *  by `label` (`auto`), or the one set for it (`manual`). A file of no category: `downloadDir`. */
export function folderFor(
  mode: CategoryFolders | undefined,
  dirs: Partial<Record<Category, string>> | undefined,
  downloadDir: string,
  category: Category,
  label: (c: Category) => string,
): string {
  if (category === 'other') return downloadDir;
  if (mode === 'auto') return joinPath(downloadDir, folderName(label(category)));
  if (mode === 'manual') return dirs?.[category]?.trim() || downloadDir;
  return downloadDir;
}
