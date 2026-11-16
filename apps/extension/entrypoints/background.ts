// The extension's background (docs/development-plan.md §5.2–5.4): takes downloads over, the
// context menu, the "download this page" shortcut, and media sniffing (the videos, audio and
// HLS streams a page plays, listed in the popup with their number on the toolbar button).
//
// Chromium: `downloads.onDeterminingFilename` holds the download while the app is asked; the
// browser goes on (`suggest()`) when the app cannot be reached or the user wants the browser to
// download it, and the download is cancelled when the app takes it.
// The answer to a form (a POST) goes to the app with what the form sent, when that can be sent
// again (`post.ts`); the app tries, and the browser keeps the download when the server does not
// give the file a second time.
// Firefox: a blocking `webRequest.onHeadersReceived` decides from the response headers before a
// download starts; `downloads.onCreated` catches the rest.

import { send } from '../lib/bridge';
import { contentLength, dispositionName, header, isDownload, sentWith, type Header, type Sent } from '../lib/headers';
import { t, tn } from '../lib/i18n';
import { classify, classifyFound, fromShownDocument, mediaKey, mediaName, mediaOf, offers, onPage, pageKey, saveMedia, topPageOf, withMedia, type Media } from '../lib/media';
import { parseMaster } from '../lib/hls';
import { scanPage, type Found } from '../lib/scan';
import { postBody, type RequestBody } from '../lib/post';
import type { PostBody } from '../lib/protocol';
import { decide, extensionOf } from '../lib/rules';
import { load } from '../lib/settings';
import { appDownloadsDash, appSendsForms, basename, handOver, notify, payloadFor, storeOf, type Download } from '../lib/takeover';

/** Links the user asked the browser to keep (Alt+click), and our own re-downloads. */
const bypass = new Map<string, number>();
/** URLs Firefox handed over in webRequest (their download item is not ours to judge again). */
const handled = new Map<string, number>();
/**
 * Answers to a form sent with POST (a download behind a security code, a search export), by
 * address: the file comes only with what the form sent, which the app sends again (`post`).
 * Without it (a form that cannot be sent again, another method than POST) the browser keeps
 * the download: asking for the address with GET would bring a page or a refusal.
 */
const posted = new Map<string, { until: number; post: PostBody | null; sent?: Sent }>();
/** POSTs of pages and frames under way, by request: what they send. */
const sending = new Map<string, { body?: RequestBody | null; type?: string; sent?: Sent }>();

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

/** The download is the answer to a POST (see `posted`): what was sent, null when that cannot be
 *  sent again. Undefined for any other download. */
function fromPost(item: { url?: string; finalUrl?: string }): { post: PostBody | null; sent?: Sent } | undefined {
  let found: { post: PostBody | null; sent?: Sent } | undefined;
  // Both read, so that neither is left behind; the address the answer came from first.
  for (const url of new Set([item.finalUrl, item.url])) {
    const p = url ? posted.get(url) : undefined;
    if (!url || !p) continue;
    posted.delete(url);
    if (p.until > Date.now()) found ??= { post: p.post, sent: p.sent };
  }
  return found;
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

async function downloadAllLinks(tabId: number, pageUrl?: string, storeId?: string | null) {
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
  const items = await Promise.all(files.map((url) => payloadFor({ url, pageUrl, storeId }, false)));
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
  // The answer to a form stays here: once cancelled, the browser could not be given it back.
  if (item.byExtensionId === browser.runtime.id || take(bypass, item.url) || take(handled, item.url) || fromPost(item)) return;
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
  const out = await handOver({ url, filename, size, mime: item.mime, referer: item.referrer, pageUrl: item.referrer, storeId: storeOf(item) });
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
    const form = fromPost(item);
    // A form that cannot be sent again.
    if (form && !form.post) return;
    void (async () => {
      const url = /^https?:/i.test(item.finalUrl) ? item.finalUrl : item.url;
      const size = item.totalBytes > 0 ? item.totalBytes : item.fileSize > 0 ? item.fileSize : null;
      const filename = basename(item.filename);
      const { rules } = await load();
      // A form is for an app that sends it again.
      if (!decide({ url, filename, size, pageUrl: item.referrer }, rules).take || (form && !(await appSendsForms()))) {
        suggest();
        return;
      }
      const out = await handOver({
        url,
        filename,
        size,
        mime: item.mime,
        referer: item.referrer,
        pageUrl: item.referrer,
        post: form?.post,
        sentCookies: form?.sent?.cookies,
        headers: form?.sent?.headers,
      });
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
  requestId: string;
  url: string;
  method: string;
  requestHeaders?: Header[];
  frameId?: number;
  /** Firefox: the frames the request's frame is in, the top one last. */
  frameAncestors?: { url: string }[];
  /** Chromium: `active`, `prerender`, `cached` or `pending_deletion`. */
  documentLifecycle?: string;
  statusCode: number;
  tabId: number;
  type: string;
  originUrl?: string;
  documentUrl?: string;
  /** Firefox: the cookie store of the tab the request comes from. */
  cookieStoreId?: string;
  responseHeaders?: Header[];
}

function firefoxTakeover() {
  const onHeaders = async (d: HeadersDetails): Promise<{ cancel?: boolean }> => {
    if (d.statusCode < 200 || d.statusCode >= 300) return {};
    // The answer to a form: with what the form sent, when that can be sent again.
    const sent = sending.get(d.requestId);
    const post = d.method === 'POST' ? postBody(sent?.body, sent?.type) : null;
    if (d.method !== 'GET' && !post) return {};
    if (!isDownload(d.responseHeaders) || take(bypass, d.url)) return {};
    const filename = dispositionName(header(d.responseHeaders, 'content-disposition'));
    const size = contentLength(d.responseHeaders);
    const pageUrl = d.type === 'main_frame' ? d.originUrl : (d.documentUrl ?? d.originUrl);
    const { rules } = await load();
    if (!decide({ url: d.url, filename, size, pageUrl }, rules).take) return {};
    // A form is for an app that sends it again.
    if (post && !(await appSendsForms())) return {};
    const mime = header(d.responseHeaders, 'content-type')?.split(';')[0] ?? null;
    const out = await handOver({
      url: d.url,
      filename,
      size,
      mime,
      referer: pageUrl,
      pageUrl,
      storeId: storeOf(d),
      post,
      ...(post ? { sentCookies: sent?.sent?.cookies, headers: sent?.sent?.headers } : {}),
    });
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

// ---------------------------------------------------------------------------------- POST

interface RequestDetails {
  requestId: string;
  method: string;
  requestBody?: RequestBody | null;
  requestHeaders?: Header[];
}

/**
 * Notes the pages and frames that came as the answer to a POST (`posted`), with what the POST
 * sent (`sending`: its body, then its `Content-Type`). Their response headers arrive before the
 * browser makes a download of them.
 *
 * Every form a page submits passes here, a login among them: what it sent is held while its
 * request runs, and kept after that (a minute, for the download item) only when the answer's
 * headers say it is a file to download. Any other answer drops it.
 */
function watchPosts() {
  const frames = { urls: ['<all_urls>'], types: ['main_frame', 'sub_frame'] };
  const webRequest = browser.webRequest as unknown as {
    onBeforeRequest: { addListener(f: (d: RequestDetails) => void, filter: object, extra: string[]): void };
    onSendHeaders: { addListener(f: (d: RequestDetails) => void, filter: object, extra: string[]): void };
    onHeadersReceived: { addListener(f: (d: HeadersDetails) => void, filter: object, extra: string[]): void };
    onCompleted: { addListener(f: (d: RequestDetails) => void, filter: object): void };
    onErrorOccurred: { addListener(f: (d: RequestDetails) => void, filter: object): void };
  };
  // Also where a redirect leads: a GET after a POST (303) sends nothing.
  webRequest.onBeforeRequest.addListener(
    (d) => {
      if (d.method === 'POST') sending.set(d.requestId, { body: d.requestBody });
      else sending.delete(d.requestId);
    },
    frames,
    ['requestBody'],
  );
  // The headers as they went out; Chromium shows `Cookie` only with `extraHeaders`, which
  // Firefox (which always shows it) does not know.
  const onSend = (d: RequestDetails) => {
    const s = sending.get(d.requestId);
    if (!s) return;
    s.type = header(d.requestHeaders, 'content-type');
    s.sent = sentWith(d.requestHeaders);
  };
  try {
    webRequest.onSendHeaders.addListener(onSend, frames, ['requestHeaders', 'extraHeaders']);
  } catch {
    webRequest.onSendHeaders.addListener(onSend, frames, ['requestHeaders']);
  }
  webRequest.onHeadersReceived.addListener(
    (d) => {
      if (d.method === 'GET' || d.method === 'HEAD' || d.statusCode < 200 || d.statusCode >= 300) return;
      const s = sending.get(d.requestId);
      // Without what was sent, the browser keeps the download (should it become one).
      const post = d.method === 'POST' && isDownload(d.responseHeaders) ? postBody(s?.body, s?.type) : null;
      posted.set(d.url, { until: Date.now() + 60_000, post, ...(post ? { sent: s?.sent } : {}) });
    },
    frames,
    ['responseHeaders'],
  );
  const done = (d: RequestDetails) => void sending.delete(d.requestId);
  webRequest.onCompleted.addListener(done, frames);
  webRequest.onErrorOccurred.addListener(done, frames);
}

// ---------------------------------------------------------------------------------- media

/** Updates of a tab's list, one after the other (each reads and writes the whole list). */
const mediaQueue = new Map<number, Promise<void>>();
/** Requests that may be media, while under way: what they went out with. */
const mediaSending = new Map<string, Sent>();
/** The `Cookie` header listed media went out with, by `mediaKey`: in memory only, never in
 *  storage (lost when the background sleeps; the download then reads the cookies). */
const mediaCookies = new Map<string, { at: number; cookies: string }>();
/** How long a media item's cookies are used, and how many are kept. */
const MEDIA_COOKIES_FOR = 30 * 60_000;
const MEDIA_COOKIES_MAX = 200;
/** Requests under way that are held at most (a page that never answers). */
const SENDING_MAX = 1000;

function keepCookies(url: string, cookies: string) {
  const key = mediaKey(url);
  mediaCookies.delete(key);
  mediaCookies.set(key, { at: Date.now(), cookies });
  for (const k of mediaCookies.keys()) {
    if (mediaCookies.size <= MEDIA_COOKIES_MAX) break;
    mediaCookies.delete(k);
  }
}

/** HLS playlists read already (or being read), by `mediaKey`. */
const described = new Set<string>();
/** Longer texts are no playlist worth reading. */
const PLAYLIST_MAX = 1024 * 1024;

/**
 * Reads an HLS playlist the tab listed, once: a master's qualities go with its item (the popup
 * offers them), and the playlists it names fold into it. With the cookies the browser holds
 * for it (its `Referer` cannot be set here: a server that wants one only leaves the qualities
 * out, and the app still downloads the best).
 */
async function describeHls(tabId: number, url: string) {
  const key = mediaKey(url);
  if (described.has(key)) return;
  described.add(key);
  let text: string;
  try {
    const r = await fetch(url, { credentials: 'include', signal: AbortSignal.timeout(10_000) });
    if (!r.ok || Number(r.headers.get('content-length') ?? 0) > PLAYLIST_MAX) return;
    text = await r.text();
  } catch {
    described.delete(key);
    return;
  }
  if (text.length > PLAYLIST_MAX) return;
  const master = parseMaster(text, url);
  if (!master) return;
  inTab(tabId, async () => {
    const list = await mediaOf(tabId);
    const i = list.findIndex((m) => mediaKey(m.url) === key);
    if (i < 0) return;
    const next = list.map((m, j) => (j === i ? { ...m, variants: master.variants, parts: master.parts } : m));
    await saveMedia(tabId, next);
    await badge(tabId, offers(next).length);
  });
}

function cookiesOf(url: string): string | null {
  const c = mediaCookies.get(mediaKey(url));
  return c && Date.now() - c.at < MEDIA_COOKIES_FOR ? c.cookies : null;
}

function inTab(tabId: number, f: () => Promise<void>) {
  const next = (mediaQueue.get(tabId) ?? Promise.resolve()).then(f, f).catch(() => undefined);
  mediaQueue.set(tabId, next);
}

async function badge(tabId: number, count: number) {
  await browser.action.setBadgeText({ tabId, text: count ? String(count) : '' }).catch(() => undefined);
}

function sniffMedia() {
  void browser.action.setBadgeBackgroundColor({ color: '#2F6FED' }).catch(() => undefined);
  // What each request went out with (the last hop's, after a redirect).
  const sendingOf = (d: HeadersDetails) => {
    if (d.tabId < 0) return;
    mediaSending.set(d.requestId, sentWith(d.requestHeaders));
    for (const k of mediaSending.keys()) {
      if (mediaSending.size <= SENDING_MAX) break;
      mediaSending.delete(k);
    }
  };
  const seen = (d: HeadersDetails) => {
    const sent = mediaSending.get(d.requestId);
    mediaSending.delete(d.requestId);
    if (d.tabId < 0 || d.method !== 'GET' || d.statusCode < 200 || d.statusCode >= 300) return;
    // A page being prerendered, or one the tab has left.
    if (!fromShownDocument(d)) return;
    const c = classify(d.url, d.responseHeaders);
    if (!c) return;
    if (sent?.cookies) keepCookies(d.url, sent.cookies);
    // The page the request was made for, when it says (Firefox). Chromium's requests do not, and
    // its tab events may come before or after them: the tab is asked when the item is written.
    const page = topPageOf(d);
    inTab(d.tabId, async () => {
      const tab = await browser.tabs.get(d.tabId).catch(() => null);
      // The tab has gone to another page since.
      if (page && tab?.url && pageKey(page) !== pageKey(tab.url)) return;
      const m: Media = {
        ...c,
        url: d.url,
        title: tab?.title ?? null,
        pageUrl: page ?? tab?.url ?? d.documentUrl ?? null,
        storeId: storeOf(d),
        referer: sent?.referer ?? null,
        origin: sent?.headers.Origin ?? null,
        at: Date.now(),
      };
      const list = withMedia(await mediaOf(d.tabId), m);
      await saveMedia(d.tabId, list);
      await badge(d.tabId, offers(list).length);
      if (m.kind === 'hls') void describeHls(d.tabId, m.url);
    });
  };
  const webRequest = browser.webRequest as unknown as {
    onSendHeaders: { addListener(f: (d: HeadersDetails) => void, filter: object, extra: string[]): void };
    onHeadersReceived: { addListener(f: (d: HeadersDetails) => void, filter: object, extra: string[]): void };
    onErrorOccurred: { addListener(f: (d: HeadersDetails) => void, filter: object): void };
  };
  const media = { urls: ['<all_urls>'], types: ['media', 'xmlhttprequest', 'other', 'object'] };
  // Chromium shows `Cookie` only with `extraHeaders`, which Firefox (which always shows it)
  // does not know.
  try {
    webRequest.onSendHeaders.addListener(sendingOf, media, ['requestHeaders', 'extraHeaders']);
  } catch {
    webRequest.onSendHeaders.addListener(sendingOf, media, ['requestHeaders']);
  }
  webRequest.onHeadersReceived.addListener(seen, media, ['responseHeaders']);
  webRequest.onErrorOccurred.addListener((d) => void mediaSending.delete(d.requestId), media);
  // Another page in the tab (a site's own navigation too): the media of that page only. A closed
  // tab: none.
  browser.tabs.onUpdated.addListener((tabId, info) => {
    if (!info.url) return;
    const url = info.url;
    inTab(tabId, async () => {
      const list = await mediaOf(tabId);
      const kept = onPage(list, url);
      if (kept.length === list.length) return;
      await saveMedia(tabId, kept);
      await badge(tabId, offers(kept).length);
    });
  });
  browser.tabs.onRemoved.addListener((tabId) => inTab(tabId, () => saveMedia(tabId, [])));
}

/** A media item the popup asked for, under the name the list shows; a DASH picture with its
 *  sound, which the app joins into one MP4. Sent as its request went out: its `Referer`,
 *  `Origin` and (while the background holds them) `Cookie`; else the page and its cookies. */
function mediaDownload(m: Media, audio?: Media | null, name?: string, hlsVariant?: string | null): Download {
  const size = m.size != null && audio?.size != null ? m.size + audio.size : m.size;
  return {
    url: m.url,
    audioUrl: audio?.url ?? null,
    pageUrl: m.pageUrl,
    referer: m.referer || m.pageUrl,
    filename: name || mediaName(m),
    size,
    mime: m.mime,
    storeId: m.storeId,
    sentCookies: cookiesOf(m.url),
    headers: m.origin ? { Origin: m.origin } : undefined,
    // One of its own qualities only.
    hlsVariant: hlsVariant && m.variants?.some((v) => v.url === hlsVariant) ? hlsVariant : null,
  };
}

/**
 * Looks at the tab's page (every frame) when the popup opens, for media the sniffing did not see,
 * and adds them to its list. Nothing for a page the extension may not script (the browser's own,
 * a store's).
 */
async function scanTab(tabId: number) {
  let found: Found[];
  try {
    const results = await browser.scripting.executeScript({ target: { tabId, allFrames: true }, func: scanPage });
    found = results.flatMap((r) => (r.result as Found[] | undefined) ?? []);
  } catch {
    return;
  }
  if (!found.length) return;
  inTab(tabId, async () => {
    const tab = await browser.tabs.get(tabId).catch(() => null);
    if (!tab?.url) return;
    let list = await mediaOf(tabId);
    const before = list.length;
    for (const f of found) {
      // What the sniffing listed stays as it is (with what its request went out with).
      if (list.some((x) => mediaKey(x.url) === mediaKey(f.url))) continue;
      const c = classifyFound(f);
      if (!c) continue;
      // A frame's media went out with that frame as the referrer (or its origin, across sites).
      const m: Media = { ...c, url: f.url, title: tab.title ?? null, pageUrl: tab.url, storeId: storeOf(tab), referer: f.top ? null : f.frameUrl, at: Date.now() };
      list = withMedia(list, m);
    }
    if (list.length === before) return;
    await saveMedia(tabId, list);
    await badge(tabId, offers(list).length);
    for (const m of list.slice(before)) if (m.kind === 'hls') void describeHls(tabId, m.url);
  });
}

interface MediaChoice {
  media: Media;
  audio?: Media | null;
  name?: string;
  hlsVariant?: string | null;
}

/** Several media the popup's list chose: each starts without the quick-download window, one
 *  after the other; one notification says how many went. */
async function downloadMediaBatch(items: MediaChoice[]) {
  if (items.some((i) => i.media.kind === 'mpd') && !(await dashAllowed())) items = items.filter((i) => i.media.kind !== 'mpd');
  let taken = 0;
  for (const i of items) {
    const out = await handOver(mediaDownload(i.media, i.audio, i.name, i.hlsVariant), true, false);
    if (out.kind === 'unreachable') {
      void notify(t('notifyUnreachable'));
      return;
    }
    if (out.kind === 'taken') taken++;
  }
  if (taken) void notify(tn('notifyBatch', taken));
}

/** Whether the app downloads a DASH manifest; says so when it does not (an older app would
 *  save the manifest itself). */
async function dashAllowed(): Promise<boolean> {
  if (await appDownloadsDash()) return true;
  void notify(t('notifyDashNeedsApp'));
  return false;
}

/** One media item the popup chose, through the quick-download window. */
async function downloadOneMedia(i: MediaChoice) {
  if (i.media.kind === 'mpd' && !(await dashAllowed())) return;
  await explicit(mediaDownload(i.media, i.audio, i.name, i.hlsVariant));
}

/** Media downloads given together, one batch after the other. */
let mediaBatches: Promise<void> = Promise.resolve();

export default defineBackground(() => {
  menus();
  browser.runtime.onInstalled.addListener(() => {
    menus();
    hello();
  });
  browser.runtime.onStartup.addListener(hello);

  browser.contextMenus.onClicked.addListener((info, tab) => {
    const pageUrl = info.pageUrl ?? tab?.url;
    const storeId = storeOf(tab);
    if (info.menuItemId === 'omni-link' && info.linkUrl) void explicit({ url: info.linkUrl, pageUrl, storeId });
    else if (info.menuItemId === 'omni-media' && info.srcUrl) void explicit({ url: info.srcUrl, pageUrl, storeId });
    else if (info.menuItemId === 'omni-all' && tab?.id !== undefined) void downloadAllLinks(tab.id, pageUrl, storeId);
  });

  browser.commands.onCommand.addListener((command, tab) => {
    if (command === 'download-page' && tab?.url && /^https?:/i.test(tab.url)) void explicit({ url: tab.url, pageUrl: tab.url, storeId: storeOf(tab) });
  });

  // Alt+click from the content script: the browser keeps that link. The popup's media list:
  // download one (here, as the popup closes when the app's window opens).
  browser.runtime.onMessage.addListener((msg: unknown) => {
    const m = msg as { type?: string; url?: string; media?: Media; audio?: Media | null; name?: string; hlsVariant?: string | null; items?: MediaChoice[]; tabId?: number };
    if (m?.type === 'scan-media' && typeof m.tabId === 'number') void scanTab(m.tabId);
    if (m?.type === 'bypass' && typeof m.url === 'string') mark(bypass, m.url, 5000);
    if (m?.type === 'download-media' && m.media) void downloadOneMedia({ media: m.media, audio: m.audio, name: m.name, hlsVariant: m.hlsVariant });
    if (m?.type === 'download-media-batch' && Array.isArray(m.items)) {
      const items = m.items.filter((i) => i?.media?.url);
      mediaBatches = mediaBatches.then(() => downloadMediaBatch(items)).catch(() => undefined);
    }
  });

  sniffMedia();
  watchPosts();

  if (import.meta.env.FIREFOX) firefoxTakeover();
  else chromiumTakeover();
});
