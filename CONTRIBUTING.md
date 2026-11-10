# 参与贡献

欢迎反馈问题、提建议和提交代码。

## 反馈问题

在 [Issues](https://github.com/VoidDel/OmniDownloadeManager/issues/new/choose) 里选对应的模板。问题反馈请写清楚：

- 版本号（“设置 › 常规 › 版本”）和操作系统。
- 做了什么、期望什么、实际发生了什么；能稳定重现的话写出步骤。
- 当天的日志文件（“设置 › 常规 › 日志 › 打开文件夹”）。日志里可能有下载链接和文件路径，介意的话先抹掉。
- 浏览器接管的问题：浏览器及版本、扩展弹窗里显示的连接状态。

安全问题请不要公开提交，用 GitHub 的 [私密漏洞报告](https://github.com/VoidDel/OmniDownloadeManager/security/advisories/new)。

## 搭建开发环境

需要 Rust stable（版本见 `rust-toolchain.toml`）、Node 22+、pnpm 10，以及各平台的 [Tauri 依赖](https://v2.tauri.app/start/prerequisites/)。

```bash
pnpm install
pnpm dev          # 启动桌面应用（会先构建浏览器通信程序）
```

界面组件库 omni-ui 在锁文件里记为 SSH 地址。没有 GitHub SSH 密钥的机器先执行一次：

```bash
git config --global url."https://github.com/".insteadOf "git@github.com:"
```

本机试下载用的 HTTP、FTP 测试服务器和 BT 做种工具，见 [README 的“开发”一节](README.md#开发)。

## 代码在哪里

| 路径 | 内容 |
|---|---|
| `apps/desktop/src/` | 界面（React + TypeScript） |
| `apps/desktop/src-tauri/` | 桌面壳：命令、托盘、通知、更新、浏览器桥 |
| `apps/extension/` | 浏览器扩展（WXT） |
| `crates/omnidl-core` | 任务模型、状态机、调度、限速 |
| `crates/omnidl-http` · `omnidl-ftp` · `omnidl-bt` | 各协议的下载引擎 |
| `crates/omnidl-storage` | SQLite 持久化 |
| `crates/omnidl-ipc` · `omnidl-nm-host` | 浏览器桥的协议与 Native Messaging host |
| `locales/` | 界面文字 |
| `docs/` | 开发方案、架构决策（`docs/architecture/ADR-*.md`）、使用手册 |

动手做较大的改动之前，先读 [开发方案](docs/development-plan.md) 里相关的章节和对应的 ADR，并开一个 Issue 说明打算怎么做。

## 提交之前

CI 会跑下面这些，本地先过一遍：

```bash
pnpm check:locales                                   # 四种语言的键一致，代码里用到的键都存在
pnpm lint && pnpm eslint                             # TypeScript 类型检查与 ESLint
pnpm test                                            # 前端与扩展的单元测试
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm ext:build && pnpm ext:e2e                       # 改了扩展时
```

## 约定

**界面文字**

- 所有用户看得到的文字都放在 `locales/<语言>/app.json`，简体中文是源语言。加一个键要同时加四种语言（zh-CN、zh-TW、en-US、ja-JP）。
- 托盘、通知等 Rust 侧的文字用同一份文件（`i18n::tr`）。
- 用用户的话写：说清楚发生了什么、接下来怎么做；不出现内部名词。错误提示的处理建议放在 `errors.hints.<错误码>`。

**界面**

- 样式、主题和基础组件来自 [omni-ui](https://github.com/VoidDel/omni-ui)，与 OmniZip 共用。颜色、间距用它的设计令牌（`var(--…)`），不要写死数值。只属于本应用的样式放在 `apps/desktop/src/styles/app.css`。
- 新界面要能只用键盘操作，控件要有可读的名称（`aria-label` 或可见文字），深色和浅色主题下都要检查。

**Rust**

- 引擎层（`crates/`）不依赖 Tauri；与界面的往来都经过 `apps/desktop/src-tauri/src/commands.rs` 和事件。
- 返回给界面的错误用 `omnidl_core::Error` 和对应的 `ErrorCode`，新增错误码时同时补上四种语言的提示。
- 数据库结构的改动加一个新的迁移文件（`crates/omnidl-storage/migrations/`），不改已有的。

**测试**

- 引擎的行为（续传、重试、错误恢复）用集成测试覆盖，对着测试服务器跑，结果逐字节比对。
- 界面改动在运行中的应用里实际看一遍，不只看类型检查通过。

**提交信息**

`类型(范围): 说明`，说明用中文。类型用 `feat`、`fix`、`build`、`docs`、`refactor`、`test`、`chore`，例如：

```
fix(ui): 侧栏较矮时上方列表先滚动
```

一个 PR 只做一件事；说明里写清楚改了什么、为什么、怎么验证的。

## 发布

维护者推送 `v<版本>` 标签后，发布流水线构建三个平台的安装包并生成草稿 Release，见 [README 的“发布”一节](README.md#发布)。

## 许可

提交的代码以本仓库的 [MIT 许可](LICENSE) 发布。
