// Icons come from omni-ui (shared with OmniZip). This module adds what is Omni Download's own.

import appIcon from '@icons/app-icon.svg';
import { Icon } from 'omni-ui';

import { CATEGORY_COLOR, CATEGORY_ICON, categoryOf } from '../lib/category';
import type { Category } from '../lib/types';

export { Icon, badgeStyle, type IconName } from 'omni-ui';

/** A file's icon by its category, the same as the sidebar's for that category (omni-ui's
 *  `FileIcon` sorts types for an archiver: `.deb` is an archive there, a program here). A
 *  folder (a torrent of several files) is a folder. */
export function FileIcon({ name, category, isDir }: { name?: string; category?: Category; isDir?: boolean }) {
  if (isDir) return <Icon name="folder" className="fi folder" />;
  const c = category ?? categoryOf(name ?? '');
  return <Icon name={CATEGORY_ICON[c]} className={`fi ${CATEGORY_COLOR[c]}`} />;
}

/** The app icon: the same artwork as the executable and package icons. */
export function Logo({ size = 22 }: { size?: number }) {
  return <img className="logo" src={appIcon} width={size} height={size} alt="" draggable={false} />;
}
