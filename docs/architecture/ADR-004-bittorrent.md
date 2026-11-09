# ADR-004 BitTorrent 与磁力链接

- 状态：已采纳（Phase 3 实施）
- 日期：2026-11-09
- 相关：[开发方案](../development-plan.md) §4.4、§7.2；[ADR-003](ADR-003-download-engine.md)

## 背景

Phase 3 要接入 BitTorrent：磁力链接取得元数据后选择文件，下载、做种，重启后任务恢复，做种达到分享率上限后自动停止。开发方案 §4.4 选了 librqbit（9.1，`rust-tls`），但有几处要按真实的 API 和 ADR-003 的引擎接口重新定：任务怎样恢复、做种怎样占用调度槽位、限速在哪一层、元数据存在哪里。

## 决策

### 沿用「一次运行」的引擎接口

`BtEngine` 实现与 HTTP 相同的 `DownloadEngine`，一次 `run` 对应 librqbit 会话里的一个种子：

1. 取元数据：任务目录里已有的 `.torrent`（`<应用数据>/torrents/<任务 id>.torrent`），否则读本地种子文件、经 HTTP 下载种子，或向节点要磁力链接的元数据（`list_only`），取到后先保存。这一步任务处于「获取种子信息」（`probing`）。
2. 定下布局并报告 `started`：单文件种子直接写在保存目录，文件名就是任务名；多文件种子写在以种子名命名的子目录里。
3. `add_torrent(overwrite: true, only_files, output_folder)`，然后每 500 ms 读一次 `stats()`，映射到 `Progress`：已校验字节、总大小（所选文件）、连接的节点数、累计上传。
4. 运行结束（完成、暂停、退出、失败）时把种子从会话中移除（不删文件）。

暂停就是取消这次运行；继续就是重新开始一次运行。不使用 librqbit 的会话持久化（`SessionPersistenceConfig`）和 fastresume：任务数据库已经是唯一的事实来源，两份状态容易不一致。

### 续传靠校验已有数据

重新添加种子时 librqbit 会按 piece 哈希检查目标位置已有的数据，保留通过校验的部分，只下载缺少的。这就是 BT 任务的「断点续传」，元数据从任务目录读取，不需要再联系节点。代价是大种子重启时要读一遍已有数据；以后需要时可以为每个任务打开 fastresume。

`ResumeState` 里只记一个覆盖整个种子的分片（已校验字节数），只为让管理器保存进度和显示百分比；详情面板对种子显示「文件」和「节点」，不显示分片。

### 做种：不占槽位，暂停即结束，退出后继续

- 所选文件下载完成后，引擎调用新增的 `Checkpoint::seeding`：任务转为「做种中」（`seeding`），记下完成时间，**不再计入同时下载数**，排队的任务可以开始。
- 做种策略取自设置：分享率（累计上传 ÷ 所选文件大小，默认 1.5）与时长（从完成时起，默认 24 小时），任一达到即结束，0 表示不限；关闭「下载完成后做种」则直接完成。达到后运行返回 `Completed`。
- 对做种中的任务点「停止做种」（即暂停）同样返回 `Completed`。
- 退出程序时做种中的任务在数据库里保持 `seeding`，下次启动排队重新运行：校验数据后直接进入做种，时长仍从最初的完成时间算起。累计上传量保存在 `tasks.uploaded`（运行中每 30 秒及结束时写入），重启后接着累计。

### 限速

- 下载：全局限速应用在 librqbit 会话上，引擎运行时跟随设置变化；任务自己的限速在添加种子时生效（librqbit 不提供修改单个种子限速的公开接口）。HTTP 与 BT 各自按全局限速计，不共享同一个令牌桶。
- 上传：设置中的「上传限速」是所有种子合计，修改后立即应用到会话。

### 不建 `torrent_meta` 表

开发方案 §4.6 预留的 `torrent_meta` 表没有建：选中的文件放在任务选项（`TaskOptions.onlyFiles`），做种策略来自全局设置，元数据是任务目录里的 `.torrent`，librqbit 的会话 id 每次运行都会变。彻底删除任务时引擎删除对应的 `.torrent`；只有勾选「同时删除文件」才删除种子的数据（种子可能写在用户已有的文件上）。

### 系统集成

- 磁力链接：tauri-plugin-deep-link 声明 `magnet` 协议（macOS 写入 Info.plist，安装包注册）；设置 › BT 可在 Windows / Linux 上再次「设为默认程序」（写入当前用户的协议关联）。
- `.torrent` 文件关联写在 `bundle.fileAssociations`。
- Windows / Linux 上系统以新进程带参数启动应用：单实例插件把参数交给正在运行的实例，`links::from_args` 从中挑出磁力链接和存在的 `.torrent`；首次启动时页面尚未加载，链接先存下，`app_bootstrap` 交给页面。macOS 通过 deep-link 插件的 `on_open_url` 收到同样的链接与文件 URL。
- 拖入窗口的 `.torrent` 文件、浏览器扩展发来的 `add_magnet` 都打开新建对话框，在对话框里选择文件。

### 磁力链接的 `x.pe`

librqbit 解析磁力链接时忽略 `x.pe`（BEP 9 的节点地址）。引擎自己读取它，作为获取元数据和下载时的初始节点。测试和 `cargo run -p omnidl-bt --example seed` 用它让应用在没有 DHT 和 Tracker 的本机环境中找到做种方。

## 后果

- 引擎接口只增加了几个带默认实现的方法（`torrent_detail`、`set_files`、`forget`、`Checkpoint::seeding`），HTTP 引擎无需改动；管理器的变化集中在做种状态和槽位计算。
- 两个 librqbit 会话在本机互传的集成测试（`crates/omnidl-bt/tests/swarm.rs`）覆盖：本地种子文件下载、磁力链接经 `x.pe` 取得元数据并只下载所选文件、做种到分享率 1.0 自动完成、退出重启后从已有数据继续并完成。
- 大种子重启时的校验耗时与磁盘读取量随数据量增长；如成为问题，再按任务启用 fastresume。
