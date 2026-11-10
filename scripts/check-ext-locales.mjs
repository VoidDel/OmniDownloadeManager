// Checks the browser extension's texts (apps/extension/public/_locales/<lang>/messages.json):
// every language has exactly the keys of zh_CN with the same `$1` placeholders, every key the
// code or the manifest uses exists, and none is left unused.
//
//   node scripts/check-ext-locales.mjs

import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ext = fileURLToPath(new URL('../apps/extension', import.meta.url));
const LANGS = ['zh_CN', 'zh_TW', 'en', 'ja'];
const load = (l) => JSON.parse(readFileSync(join(ext, 'public', '_locales', l, 'messages.json'), 'utf8'));
const placeholders = (s) => [...new Set(s.match(/\$\d/g) ?? [])].sort().join(' ');

const problems = [];
const source = load(LANGS[0]);
for (const lang of LANGS) {
  const m = load(lang);
  for (const key of Object.keys(source)) {
    if (!m[key]?.message) problems.push(`${lang}: ${key} is missing`);
    else if (placeholders(m[key].message) !== placeholders(source[key].message)) problems.push(`${lang}: ${key} has other placeholders than ${LANGS[0]}`);
  }
  for (const key of Object.keys(m)) if (!(key in source)) problems.push(`${lang}: ${key} is not in ${LANGS[0]}`);
  // The stores cut a description off at 132 characters.
  if ((m.extDescription?.message.length ?? 0) > 132) problems.push(`${lang}: extDescription is longer than 132 characters`);
}

// Keys in use: t('key'), tn('key', …) (→ keyOne / keyOther), `${X}Hint` families and status keys
// through their tables, and __MSG_key__ in the manifest.
const files = [];
const walk = (dir) => {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) walk(p);
    else if (/\.(ts|tsx)$/.test(name) && !/\.test\./.test(name)) files.push(p);
  }
};
walk(join(ext, 'entrypoints'));
walk(join(ext, 'lib'));
files.push(join(ext, 'wxt.config.ts'));
const used = new Set();
for (const file of files) {
  const text = readFileSync(file, 'utf8');
  for (const m of text.matchAll(/\bt\(\s*'([A-Za-z0-9_]+)'/g)) used.add(m[1]);
  for (const m of text.matchAll(/\btn\(\s*'([A-Za-z0-9_]+)'/g)) used.add(`${m[1]}One`).add(`${m[1]}Other`);
  for (const m of text.matchAll(/__MSG_([A-Za-z0-9_]+)__/g)) used.add(m[1]);
  // Keys named in tables or picked by a condition: any quoted word that is a key.
  for (const m of text.matchAll(/'([a-z][A-Za-z0-9]+)'/g)) if (m[1] in source) used.add(m[1]);
  // `${DOWN[reason]}Hint`: the hint of every key that has one.
  if (/\}Hint`/.test(text)) for (const key of Object.keys(source)) if (used.has(key) && `${key}Hint` in source) used.add(`${key}Hint`);
}
for (const key of used) if (!(key in source)) problems.push(`the code uses ${key}, which has no text`);
for (const key of Object.keys(source)) if (!used.has(key)) problems.push(`${key} is not used`);

if (problems.length) {
  console.error(problems.join('\n'));
  process.exit(1);
}
console.log(`ok: ${Object.keys(source).length} extension texts in ${LANGS.length} languages`);
