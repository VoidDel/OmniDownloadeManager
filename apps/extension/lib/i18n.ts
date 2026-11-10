// The extension's texts in the browser's language: public/_locales/<lang>/messages.json
// (简体中文, 繁體中文, English, 日本語; English for every other language).

/** The text for `key`, with `$1`, `$2`… replaced by `subs`. */
export function t(key: string, ...subs: (string | number)[]): string {
  return browser.i18n.getMessage(key as Parameters<typeof browser.i18n.getMessage>[0], subs.map(String)) || key;
}

/** A text with a count: `<key>One` for exactly one, `<key>Other` otherwise (English tells them
 *  apart; the other languages have the same text for both). */
export function tn(key: string, count: number): string {
  return t(count === 1 ? `${key}One` : `${key}Other`, count);
}
