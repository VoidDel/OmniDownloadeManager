// The web site the tests download from: files of any name whose bytes follow from their
// position (so any range can be served and checked), and the same behind a login.

import { createHash } from 'node:crypto';
import fs from 'node:fs';
import http from 'node:http';
import type { AddressInfo } from 'node:net';

export const SIZE = 3 * 1024 * 1024 + 17;
export const ACCOUNT = { user: 'bob', password: 's3cret' };

const byteAt = (i: number) => (i * 31 + (i >> 8)) & 0xff;
const md5 = (s: string) => createHash('md5').update(s).digest('hex');

export interface Site {
  /** `/files/<name>`: a file of `SIZE` bytes. `/private/<name>`: the same for `ACCOUNT` (Digest). */
  url(path: string): string;
  /** `/private` requests that carried a correct login. */
  logins(): number;
  close(): Promise<void>;
}

/** Whether `file` is what the site serves. */
export function isSiteFile(file: string): boolean {
  if (!fs.existsSync(file)) return false;
  const data = fs.readFileSync(file);
  if (data.length !== SIZE) return false;
  for (let i = 0; i < SIZE; i += 4093) if (data[i] !== byteAt(i)) return false;
  return true;
}

export async function startSite(): Promise<Site> {
  const realm = 'tests';
  const nonce = md5(String(Math.random()));
  let logins = 0;
  const loggedIn = (req: http.IncomingMessage): boolean => {
    const sent = req.headers.authorization ?? '';
    if (!sent.startsWith('Digest ')) return false;
    const get = (key: string) => new RegExp(`${key}="?([^",]*)"?`).exec(sent)?.[1] ?? '';
    const ha1 = md5(`${ACCOUNT.user}:${realm}:${ACCOUNT.password}`);
    const ha2 = md5(`${req.method}:${req.url}`);
    const response = md5(`${ha1}:${nonce}:${get('nc')}:${get('cnonce')}:auth:${ha2}`);
    return get('username') === ACCOUNT.user && get('nonce') === nonce && get('uri') === req.url && get('response') === response;
  };
  const server = http.createServer((req, res) => {
    const path = (req.url ?? '').split('?')[0];
    const [, area, name] = /^\/(files|private)\/([^/]+)$/.exec(path) ?? [];
    if (!area) {
      res.statusCode = 404;
      res.end();
      return;
    }
    if (area === 'private') {
      if (!loggedIn(req)) {
        res.writeHead(401, { 'WWW-Authenticate': `Digest realm="${realm}", nonce="${nonce}", qop="auth", algorithm=MD5` });
        res.end();
        return;
      }
      logins++;
    }
    const m = /^bytes=(\d+)-(\d*)$/.exec(req.headers.range ?? '');
    const start = m ? Number(m[1]) : 0;
    const end = m && m[2] ? Math.min(Number(m[2]), SIZE - 1) : SIZE - 1;
    res.writeHead(m ? 206 : 200, {
      'Content-Type': 'application/octet-stream',
      'Content-Length': String(end - start + 1),
      'Content-Disposition': `attachment; filename="${decodeURIComponent(name)}"`,
      'Accept-Ranges': 'bytes',
      ETag: '"v1"',
      ...(m ? { 'Content-Range': `bytes ${start}-${end}/${SIZE}` } : {}),
    });
    if (req.method === 'HEAD') {
      res.end();
      return;
    }
    const body = Buffer.alloc(end - start + 1);
    for (let i = 0; i < body.length; i++) body[i] = byteAt(start + i);
    res.end(body);
  });
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  const { port } = server.address() as AddressInfo;
  return {
    url: (path) => `http://127.0.0.1:${port}${path}`,
    logins: () => logins,
    close: () =>
      new Promise((resolve) => {
        server.closeAllConnections();
        server.close(() => resolve());
      }),
  };
}
