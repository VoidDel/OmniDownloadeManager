# ADR-001 技术选型与命名

- 状态：已采纳
- 日期：2026-11-09
- 相关：[开发方案](../development-plan.md) §2、§3、§9

## 背景

Omni Download 是跨平台（Windows、macOS、Linux）下载管理器，支持 HTTP / FTP / BitTorrent / 磁力链接、多连接与断点续传，并通过浏览器扩展接管下载。同一作者已有 OmniZip（Tauri 2 + Rust + React），两者属于同一产品家族，界面需保持一致。

## 决策

### 应用框架

- **Tauri 2.x**（锁定 2 系列）。Tauri 3 仍是 alpha，不采用。选 Tauri 而不是 Electron：系统 WebView，安装包小一个数量级，后端直接用 Rust。
- 窗口与 OmniZip 相同：Windows / Linux 去掉系统标题栏自绘，macOS 用 Overlay 标题栏保留红绿灯；窗口隐藏创建，首帧绘制后再显示。

### 后端（Rust workspace）

| crate | 职责 | 阶段 |
|---|---|---|
| `omnidl-storage` | SQLite（sqlx，WAL）仓储与迁移、写盘 actor、预分配 | Phase 1 |
| `omnidl-core` | 任务模型与状态机、`DownloadEngine` trait、调度器、限速、事件总线 | Phase 1 |
| `omnidl-http` | reqwest（rustls）多连接分片、动态窃取、续传 | Phase 1 |
| `omnidl-bt` | librqbit 适配层（磁力元数据、文件选择、做种策略） | Phase 3 |
| `omnidl-ftp` | suppaftp（rustls）REST 续传、多会话 | Phase 4 |
| `omnidl-ipc` | 浏览器桥消息与长度前缀 JSON 帧 | Phase 2 |
| `omnidl-nm-host` | Native Messaging host 可执行文件 | Phase 2 |
| `omnidl-desktop` | Tauri 应用（`apps/desktop/src-tauri`） | 全程 |

依赖方向：`omnidl-storage` ← `omnidl-core` ← {`omnidl-http`, `omnidl-ftp`, `omnidl-bt`} ← `omnidl-desktop`；`omnidl-ipc` 由 `omnidl-desktop` 与 `omnidl-nm-host` 共用。引擎 crate 不依赖 Tauri，可单独测试。

BitTorrent 选 **librqbit** 而不是 libtorrent-rasterbar：纯 Rust，免去 C++ 与 Boost 的三平台交叉编译；已具备 DHT、PEX、UPnP、磁力元数据、限速与会话持久化。`omnidl-bt` 只对外暴露自己的类型，日后可替换引擎。

### 前端

- **React 19 + TypeScript + Vite**，状态用 Zustand，列表虚拟化用 TanStack Virtual（与 OmniZip 相同）。
- **样式与组件来自共享包 [omni-ui](https://github.com/VoidDel/omni-ui)**：设计令牌、12 套内置主题、窗口骨架样式、基础组件与图标，与 OmniZip 共用一份代码。不使用 Tailwind 与 shadcn/ui，它们与令牌体系重复且风格不同。
- 应用通过 `components/ui.tsx`、`components/Icon.tsx` 两个薄适配层引用 omni-ui，视图代码不直接 import omni-ui，便于接入翻译与 store。
- omni-ui 以 git 依赖分发（TS / CSS 源码，由应用的 Vite 编译）。内置主题 JSON 用 `pnpm sync:omni-ui` 复制到 `themes/`，供 Rust 主题服务 `include_str!`（Phase 1），CI 校验副本未过期。
- 质量工具：`tsc`（strict）、ESLint（JS / TS 推荐规则 + React Hooks 规则）、Vitest。

### 工程

- pnpm workspace（`apps/*`）+ cargo workspace（`crates/*` + `apps/desktop/src-tauri`），Rust edition 2024，stable 工具链。
- CI：Linux 上做主题同步校验、tsc、ESLint、rustfmt、clippy（`-D warnings`）；Windows / macOS / Linux 三平台跑 `cargo test --workspace` 与前端测试。

### 命名与标识

| 项 | 取值 | 说明 |
|---|---|---|
| 产品名 | Omni Download | 与 OmniZip 同属 Omni 家族 |
| crate / 包前缀 | `omnidl-` | 对应 OmniZip 的 `omnizip-`，避免与家族共享包 `omni-ui` 混淆 |
| 可执行文件 | `omnidl-desktop` | `mainBinaryName` |
| Bundle identifier | `app.omnidownload.desktop` | 对应 OmniZip 的 `app.omnizip.desktop` |
| Native Messaging host 名 | `app.omnidownload.host` | 见 ADR-002 |
| 开发服务器端口 | 1430 | OmniZip 用 1420，可同时开发 |
| 许可证 | MIT | 与 OmniZip、omni-ui 相同；librqbit（Apache-2.0）等依赖兼容 |
| 图标 | `icons/app-icon.svg` | 家族圆角方形（渐变 `#4C86F5 → #2556C6`）+ 白色「箭头落到横线上」；`pnpm icons` 生成各平台尺寸 |

## 影响

- 改 UI 令牌或基础组件要在 omni-ui 里改，再在两个应用 `pnpm update omni-ui`。
- omni-ui 是公开仓库。锁文件把它记为 SSH 地址，CI 在安装前把 GitHub 的 SSH 地址改写为 HTTPS 匿名拉取；若日后改回私有，给 CI 配置 `OMNI_UI_TOKEN` 密钥（对 omni-ui 有读权限的令牌）即可。
- Phase 0 的主题在页面里用 omni-ui 的 `resolveTheme` 解析，只有默认的晨光 / 午夜两套；Phase 1 换成 Rust 主题服务后才支持全部主题、强调色与窗口材质。
