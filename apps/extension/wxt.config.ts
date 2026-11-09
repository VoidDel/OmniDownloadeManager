// The Omni Download extension (docs/development-plan.md §5): one TypeScript source, built for
// Chromium (Chrome, Edge, Brave: `chrome-mv3`) and Firefox (`firefox-mv3`).

import { defineConfig } from 'wxt';

/**
 * The development key's public half. It fixes the Chromium extension ID at
 * nfifehjghnnmamofhbbiagiaodhfeipn, which the app's host manifests allow
 * (src-tauri/src/browser/integration.rs). Not a secret; the private key is not needed to load
 * the extension unpacked and is not kept in the repository.
 */
const CHROMIUM_KEY = 'MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAmW2oiODM+1NUvkpBCfUqWnADqSullQxEKZRsYqU1Ly9gFPcU8iKp0QFj4hcPon2GpGePv635RKQGxEBCvs5kHsx6G5lbEa0BSgHcqYiQCmU2fvdv+7tX0JApMxYSryYzbX9sIbMQk4XpHUKIEDB2GtUkfVwZwgF92fXdV3COIgl1MStHlxR4sYQGOEJU5PhMXnYa9tcpeMS7I2BKzVEcKBfrd4KmcJZaX6PcoQ6Ot4eBFpI8TA+7yLyhSedQHqeRQ7liHvX2vhplKEGgp2AkB7aoE9SZSLDmJNykRFI6TWkgu4A9MYzdurYuNvc1m6A7pl4IxkzQst1T5HVW93/VTQIDAQAB';

/** The Firefox add-on ID (the app's Firefox host manifest allows it). */
const GECKO_ID = 'omni@omnidl.app';

export default defineConfig({
  modules: ['@wxt-dev/module-react'],
  manifest: ({ browser }) => {
    const firefox = browser === 'firefox';
    return {
      name: 'Omni Download',
      description: '把浏览器的下载交给 Omni Download：多连接加速、断点续传，带上 Cookie 与来源页面。',
      permissions: [
        'downloads',
        'contextMenus',
        'cookies',
        'storage',
        'nativeMessaging',
        'notifications',
        'scripting',
        'activeTab',
        // Firefox takes downloads over before they start (blocking webRequest); Chromium uses
        // downloads.onDeterminingFilename.
        ...(firefox ? ['webRequest', 'webRequestBlocking'] : []),
      ],
      // Cookies of the download's site, and the local fallback channel (127.0.0.1).
      host_permissions: ['<all_urls>'],
      commands: {
        'download-page': {
          suggested_key: { default: 'Alt+Shift+D' },
          description: '用 Omni Download 下载当前页面',
        },
      },
      ...(firefox
        ? { browser_specific_settings: { gecko: { id: GECKO_ID, strict_min_version: '115.0' } } }
        : { key: CHROMIUM_KEY, minimum_chrome_version: '116' }),
    };
  },
});
