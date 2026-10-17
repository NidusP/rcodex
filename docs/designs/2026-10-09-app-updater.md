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
2. 应用启动后 best-effort 检查；有新版本只在标题栏显示下载按钮，不弹可用性通知。
3. 用户点标题栏下载按钮：打开弹窗并开始拉取当前运行平台的签名安装包（`latest-{{target}}.json`，与本机 R Codex 二进制一致），弹窗显示进度。下完后「更新」才可用。点「更新」：若有活动隔离实例则拒绝；否则安装并重启。Windows 安装器拉起后退出当前进程；macOS 安装后再 relaunch。

公钥进仓库（`tauri.conf.json`）。私钥只放本机 `work/updater.key` 和 GitHub Secret `TAURI_SIGNING_PRIVATE_KEY`，密码 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`（当前生成为 `rcodex-updater`）。丢失私钥则旧客户端无法校验新包，必须换密钥并让用户手动装一版。

## 仓库地址

`src/app-release.ts` 的 `APP_GITHUB_REPO` 与 `src-tauri/tauri.conf.json` `plugins.updater.endpoints` 必须是同一个 `owner/name`。第一次打 tag 前改成真实公开仓库。

## 用户流程

- 启动后检查失败：不提示。
- 已是最新：不提示。
- 有新版本：不弹「R Codex x.y.z 可用」。标题栏出现下载按钮，提示「下载更新」。
- 点击标题栏下载按钮打开弹窗并开始下载当前系统的包。弹窗显示新版本号、进度；有 release body 时显示正文。按钮是「取消」「下载」「更新」。
- 「更新」在安装包下完前不可用。弹窗里的「下载」同样会拉取当前 target 的包。弹窗显示已下载字节；总大小未知时只显示已下载。下载或安装中不能用遮罩或 Escape 关闭。
- 下载完成后「更新」可用，不自动安装。点击「更新」才安装并重启。
- 安装时有活动隔离实例：提示「隔离实例正在运行，请先停止后再安装更新。」，不安装。下载阶段不因隔离实例运行而拒绝。
- 下载或安装失败：沿用现有错误提示。

## 发版

把 `package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`、`website/package.json` 四个版本号改成同一个新值并推到 `main`，`.github/workflows/release.yml` 会自动打包并发布。四个文件不一致则失败。对应 `v*` tag 已存在则跳过，避免重复发版。仍可直接推 tag `v0.1.x` 触发同一套作业。

同一次 Release 打三份包：macOS Apple Silicon 与 Intel 各一份 `.dmg` 和 `.app.tar.gz`，Windows x64 一份 NSIS 安装包。macOS Intel 包在 Apple Silicon 运行器上交叉编译。三平台都成功后取消草稿并标为 latest，`/releases/latest` 立刻指向新版本。在线更新仍使用各平台的安装包签名，DMG 只供手动下载。不做 Apple 公证，也不做 Windows Authenticode。手动 `Windows NSIS` 工作流仍只上传工件，不发 Release。

pnpm 版本只使用根目录 `package.json` 的 `packageManager`。`pnpm/action-setup` 不要再写 `version`；`10` 和 `pnpm@10.13.1` 会被当成两个版本，作业在安装依赖前失败。

2026-10-10：`v0.1.0` 第一次推送因此失败，未产生 Release。去掉 workflow 里的 pnpm `version` 后，标签指向修复提交并重新触发构建。
第二次构建进入 Tauri 后失败：干净检出没有 `src-tauri/binaries/codex-launcher-<triple>`，而 `build.rs` 在编译附带程序时就检查该路径。脚本改为先放占位文件，编完再覆盖为真正的二进制。
第三次构建在 `tauri build` 入口失败：Rust 的 `tauri` 2.12、`tauri-plugin-opener` 2.7 与 npm 的 `@tauri-apps/api` 2.11、`@tauri-apps/plugin-opener` 2.5 小版本不一致。CI 会把这种不一致当成错误。npm 包需要和对应 crate 保持同一个 major.minor。

2026-10-11：v0.1.1 用来验证已安装的 0.1.0 能否在线更新。版本写在 package.json、src-tauri/Cargo.toml、src-tauri/tauri.conf.json 和 website/package.json。打 tag 后 CI 仍出草稿 Release；必须人工发布，0.1.0 才会从 /releases/latest 看到 0.1.1。私钥不变。客户端先请求 latest-darwin.json（当前没有这份附件），再回落到 latest.json。

2026-10-14：v0.1.2 带上 Chat 工具转换、空前缀不再自动补 /v1，以及 DeepSeek、GLM、Qwen 的官方档位。版本仍写在上述四个文件。tag v0.1.2 出草稿 Release，人工发布后已安装的 0.1.1 才会看到更新。私钥不变。

2026-10-17：v0.1.3 带上 Windows 隔离路径拼装、托盘退出杀进程、NSIS 卸载钩子，以及商店/散装安装根启动。版本仍写在上述四个文件。tag v0.1.3 出草稿 Release，人工发布后已安装的 0.1.2 才会看到更新。私钥不变。

2026-10-17：发版工作流改为 main 上版本号更新后自动打包并发布。v0.1.3 当时已按旧草稿流程打 tag，这一版仍要人工发布一次；从 0.1.4 起不必再点发布。

2026-10-17：v0.1.4 用来验证 main 上改版本号后自动打包并发布。四个版本文件为 0.1.4，不另打本地 tag。私钥不变。

2026-10-17：v0.1.5 带上 Windows CLI 长路径、NODE_REPL_TRUSTED_CODE_PATHS 分号，以及标题栏点下载即拉包、下完再更新。四个版本文件为 0.1.5。v0.1.4 tag 已存在，不能再发 0.1.4。私钥不变。


## 验收

- `pnpm run check` 通过。
- 无 Release 时启动不报错、不挡工作区。
- 有签名 Release 且版本更高时，标题栏出现下载按钮，不弹可用性通知。
- 点击标题栏下载按钮打开弹窗并开始下载当前系统的包；下完后「更新」可用，确认后安装并重启。
- 活动隔离实例时安装被拒绝。
- 私钥不进 git。
