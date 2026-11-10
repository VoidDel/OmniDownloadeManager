import { describe, expect, it } from 'vitest';

import type { BrowserState } from '../../lib/types';
import { browserState } from './BrowserSettings';

const b = (o: Partial<BrowserState>): BrowserState => ({ id: 'edge', name: 'Edge', detected: true, registered: true, ours: true, ...o });
const earlier = Date.now() - 3 * 86_400_000;

describe('browserState', () => {
  it('is not installed when the browser is missing, whatever the registration says', () => {
    // The app registers every browser, so a registration alone says nothing.
    expect(browserState(b({ detected: false }))).toEqual(['off', '未安装']);
  });

  it('waits for the extension until it is heard from', () => {
    expect(browserState(b({}))).toEqual(['off', '等待扩展连接']);
  });

  it('says when the extension was last in touch, and that it is connected in this run', () => {
    expect(browserState(b({}), undefined, earlier)[0]).toBe('ok');
    expect(browserState(b({}), undefined, earlier)[1]).toMatch(/^上次连接 · /);
    expect(browserState(b({}), Date.now(), Date.now())[1]).toMatch(/^已连接 · /);
    // A portable browser the system does not know: its extension proves it is there.
    expect(browserState(b({ detected: false }), Date.now(), Date.now())[1]).toMatch(/^已连接 · /);
  });

  it('warns about a registration of another installation, or none', () => {
    expect(browserState(b({ ours: false }))).toEqual(['warn', '指向另一个安装']);
    expect(browserState(b({ ours: false, registered: false }))).toEqual(['warn', '未注册']);
  });
});
