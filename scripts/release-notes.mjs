// A release's notes, from docs/releases/v<version>.md (what changed; optional) and the version:
//
//   node scripts/release-notes.mjs v0.2.3 body [assets.txt]   the release page's text
//   node scripts/release-notes.mjs v0.2.3 update              the update dialog's text
//
// The release page gets what changed, a table of the installers with links that download them,
// and how to get past the first-run warnings of unsigned installers; in Chinese, then English.
// With `assets.txt` (the release's file names, one per line) it fails when an installer it links
// to is not among them. The update dialog shows plain text: what changed, without Markdown.
//
// docs/releases/v<version>.md: the Chinese part, a line `---`, the English part (both lists of
// what changed, `##` headings allowed). See docs/releases/README.md.

import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO = 'https://github.com/VoidDel/OmniDownloadeManager';
const root = fileURLToPath(new URL('..', import.meta.url));

const [tag, mode = 'body', assetsFile] = process.argv.slice(2);
if (!/^v\d+\.\d+\.\d+$/.test(tag ?? '') || !['body', 'update'].includes(mode)) {
  console.error('usage: node scripts/release-notes.mjs v<version> body [assets.txt] | update');
  process.exit(2);
}
const version = tag.slice(1);

// What changed, in each language.
const file = join(root, 'docs/releases', `${tag}.md`);
const [zh = '', en = ''] = existsSync(file)
  ? readFileSync(file, 'utf8')
      .replace(/\r\n/g, '\n')
      .split(/^---$/m)
      .map((s) => s.trim())
  : [];

if (mode === 'update') {
  // The dialog shows text as it is: no emphasis marks, links as their text, headings as lines.
  const plain = (s) =>
    s
      .replace(/^#+\s*/gm, '')
      .replace(/\*\*(.+?)\*\*/g, '$1')
      .replace(/`([^`]+)`/g, '$1')
      .replace(/\[([^\]]+)\]\([^)]+\)/g, '$1');
  process.stdout.write([zh, en].filter(Boolean).map(plain).join('\n\n'));
  process.exit(0);
}

// The installers, as the release build names them (tauri.conf.json's productName, its space a dot).
const name = 'Omni.Download';
const installers = {
  windows: `${name}_${version}_x64-setup.exe`,
  mac: `${name}_${version}_aarch64.dmg`,
  deb: `${name}_${version}_amd64.deb`,
  rpm: `${name}-${version}-1.x86_64.rpm`,
  appimage: `${name}_${version}_amd64.AppImage`,
};
if (assetsFile) {
  const have = new Set(readFileSync(assetsFile, 'utf8').split(/\r?\n/).filter(Boolean));
  const missing = Object.values(installers).filter((f) => !have.has(f));
  if (missing.length) {
    console.error(`not in the release: ${missing.join(', ')}`);
    process.exit(1);
  }
}
const url = (f) => `${REPO}/releases/download/${tag}/${f}`;
const link = (f, text = f) => `[${text}](${url(f)})`;

const guide = `使用方法见 [使用手册](${REPO}/blob/main/docs/user-guide.md) 和 [常见问题](${REPO}/blob/main/docs/faq.md)。`;
const body = [
  zh ? `${zh}\n\n${guide}` : guide,
  `## 下载

| 系统 | 安装包（点击下载） |
|---|---|
| Windows 10 / 11 | ${link(installers.windows)} |
| macOS 12+（Apple Silicon） | ${link(installers.mac)} |
| Linux（Debian、Ubuntu） | ${link(installers.deb)} |
| Linux（Fedora、openSUSE） | ${link(installers.rpm)} |
| Linux（其他发行版） | ${link(installers.appimage)} |

已安装的可在应用里直接更新。`,
  `## 安装提示

安装包目前没有代码签名，第一次运行时系统会拦一下：

- **Windows**：SmartScreen 提示“已保护你的电脑”时，点“更多信息”，再点“仍要运行”。
- **macOS**：提示“已损坏，无法打开”时，在终端执行 \`xattr -cr "/Applications/Omni Download.app"\`，或到“系统设置 › 隐私与安全性”里点“仍要打开”。`,
  '---',
  `Download: ${link(installers.windows, 'Windows')} · ${link(installers.mac, 'macOS (Apple Silicon)')} · Linux ${link(installers.deb, '.deb')} / ${link(installers.rpm, '.rpm')} / ${link(installers.appimage, '.AppImage')}`,
  en,
  `The installers are not code-signed yet. On Windows, choose "More info" → "Run anyway" in the SmartScreen prompt. On macOS, if the app "is damaged and can't be opened", run \`xattr -cr "/Applications/Omni Download.app"\` or choose "Open Anyway" in System Settings › Privacy & Security.`,
]
  .filter(Boolean)
  .join('\n\n');
process.stdout.write(`${body}\n`);
