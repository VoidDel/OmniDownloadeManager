// Handing one download to the app with what the browser would have sent: cookies for the URL,
// the page as referer, the browser's user agent.

import { BridgeError, send } from './bridge';
import { t } from './i18n';
import type { AddDownload, AddResult, Pong, PostBody, Response } from './protocol';
import { cookieHeader, sendableCookies } from './rules';
import { load } from './settings';

export interface Download {
  url: string;
  filename?: string | null;
  size?: number | null;
  mime?: string | null;
  referer?: string | null;
  /** The page it started from. */
  pageUrl?: string | null;
  /** The sound of a video whose `url` has only the picture (DASH): the app joins them. */
  audioUrl?: string | null;
  /** The cookie store of the tab it comes from (see `storeOf`); the default one when not given. */
  storeId?: string | null;
  /** The download is the answer to a form: what the form sent (see `post.ts`). */
  post?: PostBody | null;
  /** The `Cookie` header the form went out with: used instead of the cookies read now. */
  sentCookies?: string | null;
  /** Headers to send as the browser did (a form's `Origin`). */
  headers?: Record<string, string>;
  /** Of a master HLS playlist, the quality chosen (its playlist's address). */
  hlsVariant?: string | null;
}

/** `taken`: the app has it (or the user dropped it there). `browser`: the browser should go on. */
export type Outcome = { kind: 'taken'; result: AddResult } | { kind: 'browser' } | { kind: 'unreachable'; reason: string };

/**
 * The cookie store a tab, a request or a download item of Firefox belongs to: a container tab and
 * a private window each have cookies of their own, and `cookies.getAll` reads the default store
 * unless told. Chromium's objects do not say (null: the default store).
 */
export function storeOf(o: unknown): string | null {
  const id = (o as { cookieStoreId?: unknown } | null | undefined)?.cookieStoreId;
  return typeof id === 'string' && id ? id : null;
}

type GetAll = Parameters<typeof browser.cookies.getAll>[0];

/** The `Cookie` header the browser would send to `url`, from `pageUrl` in a tab of that cookie
 *  store (none when there are no cookies). */
export async function cookiesFor(url: string, storeId?: string | null, pageUrl?: string | null): Promise<string | null> {
  const store = storeId ? { storeId } : {};
  try {
    // Without a partition key only unpartitioned cookies come back; an empty one brings those
    // of every partition too, and `sendableCookies` keeps the ones this request would carry.
    const all = await browser.cookies.getAll({ url, partitionKey: {}, ...store } as GetAll);
    return cookieHeader(sendableCookies(all, url, pageUrl)) || null;
  } catch {
    // A browser without partitioned cookies.
  }
  try {
    return cookieHeader(await browser.cookies.getAll({ url, ...store })) || null;
  } catch {
    try {
      // Firefox with first-party isolation wants the partition named.
      const all = await browser.cookies.getAll({ url, firstPartyDomain: null, ...store } as unknown as GetAll);
      return cookieHeader(all) || null;
    } catch {
      return null;
    }
  }
}

export function basename(path: string | null | undefined): string | null {
  const b = path?.split(/[\\/]/).pop()?.trim();
  return b || null;
}

export async function payloadFor(d: Download, silent: boolean): Promise<AddDownload> {
  return {
    url: d.url,
    filename: basename(d.filename),
    size: d.size ?? null,
    mime: d.mime ?? null,
    referer: d.referer || d.pageUrl || null,
    tab_url: d.pageUrl ?? null,
    user_agent: navigator.userAgent,
    cookies: d.sentCookies ?? (await cookiesFor(d.url, d.storeId, d.pageUrl ?? d.referer)),
    silent,
    ...(d.audioUrl ? { audio_url: d.audioUrl } : {}),
    ...(d.post ? { post: d.post } : {}),
    ...(d.headers && Object.keys(d.headers).length ? { headers: d.headers } : {}),
    ...(d.hlsVariant ? { hls_variant: d.hlsVariant } : {}),
  };
}

/** What the app that is running understands beyond the first protocol (`Pong.capabilities`);
 *  none when it is not running (asking does not start it). */
export async function appCapabilities(): Promise<string[]> {
  try {
    const r = await send<Pong>({ type: 'ping' });
    return (r.ok && r.result?.capabilities) || [];
  } catch {
    return [];
  }
}

/**
 * The app that is running sends forms again (`AddDownload.post`). An older one would ignore
 * what the form sent, fetch the address with a GET and save the page it gets: it is not given
 * those downloads. Neither is an app that is not running.
 */
export async function appSendsForms(): Promise<boolean> {
  return (await appCapabilities()).includes('post');
}

/** The app that is running downloads DASH manifests (an older one would save the manifest). */
export async function appDownloadsDash(): Promise<boolean> {
  return (await appCapabilities()).includes('dash');
}

export async function notify(message: string): Promise<void> {
  try {
    await browser.notifications.create({
      type: 'basic',
      iconUrl: browser.runtime.getURL('/icon/128.png'),
      title: 'Omni Download',
      message,
    });
  } catch {
    // Notifications are a courtesy.
  }
}

/** Gives `d` to the app. `announce`: a notification says it started when the user was not
 *  asked (off for downloads given together, which say so once). */
export async function handOver(d: Download, silent?: boolean, announce = true): Promise<Outcome> {
  const { rules } = await load();
  let r: Response<AddResult>;
  try {
    r = await send<AddResult>({ type: 'add_download', payload: await payloadFor(d, silent ?? rules.silent) });
  } catch (e) {
    return { kind: 'unreachable', reason: e instanceof BridgeError ? e.kind : String(e) };
  }
  if (!r.ok || !r.result) return { kind: 'unreachable', reason: r.error?.message ?? 'error' };
  if (r.result.action === 'browser') return { kind: 'browser' };
  if (announce && !r.result.asked && (r.result.action === 'started' || r.result.action === 'queued')) {
    const name = basename(d.filename) ?? basename(new URL(d.url).pathname) ?? d.url;
    void notify(t(r.result.action === 'started' ? 'notifyStarted' : 'notifyQueued', name));
  }
  return { kind: 'taken', result: r.result };
}
