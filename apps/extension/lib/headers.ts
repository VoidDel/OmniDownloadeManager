// Response headers, for Firefox, which decides about a download from them (webRequest) instead of
// from a download item: is this response a download, and what is it called?

export interface Header {
  name: string;
  value?: string;
}

/** Types the browser saves instead of showing (even without `Content-Disposition: attachment`). */
const DOWNLOAD_TYPES = [
  'application/octet-stream',
  'application/zip',
  'application/x-zip-compressed',
  'application/vnd.rar',
  'application/x-rar-compressed',
  'application/x-7z-compressed',
  'application/gzip',
  'application/x-gzip',
  'application/x-tar',
  'application/x-bzip2',
  'application/x-xz',
  'application/zstd',
  'application/x-msdownload',
  'application/x-msi',
  'application/vnd.microsoft.portable-executable',
  'application/x-iso9660-image',
  'application/x-apple-diskimage',
  'application/vnd.android.package-archive',
  'application/vnd.debian.binary-package',
  'application/x-rpm',
];

export function header(headers: Header[] | undefined, name: string): string | undefined {
  const n = name.toLowerCase();
  return headers?.find((h) => h.name.toLowerCase() === n)?.value;
}

/** A response the browser would download rather than display. */
export function isDownload(headers: Header[] | undefined): boolean {
  const cd = header(headers, 'content-disposition')?.trim().toLowerCase();
  if (cd?.startsWith('attachment')) return true;
  const type = header(headers, 'content-type')?.split(';')[0]?.trim().toLowerCase();
  return !!type && DOWNLOAD_TYPES.includes(type);
}

/** The file name in `Content-Disposition` (`filename*=UTF-8''…` first, then `filename=`). */
export function dispositionName(value: string | undefined): string | null {
  if (!value) return null;
  const ext = /filename\*\s*=\s*([^']*)'[^']*'([^;]+)/i.exec(value);
  if (ext) {
    try {
      const raw = ext[2].trim().replace(/^"|"$/g, '');
      if (/^utf-?8$/i.test(ext[1].trim()) || !ext[1].trim()) return clean(decodeURIComponent(raw));
      // ISO-8859-1: each %XX byte is a code point.
      return clean(raw.replace(/%([0-9a-f]{2})/gi, (_, h: string) => String.fromCharCode(parseInt(h, 16))));
    } catch {
      // fall through to filename=
    }
  }
  const plain = /filename\s*=\s*("(?:[^"\\]|\\.)*"|[^;]+)/i.exec(value);
  if (!plain) return null;
  let v = plain[1].trim();
  if (v.startsWith('"')) v = v.slice(1, -1).replace(/\\(.)/g, '$1');
  if (/%[0-9a-f]{2}/i.test(v)) {
    try {
      v = decodeURIComponent(v);
    } catch {
      // keep it as it is
    }
  }
  return clean(v);
}

function clean(name: string): string | null {
  const base = name.split(/[\\/]/).pop()?.trim() ?? '';
  return base || null;
}

export function contentLength(headers: Header[] | undefined): number | null {
  const v = header(headers, 'content-length');
  const n = v ? Number(v) : NaN;
  return Number.isFinite(n) && n >= 0 ? n : null;
}
