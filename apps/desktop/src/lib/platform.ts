import type { Platform } from 'omni-ui';

/**
 * The desktop platform, from the web view's user agent: WebView2 on Windows, WKWebView on macOS
 * and WebKitGTK on Linux each name their system. Used for the title bar (traffic lights or own
 * window buttons) and the `data-platform` styles of omni-ui.
 */
export function detectPlatform(userAgent: string): Platform {
  if (/Windows/i.test(userAgent)) return 'win';
  if (/Macintosh|Mac OS X/i.test(userAgent)) return 'mac';
  return 'linux';
}
