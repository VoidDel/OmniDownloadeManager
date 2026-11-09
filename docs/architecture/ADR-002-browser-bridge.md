# ADR-002 浏览器桥方案

- 状态：已采纳（Phase 2 实施）
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
