# OmniDownloadManager 开发方案

> 版本：v0.1（2026-11-09）
> 技术栈：Rust + Tauri 2 + React 19
> 目标平台：Windows 10/11、macOS 12+（Apple Silicon）、Linux（Ubuntu 22.04+ / Fedora 39+ / Arch）

---

## 目录

1. [项目概述](#1-项目概述)
2. [技术选型](#2-技术选型)
3. [系统架构](#3-系统架构)
4. [下载引擎设计](#4-下载引擎设计)
5. [浏览器扩展设计](#5-浏览器扩展设计)
6. [前端与界面设计](#6-前端与界面设计)
7. [跨平台与系统集成](#7-跨平台与系统集成)
8. [安全设计](#8-安全设计)
9. [项目结构](#9-项目结构)
10. [开发路线图](#10-开发路线图)
11. [测试与质量保障](#11-测试与质量保障)
12. [风险与对策](#12-风险与对策)
13. [附录](#13-附录)

---

## 1. 项目概述

### 1.1 目标

OmniDownloadManager（以下简称 Omni）是一款开源、跨平台的通用下载管理器，定位对标 IDM / Motrix / Free Download Manager，核心卖点：

- **一个程序覆盖全部常见协议**：HTTP/HTTPS、FTP/FTPS、BitTorrent、磁力链接。
- **浏览器无缝接管**：Chrome、Edge、Firefox 扩展一键接管下载，自动携带 Cookie / Referer。
- **快**：HTTP 多连接分段下载、动态分片窃取、断点续传；BT 基于纯 Rust 引擎。
- **轻**：Tauri 使用系统 WebView，安装包 < 20 MB，空闲内存 < 80 MB。
- **美**：现代化界面，深浅主题，流畅动效，中英双语。

### 1.2 功能清单与版本规划

| 功能 | MVP (v0.1) | v1.0 | v2.0 |
|---|:-:|:-:|:-:|
| HTTP/HTTPS 多连接下载、断点续传 | ✅ | | |
| 任务列表 / 新建 / 暂停 / 继续 / 删除 | ✅ | | |
| SQLite 持久化、重启恢复 | ✅ | | |
| 全局 / 单任务限速 | ✅ | | |
| 深浅主题、自定义标题栏 | ✅ | | |
| Chrome / Edge / Firefox 扩展 + Native Messaging | | ✅ | |
| BitTorrent / 磁力（含文件选择、做种、DHT、UPnP） | | ✅ | |
| FTP / FTPS（REST 续传、多连接） | | ✅ | |
| 系统托盘、开机自启、完成通知 | | ✅ | |
| `magnet:` / `.torrent` 关联、单实例 | | ✅ | |
| 自动更新、三平台签名打包 | | ✅ | |
| 中 / 英文国际化 | | ✅ | |
| 剪贴板监控、批量下载、分类目录 | | | ✅ |
| 浏览器媒体嗅探（视频 / 音频浮动按钮） | | | ✅ |
| 悬浮窗（拖入链接下载） | | | ✅ |
| 计划任务 / 定时限速 | | | ✅ |
| FTP 目录递归下载 | | | ✅ |
| 文件校验（SHA-256 / MD5）、IP 过滤 | | | ✅ |

### 1.3 非功能需求

| 指标 | 目标 |
|---|---|
| 冷启动时间 | < 1.5 s（SSD） |
| 空闲内存 | < 80 MB（含 WebView） |
| 1000 个任务列表滚动 | 60 fps（虚拟滚动） |
| 单任务 HTTP 吞吐 | 打满千兆带宽（≥ 100 MB/s，磁盘允许时） |
| 安装包体积 | Windows < 15 MB，macOS < 20 MB，Linux AppImage < 25 MB |
| 进度刷新延迟 | ≤ 250 ms |

---

## 2. 技术选型

### 2.1 总览

| 层次 | 选型 | 版本基线 | 选择理由 | 备选 |
|---|---|---|---|---|
| 桌面框架 | **Tauri 2** | 2.11+ | 系统 WebView，体积小，Rust 后端，官方插件齐全。**Tauri 3 仍处 alpha，不采用** | Electron（体积大） |
| 后端语言 | **Rust** | stable, edition 2024 | 内存安全、高性能 IO、生态完善 | — |
| 异步运行时 | **tokio** | 1.x | Tauri 内置，生态标准 | — |
| HTTP 客户端 | **reqwest** (rustls) | 0.12+ | 流式下载、HTTP/2、代理、重定向控制 | hyper 直接使用 |
| FTP 客户端 | **suppaftp** (async + rustls) | 6.x | 支持 FTPS、REST 续传、异步 | — |
| BT 引擎 | **librqbit** | 8.x | 纯 Rust、DHT、磁力、做种、限速、UPnP、会话持久化，无需 C++ 工具链 | libtorrent-rasterbar FFI（构建复杂） |
| 持久化 | **SQLite** via `sqlx` | 0.8+ | 任务 / 分片状态结构化存储，编译期 SQL 校验，WAL 模式 | rusqlite |
| 配置存储 | **tauri-plugin-store** | 2.x | 简单 KV JSON | 存入 SQLite |
| 本地 IPC | **interprocess** (tokio) | 2.x | Windows 命名管道 / Unix 域套接字统一抽象 | 127.0.0.1 TCP |
| 类型共享 | **tauri-specta** + `specta` | 2.x | Rust 类型自动生成 TS 绑定，命令 / 事件类型安全 | ts-rs |
| 前端框架 | **React** + TypeScript | 19.x / 5.x | 生态最大，与 Tauri 模板一致 | Solid / Svelte |
| 构建工具 | **Vite** | 7.x | Tauri 官方推荐 | — |
| 样式 | **纯 CSS + 设计令牌**：共享包 [omni-ui](https://github.com/VoidDel/omni-ui) 的 `omni-ui/styles.css`（tokens + base + 窗口骨架 + 组件样式） | 0.1.x（git 依赖） | 与 OmniZip 同一份文件，改一处两端同步；主题服务在运行时重写 CSS 变量，stylelint 禁止裸色值 | Tailwind（会与令牌体系冲突，不采用） |
| 组件库 | **omni-ui** 包：`Switch` `Segmented` `Select` `Tabs` `ProgressBar` `Modal` `ContextMenu` `Toasts` `PasswordInput`，`Icon` / `FileIcon` / `badgeStyle`，`parseTheme` / `resolveTheme` / `writeTheme` 与 12 套内置主题 | 0.1.x | 从 OmniZip 抽出（2026-11-09），组件不依赖 i18n / store，应用在 `components/ui.tsx` 包一层接翻译与 store | shadcn/ui（样式体系不同，不采用） |
| 图标 | **Lucide** 风格线性 1.6 px（omni-ui `Icon`，可 `registerIcons` 追加下载器专用图标） | — | 与 OmniZip 一致；文件类型图标复用其单色 / 双色两套（已含 disc / app / audio 类别） | Phosphor |
| 动效 | **CSS 过渡** 150–220 ms ease-out（OmniZip 规范） | — | 克制动效，尊重 `prefers-reduced-motion`；复杂编排再按需引入 `motion` | motion |
| 状态管理 | **Zustand** | 5.x | 轻量，selector 精细订阅 | Jotai |
| 虚拟列表 | **@tanstack/react-virtual** | 3.x | 千级任务列表流畅 | react-window |
| 图表 | **uPlot** | 1.6+ | Canvas 渲染，速度曲线极低开销 | Recharts |
| 表单 | **react-hook-form + zod** | latest | 设置页 / 新建任务校验 | — |
| 国际化 | **i18next + react-i18next** | latest | 成熟，懒加载 | Lingui |
| 浏览器扩展框架 | **WXT** | 0.20+ | Vite 驱动，一套代码输出 Chrome / Edge / Firefox 三份 manifest，HMR | 手写 Vite 多入口 |
| 扩展 API 兼容 | **webextension-polyfill** | 0.12+ | 统一 `browser.*` Promise API | — |
| 包管理 | **pnpm** workspace + **cargo** workspace | — | monorepo | — |
| CI/CD | **GitHub Actions** + `tauri-action` | — | 三平台矩阵构建、签名、发布 | — |

### 2.2 关键决策说明

**为何选 librqbit 而非 libtorrent？**
libtorrent-rasterbar 更成熟，但需要 C++ 工具链 + Boost，三平台交叉编译和静态链接成本高，且 Rust 绑定不完善。librqbit 是 rqbit 客户端的核心库，纯 Rust，功能已覆盖桌面下载器所需（磁力解析、DHT、PEX、UPnP、限速、做种、文件选择、暂停 / 恢复持久化）。通过 adapter 层封装，未来可替换。

**为何用 Native Messaging 而非本地 HTTP 服务？**
Native Messaging 是浏览器官方的扩展 ↔ 本地程序通道，由浏览器校验扩展 ID 与 host 清单，无需开放端口、无 CORS / CSRF 面。缺点是 Snap / Flatpak 沙盒浏览器可能无法访问 host，因此保留本地 HTTP（127.0.0.1 + token）作为**备选通道**，由扩展自动降级。

**为何自定义标题栏？**
Windows / Linux 的原生标题栏与现代 UI 风格割裂。Windows / Linux 使用 `decorations: false` + 自绘标题栏（含拖拽区、最小化 / 最大化 / 关闭按钮）；macOS 使用 `titleBarStyle: "Overlay"` 保留原生红绿灯按钮，符合平台惯例。

---

## 3. 系统架构

### 3.1 总体架构图

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           浏览器 (Chrome / Edge / Firefox)                │
│  ┌──────────────────────────────────────────────────────────────────┐   │
│  │  Omni 浏览器扩展 (MV3, WXT)                                        │   │
│  │  background (service worker) · content script · popup · options   │   │
│  └───────────────┬──────────────────────────────────┬───────────────┘   │
└──────────────────┼──────────────────────────────────┼───────────────────┘
                   │ Native Messaging (stdio)          │ 备选: HTTP 127.0.0.1:port + token
                   ▼                                  │
      ┌────────────────────────┐                      │
      │ omnidl-nm-host (Rust)    │                      │
      │ 轻量 stdio ↔ IPC 转发   │                      │
      │ 主程序未运行时负责拉起   │                      │
      └───────────┬────────────┘                      │
                  │ 本地 IPC (命名管道 / Unix socket, 长度前缀 JSON)
                  ▼                                   ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                      Omni 主程序 (Tauri 2 应用, 单实例)                    │
│                                                                         │
│  ┌──────────────────────────┐   Tauri Commands / Events (tauri-specta)  │
│  │  WebView: React 19 UI    │◄───────────────────────────────────┐      │
│  │  Zustand · Tailwind ·    │                                    │      │
│  │  shadcn/ui · uPlot       │                                    │      │
│  └──────────────────────────┘                                    │      │
│                                                                  │      │
│  ┌───────────────────────────────────────────────────────────────┴───┐  │
│  │  omnidl-desktop (src-tauri)                                             │  │
│  │  命令层 · 事件桥(节流聚合) · 托盘 · 深链接 · 通知 · 更新 · IPC 服务端  │  │
│  └───────────────────────────────┬───────────────────────────────────┘  │
│                                  │                                      │
│  ┌───────────────────────────────▼───────────────────────────────────┐  │
│  │  omnidl-core: 任务模型 · 状态机 · 调度器 · 限速器 · 事件总线            │  │
│  │  ┌─────────────┐  ┌─────────────┐  ┌──────────────────────────┐    │  │
│  │  │ omnidl-http   │  │ omnidl-ftp    │  │ omnidl-bt (librqbit 适配)   │    │  │
│  │  │ reqwest     │  │ suppaftp    │  │ DHT·PEX·UPnP·做种         │    │  │
│  │  └──────┬──────┘  └──────┬──────┘  └────────────┬─────────────┘    │  │
│  │         └────────────────┴──────────────────────┘                  │  │
│  │                          ▼                                         │  │
│  │  omnidl-storage: SQLite (sqlx, WAL) · 文件写入器 · 预分配 · 原子重命名  │  │
│  └───────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────┘
```

### 3.2 进程模型

| 进程 | 生命周期 | 职责 |
|---|---|---|
| **Omni 主程序** | 用户启动 / 开机自启 / 被 host 拉起；关闭窗口后可驻留托盘 | 全部下载逻辑、UI、IPC 服务端 |
| **omnidl-nm-host** | 浏览器扩展建立 Native Messaging 连接时由浏览器启动，断开即退出 | 读 stdin → 转发到主程序 IPC → 回写 stdout；主程序未运行时启动它并等待 IPC 就绪（≤ 5 s） |
| **浏览器扩展 background** | 浏览器管理（MV3 service worker 可能被休眠） | 拦截下载、右键菜单、收集 Cookie、与 host 通信 |

主程序通过 `tauri-plugin-single-instance` 保证唯一；第二实例的命令行参数（`magnet:` 链接、`.torrent` 路径）转发给主实例后立即退出。

### 3.3 Rust 工作区划分

```
crates/
├─ omnidl-core      任务模型、TaskStatus 状态机、Scheduler、RateLimiter、EventBus、DownloadEngine trait
├─ omnidl-http      HTTP/HTTPS 引擎：探测、分片、动态窃取、续传、重试
├─ omnidl-ftp       FTP/FTPS 引擎：REST 续传、多会话
├─ omnidl-bt        librqbit 适配：Session 管理、磁力元数据、文件选择、做种策略、统计映射
├─ omnidl-storage   sqlx 仓储层、迁移、文件写入器（预分配、定位写入、原子重命名）
├─ omnidl-ipc       IPC 协议定义（serde 结构体 + 长度前缀编解码），主程序与 host 共用
├─ omnidl-nm-host   Native Messaging host 可执行文件（< 2 MB，无 tokio 多线程，启动 < 50 ms）
└─ omnidl-desktop   = apps/desktop/src-tauri，Tauri 壳：命令、事件、托盘、插件装配
```

依赖方向严格单向（箭头指向被依赖者）：`omnidl-desktop → {omnidl-http, omnidl-ftp, omnidl-bt} → omnidl-core → omnidl-storage`，`omnidl-desktop` 与 `omnidl-nm-host` 共同依赖 `omnidl-ipc`。引擎实现 `omnidl-core` 定义的 trait，核心不反向依赖引擎；引擎 crate 不依赖 Tauri，可独立编译为 CLI 做集成测试。命名见 [ADR-001](architecture/ADR-001-tech-stack.md)。

### 3.4 核心数据流：新建任务

```
用户粘贴 URL / 扩展推送
        │
        ▼
[omnidl-desktop] add_task 命令 ──► [omnidl-core] TaskManager::add()
        │                             │
        │                             ├─ 识别协议 → 选择 Engine
        │                             ├─ 写入 SQLite (status = Queued)
        │                             └─ 推入 Scheduler 队列
        │                                        │
        │                  并发槽位空闲时 ───────►  Engine::start(task)
        │                                        │
        │                            ┌───────────┴────────────┐
        │                            │  worker × N (tokio)     │
        │                            │  → mpsc 进度块          │
        │                            │  → Writer actor 落盘    │
        │                            │  → 每 1 s 持久化分片状态 │
        │                            └───────────┬────────────┘
        │                                        │ broadcast(TaskEvent)
        ▼                                        ▼
[omnidl-desktop] EventBridge：250 ms 节流，把活动任务进度聚合成一条 `task:progress-batch`
        │
        ▼
[React] Zustand store 更新 → 虚拟列表仅重渲染变化行
```

---

## 4. 下载引擎设计

### 4.1 统一任务模型

```rust
// crates/omnidl-core/src/task.rs
#[derive(Serialize, Deserialize, specta::Type, Clone)]
pub struct Task {
    pub id: Uuid,
    pub kind: TaskKind,                // Http | Ftp | Torrent
    pub source: String,                // URL / 磁力链接 / .torrent 路径
    pub save_dir: PathBuf,
    pub file_name: String,             // 单文件名或种子根目录名
    pub status: TaskStatus,
    pub total_bytes: Option<u64>,      // 未知大小时为 None
    pub downloaded_bytes: u64,
    pub uploaded_bytes: u64,           // BT
    pub speed_bps: u64,
    pub eta_secs: Option<u64>,
    pub category: Category,            // 视频 / 音乐 / 文档 / 压缩包 / 程序 / 其他
    pub priority: Priority,            // High | Normal | Low
    pub options: TaskOptions,          // 连接数、限速、headers、cookies、代理…
    pub error: Option<TaskError>,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, Deserialize, specta::Type, Clone, PartialEq)]
pub enum TaskStatus {
    Queued,          // 等待调度槽位
    Probing,         // 探测元信息（HEAD / SIZE / 磁力元数据）
    Downloading,
    Paused,
    Seeding,         // 仅 BT
    Completed,
    Failed,
    Removed,         // 回收站（可恢复）
}
```

状态机：

```
                ┌─────────────────────────────────────────────┐
                │                                             │
  add ──► Queued ──► Probing ──► Downloading ──► Completed ──► Seeding (BT 可选)
             ▲          │            │  ▲              │
             │          │            ▼  │              │
             │          │         Paused ┘ (resume)    │
             │          │            │                 │
             │          ▼            ▼                 ▼
             └───── Failed ◄─────────┘   remove ───► Removed ──► (彻底删除 / 恢复到 Queued)
                 (retry 回到 Queued)
```

引擎抽象（Phase 1 实现，见 [ADR-003](architecture/ADR-003-download-engine.md)）：一次运行跑到底，暂停 / 删除 / 退出都是取消同一个令牌，引擎不知道原因，也不接触数据库与界面。

```rust
#[async_trait]
pub trait DownloadEngine: Send + Sync + 'static {
    fn kind(&self) -> TaskKind;
    /// 探测元信息：大小、是否支持续传、文件名（新建对话框用）
    async fn probe(&self, source: &str, options: &JobOptions) -> Result<ProbeInfo>;
    /// 下载到完成、失败或 job.cancel 被取消
    async fn run(&self, job: Job) -> Outcome;
}

pub struct Job {            // 管理器交给引擎的一次运行
    pub resume: Option<ResumeState>,   // 名称、校验字段、分片表；None 表示首次开始
    pub cancel: CancellationToken,
    pub progress: Arc<Progress>,       // 原子计数：已下载、总大小、连接数、分片视图
    pub task_limit: RateLimiter, pub global_limit: RateLimiter,
    pub checkpoint: Arc<dyn Checkpoint>, // started()：文件已就绪；save()：每秒的已落盘进度
    /* id、source、save_dir、file_name、options … */
}

pub enum Outcome {
    Completed { file_name: String, total: u64 },
    Stopped(ResumeState),                       // 被取消，磁盘上已有的进度
    Failed(Error, Option<ResumeState>),
}
```

### 4.2 HTTP / HTTPS 引擎

#### 4.2.1 探测阶段

1. 发送 `HEAD` 请求（跟随重定向，上限 10 次），记录最终 URL。
2. 若 HEAD 被拒（405 / 403）或无 `Content-Length`，改用 `GET` + `Range: bytes=0-0`，从 `Content-Range` 解析总大小。
3. 记录：`Content-Length`、`Accept-Ranges: bytes` 或 206 响应（判定可分段）、`ETag`、`Last-Modified`、`Content-Disposition`、`Content-Type`。
4. 请求时**不发送 `Accept-Encoding`**（或显式 `identity`），避免压缩传输导致 `Content-Length` 与实际字节不一致。

#### 4.2.2 文件名推断

优先级：用户指定 > `Content-Disposition`（RFC 6266 / RFC 5987 `filename*=UTF-8''...`） > 最终 URL 路径最后一段（URL 解码） > `Content-Type` 推断扩展名 > `download_<timestamp>`。

清洗规则：去除 `\/:*?"<>|` 和控制字符；拒绝 `..`；Windows 保留名（`CON`、`PRN`、`AUX`、`NUL`、`COM1-9`、`LPT1-9`）追加下划线；总长 ≤ 200 字节；重名策略可配：自动重命名 `name (1).ext`（默认） / 覆盖 / 询问。

#### 4.2.3 分段与动态窃取（Work Stealing）

```
初始化：
  N = min(用户连接数, 总大小 / 1 MiB)          // 小于 1 MiB 的文件单连接
  每个分片 = [start, end]，均分

运行时（每个 worker 完成自己区间后）：
  找到剩余字节最多的活动分片 S
  if S.remaining > 2 MiB:
      mid = S.current + S.remaining / 2
      S.end = mid - 1                            // 原子更新，持有写锁
      新 worker 接管 [mid, old_end]
  else:
      worker 退出
```

- 分片表 `Vec<Segment { start, end, downloaded }>` 由 `parking_lot::RwLock` 保护，分裂时原 worker 通过 `watch` 通道感知 `end` 缩短，多读的字节在写入前裁剪。
- 服务器返回 200 而非 206（忽略 Range）→ 立即降级为单连接、不可续传模式并提示。
- 服务器限制并发（出现 429 / 503 / 连接被拒）→ 自动减半连接数并指数退避。

#### 4.2.4 写入与预分配

- 任务开始时在目标目录创建 `<file_name>.odm`（临时后缀），`set_len(total)` 预设逻辑长度；Linux 追加 `fallocate`、macOS `F_PREALLOCATE` 做真实预分配减少碎片（可选，失败降级）。
- **单一 Writer actor**：所有 worker 通过 `mpsc::channel<(offset, Bytes)>`（容量 64）发送数据块，Writer 在 `spawn_blocking` 线程中用 `FileExt::write_at` / `seek_write` 顺序写入。避免多线程争用同一文件句柄，且便于统计真实落盘字节。
- 数据块大小 256 KiB，Writer 合并相邻块后写入，每 8 MiB 或 1 s 调用一次 `sync_data`（可配置关闭以换取速度）。
- 完成后：校验下载总字节 == total → `rename` 去掉 `.odm` 后缀 → 设置修改时间为服务器 `Last-Modified`（可选）→ macOS 写入 `com.apple.quarantine` 扩展属性（与系统下载行为一致）。

#### 4.2.5 断点续传

- 分片进度每 1 s（或每累计 4 MiB）批量 `UPDATE segments SET downloaded = ?`，事务提交；进程崩溃最多丢失 1 s 进度。
- 恢复时：重新 `HEAD` 校验 `ETag` / `Last-Modified` / `Content-Length` 三者任一变化 → 标记「远端文件已变更」，提示用户重新下载；一致则对每个未完分片发起 `Range: bytes={start+downloaded}-{end}`。
- 不支持 Range 的任务暂停即视为取消（UI 明示）。

#### 4.2.6 其他

- **请求头**：UA（默认模拟 Chrome；可按任务自定义）、`Referer`、`Cookie`（来自扩展）、自定义头。
- **代理**：系统代理（默认，reqwest 自动读取） / HTTP / HTTPS / SOCKS5（含远程 DNS） / 直连；支持按域名绕过列表。
- **认证**：URL 内嵌 `user:pass@`、Basic、Digest（`diqwest` crate 或手动实现）、401 时弹窗询问。
- **限速**：令牌桶（`async-speed-limit` 或自实现），每个 worker 读到数据前 `consume(len).await`；全局限速器与任务限速器串联。
- **重试**：网络错误 / 5xx 指数退避 1 s → 2 s → 4 s … 上限 60 s，默认最多 10 次；404 / 410 / 401（无凭据）直接失败。
- **镜像 URL**（v2）：同一任务配置多个 URL，worker 轮询分配，坏链自动剔除。
- **完整性校验**（v2）：用户提供期望哈希，或读取 `Content-MD5` / `Digest` / `Repr-Digest` 头，完成后流式校验。

### 4.3 FTP / FTPS 引擎

| 项目 | 设计 |
|---|---|
| 库 | `suppaftp`，`async-rustls` feature，支持显式 FTPS（`AUTH TLS`） |
| 连接流程 | 连接 → 登录（匿名 `anonymous` / 用户凭据 / 弹窗） → `TYPE I` → `SIZE <path>` 获取大小 → `MDTM` 获取修改时间 |
| 传输模式 | 默认被动（PASV / EPSV），可切主动 |
| 多连接 | 每个 worker 独立 FTP 会话（登录 + `REST <offset>` + `RETR`），默认 2 连接、上限 8；服务器返回 `421 Too many connections` 自动降级 |
| 断点续传 | `REST` 命令；恢复前对比 `SIZE` 与 `MDTM` |
| 写入 | 复用 HTTP 引擎的 Writer actor 与分片表（抽到 `omnidl-core::segments`） |
| 目录下载（v2） | `MLSD` / `LIST` 解析（suppaftp 内置解析器），递归生成子任务，保持相对路径 |
| SFTP | **不在范围**（协议不同，属于 SSH；如需求强烈可在 v2 以 `russh-sftp` 引入） |

### 4.4 BitTorrent / 磁力引擎

#### 4.4.1 librqbit 集成

```rust
// crates/omnidl-bt/src/session.rs
let session = librqbit::Session::new_with_opts(
    default_download_dir,
    SessionOptions {
        disable_dht: false,
        disable_dht_persistence: false,
        persistence: Some(SessionPersistenceConfig::Json { folder: app_data.join("bt") }),
        listen_port_range: Some(settings.bt_port..settings.bt_port + 1),
        enable_upnp_port_forwarding: settings.upnp,
        ratelimits: LimitsConfig {
            upload_bps: settings.bt_upload_limit,
            download_bps: settings.bt_download_limit,
        },
        ..Default::default()
    },
).await?;
```

全局唯一 `Session`，在主程序启动时懒加载（首次添加 BT 任务时才初始化，避免无 BT 需求用户监听端口）。

#### 4.4.2 磁力 / 种子添加流程

```
用户输入 magnet:?xt=urn:btih:... 或 选择 .torrent 文件 或 粘贴 .torrent 的 URL
        │
        ▼
AddTorrent::from_url / from_bytes / from_local_filename
        │  AddTorrentOptions { list_only: true }
        ▼
磁力：进入 Probing 状态，通过 DHT + Tracker 获取元数据（UI 显示「正在获取元数据… 已连接 N 个节点」）
        │
        ▼
ListOnly 响应 → 展示文件树（勾选、大小、优先级）→ 用户确认
        │
        ▼
再次 add_torrent { only_files: Some(selected), output_folder, paused: false }
        │
        ▼
ManagedTorrentHandle → 定时 (500 ms) 读取 handle.stats() 映射到 Task 进度
```

#### 4.4.3 功能映射

| 功能 | librqbit 能力 | Omni 行为 |
|---|---|---|
| 暂停 / 恢复 | `session.pause()` / `unpause()` | 映射到 TaskStatus；会话 JSON 持久化保证重启恢复 |
| 删除 | `session.delete(id, delete_files)` | 回收站 → 彻底删除时才删文件 |
| 文件选择 | `only_files` + `update_only_files()` | 任务详情页可随时更改 |
| 做种 | 下载完成后自动继续 | 策略：分享率上限（默认 1.5）/ 做种时长上限（默认 24 h）/ 永久 / 不做种 |
| Peers / Trackers | `stats().live` 中的 peer 统计、tracker 状态 | 详情面板表格 |
| 限速 | 会话级 `ratelimits`，任务级通过 `LimitsConfig` | UI 全局 + 单任务 |
| DHT / PEX | 内置 | 设置页开关 |
| UPnP | `enable_upnp_port_forwarding` | 设置页开关，显示映射状态 |
| 公共 Tracker | 添加时合并用户配置列表 | 内置可编辑默认列表（可自动从 URL 更新） |
| 顺序下载 / 流式 | librqbit 支持 HTTP 流 | v2：边下边播（调用系统播放器） |
| IP 过滤 | 需适配 | v2 |

### 4.5 调度器

```rust
pub struct SchedulerConfig {
    pub max_concurrent_tasks: usize,      // 默认 3（BT 任务另算，默认 5）
    pub default_connections: usize,       // 默认 8，范围 1–64
    pub global_download_limit: Option<u64>,
    pub global_upload_limit: Option<u64>,
    pub retry: RetryPolicy,
}
```

- **队列**：三级优先级队列，同级 FIFO；用户可拖拽排序（持久化 `sort_order` 字段）。
- **并发槽位**：HTTP / FTP 任务共享槽位；BT 任务独立槽位（由 librqbit 内部管理连接）。
- **自动开始**：完成 / 失败 / 暂停释放槽位后，立即唤醒队头任务。
- **错误恢复**：按 `RetryPolicy` 重新入队；超过次数标记 `Failed`，保留错误信息和已下载部分。
- **磁盘空间**：开始前检查目标分区可用空间（`sysinfo` / `fs4`），不足则拒绝并提示。
- **事件总线**：`tokio::sync::broadcast::<TaskEvent>`（容量 4096），事件类型 `Added / StatusChanged / Progress / Completed / Failed / Removed`；EventBridge 在 Tauri 侧做 250 ms 节流聚合。

### 4.6 持久化（SQLite）

```sql
-- migrations/0001_init.sql
CREATE TABLE tasks (
    id            TEXT PRIMARY KEY,         -- UUID
    kind          TEXT NOT NULL,            -- http | ftp | torrent
    source        TEXT NOT NULL,
    final_url     TEXT,
    save_dir      TEXT NOT NULL,
    file_name     TEXT NOT NULL,
    status        TEXT NOT NULL,
    total_bytes   INTEGER,
    downloaded    INTEGER NOT NULL DEFAULT 0,
    uploaded      INTEGER NOT NULL DEFAULT 0,
    category      TEXT NOT NULL DEFAULT 'other',
    priority      INTEGER NOT NULL DEFAULT 1,
    sort_order    INTEGER NOT NULL,
    options_json  TEXT NOT NULL,            -- TaskOptions 序列化（headers、cookies 加密存储）
    etag          TEXT,
    last_modified TEXT,
    resumable     INTEGER NOT NULL DEFAULT 1,
    error_json    TEXT,
    created_at    INTEGER NOT NULL,
    started_at    INTEGER,
    completed_at  INTEGER
);

CREATE TABLE segments (
    task_id     TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    idx         INTEGER NOT NULL,
    start_pos   INTEGER NOT NULL,           -- `end` 是 SQL 关键字，列名加 _pos
    end_pos     INTEGER,                    -- 含；长度未知时为 NULL
    downloaded  INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (task_id, idx)
);

-- Phase 3 的迁移再建（0001 不含）
CREATE TABLE torrent_meta (
    task_id       TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
    info_hash     TEXT NOT NULL,
    rqbit_id      INTEGER,                  -- librqbit 会话内 ID
    selected_json TEXT NOT NULL,            -- 选中的文件索引
    seed_policy   TEXT NOT NULL
);

CREATE INDEX idx_tasks_status ON tasks(status);
CREATE INDEX idx_tasks_created ON tasks(created_at DESC);
```

- 开启 `PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;`。
- 使用 `sqlx::migrate!` 管理版本；应用升级自动迁移。实际迁移见 `crates/omnidl-storage/migrations/0001_init.sql`（`tasks` 另有 `prev_status`，回收站还原用）。
- 进度每秒保存一次：先快照分片表，再让写入线程 flush（并 `sync_data`），最后写库，数据库里的进度不会超前于磁盘（[ADR-003](architecture/ADR-003-download-engine.md)）。
- 数据库位于 `app_data_dir/omni.db`；敏感字段（Cookie、认证头）使用主程序生成的本地密钥（存于系统凭据管理器：Windows Credential Manager / macOS Keychain / Linux Secret Service，`keyring` crate）做 AES-GCM 加密。Phase 1 尚未加密（Cookie 只能在新建对话框里手动填写），Phase 2 扩展开始传 Cookie 时加上。

### 4.7 文件系统平台差异

| 事项 | Windows | macOS | Linux |
|---|---|---|---|
| 定位写入 | `FileExt::seek_write` | `FileExt::write_at` | `FileExt::write_at` |
| 真实预分配 | `set_len`（NTFS 稀疏，可接受） | `fcntl(F_PREALLOCATE)` | `fallocate` |
| 长路径 | `\\?\` 前缀（`dunce` crate 处理显示） | — | — |
| 非法字符 | `\/:*?"<>|` + 保留名 | `:`（Finder 显示为 `/`） | `/` |
| 大小写 | 不敏感 | 默认不敏感 | 敏感 |
| 下载标记 | Zone.Identifier ADS（可选写入） | `com.apple.quarantine` xattr | — |
| 跨分区重命名 | 失败时降级为复制 + 删除 | 同左 | 同左 |

---

## 5. 浏览器扩展设计

### 5.1 目标浏览器与 Manifest

| 浏览器 | Manifest | 后台模型 | 商店 |
|---|---|---|---|
| Chrome 116+ | V3 | Service Worker | Chrome Web Store |
| Edge 116+ | V3（与 Chrome 同一构建） | Service Worker | Microsoft Edge Add-ons |
| Firefox 115 ESR+ | V3 | Event Page（非持久 background scripts） | addons.mozilla.org（AMO，必须签名） |

使用 **WXT** 一套 TypeScript 源码输出 `chrome-mv3` 与 `firefox-mv3` 两个产物（Edge 复用 Chrome 产物）。扩展 ID 固定：Chrome 开发期通过 `manifest.key` 固定；Firefox 通过 `browser_specific_settings.gecko.id = "omni@omnidl.app"`。

权限清单（最小化）：`downloads`、`downloads.shelf`（Chromium）、`contextMenus`、`cookies`、`storage`、`nativeMessaging`、`notifications`、`webRequest`（Firefox 阻塞式 / Chromium 仅观察）、`host_permissions: <all_urls>`（用于 Cookie 读取与媒体嗅探，商店审核需说明）。

### 5.2 扩展模块

```
apps/extension/
├─ entrypoints/
│  ├─ background.ts        下载拦截、右键菜单、Native Messaging 客户端、降级 HTTP 客户端
│  ├─ content.ts           页面链接抓取（批量下载）、媒体浮动按钮（v2）
│  ├─ popup/               连接状态、拦截开关、最近 5 条任务、「打开 Omni」
│  └─ options/             拦截规则（大小阈值、扩展名白 / 黑名单、排除域名）、快捷键
├─ lib/
│  ├─ native.ts            NM 连接管理（连接池、重连、超时、降级）
│  ├─ intercept.ts         拦截判定策略
│  └─ protocol.ts          与 omnidl-ipc 一致的消息类型（由 specta 生成）
└─ wxt.config.ts
```

### 5.3 下载拦截策略

```
                    ┌──────────────────────────────┐
                    │ 浏览器发起下载 / 用户点击链接    │
                    └──────────────┬───────────────┘
                                   │
         Chromium                  │                  Firefox
  ┌────────────────────────────────┴──────────────────────────────────┐
  │ downloads.onDeterminingFilename   │ webRequest.onHeadersReceived (blocking) │
  │  (首选，拿到最终文件名 / 大小 / URL) │  判定 Content-Disposition: attachment    │
  │  → downloads.cancel(id)           │  或不可渲染的 Content-Type               │
  │  → downloads.erase({id})          │  → return { cancel: true }              │
  │ 回退：downloads.onCreated         │ 回退：downloads.onCreated → cancel      │
  └────────────────────────────────┬──────────────────────────────────┘
                                   │
                                   ▼
                     intercept.ts 判定是否接管：
                     · 拦截总开关开启
                     · 文件大小 ≥ 阈值（默认 1 MiB，未知大小视为通过）
                     · 扩展名在白名单 / 不在黑名单
                     · 域名不在排除列表
                     · 用户未按住 Alt（跳过接管的快捷方式）
                     · 非 blob: / data: URL
                                   │ 接管
                                   ▼
                     cookies.getAll({url}) → 组装 Cookie 头
                     收集 Referer（tab.url）、UA
                                   │
                                   ▼
                     native.send({ type: "add_download", ... })
                                   │ 成功 → 主程序弹出「快捷下载框」（置顶小窗，见 6.3）
                                   │        · 该站点已勾选「不再询问」或全局关闭快捷框 → 静默开始，
                                   │          浏览器内显示提示「已交给 Omni」（可一键撤销改用浏览器）
                                   │ 失败（host 不可用）→ 尝试 HTTP 备选通道
                                   │ 仍失败 → 放行浏览器原生下载 + 提示安装 / 启动 Omni
```

> 注意：Chromium 的 `onDeterminingFilename` 同时只允许一个扩展注册，若用户装有其他下载扩展会冲突，此时自动回退到 `onCreated` 路径。

### 5.4 其他扩展功能

- **右键菜单**：「用 Omni 下载链接」「用 Omni 下载图片 / 视频 / 音频」「下载本页所有链接…」（弹出筛选窗，按扩展名 / 关键字过滤）。
- **媒体嗅探（v2）**：`webRequest.onHeadersReceived` 观察 `video/*`、`audio/*`、`application/vnd.apple.mpegurl`（m3u8）等类型，content script 在 `<video>` 元素旁注入浮动「下载」按钮；m3u8 交给主程序的 HLS 下载器（v2 范围）。
- **快捷键**：`Alt+Shift+D` 下载当前页面 URL。
- **弹窗状态**：显示「已连接 Omni v1.x」「Omni 未运行（点击启动）」「未安装 host（修复指引）」。

### 5.5 Native Messaging 详细设计

#### 协议

- 传输：stdin / stdout，**4 字节小端长度前缀 + UTF-8 JSON**。
- 限制：host → 扩展单条 ≤ 1 MB；扩展 → host 单条 ≤ 4 GB（实际消息均 < 64 KB）。
- 消息示例：

```jsonc
// 扩展 → host
{ "id": "a1b2", "type": "add_download",
  "payload": {
    "url": "https://example.com/file.zip",
    "filename": "file.zip",
    "size": 104857600,
    "referer": "https://example.com/page",
    "user_agent": "Mozilla/5.0 ...",
    "cookies": "session=abc; theme=dark",
    "headers": { "Authorization": "Bearer ..." },
    "mime": "application/zip",
    "tab_url": "https://example.com/page"
  } }
// host → 扩展
{ "id": "a1b2", "ok": true, "result": { "task_id": "uuid", "app_version": "1.0.0" } }
{ "id": "a1b2", "ok": false, "error": { "code": "APP_NOT_RUNNING", "message": "..." } }
// 其它类型：ping / get_status / open_app / add_magnet / add_batch
```

#### host 二进制（omnidl-nm-host）

1. 启动后读取 stdin 消息（阻塞 IO 即可，无需 tokio 多线程，减小体积与启动时间）。
2. 尝试连接主程序 IPC 端点：
   - Windows：`\\.\pipe\omnidl-<user-sid-hash>`
   - macOS / Linux：`$XDG_RUNTIME_DIR/omnidl.sock` 或 `~/.local/share/omni/omnidl.sock`（权限 0600）
3. 连接失败 → 通过 host 清单旁的 `app_path` 文件定位主程序，启动 `omnidl-desktop --background`，轮询 IPC 最长 5 s。
4. 转发消息、回写响应。扩展断开连接（stdin EOF）时退出。

#### host 清单注册

主程序首次启动（以及设置页「修复浏览器集成」）时写入：

| 浏览器 | Windows（注册表 HKCU） | macOS（用户级） | Linux |
|---|---|---|---|
| Chrome | `Software\Google\Chrome\NativeMessagingHosts\app.omnidownload.host` | `~/Library/Application Support/Google/Chrome/NativeMessagingHosts/` | `~/.config/google-chrome/NativeMessagingHosts/` |
| Chromium | 同 Chrome 键 | `~/Library/Application Support/Chromium/NativeMessagingHosts/` | `~/.config/chromium/NativeMessagingHosts/` |
| Edge | `Software\Microsoft\Edge\NativeMessagingHosts\...` | `~/Library/Application Support/Microsoft Edge/NativeMessagingHosts/` | `~/.config/microsoft-edge/NativeMessagingHosts/` |
| Brave | `Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\...` | `~/Library/Application Support/BraveSoftware/Brave-Browser/NativeMessagingHosts/` | `~/.config/BraveSoftware/Brave-Browser/NativeMessagingHosts/` |
| Firefox | `Software\Mozilla\NativeMessagingHosts\...` | `~/Library/Application Support/Mozilla/NativeMessagingHosts/` | `~/.mozilla/native-messaging-hosts/` |

清单内容（Chromium 系）：

```json
{
  "name": "app.omnidownload.host",
  "description": "OmniDownloadManager native messaging host",
  "path": "C:\\Users\\xx\\AppData\\Local\\Omni\\omnidl-nm-host.exe",
  "type": "stdio",
  "allowed_origins": [ "chrome-extension://<chrome-id>/", "chrome-extension://<edge-id>/" ]
}
```

Firefox 版本将 `allowed_origins` 换为 `"allowed_extensions": ["omni@omnidl.app"]`。

Windows 注册表值为清单文件的绝对路径；清单文件放在 `%LOCALAPPDATA%\Omni\nm\`。安装采用 **per-user（NSIS currentUser）** 模式，避免需要管理员权限写 HKLM。

#### 备选通道：本地 HTTP

- 主程序监听 `127.0.0.1:0`（随机端口），端口与 32 字节随机 token 写入 `app_data/bridge.json`（0600）。
- 扩展无法读取本地文件，因此 token 通过以下方式交换：首次由 host 返回；无 host 时由用户在 Omni 设置页点击「复制配对码」粘贴到扩展选项页（一次性）。
- 接口：`POST /api/v1/tasks`，要求 `Authorization: Bearer <token>`，`Origin` 必须为已知扩展 origin。
- 仅在 Snap / Flatpak 浏览器等 NM 不可用场景启用，默认关闭。

---

## 6. 前端与界面设计

### 6.1 设计原则（与 OmniZip 共享设计系统）

Omni Download 与 OmniZip 属于同一产品家族，**共用同一个设计系统包 [omni-ui](https://github.com/VoidDel/omni-ui)**（令牌 `tokens.css`、窗口骨架与组件样式、React 基础组件、图标、主题解析与 12 套内置主题；规范叙述见 OmniZip `docs/04-ui-design.md`、`docs/05-theming.md`），不另起一套：

- **三层设计令牌**：原始色板 → 语义令牌（`bg.* / fg.* / border.* / accent / success / warning / danger / info / selection.*`）→ 组件令牌；主题 JSON 只定义语义层，其余按 OmniZip 05 §2.1 的规则派生。下载器只新增一个语义色 `bt`（种子 / 磁力徽章，浅色 `#7A5AF8`、深色 `#A48BFF`），其余不增不改。
- **内置主题与模式**：浅色 / 深色 / 跟随系统；默认主题对为晨光 Aurora（浅）+ 午夜 Midnight（深）；复用 OmniZip 全部 12 套内置主题（石墨、纸张、北极、卡布奇诺、日光、高对比…）与 `.omnitheme` 自定义格式；强调色可跟随系统。
- **状态色语义**：下载中 = `accent`，已完成 = `success`，已暂停 = `warning`，失败 = `danger`，做种 = `info`，等待 = `fg.muted`。界面其余部分保持中性，颜色只用来表达状态。
- **字体与字号**：系统字体栈（Segoe UI Variable → SF Pro → system-ui → 苹方 / 微软雅黑 / Noto Sans CJK SC），等宽字体用于路径、地址、快捷键；字号阶梯 11 / 12 / 13 / **14（基准）** / 16 / 20 / 28，行高 1.45，数字 `tabular-nums`；所有尺寸用 `rem`，基准可在 12–20 px 间缩放。
- **形状与间距**：8pt 网格；间距 4 / 8 / 12 / 16 / 24 / 32 / 48；圆角 4（控件）/ 8（卡片、输入）/ 12（浮层、对话框）；阴影只给浮层，两级（菜单 / 对话框）。
- **密度**：紧凑（行 28）/ 舒适（32，默认）/ 宽松（40），映射到 `--row-h` / `--ctrl-h`。
- **图标**：Lucide 线性 1.5–1.6 px；文件类型图标沿用 OmniZip 的单色 / 双色两套（视频紫、压缩包琥珀、文档灰蓝、程序蓝）。
- **材质**：Windows 11 侧栏 / 标题栏 Mica（`window-vibrancy`），macOS 侧栏 vibrancy，Linux 纯色；高对比、省电、远程桌面下自动关闭。
- **动效与无障碍**：150–220 ms ease-out，尊重 `prefers-reduced-motion`；键盘全覆盖，对比度 ≥ AA，进度用 `aria-live="polite"` 播报（节流 5 s）。
- **家族标志**：圆角方形（1024 画布 rx 208，渐变 `#4C86F5 → #2556C6`）+ 白色 2.2 px 线条图形；OmniZip 为 Z 形拉链，Omni Download 为「箭头入托盘」。

### 6.2 主窗口布局（OmniZip 窗口骨架）

```
┌─────────────────┬────────────────────────────────────────────────────────────────────────┐
│ ⬡ Omni Download │ 全部任务 · 12 个任务    [⛓ 粘贴链接、磁力链接，或拖入种子文件  Ctrl+N]  ☾  ─ □ ✕ │ 标题栏 40
│   1.0           ├────────────────────────────────────────────────────────────────────────┤
│ ≡ 全部       12 │ [＋ 新建下载] [▶ 全部开始] [⏸ 全部暂停] [🗑 删除] [···]    不限速 ▾  按添加时间 ▾  ⌕ │ 命令栏 48
│ ⤓ 进行中      5 ├───────────────────────────────────────────────────┬────────────────────┤
│ ✓ 已完成      6 │ 名称 ▲              进度         大小    速度   剩余   状态 │ ubuntu-24.04.3… ⇥  │ 表头 30 · 详情 340
│ 🧲 种子       2 │ 💿 ubuntu….iso HTTP  ▰▰▰▰▱ 62%  5.9 GB  48.2  47 秒 ● 下载中 │ HTTP · ● 下载中 · 主机 │
│ ⚠ 失败        1 │ 🎬 Blender….mp4 HTTP ▰▰▱▱▱ 31%  1.24 GB 12.4  1 分  ● 下载中 │ 概览  分片  文件  节点 │ 文字 + 强调色下划线
│ 🗑 回收站     3 │ 🎬 Big Buck Bunny BT ▰▰▰▰▰ 96%  643 MB  ↓3.1  9 秒  ● 下载中 │ 大小 / 已下载 / 速度 │
│                 │ 🗜 node-v22.zip HTTP ▱▱▱▱▱ 0%   48 MB   —    排队中 ● 等待中 │ 分片进度 ▓▓▓▓░░▓▓░░ │
│ 分类            │ 🗜 dataset….tar FTP  ▰▰▱▱▱ 44%  12.8 GB —    —     ● 已暂停 │ 速度 · 近 2 分钟 ╱╲╱ │
│  🎬 视频      2 │ 💿 Fedora….iso HTTP  ▰▱▱▱▱ 18%  2.9 GB  —    —     ● 失败   │                    │
│  🎵 音乐      1 │ 💿 archlinux….iso BT ▰▰▰▰▰ 100% 1.1 GB  ↑1.2 昨天  ● 做种中 │                    │
│  📄 文档      2 │ 📄 Attention….pdf HTTP ▰▰▰▰▰ 100% 2.3 MB —   10:42 ● 已完成 │                    │
│  🗜 压缩包    3 │ …                                                 │                    │
│  ⚙ 程序       1 │                                                   │                    │
│  💿 镜像      3 │                                                   │                    │
│─────────────────│                                                   │                    │
│ ● 浏览器已连接  │                                                   │ [暂停] [打开文件夹] ⧉ │
│ ⤓ 3 个任务 ▰▰▱  │                                                   │                    │
│ ⚙ 设置  Ctrl+,  ├───────────────────────────────────────────────────┴────────────────────┤
│                 │ ↓ 63.7 MB/s ↑ 2.0 MB/s     12 个任务 · 活动 3 · 等待 1 · 已选 1      D: 剩余 412 GB · ● Chrome、Edge │ 状态栏 28
└─────────────────┴────────────────────────────────────────────────────────────────────────┘
```

| 区域 | 规格与行为 |
|---|---|
| 侧栏 | 240 px，材质（Win11 Mica / macOS vibrancy / Linux 纯色），贯穿全高；可折叠为 52 px 图标栏，宽度可拖动并记忆。顶行 40 px：Logo + 名称 + 版本；固定入口：全部 / 进行中 / 已完成 / 种子 / 失败 / 回收站（右侧计数）；「分类」小标签 + 分类列表；底部：浏览器连接状态行、任务摘要（数量 + 总速度 + 进度条）、「设置」（分组列表向上展开，同 OmniZip 04 §4.9） |
| 标题栏 | 40 px。Windows / Linux 自绘（窗口按钮 46×40，关闭悬停 `#e81123`），macOS overlay 保留红绿灯。左：当前筛选名 + 计数；中右：**粘贴链接框**（复用 OmniZip 搜索框组件，28 px，`Ctrl+N` 聚焦），粘贴 / 回车即识别并打开新建对话框；右：明暗切换 |
| 命令栏 | 48 px，上下文相关：未选中 → 新建 / 全部开始 / 全部暂停 / 删除 / ···；选中 → 暂停 / 继续 / 删除 / 打开文件夹 / 属性。右侧：限速、排序、筛选。宽度 < 800 时按钮只留图标 |
| 任务表格 | 表头 30 px（可排序，列可拖动、显隐、宽度记忆），行高随密度 28 / 32 / 40，虚拟化（TanStack Virtual，同 OmniZip）。列：名称（类型图标 + 文件名 + 协议徽章 HTTP / FTP / BT）/ 进度（按连接数分段的 6 px 进度条 + 百分比）/ 大小 / 速度 / 剩余 / 状态（色点 + 文字）。悬停 `bg.hover`，选中 `selection.bg`；多选、框选、右键菜单、拖拽排序、拖入链接或 `.torrent` 直接添加 |
| 详情面板 | 340 px，停靠右侧（可隐藏）；头部 = 文件名 + 折叠按钮，其下协议徽章 + 状态 + 主机；标签页「概览 / 分片 / 文件（BT）/ 节点（BT）」用文字 + 强调色下划线；底部操作：主按钮（随状态变化：暂停 / 继续 / 重试 / 打开文件）+ 打开文件夹 + 复制链接 |
| 状态栏 | 28 px。左：全局下行 / 上行速度；中：任务计数与选择；右：磁盘剩余、浏览器连接状态 |

### 6.3 其他页面与窗口

| 页面 / 窗口 | 要点 |
|---|---|
| **新建任务对话框** | 多行 URL 文本框（自动识别 http / ftp / magnet / .torrent 路径，每行一个任务）；拖入 `.torrent`；实时探测显示文件名 / 大小 / 是否可续传；保存目录（最近使用 + 浏览）、分类、连接数、限速、自定义 UA / Referer / Cookie / 头（折叠的高级区）；BT 显示文件树勾选；「添加并开始」「仅添加」 |
| **快捷下载框**（浏览器接管） | 独立置顶小窗（约 560×400，无任务栏图标），扩展推送下载时弹出在屏幕中央偏上，主窗口不必弹出；内容：文件名（可编辑）、大小 / 协议 / 是否可续传 / 连接数、来源站点与 Cookie 状态、保存目录、分类、连接数、限速；按钮「开始下载」（Enter）、「稍后下载」（Esc，加入队列并暂停）、「改用浏览器下载」；勾选「此网站以后不再询问」后同站点静默接管；全局开关与站点列表在设置 › 浏览器集成。IPC 的 `silent` 字段为 true 时跳过此框 |
| **设置页**（独立路由，非弹窗） | 常规（语言、主题、主题色、开机自启、关闭到托盘、密度）；下载（默认目录、按分类分目录、并发数、连接数、重名策略、完成后操作）；网络（代理、UA、超时、重试、镜像）；BT（端口、DHT、PEX、UPnP、限速、做种策略、Tracker 列表）；浏览器集成（各浏览器 host 状态 ✅ / ❌、修复、扩展商店链接、拦截规则同步）；高级（日志级别、数据目录、导入 / 导出任务、重置） |
| **托盘菜单** | 显示 / 隐藏主窗口、全部暂停 / 开始、限速快捷切换、退出；托盘图标反映下载活动 |
| **悬浮窗（v2）** | 120×120 半透明圆形进度，置顶，可拖拽；拖入链接即添加；双击打开主窗口 |
| **完成通知** | `tauri-plugin-notification`，点击打开文件所在目录 |
| **首次启动向导** | 选择语言 / 下载目录 → 安装浏览器扩展引导（检测已安装浏览器，给出商店链接）→ 完成 |

### 6.4 前端架构

```
apps/desktop/src/
├─ app/                 路由（react-router 7）、Providers、ThemeProvider
├─ bindings.ts          tauri-specta 自动生成：commands + events 类型
├─ stores/
│  ├─ tasks.ts          Map<TaskId, Task>；selector 按 id 订阅，避免整表重渲染
│  ├─ settings.ts
│  └─ ui.ts             选中项、过滤器、面板开合、密度
├─ features/
│  ├─ task-list/        虚拟列表、行组件（memo）、右键菜单、拖拽排序（dnd-kit）
│  ├─ task-detail/      分片可视化（canvas）、速度曲线（uPlot）、Peers 表
│  ├─ new-task/         对话框、URL 解析、探测、BT 文件树
│  ├─ settings/
│  ├─ titlebar/         自绘标题栏（Windows / Linux），macOS 仅占位
│  └─ onboarding/
├─ components/ui.tsx    薄适配层：从 omni-ui 再导出组件，给 Modal / Toasts / PasswordInput 接上翻译与 store（同 OmniZip 做法）
├─ components/Icon.tsx  再导出 omni-ui 图标 + 下载器自己的 Logo、协议徽章颜色
├─ lib/                 format（字节 / 时间 / 速度）、i18n 初始化、theme（调 Rust 主题服务后用 omni-ui 的 writeTheme 写入）
└─ styles/app.css       只放下载器特有样式（任务表格列宽、分片地图、快捷下载框）；令牌与组件样式来自 omni-ui/styles.css
```

#### Tauri 命令（Phase 1 已实现；类型手写在 `lib/types.ts`、包装在 `lib/api.ts`，见 ADR-003）

```ts
api.bootstrap(): Bootstrap                       // 设置、全部任务、版本、系统下载目录、日志目录
api.listTasks(): Task[]                          // 事件丢失后重新同步
api.setSettings(settings) / setSpeedLimit(bytesPerSec | null)
api.probe(url, options): ProbeInfo               // 新建对话框的实时探测
api.add(items: NewTask[]): Task[]
api.pause(ids) / resume(ids) / restart(ids)      // restart：远端文件变更后从头下载
api.remove(ids) / restore(ids) / purge(ids, deleteFiles)   // 回收站
api.detail(id): TaskDetail                       // 分片视图
api.setTaskSpeedLimit(id, limit)
api.open(id) / reveal(id) / openFolder(path) / diskFree(path)
// 后续阶段：parseTorrent、reorderTasks、getBrowserIntegrationStatus / repairBrowserIntegration
```

失败统一返回 `{ code, message, status? }`，界面按 `code` 选择中文提示（`lib/words.ts`）。

#### 事件

| 事件 | 频率 | 载荷 |
|---|---|---|
| `task:progress` | 4 次/s（有任务运行时） | `{ items: [{ id, downloaded, total, speed, eta, connections }], stats: { downloadSpeed, running } }`，管理器已合并 |
| `task:changed` | 状态、名称、大小或错误变化时 | 完整 `Task` |
| `task:added` / `task:deleted` | 即时 | `Task` / `TaskId[]`（彻底删除；回收站是状态） |
| `task:resync` | 事件积压丢失时 | 无；前端重新 `listTasks` |
| `browser:status` | 变化时（Phase 2） | 哪个浏览器 host 连接中 |
| `deep-link` | 即时（Phase 3） | 由单实例 / 深链接插件转发的 URL，前端弹出新建对话框 |

### 6.5 性能策略

- 进度事件在 Rust 侧聚合节流，前端一次 `set` 更新多行。
- 行组件 `memo` + Zustand `useTaskById(id)` selector，只有数值变化的行重绘。
- 速度曲线用 uPlot（canvas），每任务保留 120 个采样点（2 分钟）。
- 分片可视化用单个 canvas 绘制，而非上千个 DOM 块。
- 列表排序 / 过滤在 store 内以 `useMemo` 缓存 id 数组。
- 首屏仅加载任务列表与设置，详情面板组件懒加载（`React.lazy`）。

---

## 7. 跨平台与系统集成

### 7.1 Tauri 插件清单

| 插件 | 用途 |
|---|---|
| `tauri-plugin-single-instance`（`deep-link` feature） | 单实例，第二实例参数转发 |
| `tauri-plugin-deep-link` | 注册 `magnet:` 与 `omni:` 协议 |
| `tauri-plugin-autostart` | 开机自启（`--background` 参数静默启动到托盘） |
| `tauri-plugin-notification` | 完成 / 失败通知 |
| `tauri-plugin-dialog` | 目录 / 文件选择、确认框 |
| `tauri-plugin-opener` | 打开文件、在文件管理器中显示、打开外链 |
| `tauri-plugin-fs` | 前端有限的文件读写（导入 / 导出） |
| `tauri-plugin-updater` | 自动更新（签名校验） |
| `tauri-plugin-store` | 设置 JSON |
| `tauri-plugin-window-state` | 窗口位置 / 大小记忆 |
| `tauri-plugin-log` | 统一日志（文件 + 控制台，按天滚动） |
| `tauri-plugin-clipboard-manager` | 剪贴板监控（v2）、复制链接 |
| `tauri-plugin-global-shortcut` | 全局快捷键（显示窗口 / 新建） |
| `tauri-plugin-os` | 平台判断 |
| `tauri-plugin-process` | 重启应用（更新后） |

### 7.2 协议与文件关联

```jsonc
// tauri.conf.json（节选）
{
  "identifier": "app.omnidownload.desktop",
  "plugins": {
    "deep-link": { "desktop": { "schemes": ["magnet", "omni"] } }
  },
  "bundle": {
    "fileAssociations": [
      { "ext": ["torrent"], "mimeType": "application/x-bittorrent",
        "description": "BitTorrent 种子文件", "role": "Viewer" }
    ]
  }
}
```

- Windows：安装器写入 `HKCU\Software\Classes\magnet`，Tauri 自动处理。
- macOS：`Info.plist` `CFBundleURLTypes` / `CFBundleDocumentTypes`，由 bundler 生成。
- Linux：`.desktop` 文件 `MimeType=x-scheme-handler/magnet;application/x-bittorrent;`，安装后 `update-desktop-database`。
- 首次启动检测是否为 `magnet:` 默认处理程序，否则提示「设为默认」。

### 7.3 窗口与托盘

- 主窗口：最小 900×600，默认 1200×760，`decorations: false`（Windows / Linux）、`titleBarStyle: Overlay`（macOS），`transparent: false`。
- 关闭按钮默认最小化到托盘（可配）；托盘图标在三平台使用模板图标（macOS 单色模板、Windows 彩色、Linux 依赖 `libayatana-appindicator`）。
- `--background` 启动参数：不创建窗口，仅托盘 + 引擎（开机自启与 host 拉起使用）。

### 7.4 打包与分发

| 平台 | 格式 | 签名 / 公证 | 说明 |
|---|---|---|---|
| Windows | **NSIS**（per-user，推荐）；MSI 可选 | Authenticode（Azure Trusted Signing 或 EV 证书） | per-user 安装便于写 HKCU NM 注册表、无 UAC；安装器同时放置 `omnidl-nm-host.exe` |
| macOS | **.dmg**（Apple Silicon / aarch64，不提供 Intel 版） | Developer ID + `notarytool` 公证，Hardened Runtime | 不启用 App Sandbox（需写 NM 清单到 `~/Library/Application Support/…`） |
| Linux | **.deb**、**.rpm**、**AppImage** | 无需签名；可提供 GPG 签名的 sha256sums | 依赖 `webkit2gtk-4.1`、`libayatana-appindicator3`；不优先 Flatpak（NM 与托盘受限），可社区维护 |

- 自动更新：`tauri-plugin-updater` + GitHub Releases 作为更新源，`latest.json` 由 CI 生成并用 minisign 密钥签名。
- 版本策略：SemVer；`main` 分支每夜构建 nightly；`release/*` 分支 tag 触发正式发布。
- `omnidl-nm-host` 作为 `externalBin`（sidecar）随主程序打包，位于与主可执行文件同级目录，清单 `path` 指向它。

### 7.5 平台差异汇总

| 事项 | Windows | macOS | Linux |
|---|---|---|---|
| WebView | WebView2（Win10/11 内置，安装器含 bootstrapper） | WKWebView | webkit2gtk 4.1（Ubuntu 22.04+ / Fedora 36+） |
| 标题栏 | 自绘 + Mica | 原生红绿灯 + Overlay | 自绘（GNOME / KDE 兼容） |
| 开机自启 | 注册表 Run 键 | LaunchAgent | `~/.config/autostart/*.desktop` |
| 通知 | WinRT Toast | UNUserNotification | libnotify |
| BT 端口 | 首次监听触发防火墙弹窗（文案提示） | 首次触发防火墙询问 | 通常无阻拦 |
| 托盘 | 稳定 | 稳定 | 依赖桌面环境（GNOME 需扩展） |
| 文件管理器显示 | `explorer /select,` | `open -R` | `xdg-open` 目录（或 DBus `FileManager1.ShowItems`） |

---

## 8. 安全设计

### 8.1 Tauri 层

- `capabilities/main.json` 最小授权：仅 `core:window`、`core:event`、所需插件的具体权限；详情窗口 / 设置窗口分别授权。
- CSP：`default-src 'self'; img-src 'self' data: asset:; style-src 'self' 'unsafe-inline'; connect-src ipc: http://ipc.localhost`，禁止远程脚本。
- 不加载任何远程 URL 到 WebView；外链一律 `opener` 打开系统浏览器。
- 前端 `fs` 权限限定在 `$APPDATA` 与用户显式选择的目录（通过 dialog scope 动态授予）。

### 8.2 下载引擎

- **路径安全**：文件名清洗（见 4.2.2）；最终路径必须位于 `save_dir` 内（`canonicalize` 后前缀校验）；BT 多文件种子中的每个路径同样校验，拒绝绝对路径与 `..`。
- **TLS**：rustls，默认严格校验；「忽略证书错误」仅可按任务开启并显著警告。
- **重定向**：拒绝从 https 降级到 http 的重定向（可配置放行）；拒绝 `file:` 等非网络 scheme。
- **凭据**：Cookie、认证头落库加密（AES-256-GCM，密钥存系统凭据库）；日志对 `Cookie` / `Authorization` 脱敏；任务导出时默认不含凭据。
- **BT**：默认不公开 IP 过滤；v2 支持导入 blocklist；DHT 仅在用户开启时监听。

### 8.3 浏览器桥

- NM 清单 `allowed_origins` / `allowed_extensions` 仅包含官方扩展 ID。
- IPC 端点权限 0600（Unix）/ 当前用户 SID DACL（Windows 命名管道），防止其他本机用户接入。
- 本地 HTTP 备选通道：仅绑 127.0.0.1、Bearer token、校验 `Origin` 为扩展 origin、不设 `Access-Control-Allow-Origin: *`、限流。
- 扩展接收的消息仅在已配对连接上处理；`add_download` 的 URL 在主程序侧再次校验 scheme。

### 8.4 供应链与发布

- CI 中运行 `cargo audit`、`cargo deny`（许可证 + 漏洞 + 来源）、`pnpm audit`。
- 依赖锁定（`Cargo.lock`、`pnpm-lock.yaml`）；Dependabot / Renovate 每周 PR。
- 更新包 minisign 签名，私钥仅存于 CI secret；安装包代码签名。
- 发布 SBOM（`cargo cyclonedx`）。

---

## 9. 项目结构

```
OmniDownloadManager/
├─ apps/
│  ├─ desktop/                      # Tauri 应用
│  │  ├─ src/                       # React 前端（见 6.4）
│  │  ├─ src-tauri/                 # omnidl-desktop crate
│  │  │  ├─ src/
│  │  │  │  ├─ main.rs / lib.rs
│  │  │  │  ├─ commands/            # 按领域拆分：tasks.rs settings.rs browser.rs system.rs
│  │  │  │  ├─ events.rs            # EventBridge 节流聚合
│  │  │  │  ├─ ipc_server.rs        # 本地 IPC 服务端（host 接入）
│  │  │  │  ├─ tray.rs
│  │  │  │  └─ integration/         # NM 清单写入、协议关联检测
│  │  │  ├─ capabilities/
│  │  │  ├─ icons/
│  │  │  └─ tauri.conf.json
│  │  ├─ index.html · vite.config.ts · package.json
│  └─ extension/                    # 浏览器扩展（WXT，见 5.2）
├─ crates/
│  ├─ omnidl-core/
│  ├─ omnidl-http/
│  ├─ omnidl-ftp/
│  ├─ omnidl-bt/
│  ├─ omnidl-storage/
│  │  └─ migrations/
│  ├─ omnidl-ipc/
│  └─ omnidl-nm-host/
├─ packages/
│  └─ protocol/                     # specta 生成的 TS 类型（桌面前端与扩展共用）；UI 令牌与组件来自外部依赖 omni-ui，不在本仓库
├─ docs/
│  ├─ development-plan.md           # 本文档
│  ├─ architecture/                 # ADR（架构决策记录）
│  └─ design/                       # 设计稿导出、设计 token
├─ scripts/                         # 生成图标、签名、发布脚本
├─ .github/workflows/
│  ├─ ci.yml                        # lint + test（三平台矩阵）
│  ├─ release.yml                   # tauri-action 打包签名发布
│  └─ extension.yml                 # 扩展构建与商店上传
├─ Cargo.toml                       # [workspace] members = ["crates/*", "apps/desktop/src-tauri"]
├─ package.json · pnpm-workspace.yaml
├─ themes/                          # omni-ui 内置主题副本（pnpm sync:omni-ui），供 Rust include_str!
├─ icons/app-icon.svg               # 应用图标源文件（pnpm icons 生成各平台尺寸）
├─ rust-toolchain.toml · rustfmt.toml · clippy.toml · deny.toml
└─ README.md · LICENSE（MIT，与 OmniZip、omni-ui 相同）
```

---

## 10. 开发路线图

按单人全职估算；两人并行（一人 Rust 引擎、一人前端 / 扩展）可压缩约 40%。

### Phase 0 — 脚手架与设计（1 周）

- [x] 初始化 monorepo：cargo workspace（7 个 `omnidl-*` crate 骨架 + `omnidl-desktop`）、pnpm workspace、Tauri 2 应用（React 19 + TS + Vite）
- [x] CI：Linux 上主题同步校验、`tsc`、`eslint`、`rustfmt`、`clippy -D warnings`；Windows / macOS / Linux 三平台 `cargo test --workspace` 与 Vitest（`.github/workflows/ci.yml`）
- [x] 依赖 `omni-ui`（`github:VoidDel/omni-ui#main`），`main.tsx` 先 `import 'omni-ui/styles.css'` 再引入自己的 `app.css`；`scripts/sync-omni-ui.mjs` 把包内主题复制到 `themes/`；窗口骨架（侧栏、标题栏与窗口按钮、命令栏、空列表、状态栏、Linux 边缘缩放、浅色 / 深色 / 跟随系统）按设计画布「OmniDownloadManager UI 设计」实现
- [x] 确定图标、名称、bundle identifier、许可证：Omni Download、`omnidl-` 前缀、`app.omnidownload.desktop`、MIT，图标 `icons/app-icon.svg`（见 ADR-001）
- [x] [ADR-001 技术选型与命名](architecture/ADR-001-tech-stack.md)、[ADR-002 浏览器桥方案](architecture/ADR-002-browser-bridge.md)

**验收**：空白 Tauri 窗口三平台可运行，CI 绿。已达成：Windows 本机验证通过，首次推送的 CI（run 42568639421）lint 与 Windows / macOS / Linux 测试全部通过。

### Phase 1 — HTTP 引擎 + 核心 UI（3–4 周）

设计决定与偏离原方案之处见 [ADR-003 下载引擎与任务管理](architecture/ADR-003-download-engine.md)。

- [x] `omnidl-core`：Task 模型与状态机、`TaskManager`（优先级 + 列表顺序调度、并发槽位、持久化、退出前保存）、分片表与动态窃取（属性测试）、文件名清洗 / 重名编号 / 分类、令牌桶限速（全局 + 任务串联）、EventBus（进度每 250 ms 合并一次）
- [x] `omnidl-storage`：sqlx 迁移与仓储、写入线程（合并相邻块，`sync_data` 在克隆句柄上异步执行）、预分配（Windows 稀疏文件，Linux / macOS `fallocate`）、剩余空间
- [x] `omnidl-http`：探测（HEAD，被拒或信息不全时 `Range: bytes=0-0`）、文件名推断（RFC 6266 / 5987、URL、Content-Type）、分片 + 动态窃取、续传前校验 ETag / Last-Modified / 大小、指数退避重试、429 / 503 让出连接、忽略 Range 时降级单连接、系统 / 自定义代理、限速、按 Last-Modified 设置文件时间
- [x] `omnidl-desktop`：命令 / 事件（类型手写，未用仍是 RC 的 tauri-specta，见 ADR-003）、单实例、日志（按天滚动，保留 7 天）、设置文件（TOML）、退出时等待任务保存进度
- [x] 前端：自绘标题栏（粘贴框识别链接）、导航与分类计数、虚拟任务列表（排序、多选、右键菜单、快捷键）、新建对话框（逐条实时探测）、详情面板（概览 / 分片 / 近 2 分钟速度）、设置页（常规 / 下载 / 网络）、12 套内置主题与密度、回收站、全局限速
- [x] 集成测试：`axum` mock 服务器（`crates/omnidl-http/tests/common`，内容按偏移生成），覆盖多连接、不支持 Range、HEAD 被拒、断流重试、暂停续传、强杀后从检查点续传、远端文件变更、404、重名、未知长度与空文件、限速，以及管理器 + 真实引擎的退出重启续传；`cargo run -p omnidl-http --example test_server` 可手动试用

**验收**：可下载 5 GB 文件，8 连接打满带宽，崩溃重启后续传成功，1000 条任务列表流畅。Windows 本机结果：

- 5 GiB 文件 8 连接（本机回环，`cargo test -p omnidl-http --release --test download -- --ignored large_file`）约 720 MiB/s；下载到 40% 时强杀，从上一个检查点续传完成，逐字节校验一致。
- 界面中关闭窗口或强杀进程后重新启动，未完成的任务自动从保存的位置继续。
- 1005 条任务的列表 3 秒内从头滚到尾再回来，平均每帧 6.3 ms、最长 6.4 ms，DOM 中只有约 32 行；1000 个链接一次添加用时 0.9 s。
- 真实网络上的带宽占用需在实际网络环境中确认；macOS / Linux 由 CI 验证。

### Phase 2 — 浏览器扩展与 Native Messaging（2 周）

实现细节与对原方案的调整见 [ADR-002 实施记录](architecture/ADR-002-browser-bridge.md#实施记录phase-22026-11-09)。

- [x] `omnidl-ipc` 协议（消息类型、长度前缀帧、每用户端点、阻塞客户端与 Tokio 服务端）、`omnidl-desktop` IPC 服务端、`omnidl-nm-host`（转发、按需以 `--background` 拉起应用）
- [x] NM 清单写入（五种浏览器 × 三平台）、设置页「浏览器」：各浏览器状态、重新写入、接管前询问、不再询问的网站、备用通道与配对码
- [x] 快捷下载框（置顶小窗，Enter 开始 / Esc 稍后 / 改用浏览器下载 / 此网站不再询问），用户的选择回传给扩展
- [x] WXT 扩展：拦截（Chromium 用 `onDeterminingFilename`、Firefox 用阻塞式 `webRequest`，各有开始后接管的备用路径）、Alt+点击不接管、右键菜单（链接、媒体、本页文件链接）、Alt+Shift+D、Cookie 收集、popup、options
- [x] 本地 HTTP 备选通道 + 配对码（默认关闭）
- [x] Cookie 与请求头落库加密（AES-256-GCM，密钥文件在 Windows 上由 DPAPI 保护）
- [x] 扩展 E2E（Playwright 加载 unpacked）：CI 中在 Chromium 跑接管、改用浏览器、小文件放行、popup、备用路径；`e2e/native.spec.ts` 用真实 Edge 与应用构建跑完整链路（本地可选）

**验收**：三浏览器点击下载自动接管，带 Cookie 的登录态文件可下载；Omni 未运行时自动拉起。Windows 本机结果：

- Edge 157（真实浏览器，加载未打包扩展）→ Native Messaging → host → 应用未运行时自动拉起（后台，不显示主窗口）→ 快捷下载框显示文件名、40 MB、支持断点续传、已附带 Cookie 与 Referer → 开始后 Edge 放弃自己的下载，应用带登录 Cookie 下载 40 MB 文件（无 Cookie 时服务器返回 403），内容逐字节核对一致（`OMNIDL_APP=… pnpm --filter omnidl-extension exec playwright test native`）。
- Chromium 内核的接管、改用浏览器、小文件放行、备用路径与 popup 均由 Playwright 测试覆盖。
- 本机没有安装 Chrome 与 Firefox，未在这两个浏览器中实测：Chrome 与 Edge 走同一套 Chromium 代码；Firefox 的阻塞式 `webRequest` 路径只经过构建与单元测试，需要在装有 Firefox 的机器上确认。

### Phase 3 — BitTorrent / 磁力（2–3 周）

设计决定与偏离原方案之处见 [ADR-004 BitTorrent 与磁力链接](architecture/ADR-004-bittorrent.md)。

- [x] `omnidl-bt`：会话首次使用时启动（端口被占用时改用任意端口）、添加流程（本地 / HTTP 种子或磁力元数据 → 文件选择）、元数据按任务保存、统计映射（已校验字节、节点数、累计上传）、暂停 / 恢复与重启后续传（校验已有数据）、做种策略（分享率 / 时长，做种不占下载槽位）、磁力链接的 `x.pe` 节点
- [x] 前端：新建对话框的种子文件树（勾选文件夹 / 文件）、详情面板的文件（可改选）/ 节点 / Tracker、上传速度与分享率、状态栏上传速度、设置页 BT 分组（做种、上传限速、端口、DHT、UPnP、设为磁力链接默认程序）
- [x] 深链接 `magnet:`（deep-link 插件）、`.torrent` 文件关联、命令行与第二个实例传来的链接、拖入窗口、浏览器扩展的 `add_magnet`
- [x] 集成测试：本地两个 librqbit 实例互传（`crates/omnidl-bt/tests/swarm.rs`）；管理器的做种状态、槽位与重启用假引擎测试

**验收**：磁力链接 30 s 内获取元数据并开始下载；重启后 BT 任务恢复；做种率达上限自动停止。Windows 本机结果：

- 测试构建（独立标识）带 `.torrent` 参数启动 → 新建对话框列出 3 个文件 → 取消勾选一个 → 从本机做种方（`cargo run -p omnidl-bt --example seed`，限速 3 MiB/s）下载，所选文件逐字节一致 → 下载完成转为做种中、释放槽位 → 第二次启动带磁力链接，约 1 s 取得元数据（经 `x.pe`）→ 停止做种后为已完成。
- 重启恢复与分享率上限由集成测试覆盖：退出时下载了一部分的任务在新会话中校验已有数据后继续并完成；分享率 1.0 时，另一节点下载完整份后任务自动完成。
- 公网 DHT / Tracker 上的磁力链接取元数据速度取决于网络与种子热度，需要在实际网络中确认。

### Phase 4 — FTP / FTPS（1–2 周）

- [x] `omnidl-ftp`：探测（登录、`SIZE`、`MDTM`）、多会话（默认 2、上限 8，`421` 时减少会话）、`REST` 续传（续传前核对大小与修改时间，服务器不支持 `REST` 时退回单会话从头下载）、FTPS（`ftps://` 显式 `AUTH TLS`，端口 990 隐式；系统证书库校验，可按任务接受自签名证书）、被动 / 主动模式
- [x] 前端：新建对话框的 FTP 账号（链接里的账号同样可用）、传输模式与证书选项，账号变化后重新探测；设置 › 网络的 FTP 会话数与默认传输模式
- [x] 集成测试：进程内 libunftp 服务器（`crates/omnidl-ftp/tests/ftp.rs`，代替 Docker `pure-ftpd`，Windows / macOS / Linux 的 CI 都能跑）：匿名多会话、账号登录与密码错误、暂停后 `REST` 续传、文件变更后拒绝续传、文件不存在、FTPS（rcgen 自签名证书）

实现上的取舍：

- 与 HTTP 引擎共用分片表、写入线程与检查点（ADR-003）；领取空间、编号命名、完成后改名和重试退避抽到 `omnidl_core::disk`。
- 一段在文件中途结束（到了分片终点或被窃取）时直接关闭这个会话，下一段重新登录，不发 `ABOR`：各服务器对 `ABOR` 的处理差异大，重新登录更可靠。传输读到文件末尾的会话会继续用于下一段。
- 链接里的密码在添加任务时移到任务选项（随 Cookie 一起加密保存），列表、详情与「复制链接」只显示不含账号的链接。
- 代理设置只用于 HTTP；FTP 直连。

**验收**：匿名与凭据 FTP 下载、续传、FTPS 均通过。Windows 本机结果：

- 上述集成测试全部通过（多次重复运行稳定）。
- 测试构建（独立标识）对本机 FTP 服务器（`cargo run -p omnidl-ftp --example ftp_server`，需要账号）：新建对话框匿名探测显示需要登录 → 填入账号后重新探测显示 24 MB、支持断点续传、2 个会话 → 限速 6 MB/s 下载完成，文件逐字节一致。

### Phase 5 — 系统集成与发布工程（2 周）

- [x] 托盘、开机自启、`--background`、通知、全局快捷键、窗口状态记忆
- [x] 自动更新（updater + minisign）
- [x] 三平台打包：NSIS、dmg（Apple Silicon）、deb / rpm / AppImage；sidecar 打包 host
- [ ] 代码签名与 macOS 公证流水线
- [x] i18n（zh-CN / zh-TW / en-US / ja-JP），首次启动向导
- [ ] 扩展上架：Chrome Web Store、Edge Add-ons、AMO（上架包、隐私政策与提交说明已就绪，见 [extension-publishing.md](extension-publishing.md)；待提交并把商店分配的 ID 加入应用）

**验收**：三平台安装包一键安装可用，更新可从 0.9 升到 1.0，扩展商店审核通过。

### Phase 6 — 打磨与 Beta（2 周）

- [x] 性能：对照 1.3 节实测（结果见下；窗口打开时的内存与 AppImage 体积未达标）
- [x] 空状态、错误提示文案（每种错误附处理建议）、无障碍审计（axe-core + 键盘走查）
- [x] 无障碍遗留：文字对比度与对话框标注已在 omni-ui 中修改（见下），依赖已升级
- [ ] 公测收集反馈，修复 P0 / P1（反馈入口已就绪：Issue 模板、“设置 › 常规 › 帮助与反馈”）
- [x] 文档：[使用手册](user-guide.md)、[常见问题](faq.md)、[贡献指南](../CONTRIBUTING.md)

**验收**：满足 1.3 节非功能指标，发布 v1.0。

**性能实测**（Windows 11，release 构建，独立标识的测试副本，WebView2 调试端口打开）：

| 指标 | 目标 | 实测 | 结论 |
|---|---|---|---|
| 启动到首屏 | < 1.5 s | 0.49 s（5 次中位数，热启动；1000 个任务时 0.53 s） | 达标；重启后的冷启动未测 |
| 空闲内存（含 WebView） | < 80 MB | 在托盘或后台启动时：只有应用进程，8–11 MB（私有工作集）。窗口打开时：应用进程 22 MB + WebView2 六个进程，合计 146–178 MB | 在托盘时达标；窗口打开时**未达标** |
| 1000 个任务列表滚动 | 60 fps | 6 秒来回滚动 959 帧，最慢一帧 8.9 ms（屏幕 160 Hz）；DOM 中只有 32 行 | 达标 |
| 单任务 HTTP 吞吐 | ≥ 100 MB/s | 本机回环 4 GiB：平均 928 MB/s，峰值 1308 MB/s，占用约 1.9 个核 | 达标 |
| 安装包体积 | Win < 15 MB，macOS < 20 MB，AppImage < 25 MB | NSIS 8.0 MB，dmg 10.2 MB，deb 12.7 MB，rpm 12.7 MB，AppImage 90 MB | AppImage **未达标** |
| 进度刷新延迟 | ≤ 250 ms | 引擎每 250 ms 推送一次；界面上进度变化的间隔中位数 250 ms | 达标 |

- 内存几乎全在 WebView2。收到托盘时窗口被销毁、WebView2 的进程随之退出，再次显示时重建（下载、托盘、通知、浏览器接管都在 Rust 侧，不受影响）；以 `--background` 启动时不创建窗口。代价是从托盘恢复窗口不再是瞬间，实测再次启动到窗口可见不到 1 秒；界面状态（筛选、选中）回到初始。窗口打开时的占用由 WebView2 决定，应用无法再压。
- AppImage 自带 WebKitGTK 及其依赖，体积由此决定；deb / rpm 依赖系统的 WebKitGTK，在 13 MB 以内。25 MB 的目标对 AppImage 不现实，建议改为只约束 deb / rpm。

**无障碍审计**（axe-core 4.14，浅色与深色下的任务列表、详情各页、右键菜单、空状态、新建下载、设置六页；另用键盘逐一走查）：

已修复（应用内）：

- 任务列表的 ARIA 结构：表头按钮放进 `columnheader` 单元格，滚动区标为 `rowgroup`。
- 列表获得键盘焦点时没有任何指示：现在光标所在行有焦点环，并通过 `aria-activedescendant` 告知读屏软件。
- 菜单键 / `Shift+F10` 打开选中任务的菜单；菜单关闭后焦点回到列表。
- 标题栏的当前视图名标为一级标题；快捷键录制框的可读名称包含它显示的内容。

已在 omni-ui 中修复：

- 对比度不足 4.5:1 的文字：`--fg-muted`（提示、分组标题、快捷键，原约 3.7:1）改为取达到 4.5:1 的最低透明度；新增文字用色令牌 `--accent-text` `--success-text` `--warning-text` `--danger-text` `--info-text` `--bt-text`，用于选中的导航项、状态文字、错误提示和协议徽标；主按钮上的文字在白色不足 4.5:1 时改用深色（午夜等主题）。
- `Modal` 以标题为可读名称，其 `<header>` / `<footer>` 不再算作页面级地标。
- `Segmented` 改为单选组：一个 Tab 停靠点，方向键切换。

升级后复查（关闭窗口材质，浅色与深色）：对比度、对话框名称、地标三类问题均为 0，只剩右键菜单的一条最佳实践提示（菜单内容不在地标内）。OmniZip 的主题由 Rust 侧派生，它的 `theme.rs` 补上同样的派生之前，OmniZip 的颜色不变。

### v2.0 候选（后续）

剪贴板监控、批量下载、媒体嗅探、悬浮窗、计划任务、FTP 目录下载、文件校验、镜像 URL、HLS（m3u8）下载、IP 过滤、边下边播。

已完成（2026-11-10）：

- [x] 悬浮窗：72 px 圆形置顶窗口（Windows 上按圆形裁切，透明角落不挡点击），显示总进度环与速度，跟随主题；拖入链接 / `.torrent` 即添加，双击打开主窗口，右键原生菜单。Windows 11 的 Mica / Acrylic 无法用于裁切过的窗口，悬浮球不使用窗口材质。
- [x] 剪贴板监控（默认关闭）：只在剪贴板内容全部是链接、且为可下载的文件类型或磁力 / 种子链接、且不在任务列表中时弹出快捷下载框；Windows 上按剪贴板序号判断变化。
- [x] 文件校验：任务选项 `checksum`（MD5 / SHA-1 / SHA-256 / SHA-512），完成时核对，不符时任务保留“已完成”并带 `checksum_mismatch` 错误、通知提示，可重新下载；详情面板可事后设置、核对，或计算 MD5 / SHA-1 / SHA-256。
- [x] 批量下载：新建下载里的链接可写编号范围（`[1-20]`、`[01-20]`、`[a-z]`、`[0-100:5]`，可多段组合），展开为多个任务，一次最多 1000 个。
- [x] 计划任务：设置 › 下载里的定时开始（开始已暂停的任务）、定时停止（暂停下载与等待中的任务）、按星期重复；全部下载完成后退出 / 睡眠 / 关机（60 秒倒计时可取消，关机前先保存进度）。

---

## 11. 测试与质量保障

| 层级 | 工具 | 覆盖重点 |
|---|---|---|
| Rust 单元测试 | `cargo test`、`proptest` | 分片分裂算法（属性测试：分片并集 == 全文件且不重叠）、文件名清洗、状态机转换、限速器 |
| Rust 集成测试 | `axum` mock HTTP（Range / 非 Range / ETag 变化 / 断连注入）、`pure-ftpd` Docker、双 librqbit 实例 | 续传正确性（下载后 SHA-256 与源一致）、错误恢复 |
| 性能基准 | `criterion` | 分片调度、Writer 吞吐 |
| 前端单元 | Vitest + React Testing Library | store 逻辑、格式化函数、URL 解析 |
| 前端组件 | Storybook（可选） | 深浅主题下的组件快照 |
| 桌面 E2E | `tauri-driver`（WebDriver，Windows / Linux） + 手动 macOS 冒烟 | 新建 → 下载 → 完成 → 打开目录 |
| 扩展 E2E | Playwright（Chromium 加载 unpacked 扩展）+ Firefox `web-ext run` | 拦截、右键菜单、NM 往返 |
| 静态检查 | clippy（`-D warnings`）、rustfmt、eslint、tsc `strict`、`cargo deny` | 每次 PR |
| CI 矩阵 | `windows-latest`、`macos-14`、`ubuntu-22.04` | 构建 + 测试 + 产物上传 |

发布前人工检查清单：三平台干净虚拟机安装、卸载残留、防火墙弹窗文案、HiDPI 缩放、深浅主题切换、NM 在五个浏览器的连接状态。

---

## 12. 风险与对策

| 风险 | 影响 | 对策 |
|---|---|---|
| Tauri 3 处于 alpha，API 可能变化 | 中 | 锁定 Tauri 2.x；插件均用 2.x 稳定版；关注 3.0 迁移指南，架构上命令层与引擎层解耦便于迁移 |
| Linux webkit2gtk 版本碎片化、GNOME 托盘需扩展 | 中 | 仅承诺 Ubuntu 22.04+ / Fedora 39+；AppImage 内置依赖；托盘不可用时降级为仅窗口 |
| librqbit API 在大版本间变动 | 中 | `omnidl-bt` 为 adapter 层，核心不直接依赖 librqbit 类型；锁定 8.x，升级有集成测试兜底 |
| Chromium `onDeterminingFilename` 与其他扩展冲突 | 低 | 自动回退 `onCreated` + cancel；popup 提示冲突 |
| Snap / Flatpak 浏览器无法使用 NM | 中 | 本地 HTTP 备选通道 + 配对码；文档说明 |
| MV3 service worker 休眠导致 NM 连接断开 | 低 | 每次请求按需 `connectNative`，host 轻量快速启动；或使用 `sendNativeMessage` 单次模式 |
| 未签名二进制触发 Windows SmartScreen / Defender 误报 | 高（影响安装率） | 购买代码签名证书（Azure Trusted Signing 成本低）；提交 Microsoft 误报申诉；开源仓库可见 |
| macOS 公证流程复杂 | 中 | CI 自动化 `notarytool`；预留 Apple Developer 账号费用 |
| 高并发写盘导致 HDD 性能下降 | 低 | 单 Writer 顺序合并写入；预分配；可配置连接数 |
| 服务器反爬 / 限速（429） | 中 | 自动降并发、指数退避、可自定义 UA / Cookie |
| BT 功能涉及版权争议 | 中 | 不内置搜索 / 资源站；默认不做种到无限；许可证与免责声明明确 |
| 商店审核拒绝 `<all_urls>` 权限 | 中 | 审核说明用途；Cookie 读取改为可选权限（`optional_host_permissions`），用户按需授予 |
| 单人开发周期长 | 中 | 严格按 Phase 交付，MVP 先只做 HTTP；社区贡献 |

---

## 13. 附录

### A. omnidl-ipc 消息定义（Rust，specta 导出到 TS）

```rust
#[derive(Serialize, Deserialize, specta::Type)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum BridgeRequest {
    Ping,
    GetStatus,
    OpenApp,
    AddDownload(AddDownloadPayload),
    AddMagnet { uri: String },
    AddBatch { items: Vec<AddDownloadPayload> },
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct AddDownloadPayload {
    pub url: String,
    pub filename: Option<String>,
    pub size: Option<u64>,
    pub referer: Option<String>,
    pub user_agent: Option<String>,
    pub cookies: Option<String>,
    pub headers: Option<HashMap<String, String>>,
    pub mime: Option<String>,
    pub tab_url: Option<String>,
    pub silent: bool,              // true 时不弹新建对话框，直接按默认设置开始
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct BridgeEnvelope<T> { pub id: String, #[serde(flatten)] pub body: T }

#[derive(Serialize, Deserialize, specta::Type)]
#[serde(untagged)]
pub enum BridgeResponse {
    Ok { ok: bool, result: serde_json::Value },
    Err { ok: bool, error: BridgeError },
}
```

### B. TaskOptions

```rust
#[derive(Serialize, Deserialize, specta::Type, Clone, Default)]
pub struct TaskOptions {
    pub connections: Option<u8>,            // None = 使用全局默认
    pub speed_limit_bps: Option<u64>,
    pub user_agent: Option<String>,
    pub referer: Option<String>,
    pub cookies: Option<String>,            // 落库加密
    pub headers: HashMap<String, String>,   // 落库加密
    pub proxy: ProxyConfig,                 // System | None | Http(url) | Socks5(url)
    pub auth: Option<Credentials>,
    pub insecure_tls: bool,
    pub mirrors: Vec<String>,
    pub checksum: Option<Checksum>,         // { algo, hex }
    pub bt: Option<BtOptions>,              // 选中文件、做种策略、顺序下载
    pub on_complete: OnComplete,            // None | Notify | OpenFolder | RunCommand(..)
}
```

### C. 核心依赖（Cargo.toml 工作区节选）

```toml
[workspace.dependencies]
tokio = { version = "1", features = ["full"] }
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "stream", "socks", "http2", "cookies"] }
suppaftp = { version = "6", features = ["async-rustls"] }
librqbit = "8"
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite", "migrate", "chrono", "uuid"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
specta = "2"
tauri-specta = { version = "2", features = ["derive", "typescript"] }
tauri = { version = "2", features = ["tray-icon", "image-png"] }
interprocess = { version = "2", features = ["tokio"] }
tokio-util = { version = "0.7", features = ["codec"] }
uuid = { version = "1", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
thiserror = "2"
anyhow = "1"
tracing = "0.1"
parking_lot = "0.12"
bytes = "1"
url = "2"
percent-encoding = "2"
keyring = "3"
aes-gcm = "0.10"
sysinfo = "0.33"
async-speed-limit = "0.4"
```

### D. 前端依赖（package.json 节选）

```json
{
  "dependencies": {
    "react": "^19", "react-dom": "^19", "react-router": "^7",
    "@tauri-apps/api": "^2", "@tauri-apps/plugin-*": "^2",
    "zustand": "^5", "@tanstack/react-virtual": "^3",
    "motion": "^12", "lucide-react": "latest",
    "uplot": "^1.6", "react-hook-form": "^7", "zod": "^3",
    "i18next": "^25", "react-i18next": "^15",
    "@dnd-kit/core": "^6", "@dnd-kit/sortable": "^10",
    "radix-ui": "latest", "class-variance-authority": "^0.7", "tailwind-merge": "^3"
  },
  "devDependencies": {
    "vite": "^7", "typescript": "^5", "@vitejs/plugin-react": "^4",
    "tailwindcss": "^4", "@tailwindcss/vite": "^4",
    "vitest": "^3", "@testing-library/react": "^16",
    "eslint": "^9", "@tauri-apps/cli": "^2", "wxt": "^0.20"
  }
}
```

### E. 参考资料

- Tauri 2 文档：https://v2.tauri.app/
- Tauri 官方插件工作区：https://github.com/tauri-apps/plugins-workspace
- tauri-specta：https://github.com/specta-rs/tauri-specta
- librqbit（rqbit）：https://github.com/ikatson/rqbit · https://docs.rs/librqbit
- suppaftp：https://github.com/veeso/suppaftp
- Chrome Native Messaging：https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging
- Firefox Native Messaging：https://developer.mozilla.org/docs/Mozilla/Add-ons/WebExtensions/Native_messaging
- Chrome `downloads` API：https://developer.chrome.com/docs/extensions/reference/api/downloads
- WXT：https://wxt.dev/
- shadcn/ui：https://ui.shadcn.com/ · Tailwind CSS v4：https://tailwindcss.com/
- RFC 6266（Content-Disposition）、RFC 7233（Range Requests）、BEP 3 / 5 / 9 / 11（BitTorrent 协议、DHT、磁力元数据、PEX）
