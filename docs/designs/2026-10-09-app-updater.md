# R Codex 应用在线更新

> 状态：首版落地。参考 Cockpit Tools 的 GitHub Release + Tauri updater，不复制其代码。不依赖 Apple Developer ID / 公证。

## 目标

公开 GitHub 仓库发版后，已安装的 R Codex 能检查新版本、在用户确认后下载并重启。失败不影响渠道、启动和 Gateway。

## 非目标

- Apple Developer ID、公证、Windows Authenticode。
- 静默替换、强制升级、降级。
- 更新 Codex CLI/App（仍走现有 `check_codex_updates`，本方案不管）。
- 隔离实例运行中自动安装。

## 做法

与本机 Cockpit Tools 1.3.66 相同形状：`tauri-plugin-updater` 读取 GitHub Release 附件。

1. 发版 CI 上传 `latest-{{target}}.json` 与兼容用的 `latest.json`，并用 minisign 私钥签名安装包。
2. 应用启动后 best-effort 检查；有新版本才提示。
3. 用户点安装：若有活动隔离实例则拒绝；否则下载、校验签名、替换、重启。

公钥进仓库（`tauri.conf.json`）。私钥只放本机 `work/updater.key` 和 GitHub Secret `TAURI_SIGNING_PRIVATE_KEY`，密码 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`（当前生成为 `rcodex-updater`）。丢失私钥则旧客户端无法校验新包，必须换密钥并让用户手动装一版。

## 仓库地址

`src/app-release.ts` 的 `APP_GITHUB_REPO` 与 `src-tauri/tauri.conf.json` `plugins.updater.endpoints` 必须是同一个 `owner/name`。第一次打 tag 前改成真实公开仓库。

## 用户流程

- 启动后检查失败：不提示。
- 已是最新：不提示。
- 有新版本：通知「R Codex x.y.z 可用」，可「安装并重启」或「打开发布页」；标题栏出现下载按钮。
- 安装时有活动实例：提示先停止，不下载。

## 发版

打 tag `v0.1.x` 触发 `.github/workflows/release.yml`。macOS runner 走 `pnpm run tauri --bundles app`（先编 sidecar）。`tauri-action` 建 draft Release，带 `.app.tar.gz`、`.sig` 和 updater 清单。人工确认附件后再发布。

pnpm 版本只使用根目录 `package.json` 的 `packageManager`。`pnpm/action-setup` 不要再写 `version`；`10` 和 `pnpm@10.13.1` 会被当成两个版本，作业在安装依赖前失败。

2026-10-10：`v0.1.0` 第一次推送因此失败，未产生 Release。去掉 workflow 里的 pnpm `version` 后，标签指向修复提交并重新触发构建。
第二次构建进入 Tauri 后失败：干净检出没有 `src-tauri/binaries/codex-launcher-<triple>`，而 `build.rs` 在编译附带程序时就检查该路径。脚本改为先放占位文件，编完再覆盖为真正的二进制。
第三次构建在 `tauri build` 入口失败：Rust 的 `tauri` 2.12、`tauri-plugin-opener` 2.7 与 npm 的 `@tauri-apps/api` 2.11、`@tauri-apps/plugin-opener` 2.5 小版本不一致。CI 会把这种不一致当成错误。npm 包需要和对应 crate 保持同一个 major.minor。

## 验收

- `pnpm run check` 通过。
- 无 Release 时启动不报错、不挡工作区。
- 有签名 Release 且版本更高时出现提示；确认后替换并重启。
- 活动隔离实例时安装被拒绝。
- 私钥不进 git。
