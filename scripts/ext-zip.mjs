// Packages the browser extension for the stores (docs/extension-publishing.md):
//
//   apps/extension/.output/omni-download-<version>-chrome.zip    Chrome Web Store
//   apps/extension/.output/omni-download-<version>-edge.zip      Microsoft Edge Add-ons
//   apps/extension/.output/omni-download-<version>-firefox.zip   addons.mozilla.org
//   apps/extension/.output/omni-download-<version>-sources.zip   the sources Mozilla asks for
//
// Store packages have no `key` in the manifest (the stores sign with their own, see
// apps/extension/wxt.config.ts). Each package is checked before it is listed.

import { execSync } from 'node:child_process';
import { readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ext = fileURLToPath(new URL('../apps/extension', import.meta.url));
const { version } = JSON.parse(readFileSync(join(ext, 'package.json'), 'utf8'));

execSync('pnpm run zip', { cwd: ext, stdio: 'inherit', env: { ...process.env, OMNIDL_STORE: '1' } });

const manifest = (browser) => JSON.parse(readFileSync(join(ext, '.output', `${browser}-mv3`, 'manifest.json'), 'utf8'));
const problems = [];
for (const browser of ['chrome', 'edge']) {
  const m = manifest(browser);
  if (m.key) problems.push(`${browser}: the manifest has "key" (the store refuses it)`);
  if (m.version !== version) problems.push(`${browser}: manifest version ${m.version}, package.json ${version}`);
}
const gecko = manifest('firefox').browser_specific_settings?.gecko;
if (!gecko?.id) problems.push('firefox: no add-on ID');
if (!gecko?.data_collection_permissions?.required?.length) problems.push('firefox: no data_collection_permissions (required for new add-ons)');
if (problems.length) {
  console.error(problems.join('\n'));
  process.exit(1);
}

console.log('\nPackages for the stores:');
for (const name of ['chrome', 'edge', 'firefox', 'sources']) {
  const file = join(ext, '.output', `omni-download-${version}-${name}.zip`);
  console.log(`  ${file}  ${(statSync(file).size / 1024).toFixed(0)} KB`);
}
