// What a form sent, for a download that is the answer to a POST (a datasheet behind a security
// code, an export): the app sends the same request again, so it needs the body as it went out.
//
// `webRequest` shows a body either parsed (`formData`: names and values) or as bytes (`raw`).
// Parsed fields are written again the way a form of type `application/x-www-form-urlencoded`
// writes them; bytes go as they are. What cannot be written again faithfully (files, a
// `multipart/form-data` form, text that did not survive the parsing, a large body) gives
// nothing: the browser then keeps that download.

import type { PostBody } from './protocol';

/** A request body as `webRequest.onBeforeRequest` gives it. */
export interface RequestBody {
  error?: string;
  formData?: Record<string, unknown[]>;
  raw?: { bytes?: ArrayBuffer; file?: string }[];
}

/** Larger bodies are not a form's fields. */
export const MAX_POST = 256 * 1024;

/** What else of the browser's request a form is sent again with. */
export interface Sent {
  /** The `Cookie` header as it went out. The cookies read after the answer may differ: a site
   *  that ties a security code to a cookie may set a new one with the file. */
  cookies: string | null;
  /** `Origin`, which servers check on a form against forgery. */
  headers: Record<string, string>;
}

/** The `Cookie` and `Origin` of a request's headers, as `webRequest.onSendHeaders` gives them. */
export function sentWith(requestHeaders: { name: string; value?: string }[] | undefined): Sent {
  const get = (n: string) => requestHeaders?.find((h) => h.name.toLowerCase() === n)?.value?.trim() || null;
  const origin = get('origin');
  return { cookies: get('cookie'), headers: origin && origin !== 'null' ? { Origin: origin } : {} };
}

const URLENCODED = 'application/x-www-form-urlencoded';

function base64(bytes: Uint8Array): string {
  let s = '';
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(s);
}

/**
 * The body of a POST with that `Content-Type`, to send again; null when it cannot be (see
 * above).
 */
export function postBody(body: RequestBody | null | undefined, contentType: string | null | undefined): PostBody | null {
  const type = contentType?.trim();
  if (!type || body?.error) return null;
  if (body?.raw?.length) {
    const parts: Uint8Array[] = [];
    let size = 0;
    for (const part of body.raw) {
      if (part.file !== undefined || !part.bytes) return null;
      size += part.bytes.byteLength;
      if (size > MAX_POST) return null;
      parts.push(new Uint8Array(part.bytes));
    }
    const all = new Uint8Array(size);
    let at = 0;
    for (const p of parts) {
      all.set(p, at);
      at += p.length;
    }
    return { content_type: type, body: base64(all) };
  }
  if (body?.formData) {
    // A multipart form may carry files, which show here as their names only.
    if (type.split(';')[0].trim().toLowerCase() !== URLENCODED) return null;
    const fields = new URLSearchParams();
    for (const [name, values] of Object.entries(body.formData)) {
      for (const value of values) {
        // Text that was not UTF-8 came out of the parsing as replacement characters.
        if (typeof value !== 'string' || value.includes('�') || name.includes('�')) return null;
        fields.append(name, value);
      }
    }
    const text = fields.toString();
    if (text.length > MAX_POST) return null;
    // Percent-encoded: ASCII only.
    return { content_type: type, body: btoa(text) };
  }
  // A POST without a body.
  return body ? null : { content_type: type, body: '' };
}
