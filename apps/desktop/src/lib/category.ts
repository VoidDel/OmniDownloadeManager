// A file's category and its icon, as the sidebar shows them. The category is the Rust side's
// (crates/omnidl-core/src/filename.rs `category`); this copy is for names that are not tasks yet
// (the new-download dialog, the quick window, the files of a torrent). category.test.ts keeps the
// two in step.

import type { FileIconClass, IconName } from 'omni-ui';

import type { Category } from './types';

const TABLE: [Category, RegExp][] = [
  ['video', /^(mp4|mkv|avi|mov|wmv|flv|webm|m4v|mpg|mpeg|ts|m2ts|3gp|rmvb)$/],
  ['audio', /^(mp3|flac|wav|aac|ogg|m4a|wma|opus|ape|aiff)$/],
  ['document', /^(pdf|doc|docx|xls|xlsx|ppt|pptx|odt|ods|odp|txt|rtf|epub|mobi|md|csv)$/],
  ['archive', /^(zip|rar|7z|tar|gz|bz2|xz|zst|tgz|lz4|cab)$/],
  ['program', /^(exe|msi|msix|appx|pkg|deb|rpm|apk|appimage|flatpak)$/],
  ['disk_image', /^(iso|img|dmg|vhd|vhdx|vmdk|qcow2|wim)$/],
  ['image', /^(jpg|jpeg|png|gif|webp|bmp|svg|tif|tiff|heic|avif|ico|psd)$/],
];

/** The category of a file name, from its extension. */
export function categoryOf(name: string): Category {
  const dot = name.lastIndexOf('.');
  const ext = dot > 0 ? name.slice(dot + 1).toLowerCase() : '';
  return TABLE.find(([, re]) => re.test(ext))?.[0] ?? 'other';
}

/** Each category's icon: the sidebar's, the task list's and the dialogs'. */
export const CATEGORY_ICON: Record<Category, IconName> = {
  video: 'fileVideo',
  audio: 'fileAudio',
  document: 'fileText',
  archive: 'archive',
  program: 'package',
  disk_image: 'disc',
  image: 'fileImage',
  other: 'file',
};

/** The colour of a file icon of the category (omni-ui's duotone classes, `.fi.<class>`). */
export const CATEGORY_COLOR: Record<Category, FileIconClass> = {
  video: 'video',
  audio: 'audio',
  document: 'text',
  archive: 'archive',
  program: 'app',
  disk_image: 'disc',
  image: 'image',
  other: 'file',
};
