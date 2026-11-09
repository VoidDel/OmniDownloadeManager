// Icons come from omni-ui (shared with OmniZip). This module adds what is Omni Download's own.

import appIcon from '@icons/app-icon.svg';

export { FileIcon, Icon, badgeStyle, fileIcon, type IconName } from 'omni-ui';

/** The app icon: the same artwork as the executable and package icons. */
export function Logo({ size = 22 }: { size?: number }) {
  return <img className="logo" src={appIcon} width={size} height={size} alt="" draggable={false} />;
}
