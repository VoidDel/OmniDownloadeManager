// Installers for this platform, as a release has them (docs/development-plan.md §7.4): the
// Native Messaging host bundled, and update packages signed with the minisign key in
// TAURI_SIGNING_PRIVATE_KEY (its content or path). Without the key the installers are still made,
// without update packages.
//
//   pnpm bundle                                  Windows: NSIS; macOS: .app + .dmg; Linux: deb, rpm, AppImage
//   pnpm bundle --target universal-apple-darwin  any `tauri build` arguments pass through
//
// Output: target/[<triple>/]release/bundle/ (or under CARGO_TARGET_DIR).

import { execFileSync, execSync } from 'node:child_process';
import { writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const args = process.argv.slice(2);
const at = args.indexOf('--target');

execFileSync(process.execPath, ['scripts/sidecar.mjs', ...(at >= 0 ? ['--target', args[at + 1]] : [])], { cwd: root, stdio: 'inherit' });
const config = ['--config', 'src-tauri/tauri.release.conf.json'];
if (!process.env.TAURI_SIGNING_PRIVATE_KEY) {
  console.warn('TAURI_SIGNING_PRIVATE_KEY is not set: no update packages (installers only).');
  const unsigned = join(tmpdir(), 'omnidl-unsigned.conf.json');
  writeFileSync(unsigned, JSON.stringify({ bundle: { createUpdaterArtifacts: false } }));
  config.push('--config', unsigned);
}
// pnpm is a script (pnpm.cmd on Windows): one command line for the shell, arguments quoted.
const quote = (a) => (/^[\w./:=@+-]+$/.test(a) ? a : `"${a.replace(/(["\\$`])/g, '\\$1')}"`);
execSync(['pnpm', 'tauri', 'build', ...config, ...args].map(quote).join(' '), { cwd: join(root, 'apps/desktop'), stdio: 'inherit' });
