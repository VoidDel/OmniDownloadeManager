# 浏览器扩展上架

给维护者：把扩展提交到 Chrome Web Store、Microsoft Edge Add-ons 和 addons.mozilla.org（AMO）的步骤和要填的内容。

## 1. 打包

```bash
pnpm install --frozen-lockfile   # 依赖与锁文件一致，源码包才能重现同样的构建
pnpm ext:zip
```

产物在 `apps/extension/.output/`：

| 文件 | 用途 |
|---|---|
| `omni-download-<版本>-chrome.zip` | Chrome Web Store |
| `omni-download-<版本>-edge.zip` | Microsoft Edge Add-ons（内容与 Chrome 包相同） |
| `omni-download-<版本>-firefox.zip` | AMO |
| `omni-download-<版本>-sources.zip` | AMO 要求的源码包（代码经过打包压缩，审核员要从源码重新构建） |

商店包与开发版的区别只有一处：清单里没有 `key`。Chrome Web Store 不接受带 `key` 的清单，商店会用自己的密钥签名，所以**商店版的扩展 ID 与开发版（`nfifehjg…`）不同**，见第 3 节。

版本号取自 `apps/extension/package.json`，每次提交新版本前先改它。

上架素材在 `apps/extension/store/`：`logo-300.png`（Edge 的商店徽标）、`screenshot-1-options.png` 和 `screenshot-2-popup.png`（1280×800）。128×128 的图标已在扩展包里。建议再补一两张桌面应用接管下载时的截图。

## 2. 顺序

1. 在三个商店各建一个条目、上传包，**先不要提交审核**（或提交后也可以，审核需要几天）。Chrome 和 Edge 在第一次上传后就会给出扩展 ID。
2. 把两个 ID 加进应用（第 3 节），发布一个新的应用版本。
3. 提交审核。审核员需要装上这个新版本的应用才能看到扩展工作。

Firefox 的 ID（`omni@omnidl.app`）写在清单里，不受商店影响，应用已经认识它。

## 3. 让应用认识商店版的扩展

浏览器只允许清单里列出的扩展连接本机通信程序。拿到 ID 后改两处：

- `apps/desktop/src-tauri/src/browser/integration.rs` 的 `CHROMIUM_EXTENSION_IDS`：加上 Chrome Web Store 和 Edge Add-ons 给的两个 ID（开发版的保留）。
- `apps/desktop/src-tauri/src/commands.rs` 里 `extension_open` 的 `STORES`：填上各浏览器的商店页面地址，应用里的“获取扩展”就会直接打开商店。

ID 在哪里看：Chrome 开发者信息中心的条目地址和“商品 ID”；Edge 合作伙伴中心的扩展概览页（Extension ID / Product ID 中的 Extension ID）。

## 4. 各商店要填的内容

三个商店通用的文字放在第 5 节，直接粘贴。

### Chrome Web Store

<https://chrome.google.com/webstore/devconsole> ›“新建商品”› 上传 `…-chrome.zip`。

- **商品详情**：说明（第 5 节）、类别“工具”、语言“中文（简体）”、至少一张截图。
- **隐私权规范**：
  - 单一用途、各项权限的理由：第 5 节（英文，审核员阅读）。
  - 远程代码：否。
  - 数据使用：扩展会经手“网络历史记录”（下载地址和来源页地址）和“身份验证信息”（网站 Cookie，用于下载需要登录的文件），建议如实勾选这两项，并勾选三条合规声明。这些数据只交给本机的应用，隐私政策里有说明。
  - 隐私权政策网址：`https://github.com/VoidDel/OmniDownloadeManager/blob/main/docs/privacy.md`
- **测试说明**（“测试说明”页）：第 5 节的审核说明。

申请了 `<all_urls>` 主机权限的扩展会进入更细的人工审核，时间比普通扩展长。

### Microsoft Edge Add-ons

<https://partner.microsoft.com/dashboard/microsoftedge> ›“创建新扩展”› 上传 `…-edge.zip`。

- **属性**：类别“效率”；隐私政策网址同上；网站 `https://github.com/VoidDel/OmniDownloadeManager`；支持联系方式填 Issues 地址。
- **商店一览**（语言选中文）：说明、徽标 `logo-300.png`（必填，300×300）、截图、搜索词（如 `下载`、`下载管理器`、`断点续传`、`磁力`）。
- **认证说明**：第 5 节的审核说明。

### addons.mozilla.org

<https://addons.mozilla.org/developers/> ›“提交新附加组件”›“在此网站上”› 上传 `…-firefox.zip`。

- 兼容性只勾 Firefox（桌面）。扩展用到本机通信，Android 上不可用。
- 问到“是否使用了代码生成器、压缩工具或打包工具”时选“是”，上传 `…-sources.zip`。构建步骤在包里的 `apps/extension/SOURCE_CODE_REVIEW.md`，按它构建出的文件与提交的包逐字节相同。
- **列表信息**：名称、摘要（250 字以内）、说明、类别“下载管理”、许可证 MIT、支持网站填仓库地址。
- **隐私政策**：把 `docs/privacy.md` 的内容粘贴进去。
- **给审核员的说明**：第 5 节的审核说明。

提交时验证器会给出三条警告，都不影响提交：

- 两条 `innerHTML`：来自打包进去的 React，不是扩展自己的代码。
- 一条关于 Firefox for Android 的最低版本：扩展不面向 Android。

清单里的 `data_collection_permissions` 声明了扩展会把浏览活动、网站内容（Cookie）和网站活动（下载）交给本机应用。2025 年 11 月起新提交的附加组件必须有这项声明；安装时 Firefox 会把它显示给用户。因为这项声明要 Firefox 140 才支持，扩展的最低版本定为 140。

## 5. 可直接粘贴的文字

### 名称

Omni Download

### 摘要（一句话）

把浏览器的下载交给 Omni Download：多连接加速、断点续传，带上 Cookie 与来源页面。

### 说明

```
把浏览器里的下载交给 Omni Download 桌面应用。

· 点击下载时自动接管，用多个连接同时下载，支持断点续传
· 带上网站的登录信息（Cookie）和来源页面，需要登录才能下载的文件也能下
· 接管前可以确认文件名和保存位置，也可以设为直接开始
· 按文件大小、扩展名、网站设置哪些下载交给应用，哪些留在浏览器
· 右键菜单：下载链接、图片、视频，或下载本页的所有文件链接
· 按住 Alt 点击链接，这一次仍由浏览器下载

需要先安装 Omni Download 桌面应用（Windows、macOS、Linux，免费、开源）：
https://github.com/VoidDel/OmniDownloadeManager/releases

扩展只与本机的 Omni Download 应用通信，不向任何服务器发送数据。
```

### Single purpose (Chrome)

```
Hands downloads from the browser to the Omni Download desktop application on the same computer, which downloads them with multiple connections and resume support.
```

### Permission justifications (Chrome)

| Permission | Justification |
|---|---|
| `downloads` | Detects a download the browser is about to make, reads its address, file name and size to decide whether to hand it to the desktop application, and cancels the browser's copy when it is handed over. |
| `nativeMessaging` | Sends the download to the Omni Download desktop application installed on the same computer. This is the extension's core function. |
| `cookies` | Reads the cookies of the site a download comes from and passes them to the desktop application, so it can fetch files that require the user to be signed in. Cookies are read only for the download being handed over and go only to the local application. |
| `contextMenus` | Adds "Download with Omni Download" to the menu of links, images, video and audio, and "Download this page's file links" to the page menu. |
| `scripting` | When the user chooses "Download this page's file links", reads the link addresses on that page. Nothing is injected otherwise. |
| `activeTab` | Gives the menu commands and the keyboard shortcut access to the current page only when the user invokes them. |
| `storage` | Saves the user's take-over rules (minimum file size, file types, excluded sites) and connection settings. |
| `notifications` | Tells the user when a download could not be handed to the desktop application. |
| Host permission (`<all_urls>`) | A download can come from any site: the extension needs that site's cookies and referrer to hand the download over, and its content script notices Alt+click, which lets the user keep a download in the browser. It also talks to the desktop application's optional interface on 127.0.0.1. |

Firefox 版另有 `webRequest`、`webRequestBlocking`：Firefox 没有 Chromium 的 `downloads.onDeterminingFilename`，扩展用它们在下载开始前判断是否接管。

### 审核说明（Notes for reviewers）

```
This extension is a companion to the Omni Download desktop application (free, open source):
https://github.com/VoidDel/OmniDownloadeManager/releases

To test:
1. Install the desktop application (Windows, macOS or Linux) and start it once. It registers the Native Messaging host "app.omnidownload.host" for the installed browsers.
2. Install the extension. Its popup shows "已连接" (connected) with the application's version.
3. Download any file larger than 1 MB. The application opens a small confirmation window; "开始下载" starts the download in the application and the browser's own download is cancelled.
4. Alt+click a link to let the browser download it instead.

Without the desktop application the extension does nothing and its popup says the application was not found.

Data: the download's URL, referrer, the site's cookies and the user agent are passed to the application on the same computer through Native Messaging (or, if the user pairs it in the options, a 127.0.0.1 interface). Nothing is sent to any remote server. There is no analytics and no remote code.

The interface is in Simplified Chinese. Source: https://github.com/VoidDel/OmniDownloadeManager/tree/main/apps/extension
```

## 6. 之后的更新

改 `apps/extension/package.json` 的版本号，`pnpm ext:zip`，把新包上传到各商店的同一条目。AMO 每次都要同时上传源码包。扩展界面目前只有简体中文；加入其他语言后，在各商店补充对应语言的列表文字。
