# Omni Download

跨平台（Windows、macOS、Linux）下载管理器：HTTP / FTP 多连接与断点续传、BitTorrent 与磁力链接，以及 Chrome、Edge、Firefox 扩展。Rust + Tauri 2 + React，界面与 [OmniZip](https://github.com/VoidDel/OmniZip) 同属 Omni 家族，共用设计系统 [omni-ui](https://github.com/VoidDel/omni-ui)。

> 当前处于 **Phase 0**：仓库骨架、窗口框架与 CI。下载引擎从 Phase 1 开始，路线图见 [开发方案](docs/development-plan.md) §10。

## 目录

| 路径 | 内容 |
|---|---|
| `apps/desktop/` | Tauri 应用：`src/` 为 React 前端，`src-tauri/` 为 Rust 外壳（`omnidl-desktop`） |
| `crates/omnidl-core` | 任务模型、状态机、调度器、限速、事件总线 |
| `crates/omnidl-storage` | SQLite 持久化与写盘 |
| `crates/omnidl-http` · `omnidl-ftp` · `omnidl-bt` | 各协议下载引擎 |
| `crates/omnidl-ipc` · `omnidl-nm-host` | 浏览器桥的消息协议与 Native Messaging host |
| `themes/` | omni-ui 内置主题的副本（`pnpm sync:omni-ui`），供 Rust 嵌入 |
| `icons/app-icon.svg` | 应用图标源文件（`pnpm icons` 生成各平台尺寸） |
| `docs/` | 开发方案与架构决策记录（`docs/architecture/ADR-*.md`） |

## 开发

需要 Rust stable（1.97+）、Node 22+、pnpm 10，以及各平台的 [Tauri 依赖](https://v2.tauri.app/start/prerequisites/)。锁文件里 omni-ui 记为 SSH 地址；没有 GitHub SSH 密钥的机器先执行一次 `git config --global url."https://github.com/".insteadOf "git@github.com:"`（CI 也是这样做的）。

```bash
pnpm install
pnpm dev              # 启动开发窗口（Vite 端口 1430）
pnpm build            # 打包安装程序
```

检查（与 CI 相同）：

```bash
pnpm lint             # TypeScript
pnpm eslint
pnpm test             # Vitest
cargo fmt --all --check
pnpm --filter omnidl-desktop build   # clippy / cargo test 前需要先构建前端
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## 界面与 omni-ui

令牌、主题、窗口骨架样式和基础组件都来自 omni-ui，本仓库只放下载器特有的样式（`apps/desktop/src/styles/app.css`）。视图从 `components/ui.tsx`、`components/Icon.tsx` 引用组件，不直接引用 omni-ui。

改共享的 UI 时：

1. 在 omni-ui 修改并推送到 `main`。
2. 在本仓库与 OmniZip 分别执行 `pnpm update omni-ui`，再执行 `pnpm sync:omni-ui`，然后提交锁文件与 `themes/`。

两边同时调试时可以 `pnpm --filter omnidl-desktop link ../../../omni-ui` 让改动实时生效。

设计稿：Claude 设计画布「OmniDownloadManager UI 设计」。

## 许可

MIT
