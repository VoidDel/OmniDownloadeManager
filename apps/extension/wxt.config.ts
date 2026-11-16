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

/**
 * A package for the stores (`pnpm ext:zip` sets `OMNIDL_STORE=1`). The Chrome Web Store and Edge
 * Add-ons sign the package with their own key, which gives it their own ID, and the Chrome Web
 * Store refuses a manifest that carries `key`: store packages go without it. The IDs the stores
 * assign are added to the app's host manifests (`CHROMIUM_EXTENSION_IDS`).
 */
const STORE = process.env.OMNIDL_STORE === '1';

export default defineConfig({
  modules: ['@wxt-dev/module-react'],
  manifest: ({ browser }) => {
    const firefox = browser === 'firefox';
    return {
      name: 'Omni Download',
      // Texts are in public/_locales (简体中文, 繁體中文, English, 日本語); the stores list the
      // extension in those languages. English is what every other language gets.
      default_locale: 'en',
      description: '__MSG_extDescription__',
      permissions: [
        'downloads',
        'contextMenus',
        'cookies',
        'storage',
        'nativeMessaging',
        'notifications',
        'scripting',
        'activeTab',
        // Media sniffing reads response headers (both); Firefox also takes downloads over before
        // they start (blocking webRequest), Chromium with downloads.onDeterminingFilename.
        'webRequest',
        ...(firefox ? ['webRequestBlocking'] : []),
      ],
      // Cookies of the download's site, and the local fallback channel (127.0.0.1).
      host_permissions: ['<all_urls>'],
      commands: {
        'download-page': {
          suggested_key: { default: 'Alt+Shift+D' },
          description: '__MSG_cmdDownloadPage__',
        },
      },
      ...(firefox
        ? {
            browser_specific_settings: {
              gecko: {
                id: GECKO_ID,
                // Firefox 140 shows what an extension collects when it is installed
                // (`data_collection_permissions`); earlier versions would need a consent page
                // of the extension's own.
                strict_min_version: '140.0',
                // What goes to the Omni Download app on the same computer, and nowhere else:
                // the download's address and the page it came from, that site's cookies (so
                // the app can fetch a file that needs a login), and the fact of the download.
                data_collection_permissions: { required: ['browsingActivity', 'websiteContent', 'websiteActivity'] },
              },
            },
          }
        : { ...(STORE ? {} : { key: CHROMIUM_KEY }), minimum_chrome_version: '116' }),
    };
  },
  zip: {
    name: 'omni-download',
    artifactTemplate: '{{name}}-{{version}}-{{browser}}.zip',
    sourcesTemplate: '{{name}}-{{version}}-sources.zip',
    // Mozilla builds the add-on from its sources to review it. The extension is one package of
    // a pnpm workspace whose lock file is at the repository's root, so the sources start there
    // and take what the build needs (SOURCE_CODE_REVIEW.md has the steps).
    sourcesRoot: '../..',
    includeSources: ['package.json', 'pnpm-lock.yaml', 'pnpm-workspace.yaml', 'LICENSE', 'apps/extension/**', 'apps/desktop/package.json'],
    excludeSources: ['apps/extension/test-results/**', 'apps/extension/playwright-report/**', 'apps/extension/e2e/**', 'apps/extension/playwright.config.ts', 'apps/extension/store/**'],
  },
});
