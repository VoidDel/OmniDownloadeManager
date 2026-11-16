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

The add-on's dependencies all come from the npm registry through pnpm. The lock file also
lists `omni-ui` (<https://github.com/VoidDel/omni-ui>, plain TypeScript and CSS source, fetched
from its public repository at the pinned commit) for the desktop application of the same
workspace, whose `package.json` is included so that `pnpm install` matches the lock file; the
add-on does not use it.

## What the add-on does with data

It hands downloads to the Omni Download desktop application on the same computer, through Native
Messaging (host `app.omnidownload.host`) or, when the user pairs it in the options, through an
HTTP interface on `127.0.0.1`. For a download it passes the file's address, the page it came
from, the site's cookies (so the application can fetch files that need a login), the browser's
user agent and the file name; for a download that is the answer to a form the user submitted,
also what the form sent (`lib/post.ts`; read with `webRequest`, held in memory while the request
runs and dropped unless the answer is a file to download); for media the user downloads from the
popup, the referrer, origin and cookies its request was sent with (`sniffMedia` in
`entrypoints/background.ts`; the cookies in memory only). Opening the popup runs `scanPage`
(`lib/scan.ts`) once in the tab's frames, through `scripting.executeScript`: it reads the page's
media element sources, media metadata and resource timing entries and returns them; it changes
nothing in the page. For an HLS playlist the page played, `describeHls` fetches that playlist
once (credentials included) and parses its qualities (`lib/hls.ts`); the only request the
extension itself makes to a website. Nothing is sent anywhere else; there is no remote server, no
analytics and no remote code. This is what `data_collection_permissions` in the manifest declares.

To try it, install the desktop application from
<https://github.com/VoidDel/OmniDownloadeManager/releases> and start it once; it registers the
Native Messaging host for Firefox.
