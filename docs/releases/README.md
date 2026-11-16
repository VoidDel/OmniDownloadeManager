# 发布

1. 改版本号：`Cargo.toml`（`[workspace.package]`）、`apps/desktop/package.json`、`apps/desktop/src-tauri/tauri.conf.json`，再运行 `cargo update --workspace` 更新 `Cargo.lock`。扩展有改动时，也改 `apps/extension/package.json`。
2. 在本目录写 `v<版本>.md`，列出这一版改了什么（可选，没有就只有下载和安装提示）：先中文，一行 `---`，再英文。可以用 `##` 小标题、`**加粗**` 和链接。
3. 提交，推送，打 tag 并推送：`git tag -a v<版本> -m "v<版本>" && git push origin v<版本>`。

推送 tag 后，`.github/workflows/release.yml` 在三个平台打包，放进一个草稿 release。最后一步由 `scripts/release-notes.mjs` 生成说明：

- **release 页面**：这一版的改动、各平台安装包的下载链接（点击即下载）、未签名安装包的安装提示，中英文各一份。生成前会核对链接的每个安装包都已上传，缺了就报错。
- **应用内的更新提示**（`latest.json` 的 `notes`）：只有改动部分，去掉 Markdown 标记的纯文本。

在 GitHub 上检查草稿，点“发布”后，已安装的应用才会提示更新。草稿里的下载链接在发布之前打不开。

本地预览：

```
node scripts/release-notes.mjs v0.2.3 body
node scripts/release-notes.mjs v0.2.3 update
```
