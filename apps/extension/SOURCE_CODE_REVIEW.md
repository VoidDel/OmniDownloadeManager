# Building the Omni Download add-on from source

For add-on reviewers. The add-on is written in TypeScript and React and bundled with
[WXT](https://wxt.dev) (Vite); nothing is obfuscated. This package holds everything the build
needs. It is a cut of the public repository <https://github.com/VoidDel/OmniDownloadeManager>,
where the extension is one package (`apps/extension`) of a pnpm workspace:

| Path | What it is |
|---|---|
| `apps/extension/entrypoints/` | Background script, content script, popup and options pages |
| `apps/extension/lib/` | Take-over rules, the bridge to the desktop app, settings |
| `apps/extension/wxt.config.ts` | Manifest and build configuration |
| `package.json`, `pnpm-workspace.yaml`, `pnpm-lock.yaml` | The workspace and its lock file (exact dependency versions) |
| `apps/desktop/package.json` | The workspace's other package, present only so the lock file matches; it is not built |

## Requirements

- Any OS (the submitted package was built on Windows 11; Linux and macOS give the same files)
- Node.js 22 or later
- pnpm 10 (`corepack enable` picks the version named in `package.json`, or `npm install -g pnpm@10`)

## Steps

From the root of this package:

```bash
pnpm install --frozen-lockfile --filter omnidl-extension
pnpm --filter omnidl-extension exec wxt zip -b firefox --mv3
```

The add-on is then in `apps/extension/.output/firefox-mv3/` (unpacked) and
`apps/extension/.output/omni-download-<version>-firefox.zip`. Its files are identical to the
submitted package's.

All dependencies come from the npm registry through pnpm, except `omni-ui`, the project's own
component library, which pnpm fetches from its public repository
(<https://github.com/VoidDel/omni-ui>) at the commit pinned in the lock file. It is plain
TypeScript and CSS source, compiled by this build.

## What the add-on does with data

It hands downloads to the Omni Download desktop application on the same computer, through Native
Messaging (host `app.omnidownload.host`) or, when the user pairs it in the options, through an
HTTP interface on `127.0.0.1`. For a download it passes the file's address, the page it came
from, the site's cookies (so the application can fetch files that need a login), the browser's
user agent and the file name. Nothing is sent anywhere else; there is no remote server, no
analytics and no remote code. This is what `data_collection_permissions` in the manifest declares.

To try it, install the desktop application from
<https://github.com/VoidDel/OmniDownloadeManager/releases> and start it once; it registers the
Native Messaging host for Firefox.
