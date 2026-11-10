// The extension's background (docs/development-plan.md §5.2–5.4): takes downloads over, the
// context menu, the "download this page" shortcut, and media sniffing (the videos, audio and
// HLS streams a page plays, listed in the popup with their number on the toolbar button).
//
// Chromium: `downloads.onDeterminingFilename` holds the download while the app is asked; the
// browser goes on (`suggest()`) when the app cannot be reached or the user wants the browser to
// download it, and the download is cancelled when the app takes it.
// Firefox: a blocking `webRequest.onHeadersReceived` decides from the response headers before a
// download starts; `downloads.onCreated` catches the rest.

import { send } from '../lib/bridge';
import { contentLength, dispositionName, header, isDownload, type Header } from '../lib/headers';
import { t } from '../lib/i18n';
import { classify, mediaName, mediaOf, saveMedia, withMedia, type Media } from '../lib/media';
import { decide, extensionOf } from '../lib/rules';
import { load } from '../lib/settings';
import { basename, handOver, notify, payloadFor, type Download } from '../lib/takeover';

/** Links the user asked the browser to keep (Alt+click), and our own re-downloads. */
const bypass = new Map<string, number>();
/** URLs Firefox handed over in webRequest (their download item is not ours to judge again). */
const handled = new Map<string, number>();

function mark(map: Map<string, number>, url: string | undefined, ms = 10_000) {
  if (url) map.set(url, Date.now() + ms);
}

function take(map: Map<string, number>, url: string | undefined): boolean {
  if (!url) return false;
  const until = map.get(url);
  if (until === undefined) return false;
  map.delete(url);
  return until > Date.now();
}

/** Extensions offered by "download all links on this page". */
const FILE_EXTENSIONS = new Set(
  'zip rar 7z tar gz tgz bz2 xz zst iso img dmg exe msi msix apk deb rpm appimage pkg pdf doc docx xls xlsx ppt pptx epub mobi mp4 mkv avi mov webm flv m4v mp3 flac wav aac ogg m4a opus jpg jpeg png gif webp svg bin torrent'.split(' '),
);

function menus() {
  void browser.contextMenus.removeAll().then(() => {
    browser.contextMenus.create({ id: 'omni-link', title: t('menuLink'), contexts: ['link'] });
    browser.contextMenus.create({ id: 'omni-media', title: t('menuMedia'), contexts: ['image', 'video', 'audio'] });
    browser.contextMenus.create({ id: 'omni-all', title: t('menuAll'), contexts: ['page'] });
  });
}

/** Lets the app know this browser has the extension (shown in its status bar). */
function hello() {
  void send({ type: 'get_status' }).catch(() => undefined);
}

async function explicit(d: Download) {
  const out = await handOver(d);
  if (out.kind === 'unreachable') void notify(t('notifyUnreachable'));
}

async function downloadAllLinks(tabId: number, pageUrl?: string) {
  let links: string[];
  try {
    const [res] = await browser.scripting.executeScript({
      target: { tabId },
      func: () => Array.from(document.links, (a) => a.href),
    });
    links = (res?.result as string[] | undefined) ?? [];
  } catch {
    void notify(t('notifyNoLinksRead'));
    return;
  }
  const files = [...new Set(links)].filter((u) => /^https?:\/\//i.test(u) && FILE_EXTENSIONS.has(extensionOf({ url: u })));
  if (!files.length) {
    void notify(t('notifyNoFileLinks'));
    return;
  }
  const items = await Promise.all(files.map((url) => payloadFor({ url, pageUrl }, false)));
  try {
    await send({ type: 'add_batch', payload: { items } });
  } catch {
    void notify(t('notifyUnreachable'));
  }
}

// ---------------------------------------------------------------------------------- Chromium

type DownloadItem = Browser.downloads.DownloadItem;

/**
 * Takes over a download the browser has already started (the fallback paths): only when the
 * app answers, then cancels the browser's copy; if the app does not keep it, the browser
 * downloads it again.
 */
async function takeStarted(item: DownloadItem) {
  if (item.byExtensionId === browser.runtime.id || take(bypass, item.url) || take(handled, item.url)) return;
  const url = /^https?:/i.test(item.finalUrl ?? '') ? item.finalUrl : item.url;
  if (!/^https?:/i.test(url)) return;
  const size = item.totalBytes > 0 ? item.totalBytes : null;
  // Whoever named it may have chosen anything (a temporary name): the app names the file from
  // the server's answer instead.
  const filename = null;
  const { rules } = await load();
  if (!decide({ url, size, pageUrl: item.referrer }, rules).take) return;
  const ping = await send({ type: 'ping' }).catch(() => null);
  if (!ping?.ok) return;
  await browser.downloads.cancel(item.id).catch(() => undefined);
  await browser.downloads.erase({ id: item.id }).catch(() => undefined);
  const out = await handOver({ url, filename, size, mime: item.mime, referer: item.referrer, pageUrl: item.referrer });
  if (out.kind !== 'taken') {
    mark(bypass, url);
    await browser.downloads.download({ url }).catch(() => undefined);
  }
}

function chromiumTakeover() {
  // Downloads this listener saw. One named without asking it (another extension, an automation
  // tool) goes to the fallback once its name is settled.
  const asked = new Set<number>();
  browser.downloads.onChanged.addListener((delta) => {
    if (!delta.filename?.current) return;
    if (asked.delete(delta.id)) return;
    void browser.downloads.search({ id: delta.id }).then(([item]) => {
      if (item?.state === 'in_progress') void takeStarted(item);
    });
  });

  browser.downloads.onDeterminingFilename.addListener((item: DownloadItem, suggest: (s?: { filename: string }) => void) => {
    asked.add(item.id);
    if (item.byExtensionId === browser.runtime.id || take(bypass, item.url) || take(bypass, item.finalUrl)) return;
    void (async () => {
      const url = /^https?:/i.test(item.finalUrl) ? item.finalUrl : item.url;
      const size = item.totalBytes > 0 ? item.totalBytes : item.fileSize > 0 ? item.fileSize : null;
      const filename = basename(item.filename);
      const { rules } = await load();
      if (!decide({ url, filename, size, pageUrl: item.referrer }, rules).take) {
        suggest();
        return;
      }
      const out = await handOver({ url, filename, size, mime: item.mime, referer: item.referrer, pageUrl: item.referrer });
      if (out.kind === 'taken') {
        await browser.downloads.cancel(item.id).catch(() => undefined);
        await browser.downloads.erase({ id: item.id }).catch(() => undefined);
      } else {
        suggest();
      }
    })();
    // The answer comes later.
    return true;
  });
}

// ---------------------------------------------------------------------------------- Firefox

interface HeadersDetails {
  url: string;
  method: string;
  statusCode: number;
  tabId: number;
  type: string;
  originUrl?: string;
  documentUrl?: string;
  responseHeaders?: Header[];
}

function firefoxTakeover() {
  const onHeaders = async (d: HeadersDetails): Promise<{ cancel?: boolean }> => {
    if (d.method !== 'GET' || d.statusCode < 200 || d.statusCode >= 300) return {};
    if (!isDownload(d.responseHeaders) || take(bypass, d.url)) return {};
    const filename = dispositionName(header(d.responseHeaders, 'content-disposition'));
    const size = contentLength(d.responseHeaders);
    const pageUrl = d.type === 'main_frame' ? d.originUrl : (d.documentUrl ?? d.originUrl);
    const { rules } = await load();
    if (!decide({ url: d.url, filename, size, pageUrl }, rules).take) return {};
    const mime = header(d.responseHeaders, 'content-type')?.split(';')[0] ?? null;
    const out = await handOver({ url: d.url, filename, size, mime, referer: pageUrl, pageUrl });
    if (out.kind === 'taken') {
      mark(handled, d.url);
      return { cancel: true };
    }
    // The browser downloads it after all: its download item is not taken again.
    mark(bypass, d.url);
    return {};
  };
  const webRequest = browser.webRequest as unknown as {
    onHeadersReceived: { addListener(f: (d: HeadersDetails) => Promise<{ cancel?: boolean }>, filter: object, extra: string[]): void };
  };
  webRequest.onHeadersReceived.addListener(onHeaders, { urls: ['<all_urls>'], types: ['main_frame', 'sub_frame'] }, ['blocking', 'responseHeaders']);

  // Downloads that did not go through a navigation (a download API call, `<a download>`).
  browser.downloads.onCreated.addListener((item: DownloadItem) => void takeStarted(item));
}

// ---------------------------------------------------------------------------------- media

/** Updates of a tab's list, one after the other (each reads and writes the whole list). */
const mediaQueue = new Map<number, Promise<void>>();

function inTab(tabId: number, f: () => Promise<void>) {
  const next = (mediaQueue.get(tabId) ?? Promise.resolve()).then(f, f).catch(() => undefined);
  mediaQueue.set(tabId, next);
}

async function badge(tabId: number, count: number) {
  await browser.action.setBadgeText({ tabId, text: count ? String(count) : '' }).catch(() => undefined);
}

function sniffMedia() {
  void browser.action.setBadgeBackgroundColor({ color: '#2F6FED' }).catch(() => undefined);
  const seen = (d: HeadersDetails) => {
    if (d.tabId < 0 || d.method !== 'GET' || d.statusCode < 200 || d.statusCode >= 300) return;
    const c = classify(d.url, d.responseHeaders);
    if (!c) return;
    inTab(d.tabId, async () => {
      const tab = await browser.tabs.get(d.tabId).catch(() => null);
      const m: Media = { ...c, url: d.url, title: tab?.title ?? null, pageUrl: tab?.url ?? d.documentUrl ?? null, at: Date.now() };
      const list = withMedia(await mediaOf(d.tabId), m);
      await saveMedia(d.tabId, list);
      await badge(d.tabId, list.length);
    });
  };
  const webRequest = browser.webRequest as unknown as {
    onHeadersReceived: { addListener(f: (d: HeadersDetails) => void, filter: object, extra: string[]): void };
  };
  webRequest.onHeadersReceived.addListener(seen, { urls: ['<all_urls>'], types: ['media', 'xmlhttprequest', 'other', 'object'] }, ['responseHeaders']);
  // Another page in the tab: a new list. A closed tab: none.
  browser.tabs.onUpdated.addListener((tabId, info) => {
    if (info.status === 'loading' && info.url) inTab(tabId, async () => {
      await saveMedia(tabId, []);
      await badge(tabId, 0);
    });
  });
  browser.tabs.onRemoved.addListener((tabId) => inTab(tabId, () => saveMedia(tabId, [])));
}

/** A media item the popup asked for: an HLS stream is named after its page. */
function downloadMedia(m: Media) {
  void explicit({ url: m.url, pageUrl: m.pageUrl, referer: m.pageUrl, filename: m.kind === 'hls' ? mediaName(m) : null, size: m.size, mime: m.mime });
}

export default defineBackground(() => {
  menus();
  browser.runtime.onInstalled.addListener(() => {
    menus();
    hello();
  });
  browser.runtime.onStartup.addListener(hello);

  browser.contextMenus.onClicked.addListener((info, tab) => {
    const pageUrl = info.pageUrl ?? tab?.url;
    if (info.menuItemId === 'omni-link' && info.linkUrl) void explicit({ url: info.linkUrl, pageUrl });
    else if (info.menuItemId === 'omni-media' && info.srcUrl) void explicit({ url: info.srcUrl, pageUrl });
    else if (info.menuItemId === 'omni-all' && tab?.id !== undefined) void downloadAllLinks(tab.id, pageUrl);
  });

  browser.commands.onCommand.addListener((command, tab) => {
    if (command === 'download-page' && tab?.url && /^https?:/i.test(tab.url)) void explicit({ url: tab.url, pageUrl: tab.url });
  });

  // Alt+click from the content script: the browser keeps that link. The popup's media list:
  // download one (here, as the popup closes when the app's window opens).
  browser.runtime.onMessage.addListener((msg: unknown) => {
    const m = msg as { type?: string; url?: string; media?: Media };
    if (m?.type === 'bypass' && typeof m.url === 'string') mark(bypass, m.url, 5000);
    if (m?.type === 'download-media' && m.media) downloadMedia(m.media);
  });

  sniffMedia();

  if (import.meta.env.FIREFOX) firefoxTakeover();
  else chromiumTakeover();
});
