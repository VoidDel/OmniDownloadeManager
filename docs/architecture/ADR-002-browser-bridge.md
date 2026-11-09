# ADR-002 浏览器桥方案

- 状态：已采纳（Phase 2 已实施，见文末「实施记录」）
- 日期：2026-11-09
- 相关：[开发方案](../development-plan.md) §5、§8.3

## 背景

Chrome、Edge、Firefox 扩展要把用户点击的下载连同 Cookie、Referer、User-Agent 交给桌面程序，并显示连接状态。可选通道有三种：Native Messaging、本地 HTTP / WebSocket 服务、自定义 URL scheme。

## 决策

### 主通道：Native Messaging + 独立 host + 本地 IPC

```
扩展 ──stdio（4 字节长度 + JSON）──► omnidl-nm-host ──命名管道 / Unix socket──► Omni Download
```

- 浏览器按扩展 ID 校验后才启动 host，不开放任何端口，没有 CORS / CSRF 攻击面。
- `omnidl-nm-host` 是很小的独立可执行文件（不带 tokio 多线程，启动 < 50 ms），只做转发：主程序未运行时以 `--background` 拉起它，并等待 IPC 就绪（最多 5 秒）。不把主程序本身当 host，是因为浏览器每次连接都会新启动一个 host 进程，主程序需要单实例常驻。
- IPC 端点只允许当前用户访问：Windows 命名管道 `\\.\pipe\omnidl-<用户 SID 哈希>`（DACL 限当前 SID），macOS / Linux `$XDG_RUNTIME_DIR/omnidl.sock`（权限 0600）。
- 消息类型与帧格式定义在 `omnidl-ipc`，导出 TypeScript 类型给扩展，两端共用一份定义。

### host 清单

- 名称 `app.omnidownload.host`。Chromium 系清单用 `allowed_origins` 列出 Chrome 与 Edge 商店的扩展 ID，Firefox 清单用 `allowed_extensions: ["omni@omnidl.app"]`。
- 主程序首次启动时写入，设置页「重新写入通信配置」可修复。安装采用 per-user 模式：Windows 写 `HKCU`，macOS / Linux 写用户目录，不需要管理员权限。
- 覆盖 Chrome、Chromium、Edge、Brave、Firefox，路径见开发方案 §5.5。

### 备用通道：本地 HTTP + 配对码（默认关闭）

- 只在 Native Messaging 不可用时启用，例如 Snap / Flatpak 打包的浏览器。
- 监听 `127.0.0.1` 的随机端口，要求 `Authorization: Bearer <32 字节随机 token>`，并校验 `Origin` 为已知扩展。
- token 由用户在设置页「显示配对码」复制到扩展选项页，只需配对一次。

### 不采用

- **仅本地 HTTP**：要开放端口、处理 CORS 与跨站请求伪造，网页可以探测端口。
- **自定义 URL scheme**：单向，无法回传状态，也无法安全地携带 Cookie。

## 影响

- 需要随安装包分发第二个可执行文件（`omnidl-nm-host`，作为 Tauri sidecar），并维护五种浏览器 × 三平台的清单位置。
- 扩展 ID 必须固定。Chrome 开发期用 `manifest.key`，Firefox 用 `browser_specific_settings.gecko.id`。
- MV3 service worker 会休眠，扩展按需 `connectNative`，每次请求后断开；host 必须启动足够快。

## 实施记录（Phase 2，2026-11-09）

实现时定下或改动的细节：

- **端点名**：Windows 管道 `\.\pipe\omnidl-<DOMAIN\用户名 的 FNV 哈希>`，不取 SID（免去 Win32 安全 API）。管道保持默认安全描述（所有者、管理员、SYSTEM 可读写，其他账户只读，不足以通信）并拒绝远程客户端。Linux 用 `$XDG_RUNTIME_DIR/omnidl.sock`，没有时用 `~/.local/share/omnidl/omnidl.sock`；macOS 用 `~/Library/Application Support/app.omnidownload.desktop/omnidl.sock`；socket 0600、目录 0700。
- **端点覆盖**：环境变量 `OMNIDL_IPC_ENDPOINT` 改用私有端点（测试、两个构建并存）。用了它的应用不自动注册 host，因为已注册的 host 连的是默认端点。
- **host 拉起应用**：只有需要应用的消息（下载、打开主窗口）才拉起，状态查询从不拉起。Windows 上以 `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB` 启动（脱离浏览器的作业对象，浏览器关闭端口时不会连带结束应用；浏览器不允许脱离时退回不带该标志），并调用 `AllowSetForegroundWindow`，让快捷下载框能来到浏览器前面。等待管道就绪最多 10 秒（调试版冷启动较慢）。
- **消息**：请求增加可选的 `browser` 字段（`chrome`、`edge`、`firefox`…），应用据此在状态栏显示哪些浏览器的扩展连过。`add_download` 的结果是 `{ action, asked, task_id? }`，`action` 为 `started` / `queued` / `browser` / `cancelled`，`asked` 表示用户看到了快捷下载框；没看到时扩展弹一条“已交给 Omni Download”的系统通知。扩展的 TypeScript 类型手写在 `apps/extension/lib/protocol.ts`（与 ADR-003 相同的理由，不用代码生成）。
- **快捷下载框的答复回到浏览器**：应用等用户在快捷下载框里选择后才回复扩展。Chromium 在 `downloads.onDeterminingFilename` 里暂停这次下载直到收到回复：应用接管（开始或稍后）就取消浏览器的下载；用户选“改用浏览器下载”、关掉窗口以外的失败、应用不可达时，就放行浏览器继续下载同一个文件，不用重新请求。Firefox 在阻塞式 `webRequest.onHeadersReceived` 里按响应头判断，同样等回复后决定取消还是放行。
- **备用路径**：有别的扩展或自动化工具替浏览器定了文件名时，Chromium 不会调用 `onDeterminingFilename`；扩展在文件名确定后发现这次下载没经过自己，就改走“开始后再接管”：确认应用可达后取消浏览器的下载并交给应用（这时不传浏览器定的文件名，由应用按服务器响应命名），应用不要时让浏览器重新下载。Firefox 用 `downloads.onCreated` 走同一条路径接住 `webRequest` 之外的下载。
- **Cookie 落库加密**：任务选项带 Cookie 或请求头时整体用 AES-256-GCM 加密后存入 `options_json`（`enc:v1:` 前缀）。密钥是应用数据目录里的 `secret.key`：Windows 用 DPAPI 保护，macOS / Linux 权限 0600。没用系统钥匙串，以免未签名构建弹钥匙串授权、Linux 依赖 Secret Service。
- **注册策略**：启动时只为缺少注册或注册已失效（清单或 host 不存在）的浏览器写入，别的安装写好的有效注册不覆盖；设置页「重新写入通信配置」强制改为本安装。Chromium 以 `HKCU\Software\Chromium\BLBeacon` 判断是否安装。
- **扩展 ID**：开发用密钥对的公钥写在 `wxt.config.ts` 的 `key` 中，固定 Chromium 系扩展 ID 为 `nfifehjghnnmamofhbbiagiaodhfeipn`（Chrome、Edge、Brave、Chromium 加载未打包扩展时相同）；私钥不入库，加载未打包扩展也不需要。上架后把商店 ID 加入 `CHROMIUM_EXTENSION_IDS`。
- **连接方式**：扩展选项可选「自动」（先本机通信，再备用通道）、「本机通信」或「备用通道」。Firefox（MV3）安装时不自动授予网站权限，选项页在缺少时提供「授予权限」。
