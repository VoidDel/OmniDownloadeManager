// Checks the UI translations (locales/<lang>/app.json): every language has exactly the keys of
// zh-CN (the source), with the same ICU placeholders, and every key the code uses exists (the
// window's, and the app's own for the notification-area menu and notifications).
//
//   node scripts/check-locales.mjs

import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const LANGS = ['zh-CN', 'zh-TW', 'en-US', 'ja-JP'];
const load = (l) => JSON.parse(readFileSync(join(root, 'locales', l, 'app.json'), 'utf8'));

const flat = (o, p = '', out = new Map()) => {
  for (const [k, v] of Object.entries(o)) {
    const key = p ? `${p}.${k}` : k;
    if (v && typeof v === 'object') flat(v, key, out);
    else out.set(key, String(v));
  }
  return out;
};
// Top-level ICU arguments: `{name}`, `{count, plural, ...}` (names only, not plural branches).
const args = (s) => {
  const names = new Set();
  let depth = 0;
  for (let i = 0; i < s.length; i++) {
    if (s[i] === '{') {
      if (depth === 0) {
        const m = /^\{\s*([A-Za-z_][\w]*)/.exec(s.slice(i));
        if (m) names.add(m[1]);
      }
      depth++;
    } else if (s[i] === '}') depth--;
  }
  return [...names].sort().join(',');
};

const source = flat(load('zh-CN'));
let problems = 0;
const report = (msg) => {
  problems++;
  console.log(msg);
};

for (const lang of LANGS.slice(1)) {
  const t = flat(load(lang));
  for (const [k, v] of source) {
    if (!t.has(k)) report(`${lang}: missing ${k}`);
    else if (args(t.get(k)) !== args(v)) report(`${lang}: ${k} has {${args(t.get(k))}}, zh-CN has {${args(v)}}`);
  }
  for (const k of t.keys()) if (!source.has(k)) report(`${lang}: extra ${k}`);
}

// Keys in the code: t('a.b') / tr('a.b') / i18n.t('a.b') with a literal key (`tr` where `t` already
// names a task).
const walk = (d) => readdirSync(d).flatMap((f) => (statSync(join(d, f)).isDirectory() ? walk(join(d, f)) : [join(d, f)]));
const files = walk(join(root, 'apps/desktop/src')).filter((f) => /\.(ts|tsx)$/.test(f) && !f.includes('.test.'));
for (const f of files) {
  const text = readFileSync(f, 'utf8');
  for (const m of text.matchAll(/\btr?\(\s*'([a-zA-Z][\w.]*)'/g)) {
    if (!source.has(m[1]) && ![...source.keys()].some((k) => k.startsWith(`${m[1]}.`))) report(`${f.slice(root.length)}: unknown key ${m[1]}`);
  }
}

// The app (src-tauri/src/i18n.rs) names keys as string literals: "tray.show", "notify.done".
const rust = walk(join(root, 'apps/desktop/src-tauri/src')).filter((f) => f.endsWith('.rs'));
const NAMESPACES = [...new Set([...source.keys()].map((k) => k.split('.')[0]))].join('|');
for (const f of rust) {
  const text = readFileSync(f, 'utf8');
  for (const m of text.matchAll(new RegExp(`"((?:${NAMESPACES})\\.[a-zA-Z][\\w.]*)"`, 'g'))) {
    // File names look alike ("settings.toml").
    if (/\.(toml|json|db|log|txt)$/.test(m[1])) continue;
    if (!source.has(m[1])) report(`${f.slice(root.length)}: unknown key ${m[1]}`);
  }
}

console.log(problems ? `${problems} problem(s)` : `ok: ${source.size} keys in ${LANGS.length} languages`);
process.exit(problems ? 1 : 0);
