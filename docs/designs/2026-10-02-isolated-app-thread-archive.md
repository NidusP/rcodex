# 隔离 App 会话归档修复

> 状态：Step 2 与 Step 5 已实施并完成隔离 App 的手动归档实机验收。剩余问题是模型通过本会话调用官方线程归档工具，和 JSONL/SQLite 归档链路无关。
> 日期：2026-10-02
> 合同同步：[独立工作区改造](2026-09-21-isolated-only-product-plan.md)、[混合路由详细设计](../codex-hybrid-routing-design.md)、[产品原理](../3rcodex-product-principles.md)

## Goal

- User-visible outcome: 隔离 Codex App 侧栏可以把本地会话归档到 `archived_sessions/`，并在「已归档」列表里再打开；重启后仍在。
- Scope: 受管理 App 的 `CODEX_HOME` / `CODEX_SQLITE_HOME` 布局、启动前一次性库迁移、实例删除残留清理、导入读路径、相关测试与文档。
- Non-goals: 不改 ChatGPT.app / Codex Desktop；不改默认 App；不把 `archived_sessions` 纳入项目导入；不关闭 Codex 对「当前仍有 writer 的会话」的拒绝；不改 3R 工作区级完整请求档案。

## Requirements And Acceptance

- Requirements:
  1. 受管理 App 进程看到的 `CODEX_SQLITE_HOME` 必须等于本次启动的 `CODEX_HOME`。**Step 2 历史布局：** macOS 启动 `CODEX_HOME` 是 `r/<short-id>` 符号链接，指向 `codex-instances/<uuid>/codex-home`；Windows / CLI 用实例 `codex-home` 真目录。**Step 5 已实施（仅 macOS 受管理 App）：** 下一次已退出 App 的启动准备把该短路径作为真目录，旧长路径降为兼容 symlink；尚未触发迁移的既有实例仍保持历史布局，直到该启动发生。
  2. 线程索引 `state_5.sqlite`、会话 JSONL、`archived_sessions/` 都落在同一隔离 home 下，符合 Codex Desktop `ThreadArchiveService` 的 `join(codexHome, "state_5.sqlite")` 合同。
  3. 已有 `managed-app-sqlite/<launch-preset-id>` 在 App 未运行时一次性迁入该 home；迁完后不再把拆开目录当活动库。
  4. 删除结束实例时，对话库随 `CODEX_HOME` 删除；旧拆开目录若仍在 3R 数据根下，作为残留一并清掉。
- Acceptance criteria:
  - 启动准备结果：`PreparedManagedLaunch.sqlite_home == Some(launch_codex_home)`。
  - 启动器环境：`CODEX_HOME` 与 `CODEX_SQLITE_HOME` 字节相同。
  - 迁移：仅旧库、仅新库、两边都有、App 仍在运行，四条路径有单测。
  - 实机（Step 5 后）：隔离 App 的手动归档须产生 `archived_sessions/` JSONL，并把 `threads.archived` 置为 `1`。app-server 的 `already has an active writer` 是其直连归档路径的观察结果，不能据此断言桌面手动归档必然失败。
- Edge cases:
  - **Step 2 历史布局：** 短链 `.../r/<short-id>` 与长目录是同一棵文件树（symlink）。**Step 5 已实施后的布局：** 短路径是唯一真目录；长路径若存在，只许是指向短目录的兼容 symlink。
  - 旧库带 `-wal`/`-shm`：必须整组移动，禁止只拷主文件。
  - 两边都有 `state_5.sqlite`：fail-closed，不合并、不覆盖。
  - CLI 已经把两个环境变量指到实例 home，本票不改 CLI 布局。

## 根因

Codex 桌面归档有两条路：

1. app-server `thread/archive`：在 `CODEX_HOME/sessions` 与 `archived_sessions` 之间 rename rollout，再更新 `CODEX_SQLITE_HOME` 里的 `threads.archived`。
2. Electron `ThreadArchiveService.archiveInactiveThread`：写死打开 `CODEX_HOME/state_5.sqlite`。默认 App 不设 `CODEX_SQLITE_HOME`，库就在 `~/.codex/state_5.sqlite`，所以这条路通。当前本机默认库 116 条活动 / 161 条已归档。

3R 隔离 App 把 `CODEX_SQLITE_HOME` 指到 `managed-app-sqlite/<preset-id>`，会话 JSONL 仍在实例 `CODEX_HOME`。第一段若因当前会话占用 writer、或 paginated 路径对不上而回到第二段，Electron 在 home 根找不到 `threads` 表，归档失败。当前隔离实例 3 条会话全部 `archived=0`，home 没有 `archived_sessions/`。

拆库当初是为了「方案级索引跨新实例还在」。现在受管理 App 退出后复用同一 runtime binding，实例 home 已经稳定；删除结束实例时本来就会把 home 和 sqlite 一起删。继续拆开只破坏 Codex 归档合同，不再换来产品能力。

## Design Notes

- Modules and contracts: `LaunchService` 是唯一写入启动环境变量的路径。归档本身仍由 Codex Desktop 执行；3R 只保证目录合同。
- Data flow: 启动前（App 未运行）迁移旧库 → 物化 `sqlite_home = launch_codex_home` → launcher 同时设置两个环境变量 → Codex 在同一 home 读写 `state_5.sqlite` 与 `sessions/`。
- Consistency: 强一致、单次迁移。禁止双写。冲突时拒绝启动并给出可操作错误，而不是选一份库猜。
- Rejected options:
  - 改 ChatGPT.app：不是 3R 资产。
  - 在 home 根做 `state_5.sqlite` 符号链接、库仍留在拆开目录：WAL 双开。
  - 每次启动复制一份库到 home：两个真相源。
  - 取消 `CODEX_SQLITE_HOME`：可能继承父进程；显式赋成同一路径更安全。

## 要改哪里

### 1. 启动物化 — 主修复

文件：[src-tauri/src/managed_launch.rs](../../src-tauri/src/managed_launch.rs)

| 位置 | 现状 | 改为 |
| --- | --- | --- |
| `prepare_launch_with_target` 约 1063–1096 行 | `managed_app` 时 `sqlite_home = managed_app_sqlite_home(preset)`，并 `create_dir_all` 拆开目录 | `sqlite_home = Some(launch_codex_home.clone())`。不再创建 `managed-app-sqlite/<id>`。在写入启动快照前调用迁移 helper |
| `PreparedManagedLaunch.sqlite_home` 注释（[models.rs](../../src-tauri/src/managed_launch/models.rs) 约 117–120 行） | 「方案级稳定 SQLite，与实例 home 分离」 | 「等于 `launch_codex_home`；仅兼容旧拆开目录的一次性迁移源」 |
| `isolated_codex_storage_for_preset` 约 1566–1572 行 | 返回 `(home, managed_app_sqlite_home)` | 返回 `(home, home)`；导入因此读 `CODEX_HOME` 下的 `state_5.sqlite` |
| `delete_instance_storage` 约 1664–1678 行 | 必删 `managed-app-sqlite/<preset>` | 对话库已在 home 内，随 lifecycle 删除。仍 **best-effort** 删除 3R 数据根下该 preset 的旧拆开目录（若存在），避免残留索引被下次误迁 |
| `managed_app_sqlite_home` / `managed_app_sqlite_path` | 启动期活动路径 | 降为「旧布局探测/残留清理」私有 helper，启动主路径不再调用 |

新增私有函数（建议放在 `managed_launch.rs` 或 `managed_launch/sqlite_layout.rs`）：

```text
migrate_managed_app_sqlite_into_home(
    old_sqlite_home: &Path,   // 3R 数据根/managed-app-sqlite/<preset>
    dest_home: &Path,         // 真实实例 codex-home
) -> Result<(), String>
```

规则：

1. 目标集合：`state_*.sqlite`、`logs_*.sqlite`、`memories_*.sqlite`、`goals_*.sqlite`、`queue_*.sqlite`、`thread_history_*.sqlite`，以及同名 `-wal`/`-shm`。
2. 旧目录不存在或为空 → Ok，无操作。
3. 旧目录有 `state_*.sqlite`，目标 home 没有 → 在同一所有权根下 `rename` 整组文件；禁止 copy-leave。
4. 目标已有 `state_*.sqlite` 且旧目录也有 → `managed_sqlite_layout_conflict`，不启动。
5. 目标已有、旧目录没有活动库 → Ok；启动后可删空旧目录。
6. 迁移只在 `prepare_launch` 且该 preset 的 managed App **未在 running/launching/stopping** 时执行。

### 2. 启动器环境

文件：[src-tauri/src/bin/codex-launcher.rs](../../src-tauri/src/bin/codex-launcher.rs)

| 位置 | 改动 |
| --- | --- |
| `configure_managed_app_command` 约 503–508 行 | 仍显式 `.env("CODEX_SQLITE_HOME", sqlite_home)`，调用方传入与 `codex_home` 相同路径 |
| `managed_app_uses_isolated_home_and_electron_data_dir_without_provider_key` 约 747–790 行 | 断言 `CODEX_SQLITE_HOME == CODEX_HOME`，fixture 不再用 `/tmp/3r-managed-app-sqlite` |
| `--sqlite-home` 参数 | 保留。控制面继续传，避免旧脚本缺参；值必须与 `--codex-home` 相同。可选：路径不同则 launcher fail-closed |

[src-tauri/src/lib.rs](../../src-tauri/src/lib.rs) 约 1551–1555 行：继续传 `--sqlite-home`，值为 `prepared.launch_codex_home`。约 441–447 行导入改为同一 home。

CLI / 探针 / 官方认证探测已经把两个变量设成同一目录（`managed_gateway.rs`、`managed_codex_config.rs`、`official_auth.rs`），本票不改行为，回归里确认不被拆开。

### 3. 导入

文件：[src-tauri/src/session_import.rs](../../src-tauri/src/session_import.rs)

- 仍接受 `isolated_home` 与 `isolated_sqlite_home` 两个参数，避免一次改光调用面。
- 调用方传入同一路径后，现有 `canonicalize` + 重叠检查仍然有效。
- 补一条测试：sqlite 与 home 为同一目录时，活动会话可导入；`archived_sessions` 仍排除。

不把已归档会话纳入导入。那是独立产品决定，见独立工作区计划 §4.5。

### 4. 前端文案

文件：[src/codex-control.ts](../../src/codex-control.ts)

- 保留 `managed_sqlite_home_cleanup_failed`，用于删残留旧目录失败。
- 新增 `managed_sqlite_layout_conflict`：隔离对话索引同时存在于旧拆开目录和 `CODEX_HOME`，请退出隔离 App 后联系修复，避免覆盖。
- 新增 `managed_sqlite_migrate_failed`：迁库失败，实例未启动。

本票不改归档 UI。归档按钮在 Codex Desktop 内。

### 5. 测试（必改）

文件：`src-tauri/src/managed_launch.rs` 测试模块、`src-tauri/src/bin/codex-launcher.rs` 测试、`src-tauri/src/session_import.rs` 测试。

| 测试 | 期望 |
| --- | --- |
| `managed_app_sqlite_home_is_stable_per_launch_preset` | 改成：启动准备的 `sqlite_home == launch_codex_home`，且跨重启仍相等 |
| `managed_app_reopens_the_same_runtime_binding_after_exit` | 继续断言实例/home 复用；加上 sqlite 与 launch home 相同 |
| `deleting_managed_app_storage_removes_home_sqlite_and_electron_data` | home 内放 `state_5.sqlite` 应随 lifecycle 消失；若测试预置了旧 `managed-app-sqlite/<preset>`，也应消失 |
| 新增 `migrates_legacy_managed_app_sqlite_into_codex_home` | 旧目录有库、home 无 → 文件出现在 home，旧目录空或删除 |
| 新增 `refuses_sqlite_layout_conflict` | 两边都有 `state_5.sqlite` → 错误码，双方文件字节不变 |
| 新增 `skips_sqlite_migration_when_home_already_has_catalog` | home 已有库、旧目录无 → 启动成功 |
| launcher 环境测试 | `CODEX_HOME` 与 `CODEX_SQLITE_HOME` 相同 |

### 6. 文档

| 文件 | 改动 |
| --- | --- |
| 本文件 | 方案与验收真相源 |
| [2026-09-21-isolated-only-product-plan.md](2026-09-21-isolated-only-product-plan.md) | §2 / §5：隔离 App 的 `CODEX_SQLITE_HOME` 等于启动 `CODEX_HOME` |
| [codex-hybrid-routing-design.md](../codex-hybrid-routing-design.md) | 启动布局与删除路径不再写「方案专属 sqlite 目录是活动对话库」 |
| [3rcodex-product-principles.md](../3rcodex-product-principles.md) | 启动/删除段落与上项对齐 |
| [codex-app-support-research.md](../codex-app-support-research.md) | 将 `managed-app-sqlite/<preset>` 标为已废止的活动布局，仅残留迁移 |
| [codex-hybrid-routing-execution-plan.md](../codex-hybrid-routing-execution-plan.md) | 文首加本票交接 |
| [AGENTS.md](../../AGENTS.md) | 配置节：受管理 App 的 sqlite 与 home 同一路径 |

## Migration or rollback

- 迁移点：下一次 `prepare_launch` managed App，且该实例不在运行态。
- 不改 3R 控制面 schema。
- 回滚代码：恢复拆开 `managed-app-sqlite` 赋值。若用户已经用新布局归档，回滚后 Electron 又会找不到 `CODEX_HOME/state_5.sqlite`；回滚前需把库再挪回旧目录。因此回滚窗口是「尚未实机迁库」；迁库后只能向前修。
- 不触碰 `~/.codex`。

## Execution

- [x] Step 1 — 文档合同：本文件 + 独立工作区 / 详细设计 / 产品原理 / AGENTS 同步。
  - Owner: 本会话
  - Verification: 文档与「sqlite_home == launch_codex_home」一致
- [x] Step 2 — 迁库 helper + 启动赋值 + 删除残留。
  - Owner: 实施会话
  - Dependencies: Step 1
  - Verification: `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`; `cargo check --manifest-path src-tauri/Cargo.toml --all-targets`; `cargo test --manifest-path src-tauri/Cargo.toml --all-targets`（282 lib + 1 gateway + 12 launcher + 16 official_auth passed）；迁移、冲突、跳过、删除、运行中保留 WAL 的回归均通过
- [ ] Step 3 — launcher / lib.rs / 导入调用面与错误文案。
  - Dependencies: Step 2
  - Verification: `cargo test --lib managed_launch::`、`cargo test --bin codex-launcher`、`cargo test --lib session_import::`、`pnpm run check`
- [x] Step 4 — 手动归档实机验收（依赖 Step 5）。
  - Verification: `archived_sessions/rollout-2026-10-01T09-08-37-01a0f501-9636-7c50-9e1b-ef251b70678a.jsonl` 已存在；`threads.id=01a0f501-9636-7c50-9e1b-ef251b70678a` 的 `archived=1`，且 `rollout_path` 以短真实 `CODEX_HOME` 开头。
- [x] Step 5 — macOS 受管理 App：短 `CODEX_HOME` 改为真目录。新实例直接建短真实 home；旧实例按状态机迁移；删除与导入读路径已对齐。
  - Owner: 本会话
  - Dependencies: Step 2 已落地；真实数据迁移与手动归档均已完成。
  - Verification: `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`、`cargo test --manifest-path src-tauri/Cargo.toml --lib managed_launch::`（34 passed）、`cargo test --manifest-path src-tauri/Cargo.toml --all-targets`（287 lib + 1 Gateway + 13 Launcher passed）；手动 Electron 归档证据见 Step 4。

## Active Status

- Current state: 手动归档已通过。短真实 `CODEX_HOME` 内同时存放 `state_5.sqlite`、`sessions/` 与 `archived_sessions/`；长生命周期路径是兼容 symlink。
- Verified: `01a0f501-9636-7c50-9e1b-ef251b70678a` 已移入 `archived_sessions/`，SQLite 记录 `archived=1`，路径使用短 home 前缀。
- Remaining blocker: 通过对话归档尚不可用。本会话没有 `set_thread_archived`、`codex_app` 或 `tool_search`；从 Grok 切换到 GPT 后工具集没有变化。因此模型不能向桌面发起归档调用。
- Next action: 单独修复 Codex Desktop 的动态工具装配：确认隔离 app-server 实际启动 `codex-app-tools` MCP，并让本地对话拿到 `codex_app` namespace 与 deferred 的 `set_thread_archived`。不得在 3R Gateway 伪造该工具，也不得直接移动 JSONL/改 SQLite 来替代桌面归档。

## 手动归档已解决；通过会话归档仍阻塞

两条路径的事实不同：

| 路径 | 当前结果 | 结论 |
| --- | --- | --- |
| Codex Desktop 的侧栏/Recents 手动归档 | 通过 | 3R 的 home、SQLite、JSONL 与短路径布局问题已修复。 |
| 模型在对话中调用 `set_thread_archived` | 不可调用 | 当前问题是桌面工具没有装配到会话，不是会话 JSONL、`state_5.sqlite` 或归档目录。 |

先前观察到 app-server 对持有 `thread-writer-locks/*.lock` 的线程会返回 `already has an active writer`。这是该 app-server 调用的限制；commander 的手动归档成功已经证明它不能作为「已打开会话必然无法归档」或「侧栏归档仍坏」的结论。3R 仍不应释放、删除或伪造这些锁。

## Step 5 — 短路径改为真实目录（代码已实施，实机待验收）

> 状态：方案、代码与真实数据迁移均已完成；短真实目录上的手动归档已验收。下方保留迁移合同与历史诊断，供后续维护使用。

### 2026-10-02 审阅结论

Step 5 的根因判断成立，但原草案不能直接实施。Step 2 后的现场已经证明：

- `CODEX_HOME=.../r/<short-id>` 仍是指向长实例目录的符号链接；
- `threads.rollout_path` 仍保存长目录前缀；
- Electron `ThreadArchiveService` 的 `path.relative()` 不解析该符号链接，因此无 writer lock 的会话也可能归档失败；
- 有 writer lock 的会话仍会被 Codex 拒绝，Step 5 不代 Codex 释放 writer。

实施前必须补齐以下合同，否则会在重启、删除或迁移中间态引入回归：

1. **路径事实源必须唯一。** 明确 `managed_codex_home` 保存短真实目录，或保存长兼容链接；不能让数据库字段、`CODEX_HOME`、生命周期目录各自代表不同事实源。当前 `ensure_owned_path` 要求路径位于 `codex-instances` 下且拒绝 symlink；若数据库保存 `app_data/r/<short-id>` 或长 symlink，必须同步调整所有权检查、实例回执和启动复用逻辑。
2. **短目录必须跨重启复用。** 停止/重启不能删除 `r/<short-id>` 真实目录；下一次启动必须按实例绑定复用它。长路径兼容 symlink 与短真实目录要分别创建、校验和删除。`short_codex_home_link` 不能把同一实例已有真实目录当成路径碰撞。
3. **删除必须覆盖两棵路径表示。** 删除实例时先安全移除长路径兼容 symlink，再删除短真实目录和生命周期目录；不得让现有 `remove_owned_directory` 因 symlink 拒绝而留下实例残留。
4. **迁移必须按可恢复顺序执行。** 唯一顺序：`rename 长真实树 → 短路径` → `在旧长路径建立指向短目录的兼容 symlink` → `改写短目录 sqlite 路径字段` → `把 managed_codex_home 写成短路径`。禁止在搬树前把长路径改成链接。状态机见下文「崩溃恢复」。
5. **路径改写要按字段和边界执行。** 至少更新 `threads.rollout_path`；同时检查 `rollout_migration_skipped_rollouts.rollout_path` 等持久化路径字段。只改旧长前缀且要求路径分隔符边界，不能做无边界的字符串替换；已归档线程也要保持可重新打开。
6. **平台与入口。** Step 5 只适用于 **macOS + `managed_app`**。Windows 受管理 App、以及 CLI（任何平台）不进入短目录迁移，继续用实例 `codex-home` 真目录作为 `CODEX_HOME`。不得把 `short_codex_home_link` 的新语义套到 CLI。

### 锁定选择（实施不得再选）

1. **事实源：** `codex_instances.managed_codex_home` 存短真实目录 `<3R数据根>/r/<short-id>`。`lifecycle_dir` 仍在 `codex-instances/<uuid>`。启动 `CODEX_HOME` / `CODEX_SQLITE_HOME` 都等于这份短真实目录。
2. **兼容链接：** 迁移完成后，旧长路径 `lifecycle_dir/codex-home` 只允许是指向短真实目录的 symlink，不得再是第二份文件树。
3. **所有权：** 短真实目录的 `ensure_owned_path` 以 `instances_root` 的父目录（3R 数据根）为 root，并允许 `r/<id>` 为普通目录；不得把现有「路径上禁止任何 symlink」套在这份短 home 上以致拒收。长兼容链接用单独的 symlink 删除路径，禁止走 `remove_owned_directory`。
4. **复用：** `short_codex_home_link` 若发现 `r/<id>` 已是本实例的真实目录，必须直接复用，不得 `exists() → continue` 去抢更长前缀。
5. **迁移顺序：** App 已退出且无 writer → `rename` 长真实树到短路径 → 在旧长位置建指向短目录的 symlink → 在短目录 sqlite 里按路径分隔符改写旧长前缀。禁止先在长路径上建链接再搬树。两边都是真目录或状态无法判定 → fail-closed。
6. **Windows：** 不跑这次短目录迁移；继续用实例 `codex-home` 当真实 `CODEX_HOME`。
7. **非目标：** 不代 Codex 释放 writer lock；不改 `~/.codex`；不启动/停止 `pnpm run tauri:dev`。

### 既有实例如何切到短真实目录

仅 **macOS + managed_app**。输入是 `codex_instances` 一行。

- `lifecycle_dir` 始终为 `codex-instances/<uuid>`，本步不改这个字段。
- 旧长 home 一律由 `lifecycle_dir/codex-home` **推导**，不要猜别的路径。
- 短 home 为 `<3R数据根>/r/<instance_id 去横线后的稳定短名>`，与今天 `short_codex_home_link` 的命名规则相同，但结果必须是真目录。
- `prepare_launch` 在 App 未运行时读取该行：
  - 若 `managed_codex_home` 已是短真实目录且 `lifecycle_dir/codex-home` 是指向它的 symlink → 视为完成，复用，不迁。
  - 若 `managed_codex_home` 仍是长真实目录（当前生产）→ 进入状态机，从 `legacy` 开始。
  - 若存在 `lifecycle_dir/home-layout.json` → 按其中 `phase` 恢复，不从头搬。
- `managed_codex_home` **只在状态机 `updating_db` 阶段**写成短路径。在此之前数据库仍指向长路径；启动遇到未完成 phase 只走恢复，不得用旧长真实树再拉起 Codex。

### 崩溃恢复状态机

持久状态写在 `lifecycle_dir/home-layout.json`（JSON，0600）。3R 控制库、Codex `state_5.sqlite` 和文件系统不能组成一个原子事务，**phase 文件是恢复的唯一判据**。目录形态必须与 phase 一致，否则 `conflict`。

| phase | 磁盘必须满足 | 下一步（幂等） | 失败 |
| --- | --- | --- | --- |
| `legacy` | 长路径是真目录；短路径是指向它的 symlink 或不存在 | 写 phase=`renaming`，然后 `rename(长真实树 → 短路径)` | 短路径已是另一棵真目录 → conflict |
| `renaming` | 若短路径已是真目录且长路径不存在 → rename 已成功 | 写 phase=`linking` | 两边都是真目录 → conflict |
| `linking` | 短路径是真目录 | `symlink(短路径, 旧长路径)`；已是正确 symlink 则跳过 | 长路径是真目录 → conflict |
| `rewriting_paths` | 长路径是指向短目录的 symlink | 在短目录每个 `state_*.sqlite` 的所有 `rollout_path` 列按「旧长前缀 + `/`」改写，覆盖 `threads`、`rollout_migration_skipped_rollouts`；已是短前缀则跳过 | sqlite 打不开或仍有 writer → 拒绝启动 |
| `updating_db` | sqlite 路径已是短前缀 | `UPDATE codex_instances.managed_codex_home` 为短路径 | 控制库失败则留在本 phase，下次重试 |
| `complete` | `managed_codex_home` 为短真实目录；长路径为兼容 symlink 或已删除 | 启动 | — |
| `conflict` | 任一无法判定 | 拒绝启动，`managed_home_layout_conflict`，不自动合并 | — |

唯一顺序：

`legacy → renaming → linking → rewriting_paths → updating_db → complete`

即 `rename 长树到短路径` → `旧长路径建兼容 symlink` → `改写短目录 sqlite` → `更新 managed_codex_home`。

### Step 5 测试矩阵

| 用例 | 期望 |
| --- | --- |
| 新 macOS managed_app | `r/<id>` 为真目录；`CODEX_HOME==CODEX_SQLITE_HOME`；`managed_codex_home` 为该短路径 |
| 重启复用 | 同一 `r/<id>`，不因 `exists()` 改用更长前缀 |
| 旧实例从长真实树迁移 | sqlite `rollout_path` 以短前缀开头；长路径是 symlink |
| 已归档线程 | `archived=1` 的路径同样改写，仍能打开 |
| 删除实例 | 短真实目录、长兼容 symlink、lifecycle 均删除 |
| 两边都是真目录 | `conflict`，双方字节不变 |
| App 仍在运行 | 不进入 renaming |
| 崩溃在 rename 成功、尚未 linking | 下次从 `linking` 继续 |
| 崩溃在 rewriting_paths | 下次继续改写，已是短前缀的行跳过 |
| 崩溃在 updating_db 前 | sqlite 已短、DB 仍长 → 只更新 DB |
| Windows managed_app | 不跑 `r/` 真目录迁移 |
| CLI | 不走 `short_codex_home_link` 新语义 |
| Electron `path.relative(CODEX_HOME/sessions, rollout_path)` | 迁移后为真 |

2026-10-02 实际代码验证：`managed_launch::` 34 项通过，覆盖短真实目录新建与重启复用、旧 symlink 整树迁移、`threads` 与其他 `rollout_path` 表的边界改写、rename 后恢复、双真目录拒绝，以及迁移后的链接/短目录/lifecycle 删除。未启动 Tauri 开发服务，未触碰 `~/.codex`；真实桌面归档随后已按 Step 4 验收通过。

Windows 保持原 `codex-home` 分支；本机只安装 `aarch64-apple-darwin` Rust target，未交叉编译 Windows 分支，也没有安装工具链。Windows 发布验收仍按项目既有发布门执行。

### 前因

macOS Unix domain socket 路径大约只能装 104 字节。实例真实目录是：

`/Users/populus/Library/Application Support/io.3r.codex/codex-instances/<uuid>/codex-home`

再加上 `/ipc/....sock` 会超限，Codex 沙箱也会拒掉过长的可写根。3R 因此在 `.../io.3r.codex/r/<short-id>` 建一条**符号链接**，启动时把 `CODEX_HOME` 指到短链，Profile 写 `allow_symlinked_codex_home = true`。文件仍住在长目录里。

这只解决 socket 和沙箱，并不改变「磁盘上的规范路径是长目录」。Codex 把 rollout 写进 sqlite 时记下的是真实长路径，例如：

`.../codex-instances/54ad4fcb-.../codex-home/sessions/2026/10/01/rollout-....jsonl`

### 已做完的一步（拆库收回）

2026-10-02 Step 2 已让 `CODEX_SQLITE_HOME == launch_codex_home`，并把 `managed-app-sqlite/<preset>` 迁进实例 home。本机证据：`state_5.sqlite` 与 `sessions/` 已在同一 `codex-home`；旧拆开目录已空。

这一步修的是 Electron 打开 `CODEX_HOME/state_5.sqlite` 找不到 `threads` 表。**它没有修短链和长 `rollout_path` 字面不一致。**

### 迁移前为何归档仍失败（历史诊断）

Codex 桌面归档两条路：

1. app-server `thread/archive`：`canonicalize` 后再判断文件是否在 `sessions/` 下，短链按理可以通过。但若该线程在 `live_recorders` 里，立刻 `already has an active writer`。`beforeArchiveConversation` 是空函数。界面 toast 是 `Failed to archive conversation`（`localTaskRow.archiveError`）。
2. Electron `ThreadArchiveService`：`path.relative(CODEX_HOME/sessions, rollout_path)`，**不跟随 symlink**。短链 `.../r/<id>/sessions` 和长 `rollout_path` 对不上，返回「文件不在 sessions 下」，回退失败。

2026-10-02 14:49、短真实目录迁移前的现场：

- `01a0f502`（本会话）、`01a0f1db`：app-server 直连归档会因 `codex` 持有的 writer lock 返回失败。
- `01a0f501`：无 lock，但当时仍归档失败，说明即便没有 writer，也会走到 Electron 那条不认 symlink 的判断。

默认 App 能归档，是因为 `~/.codex` 是真目录，`CODEX_HOME` 与 `rollout_path` 同一字面前缀，且大量旧会话没有 writer。

Cockpit 多开只设 `CODEX_HOME` + Electron 数据目录，不拆 sqlite，也没有这条短链合同；它不能证明 3R 短链布局可归档。

### 改动是什么

把短路径从「指向长目录的 symlink」改成「文件就住在这里的真目录」。

现在：

```text
CODEX_HOME（环境变量） = .../io.3r.codex/r/<short-id>     ← symlink
真实文件               = .../codex-instances/<uuid>/codex-home
sqlite rollout_path    = .../codex-instances/<uuid>/codex-home/sessions/...
```

改完：

```text
CODEX_HOME（环境变量） = .../io.3r.codex/r/<short-id>     ← 真目录
JSONL / state_5.sqlite / archived_sessions 都在这个目录
sqlite rollout_path    = .../r/<short-id>/sessions/...
长路径（可选）         = .../codex-instances/<uuid>/codex-home → 指向短目录的链接，只收容旧绝对路径
```

短，是为了 socket 仍然合法。真目录，是为了 Electron 按字符串判断 `rollout_path` 是否在 `CODEX_HOME/sessions` 下。不是再拆一套库，也不是取消隔离。

### 合同

- 受管理 App 启动用的 `CODEX_HOME` 必须是真实目录，不得再把活动 home 做成指向另一棵树的 symlink。
- `CODEX_SQLITE_HOME` 仍等于该 `CODEX_HOME`（Step 2 合同保留）。
- `managed_codex_home`、启动环境、SQLite 和会话 JSONL 必须有一个明确的短真实目录事实源；长路径只能作为兼容 symlink，不能作为第二份文件树。
- 启动前（App 未运行且无 writer）把现有长目录树迁到短路径；SQLite 中以旧长前缀开头的路径字段按边界改写成短前缀。短目录和长目录均为真实目录，或迁移状态无法判定时，必须 fail-closed。
- 迁完后，长路径若仍被引用，只允许作为指向短目录的 symlink，不能再当第二份文件树。
- Profile 继续保留 `allow_symlinked_codex_home = true`，以防残留链接；活动 home 本身不再依赖它。
- 不改 ChatGPT.app。不改默认 `~/.codex`。不代 Codex 释放 writer lock。writer lock 只作为 app-server 直连归档路径的限制；不能据此否定桌面手动归档。

### 已改哪里

仅 macOS `managed_app`。`src-tauri/src/managed_launch.rs` 已加入 home 布局协调器、短 home 的专用所有权检查、`home-layout.json` 恢复、SQLite 路径改写、`delete_instance_storage` 两类布局清理，以及 `prepare_launch_with_target` 的启动复用。`short_codex_home_link` 保留给 CLI 旧布局；Windows 与 CLI 不进入真目录迁移。回归已覆盖短路径不是 symlink、重启复用、长路径兼容链接、`rollout_path` 与 `CODEX_HOME` 同一前缀、边界不误改、迁移中断恢复与冲突拒绝。

### 验收

- 短 `CODEX_HOME` 是目录不是 symlink；`state_5.sqlite` 与 `sessions/` 在其下。
- 新会话的 `threads.rollout_path` 以短 `CODEX_HOME` 开头。
- 既有实例迁移后，`managed_codex_home`、启动回执和 `isolated_codex_storage_for_preset` 都返回同一短真实目录；重启不会重新分配或删除它。
- 删除实例后短真实目录、长兼容 symlink、生命周期目录均不存在。
- 退出隔离 App 后重启，不打开目标会话，Recents 归档空闲会话：出现 `archived_sessions/`，`archived=1`。
- app-server 直连归档在 writer lock 存在时仍可失败；桌面手动归档以实际结果为准，本机已有成功证据。

### 回滚

未迁树前：恢复 `short_codex_home_link` 为 symlink 即可。迁树后只能向前修，或把短目录再搬回长路径并重建 symlink；不能两棵树同时当真相源。

## Continuation / Step 3 handoff

Step 2 与 Step 5 已在本工作区完成，未启动或停止 Tauri 开发服务，也未触碰 `~/.codex`。手动归档已经验收；后续会话先读本文件和执行计划文首「2026-10-02」条，并将“通过对话归档”作为独立的桌面动态工具装配问题处理。

Step 5 接手提示（归档仍失败的根因修复，可原样粘贴）：

> Step 5 已完成并完成手动归档验收。若后续回归，先检查 `CODEX_HOME` 是否是短真实目录、`threads.rollout_path` 是否使用相同短前缀，以及 `archived_sessions/` 与 `threads.archived=1` 是否同步；不要启动、停止或重启 pnpm run tauri:dev，不要改 ~/.codex，也不要代 Codex 释放 writer lock。

Step 3 接手提示（可原样粘贴）：

> 按 docs/designs/2026-10-02-isolated-app-thread-archive.md 继续隔离 App 归档修复，从 Step 3 实施 launcher/lib.rs/导入调用面与错误文案。Step 2 已让受管理 App 的 CODEX_SQLITE_HOME 等于 launch_codex_home，并在启动前迁移旧 SQLite/WAL；不要启动、停止或重启 pnpm run tauri:dev，不要改 ~/.codex。

实施顺序：

1. Step 2 代码与回归测试已完成。
2. 后续会话按上面提示实施 Step 3。
3. Step 3 单测通过后，退出隔离 App，再让 commander 自己重启 3R 并启动隔离 App 做归档验收。
4. 验收：退出隔离 App 后重启，不要打开目标会话，从 Recents 归档。

Step 2 已完成 `managed_launch.rs`、`models.rs` 注释和迁移回归；`codex-launcher.rs` 环境断言、`codex-control.ts` 文案和 `session_import.rs` 回归属于 Step 3，可继续补齐。
