# Omni Download

跨平台（Windows、macOS、Linux）下载管理器：HTTP / FTP 多连接与断点续传、BitTorrent 与磁力链接，以及 Chrome、Edge、Firefox 扩展。Rust + Tauri 2 + React，界面与 [OmniZip](https://github.com/VoidDel/OmniZip) 同属 Omni 家族，共用设计系统 [omni-ui](https://github.com/VoidDel/omni-ui)。

> 当前进度：**Phase 4** 已完成：HTTP / HTTPS 多连接下载与断点续传、任务列表与设置（Phase 1），Chrome、Edge、Firefox 扩展接管下载（Phase 2），BitTorrent 与磁力链接（文件选择、做种策略、`magnet:` 与 `.torrent` 关联，Phase 3），以及 FTP / FTPS（账号、多会话、`REST` 续传，Phase 4）。之后是系统集成与发布工程，路线图见 [开发方案](docs/development-plan.md) §10。

## 目录

| 路径 | 内容 |
|---|---|
| `apps/desktop/` | Tauri 应用：`src/` 为 React 前端，`src-tauri/` 为 Rust 外壳（`omnidl-desktop`） |
| `apps/extension/` | 浏览器扩展（WXT），输出 Chromium 与 Firefox 两个版本 |
| `crates/omnidl-core` | 任务模型、状态机、调度器、限速、事件总线 |
| `crates/omnidl-storage` | SQLite 持久化与写盘 |
| `crates/omnidl-http` · `omnidl-ftp` · `omnidl-bt` | 各协议下载引擎 |
| `crates/omnidl-ipc` · `omnidl-nm-host` | 浏览器桥的消息协议与 Native Messaging host |
| `themes/` | omni-ui 内置主题的副本（`pnpm sync:omni-ui`），供 Rust 嵌入 |
| `locales/` | 界面文字：简体中文（源）、繁體中文、English、日本語（i18next + ICU，与 OmniZip 相同） |
| `icons/app-icon.svg` | 应用图标源文件（`pnpm icons` 生成各平台尺寸） |
| `docs/` | 开发方案与架构决策记录（`docs/architecture/ADR-*.md`） |

## 开发

需要 Rust stable（1.97+）、Node 22+、pnpm 10，以及各平台的 [Tauri 依赖](https://v2.tauri.app/start/prerequisites/)。锁文件里 omni-ui 记为 SSH 地址；没有 GitHub SSH 密钥的机器先执行一次 `git config --global url."https://github.com/".insteadOf "git@github.com:"`（CI 也是这样做的）。

```bash
pnpm install
pnpm dev              # 启动开发窗口（Vite 端口 1430）
pnpm bundle           # 本平台的安装包（含 omnidl-nm-host），输出在 target/release/bundle
```

`pnpm bundle` 与发布用的配置相同（`src-tauri/tauri.release.conf.json`）：Windows 出 NSIS，macOS 出 .app 与 .dmg（`pnpm bundle --target universal-apple-darwin` 为通用版），Linux 出 deb、rpm 与 AppImage。设置了 `TAURI_SIGNING_PRIVATE_KEY`（minisign 私钥的内容或路径）时同时生成签过名的更新包。

### 发布

1. 把 `apps/desktop/src-tauri/tauri.conf.json`、`apps/desktop/package.json` 与 `Cargo.toml` 中的版本改成新版本并提交。
2. 推送标签 `v<版本>`（如 `v0.2.0`）。Release 工作流在三个平台构建安装包，放进同名的草稿 Release，并生成应用内更新用的 `latest.json`。
3. 在 GitHub 上检查并发布草稿；发布之后，已安装的应用就能检查到这个版本。

仓库需要两个 Secret：`TAURI_SIGNING_PRIVATE_KEY`（更新签名私钥，对应 `tauri.conf.json` 里的公钥；私钥丢失后旧版本将无法再更新）与 `OMNI_UI_TOKEN`（与 CI 相同）。私钥有密码时再加 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。代码签名与 macOS 公证尚未接入。

检查（与 CI 相同）：

```bash
pnpm check:locales   # 各语言的键与占位符与简体中文一致，代码用到的键都存在
pnpm lint             # TypeScript
pnpm eslint
pnpm test             # Vitest
cargo fmt --all --check
pnpm --filter omnidl-desktop build   # clippy / cargo test 前需要先构建前端
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

手动试下载不必依赖外网：引擎测试用的本地服务器可以单独启动，内容按偏移生成，任意大小都有。

```bash
cargo run -p omnidl-http --release --example test_server -- 127.0.0.1:18080
# 然后在应用里添加，例如：
#   http://127.0.0.1:18080/dl/3221225472/ubuntu.iso      每个连接约 1 MB/s，便于观察进度
#   http://127.0.0.1:18080/get/52428800/node.zip         全速

# FTP：把一个文件夹放到本机 FTP 上（不带账号参数时匿名登录）
cargo run -p omnidl-ftp --example ftp_server -- <文件夹> [端口，默认 2121] [user:password]

# BT：在本机做种一个文件或文件夹（写出 .torrent，打印带 x.pe 的磁力链接），再把它们加进应用
cargo run -p omnidl-bt --example seed -- <文件或文件夹> [端口] [KiB/s]

# Phase 1 验收用的大文件测试（写 5 GiB，中途强杀后续传并逐字节校验；OMNIDL_LARGE_GIB 改大小）
cargo test -p omnidl-http --release --test download -- --ignored large_file --nocapture
```

### 浏览器扩展

```bash
pnpm ext:build        # apps/extension/.output/chrome-mv3（Chrome、Edge、Brave）与 firefox-mv3
pnpm --filter omnidl-extension dev   # 开发模式：启动带扩展的浏览器，改代码自动重载
```

加载开发版：Chrome / Edge 打开扩展管理页并开启“开发者模式”，选择“加载解压缩的扩展”，指向 `apps/extension/.output/chrome-mv3`；Firefox 在 `about:debugging` 中“临时载入附加组件”，选 `apps/extension/.output/firefox-mv3/manifest.json`。扩展通过 Native Messaging 找到应用：应用启动时为五种浏览器注册 `omnidl-nm-host`（它必须在应用可执行文件旁边；`pnpm dev` 会先构建它），设置 › 浏览器可查看状态和重新写入。

测试：

```bash
pnpm ext:e2e          # Playwright：在 Chromium 中加载扩展，对本地网站和模拟的应用测试接管
# 完整链路（Windows，可选）：真实 Edge → Native Messaging → host → 应用。应用需先构建且未在运行
OMNIDL_APP=<omnidl-desktop.exe 的路径> pnpm --filter omnidl-extension exec playwright test native
```

数据位置：数据库 `omni.db` 与 `settings.toml` 在系统的应用数据目录（Windows 为 `%APPDATA%\app.omnidownload.desktop`），日志按天写在应用日志目录（设置 › 常规 › 日志）。

关闭主窗口默认只是隐藏，下载继续，应用留在通知区域（托盘）；从托盘图标的菜单退出。随系统启动与浏览器拉起时以 `--background` 启动，不显示窗口。

## 界面与 omni-ui

令牌、主题、窗口骨架样式和基础组件都来自 omni-ui，本仓库只放下载器特有的样式（`apps/desktop/src/styles/app.css`）。视图从 `components/ui.tsx`、`components/Icon.tsx` 引用组件，不直接引用 omni-ui。

改共享的 UI 时：

1. 在 omni-ui 修改并推送到 `main`。
2. 在本仓库与 OmniZip 分别执行 `pnpm update omni-ui`，再执行 `pnpm sync:omni-ui`，然后提交锁文件与 `themes/`。

两边同时调试时可以 `pnpm --filter omnidl-desktop link ../../../omni-ui` 让改动实时生效。

设计稿：Claude 设计画布「OmniDownloadManager UI 设计」。

## 许可

MIT
