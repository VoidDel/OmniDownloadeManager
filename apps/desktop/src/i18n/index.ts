// i18next with ICU MessageFormat, as in OmniZip. All four languages are bundled: together they
// are small, and switching must not wait on the disk. Texts live in locales/<lang>/app.json at
// the repository root; components use `useTranslation()`, other code `i18n.t`.

import i18next from 'i18next';
import ICU from 'i18next-icu';
import { initReactI18next } from 'react-i18next';

import enUS from '@locales/en-US/app.json';
import jaJP from '@locales/ja-JP/app.json';
import zhCN from '@locales/zh-CN/app.json';
import zhTW from '@locales/zh-TW/app.json';

export const SUPPORTED = ['zh-CN', 'zh-TW', 'en-US', 'ja-JP'] as const;
export type Lang = (typeof SUPPORTED)[number];

/** Each language in its own words, for the language menu. */
export const LANGUAGE_NAMES: Record<Lang, string> = {
  'zh-CN': '简体中文',
  'zh-TW': '繁體中文',
  'en-US': 'English',
  'ja-JP': '日本語',
};

const resources = {
  'en-US': { app: enUS },
  'zh-CN': { app: zhCN },
  'zh-TW': { app: zhTW },
  'ja-JP': { app: jaJP },
};

/** Maps any locale tag to a supported language. */
export function normalizeLang(tag: string | null | undefined): Lang {
  const l = (tag ?? '').replace('_', '-').toLowerCase();
  if (l.startsWith('zh')) return /hant|tw|hk|mo/.test(l) ? 'zh-TW' : 'zh-CN';
  if (l.startsWith('ja')) return 'ja-JP';
  return 'en-US';
}

/** The system's language as the page sees it (WebView2, WKWebView and WebKitGTK report the
 *  system language). */
export function systemLang(): Lang {
  return normalizeLang(typeof navigator === 'undefined' ? null : navigator.language);
}

/** The UI language for a setting (`auto` follows the system). */
export function resolveLang(setting: string): Lang {
  return setting === 'auto' || !setting ? systemLang() : normalizeLang(setting);
}

let ready: Promise<void> | null = null;

export function initI18n(lang: Lang): Promise<void> {
  ready ??= i18next
    .use(ICU)
    .use(initReactI18next)
    .init({
      resources,
      lng: lang,
      fallbackLng: 'en-US',
      ns: ['app'],
      defaultNS: 'app',
      interpolation: { escapeValue: false },
      returnNull: false,
    })
    .then(() => {
      if (typeof document !== 'undefined') document.documentElement.lang = lang;
    });
  return ready;
}

export async function setLanguage(lang: Lang): Promise<void> {
  if (i18next.language === lang) return;
  await i18next.changeLanguage(lang);
  document.documentElement.lang = lang;
}

export default i18next;
