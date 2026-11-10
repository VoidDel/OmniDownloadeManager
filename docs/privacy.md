# Privacy Policy · 隐私政策

Omni Download（桌面应用与浏览器扩展）· 生效日期 / Effective: 2026-11-10

## English

**In short: Omni Download has no servers of its own. Nothing you download, and nothing about you, is sent to the developers or to any third party for analytics, advertising or sale.**

### The browser extension

The extension's single purpose is to hand downloads from your browser to the Omni Download desktop application on the same computer.

When a download starts (or when you choose "Download with Omni Download" from a menu), the extension reads:

- the address of the file, its name and size;
- the address of the page the download came from (the referrer);
- the cookies your browser holds for the site of that file, so the application can fetch files that require you to be signed in;
- your browser's user-agent string.

It passes these to the Omni Download application running on your computer, through the browser's Native Messaging channel or, if you have paired it in the extension's options, through an interface on `127.0.0.1` (your own machine). They are not sent anywhere else.

"Download this page's file links" reads the link addresses on the current page, only when you choose that menu item, and passes them to the application in the same way.

The extension stores your take-over rules (minimum size, file types, excluded sites) and, if you use it, the pairing code for the local interface, in the browser's extension storage on your device.

The extension contains no analytics, no advertising, and loads no remote code.

### The desktop application

- **What it stores, on your computer only:** the list of downloads (addresses, file names, save locations, progress), your settings, and log files. Cookies and FTP passwords that belong to a download are stored encrypted.
- **What it connects to:** the servers you download from, with the cookies and headers of that download; for BitTorrent, trackers, the DHT network and other peers, who can see your IP address, as with any BitTorrent client.
- **Update checks:** once after starting (you can turn this off) the application asks GitHub whether a newer release exists. This is an ordinary HTTPS request to `github.com`; GitHub's own privacy policy applies to it.
- **No telemetry:** the application sends no usage statistics, crash reports or identifiers to anyone.

### Removing your data

Uninstalling the extension removes what it stored. The application's data is in the folders listed in the [user guide](user-guide.md#数据存放在哪里); delete them to remove it.

### Changes and contact

Changes to this policy are published in this file, with a new date. Questions: open an issue at <https://github.com/VoidDel/OmniDownloadeManager/issues>.

## 中文

**简单地说：Omni Download 没有自己的服务器。你下载的内容和关于你的任何信息，都不会发送给开发者，也不会为统计、广告或出售而提供给任何第三方。**

### 浏览器扩展

扩展只做一件事：把浏览器里的下载交给同一台电脑上的 Omni Download 桌面应用。

下载开始时（或你从菜单里选择“用 Omni Download 下载”时），扩展会读取：

- 文件的地址、文件名和大小；
- 发起下载的页面地址（来源页）；
- 浏览器为该文件所在网站保存的 Cookie，这样应用才能下载需要登录的文件；
- 浏览器的 User-Agent。

这些信息通过浏览器的 Native Messaging 通道，或者（在扩展选项里配对后）通过本机 `127.0.0.1` 上的接口，交给在你电脑上运行的 Omni Download 应用，不会发送到别处。

“下载本页的文件链接”只在你选择这个菜单项时读取当前页面上的链接地址，并以同样的方式交给应用。

扩展把你的接管规则（最小文件大小、文件类型、不接管的网站），以及备用通道的配对码（如果使用）保存在本机的浏览器扩展存储里。

扩展不含统计、广告，也不加载任何远程代码。

### 桌面应用

- **只保存在你电脑上的内容：** 下载列表（地址、文件名、保存位置、进度）、设置和日志。属于某个下载的 Cookie 和 FTP 密码加密保存。
- **它会连接：** 你下载文件的服务器（带上该下载的 Cookie 和请求头）；BT 下载时连接 Tracker、DHT 网络和其他节点，对方能看到你的 IP 地址，这与任何 BT 客户端相同。
- **检查更新：** 启动后向 GitHub 询问一次是否有新版本（可以关闭）。这是一次普通的 HTTPS 请求，适用 GitHub 自己的隐私政策。
- **没有遥测：** 应用不向任何人发送使用统计、崩溃报告或标识符。

### 删除数据

卸载扩展即删除它保存的内容。应用的数据在 [使用手册](user-guide.md#数据存放在哪里) 列出的文件夹里，删除这些文件夹即可。

### 变更与联系

本政策的变更会发布在这个文件里，并更新日期。有疑问请在 <https://github.com/VoidDel/OmniDownloadeManager/issues> 提出。
