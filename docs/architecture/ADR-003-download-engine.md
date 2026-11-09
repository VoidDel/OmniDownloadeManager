# ADR-003 下载引擎与任务管理

- 状态：已采纳（Phase 1 实施）
- 日期：2026-11-09
- 相关：[开发方案](../development-plan.md) §4.1–§4.6、§6.4；[ADR-001](ADR-001-tech-stack.md)

## 背景

Phase 1 要交付 HTTP 引擎、任务持久化和核心界面，验收要求是：5 GB 文件 8 连接跑满带宽，崩溃重启后能续传，1000 条任务的列表依然流畅。开发方案 §4 给出了方向，实现时有几处需要定下来或改掉：引擎接口的形状、分片如何分裂、进度落盘的顺序、Windows 上的预分配、HTTP/2，以及前后端类型怎么共享。

## 决策

### 引擎接口：一次运行，取消令牌，检查点

§4.1 原设计里，`start` 返回一个 `TaskHandle`，再由它 `pause` / `cancel`。实现改成一次运行跑到底：

```rust
trait DownloadEngine {
    fn kind(&self) -> TaskKind;
    async fn probe(&self, source: &str, options: &JobOptions) -> Result<ProbeInfo>;
    async fn run(&self, job: Job) -> Outcome;   // Completed / Stopped(ResumeState) / Failed(Error, Option<ResumeState>)
}
```

- `Job` 携带取消令牌（`CancellationToken`）、实时进度（`Progress`，原子计数）、任务级与全局限速器，以及 `Checkpoint`，引擎通过它报告「文件已就绪、开始传输」和每秒的进度。
- 暂停、移入回收站、退出程序都是取消同一个令牌，区别只在管理器怎样处理结果。引擎不需要知道为什么被停下。
- 引擎不接触数据库和界面：`omnidl-core` 的 `TaskManager` 负责调度、持久化和事件，FTP 引擎（Phase 4）沿用同一个接口。

### 分片与动态窃取

- 初始分片：`n = min(连接数, 大小 / 1 MiB)`，等分。
- 连接做完自己的分片后，先领取没人负责的未完成分片（续传后分片可能多于连接，或者某个连接放弃了），再去分裂剩余最多的分片：后一半交给自己，前提是剩余至少 2 MiB。
- 不另开 `watch` 通道通知被分裂的连接：每块数据写入前要先在分片表里 `reserve`，返回值小于收到的字节数就说明分片在这里结束，连接写完这部分便停止。
- 分片表在 `omnidl-core::segments`，有属性测试：任意顺序的读取、失败、加入、分裂之后，分片仍然恰好拼成整个文件，最终每个字节只下载一次。

### 进度永远不超前于磁盘

字节要经过两步：先 `reserve`，把数据送进写入线程的队列后再 `commit`。分片表里保存的已下载数只统计 commit 过的字节。

每秒保存一次进度，顺序如下：

1. 先对分片表做快照。
2. 再让写入线程 flush。它是 FIFO 的，所以快照之前入队的数据都已经写入，开启 `syncWrites` 时还会 `sync_data`。
3. 最后把快照写进 SQLite。

这样数据库记录的进度不会多于磁盘上实际有的数据。程序被强杀时最多丢掉 1 秒的进度，重新下载这一段即可。测试中把运行中的任务强行中止，再从上一个检查点续传，结果逐字节一致。

`sync_data` 在克隆出来的文件句柄上、由单独线程执行。写入线程不用停下等它，所以 8 连接、每秒一次同步时吞吐量不受影响（本机回环约 720 MiB/s）。

### Windows 上用稀疏文件预分配

NTFS 在超出「有效数据长度」的位置写入时，会先把中间的空隙同步填零。如果只调用 `set_len` 设置长度，最后一个连接写下第一块数据时，就要等系统填完前面几 GB 的零。

因此在 Windows 上先用 `FSCTL_SET_SPARSE` 把文件设为稀疏，再 `set_len`。Linux 和 macOS 用 `fallocate` / `F_PREALLOCATE` 真正预留空间，这样磁盘不够会在开始时就报出来。另外，开始下载前会检查目标分区的剩余空间。

### 只用 HTTP/1.1

reqwest 不启用 `http2` 特性，客户端设置为 `http1_only`。HTTP/2 会把同一主机的多个请求复用到一条 TCP 连接上，「8 个连接」实际只有 1 条，多连接加速就失去了意义。

同样，客户端不启用任何解压特性，并显式发送 `Accept-Encoding: identity`，保证写入的字节数和 `Content-Length` 一致。

### 不支持 Range 的服务器

以下两种情况任意一种出现，就整体改为单连接、从头下载：

- 探测时 `Range: bytes=0-0` 得到的是 200。
- 下载中途对 `start > 0` 的请求返回了 200。

这类任务暂停后无法续传，详情面板会显示「不支持（暂停后需要重新下载）」。服务器返回 429 或 503，且还有其他连接在工作时，这个连接会让出自己的分片，相当于自动减少连接数。

### 续传前的校验

续传前重新探测远端文件。大小、ETag、Last-Modified 中任意一项与开始时不同，任务就失败并显示「服务器上的文件已变更」，同时保留已下载的部分，由用户决定是否「重新下载」（`task_restart`）。只比较两次都提供了的校验字段，因为有些 CDN 不一定每次都返回这些头。

### 前后端类型：手写，不用 tauri-specta

tauri-specta 2 至今仍是 RC 版本（2.0.0-rc.26），而且要求 specta 精确匹配 RC 版本号。OmniZip 的做法是：Rust 侧用 serde camelCase 序列化，TypeScript 类型手写在 `lib/types.ts`，命令包装在 `lib/api.ts`。

这里沿用同样的做法，开发方案 §6.4 中「经 tauri-specta 导出」改为手写。等 tauri-specta 发布稳定版后再评估是否切换。

### 主题

全部 12 套内置主题都在页面里用 omni-ui 的 `resolveTheme` 解析，它与 OmniZip 的 Rust 主题服务逻辑相同。设置页可以分别为浅色和深色选择主题，也可以调整列表密度。用户自定义主题文件和窗口材质（Mica / vibrancy）需要 Rust 主题服务，等到 Phase 5 做系统集成时再引入。ADR-001 原写「Phase 1 换成 Rust 主题服务」，这里修正为 Phase 5。

## 影响

- `omnidl-core` 负责任务模型、状态机、分片表、文件名规则、限速器、事件总线和 `TaskManager`；`omnidl-storage` 负责 SQLite 仓储、写入线程和预分配；`omnidl-http` 负责探测、命名和多连接下载。
- 数据库表 `segments` 的列名为 `start_pos` / `end_pos`，因为 `end` 是 SQL 关键字。`tasks` 表增加 `prev_status`，用于从回收站还原。`torrent_meta` 表推迟到 Phase 3 的迁移中再建。
- Cookie 和自定义请求头目前以明文 JSON 存在 `options_json` 中。等 Phase 2 扩展开始传入 Cookie 时，再按 §4.6 用系统凭据库中的密钥做 AES-GCM 加密。
