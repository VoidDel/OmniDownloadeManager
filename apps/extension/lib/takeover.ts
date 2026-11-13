// Handing one download to the app with what the browser would have sent: cookies for the URL,
// the page as referer, the browser's user agent.

import { BridgeError, send } from './bridge';
import { t } from './i18n';
import type { AddDownload, AddResult, Response } from './protocol';
import { cookieHeader } from './rules';
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

/** The `Cookie` header the browser would send to `url` from a tab of that cookie store (none
 *  when there are no cookies). */
export async function cookiesFor(url: string, storeId?: string | null): Promise<string | null> {
  const store = storeId ? { storeId } : {};
  try {
    return cookieHeader(await browser.cookies.getAll({ url, ...store })) || null;
  } catch {
    try {
      // Firefox with first-party isolation wants the partition named.
      const all = await browser.cookies.getAll({ url, firstPartyDomain: null, ...store } as unknown as Parameters<typeof browser.cookies.getAll>[0]);
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
    cookies: await cookiesFor(d.url, d.storeId),
    silent,
    ...(d.audioUrl ? { audio_url: d.audioUrl } : {}),
  };
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

/** Gives `d` to the app. */
export async function handOver(d: Download, silent?: boolean): Promise<Outcome> {
  const { rules } = await load();
  let r: Response<AddResult>;
  try {
    r = await send<AddResult>({ type: 'add_download', payload: await payloadFor(d, silent ?? rules.silent) });
  } catch (e) {
    return { kind: 'unreachable', reason: e instanceof BridgeError ? e.kind : String(e) };
  }
  if (!r.ok || !r.result) return { kind: 'unreachable', reason: r.error?.message ?? 'error' };
  if (r.result.action === 'browser') return { kind: 'browser' };
  if (!r.result.asked && (r.result.action === 'started' || r.result.action === 'queued')) {
    const name = basename(d.filename) ?? basename(new URL(d.url).pathname) ?? d.url;
    void notify(t(r.result.action === 'started' ? 'notifyStarted' : 'notifyQueued', name));
  }
  return { kind: 'taken', result: r.result };
}
