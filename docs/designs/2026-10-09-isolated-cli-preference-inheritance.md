# 隔离 CLI 继承默认环境策略

> 状态：已落地（播种与终端选择）。不改产品边界：不复制登录、会话、MCP 账号或默认 `config.toml` 路由。对齐 [独立工作区改造](2026-09-21-isolated-only-product-plan.md) 已写的「每个隔离目录首次启动时播种一次」，把 CLI 补进同一合同。

## 结论

隔离 **App** 已经按白名单继承默认环境的一部分策略。隔离 **CLI** 没有。本次 git / 出网失败，是 CLI 启动路径跳过了 App 已经在走的首次播种，不是渠道或 API Key 缺字段。

本机证据（2026-10-09）：

| | 隔离 App `9d6280eb-…` | 隔离 CLI `8e5b6a1e-…` |
| --- | --- | --- |
| `config.toml` | 有，含 `network_access = "enabled"` | 无 |
| `.rcodex-preference-seed` | 有 | 无 |
| `.codex-global-state.json` | 有 | 无 |
| `AGENTS.md` / `skills/` / `rules/` | 有 | 无用户副本 |
| 不可变 profile | 有，再合并进 `config.toml` | 只有 profile，`--profile` 启动 |
| `approvals_reviewer` | 不写；合并时删掉旧值，沙箱提权询问 | 同样不写 |

默认 `~/.codex/config.toml` 有 `network_access = "enabled"`，没有 `sandbox_mode`。App 能带上网络，是因为播种白名单包含 `network_access`。CLI 连 `config.toml` 都没有，Codex 只能用更严的内置默认：`workspace-write`、`.git` 只读、网络关闭。

## 现状代码

[`prepare.rs`](../../src-tauri/src/managed_launch/prepare.rs) 只在 `LaunchTarget` 是隔离 App 时：

1. `seed_default_codex_preferences(managed_home, ~/.codex)` 一次
2. `materialize_managed_app_config` 把不可变 profile 合并进 `config.toml`
3. 隔离 Computer Use MCP
4. `keep_managed_permission_selector_available`

CLI 只生成 `managed_*.config.toml` + 目录，然后 `CODEX_HOME` + `--profile` 启动。

播种白名单（已有，App 在用）见 [`preferences.rs`](../../src-tauri/src/managed_codex_config/preferences.rs)：

- 复制：`notify`、`network_access`、`disable_response_storage`、`personality`、`hide_agent_reasoning`、`windows_wsl_setup_acknowledged`、`[desktop]`、`[features]` 插件开关、`[projects]` 信任、onboarding 原子、`AGENTS.md`、`rules/`、个人 `skills/`
- 每次启动覆盖：`[features].multi_agent_v2 = true`
- 不复制：MCP 服务器表、marketplace、shell 环境、模型路由、凭据、会话、Cookie、浏览器/Computer Use 账号

`sandbox_mode`、`approval_policy` 目前也不在白名单里。默认环境若只靠桌面权限选择器、不写进 `config.toml`，App 也不会从 toml 继承「完全访问」。CLI 的 `.git` 只读仍是 Codex `workspace-write` 默认，不是漏拷 `sandbox_mode`。

隔离 Profile 不再写入 `approvals_reviewer`。合并进 `config.toml` 时会删掉以前的 `auto_review`，沙箱提权沿用 Codex 缺省的询问，而不是自动拒绝。目录里的 `auto_review_model_override` 仍保留，只在真正进入自动审核时使用。

## 目标

隔离 CLI 首次启动与隔离 App 走同一套播种，使 CLI 的 `CODEX_HOME/config.toml` 带上白名单策略。之后默认环境变化仍不覆盖隔离目录；用户要更新再用现有 `sync_managed_preferences`（首页入口已摘掉，命令保留）。

CLI 启动仍用独立 `CODEX_HOME` 和 `--profile`。Codex 先读 home 的 `config.toml`，再用 profile 覆盖路由字段。profile 继续只负责 provider、模型、目录、Gateway、实例头、`cli_auth_credentials_store`、`allow_symlinked_codex_home`。

## 非目标

- 不把默认 `model` / `model_provider` / `model_providers.*` 拷进隔离 CLI。
- 不复制 `auth.json`、会话、MCP 账号、Electron 数据。
- 不在本方案里改 Computer Use MCP（那是 App 启动路径）。
- 不把 `auto_review` 从 profile 拿掉；若要 CLI 能批准 git/网络提权，另开票。
- 不把 `sandbox_mode` 扩进白名单，除非默认 `config.toml` 里真有、且产品明确要 CLI 继承沙箱档。

## 行为合同

1. 隔离 CLI 第一次为该实例生成 home 时调用与 App 相同的 `seed_default_codex_preferences`。已有 `.rcodex-preference-seed` 则跳过。
2. 播种后写 `config.toml`。没有来源文件时允许空表，再让后续 profile 合并。
3. CLI 也要把不可变 profile 合并进 `config.toml`（复用 `materialize_managed_app_config` 或抽成与 App 共用的「home 配置合并」）。否则只有 `--profile`，home 级 `network_access` 可能被部分 Codex 版本忽略。
4. 合并顺序：播种白名单 → profile 覆盖路由 → `multi_agent_v2 = true`。
5. 已存在的 CLI 实例（例如 `8e5b6a1e-…`）没有 seed 标记：下次启动补一次播种，不得二次覆盖用户已在该隔离 home 里改过的非路由键。判定：无 marker 则播种；有 marker 不再自动播种。
6. 显式同步命令对 CLI home 与 App home 使用同一白名单。

## 对 git / 出网的预期

播种 `network_access = "enabled"` 之后，隔离 CLI 应能访问 GitHub 等外网（与默认环境一致）。

这**不**自动允许改 `.git`。`workspace-write` 下 Codex 仍把 `.git` 标只读。2026-10-10 起隔离启动写入 `approval_policy = "on-request"`，写 `.git` 和出网会询问用户，不再因缺省的 `granular` 策略直接拒绝。批准前仍然不能提交或访问网络。

## 实现要点

- [`prepare.rs`](../../src-tauri/src/managed_launch/prepare.rs)：`seed_default_codex_preferences` 与 home 配置合并从 `if managed_app` 挪到「所有受管理启动」。Computer Use MCP 和权限选择器仍可仅 App。
- 抽「合并 profile 到 `config.toml`」避免函数名绑死 App。
- 单测：CLI prepare 后存在 `config.toml`、含来源 `network_access`、含 `model_provider = rcodex`、不含默认 `model_providers.OpenAI`；第二次启动不重复覆盖；marker 已存在则跳过播种。
- 不读、不改用户 `~/.codex`。

## 验收

- 新隔离 CLI：home 有 `config.toml` 和 seed 标记；`network_access` 与默认环境一致（若默认写了）。
- 路由仍是 `rcodex` + 本机 Gateway，登录仍是隔离 `auth.json`。
- 已有 CLI 实例下次启动补齐播种，不丢掉已有会话库。
- 外网请求不再因「未继承 network_access」失败。
- `.git` 只读若仍在，文档与 UI 不把它说成「缺网络配置」。
- `cargo test --lib managed_launch::`、`managed_codex_config::`、`pnpm run check`。

## 回滚

把播种与合并重新限制在 `managed_app` 分支。已写入的 CLI `config.toml` 可留在 home，不自动删除。


## 落地（2026-10-09）

隔离 CLI 与隔离 App 共用首次播种和 `config.toml` 合并。Computer Use MCP 与权限选择器仍只在 App 启动。已有无 marker 的 CLI home 会在下次启动补种。

## 打开 CLI 的终端

macOS：若 `/Applications/iTerm.app`、`/Applications/iTerm2.app` 或用户 `~/Applications` 下同名 bundle 存在，用 `open -a iTerm` / `iTerm2` 跑启动脚本；失败再退回 `Terminal`。日志写 `terminal_open_dispatched app=...`。

Windows：当前仍返回错误，不打开终端。计划优先 `wt.exe`（Windows Terminal），没有再 `cmd.exe`。凭据 FIFO 与 `.command` 脚本还是 macOS 专用，Windows CLI 不能只换一个 exe 名。


## 审批询问（2026-10-09）

隔离 App 与 CLI 的 Profile 不再写 `approvals_reviewer`，并写入 `approval_policy = "on-request"`。已有 home 在下次启动合并时删除旧的审核键并补上询问策略。模型目录的 `auto_review_model_override` 不变。
