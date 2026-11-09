// Talking to the app (docs/development-plan.md §5.5, ADR-002): Native Messaging, one message per
// connection (the host starts per message, the service worker may sleep in between); when it is
// not available, the HTTP channel if the user paired it.

import { HOST_NAME, type Request, type RequestBody, type Response } from './protocol';
import { load } from './settings';

/** Why the app could not be reached. */
export type Unreachable = 'no_host' | 'not_running' | 'unpaired' | 'error';

export class BridgeError extends Error {
  constructor(
    readonly kind: Unreachable,
    message: string,
  ) {
    super(message);
  }
}

/** `chrome`, `edge`, `brave`, `chromium` or `firefox`. */
export function browserName(): string {
  const ua = navigator.userAgent;
  if (/Firefox\//.test(ua)) return 'firefox';
  if (/Edg\//.test(ua)) return 'edge';
  if ((navigator as Navigator & { brave?: unknown }).brave) return 'brave';
  const brands = (navigator as Navigator & { userAgentData?: { brands?: { brand: string }[] } }).userAgentData?.brands ?? [];
  return brands.some((b) => b.brand === 'Google Chrome') ? 'chrome' : 'chromium';
}

function id(): string {
  return crypto.randomUUID().slice(0, 8);
}

async function viaNative(req: Request): Promise<Response> {
  return (await browser.runtime.sendNativeMessage(HOST_NAME, req)) as Response;
}

async function viaHttp(req: Request, port: number, token: string): Promise<Response> {
  const r = await fetch(`http://127.0.0.1:${port}/v1/request`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
    body: JSON.stringify(req),
  });
  const body = (await r.json()) as Response;
  if (r.status === 401 || r.status === 403) throw new BridgeError('unpaired', body.error?.message ?? 'pairing rejected');
  return body;
}

/**
 * Sends a request. Rejects with a `BridgeError` when the app cannot be reached; a reply the app
 * gave (even an error) resolves.
 */
export async function send<T = unknown>(body: RequestBody): Promise<Response<T>> {
  const req = { ...body, id: id(), browser: browserName() } as Request;
  const { pairing, transport } = await load();
  let nativeError = '';
  if (transport !== 'http') {
    try {
      const r = await viaNative(req);
      // The host answers for an app that is not running.
      if (!r.ok && r.error?.code === 'app_not_running' && pairing && transport === 'auto') {
        nativeError = r.error.message;
      } else if (!r.ok && r.error?.code === 'app_not_running') {
        throw new BridgeError('not_running', r.error.message);
      } else {
        return r as Response<T>;
      }
    } catch (e) {
      if (e instanceof BridgeError) throw e;
      nativeError = String((e as Error)?.message ?? e);
    }
  }
  if (transport !== 'native' && pairing) {
    try {
      return (await viaHttp(req, pairing.port, pairing.token)) as Response<T>;
    } catch (e) {
      if (e instanceof BridgeError) throw e;
      throw new BridgeError('not_running', `${nativeError ? `${nativeError}; ` : ''}HTTP: ${String((e as Error)?.message ?? e)}`);
    }
  }
  const noHost = /not found|not exist|no such native application|access to the specified native messaging host is forbidden/i.test(nativeError);
  throw new BridgeError(noHost ? 'no_host' : transport === 'http' ? 'unpaired' : 'error', nativeError || 'not paired');
}
