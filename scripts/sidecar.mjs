// Builds the Native Messaging host (omnidl-nm-host) for bundling with the app: Tauri's
// `externalBin` wants apps/desktop/src-tauri/binaries/omnidl-nm-host-<target triple>[.exe], and
// installs it next to the app's executable, where the app looks for it.
//
//   node scripts/sidecar.mjs                                 this machine's target
//   node scripts/sidecar.mjs --target x86_64-pc-windows-msvc

import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const out = join(root, 'apps/desktop/src-tauri/binaries');
const args = process.argv.slice(2);
const at = args.indexOf('--target');
const host = /host: (\S+)/.exec(execFileSync('rustc', ['-vV'], { encoding: 'utf8' }))[1];
const target = at >= 0 ? args[at + 1] : host;

const run = (cmd, a) => execFileSync(cmd, a, { cwd: root, stdio: 'inherit' });
/** Builds the host for `t` and returns the binary's path. */
const build = (t) => {
  run('cargo', ['build', '-p', 'omnidl-nm-host', '--release', '--target', t]);
  return join(process.env.CARGO_TARGET_DIR || join(root, 'target'), t, 'release', t.includes('windows') ? 'omnidl-nm-host.exe' : 'omnidl-nm-host');
};

mkdirSync(out, { recursive: true });
const ext = target.includes('windows') ? '.exe' : '';
copyFileSync(build(target), join(out, `omnidl-nm-host-${target}${ext}`));
console.log(`omnidl-nm-host for ${target} in ${out}`);
