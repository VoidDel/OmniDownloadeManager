// What the extension keeps (browser.storage.local): the takeover rules and the HTTP channel's
// pairing.

import { DEFAULT_RULES, type Rules } from './rules';

export interface Pairing {
  port: number;
  token: string;
}

/** How to reach the app. */
export type Transport = 'auto' | 'native' | 'http';

export interface Stored {
  rules: Rules;
  /** The HTTP channel (Settings › Browser › pairing code in the app). */
  pairing: Pairing | null;
  /** `auto`: Native Messaging, then the paired HTTP channel. */
  transport: Transport;
}

const DEFAULTS: Stored = { rules: DEFAULT_RULES, pairing: null, transport: 'auto' };

export async function load(): Promise<Stored> {
  const got = (await browser.storage.local.get(['rules', 'pairing', 'transport'])) as Partial<Stored>;
  return {
    rules: { ...DEFAULT_RULES, ...(got.rules ?? {}) },
    pairing: got.pairing ?? null,
    transport: got.transport ?? 'auto',
  };
}

export async function save(patch: Partial<Stored>): Promise<void> {
  await browser.storage.local.set(patch);
}

export { DEFAULTS };

/** `omnidl:<port>:<token>` → the pairing; `null` when it does not look like one. */
export function parsePairing(code: string): Pairing | null {
  const m = /^omnidl:(\d{1,5}):([0-9a-f]{32,128})$/i.exec(code.trim());
  if (!m) return null;
  const port = Number(m[1]);
  return port > 0 && port < 65536 ? { port, token: m[2] } : null;
}
