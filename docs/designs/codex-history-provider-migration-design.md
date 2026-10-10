# 会话 Provider 修复与后续同步

> 历史记录（2026-09-22）：相关默认 App / 源历史修复入口已移除，不再执行本文的写入或恢复步骤。当前合同见 [独立工作区改造](2026-09-21-isolated-only-product-plan.md)；不做默认配置清理向导。

> 状态：实现及本地自动验收完成，2026-09-16；真实 Codex 恢复与 Windows 实机待验。当前仅交付显式会话修复；默认 App ↔ 隔离 App 同步暂缓。

## 参考实现

核对 Cockpit 固定提交 `c9e166e7595ea81aefd5ffb5f2ec82d999841618` 的 [codex_session_manager.rs](https://github.com/jlcodes99/cockpit-tools/blob/c9e166e7595ea81aefd5ffb5f2ec82d999841618/src-tauri/src/modules/codex_session_manager.rs)、[codex_thread_sync.rs](https://github.com/jlcodes99/cockpit-tools/blob/c9e166e7595ea81aefd5ffb5f2ec82d999841618/src-tauri/src/modules/codex_thread_sync.rs) 和 [codex_account_model_catalog.rs](https://github.com/jlcodes99/cockpit-tools/blob/c9e166e7595ea81aefd5ffb5f2ec82d999841618/src-tauri/src/modules/codex_account_model_catalog.rs)。借鉴其配置身份、rollout 元信息、threads 与本机 catalog 的一致性目标；3R 独立实现显式维护服务及恢复日志，当前不复制其自动切换、可见性重建或跨实例同步行为。

## 产品合同

用户在“历史与测试记录 / 会话修复”选择一个环境，查看历史 `model_provider` 分布，选择源名称并输入自定义目标名称，先预览，再确认已退出 Codex 后应用。目标必须是该环境 config.toml 已定义的 Provider，或内建 `openai`。自定义名称大小写精确匹配；不把 3R 的内部 Provider ID 误当成 Codex 配置定义。界面展示已配置的候选名称；不存在时明确要求先定义，禁止写出新的悬空引用。

本功能修复 `Model provider … not found` 的身份不一致，不声称解决第三方模型对旧消息、推理和工具调用的协议兼容性。原方案“Gateway 运行时别名解析优先”撤销：配置加载发生在网关请求前，3R 内部映射无法拦截。当前不自动复制服务商定义、密钥、认证数据或修改 config.toml 的默认 Provider；用户配置已经定义的自定义名称即可作为目标。

## 写入边界

- `HistoryRepairService` 是唯一历史写入入口；Tauri 命令只适配输入/输出，React 只发起扫描、预览、应用和恢复。
- 每次只处理一个后端登记环境，前端不得传任意目录。默认环境为当前 CODEX_HOME；隔离环境从 3R 数据库已有 managed_app runtime binding 解析独立 home/sqlite 根，不遍历整个用户目录。
- rollout 只发现 `sessions/`、`archived_sessions/` 内 JSONL，递归不跟随链接。每个 `type=session_meta` 记录只替换 `payload.model_provider` 的 JSON 字符串字节；其它字节、换行与未知数字原样保留。同一文件多条元信息全部检查，无元信息、损坏记录或重复 model_provider 字段时阻止应用，不静默跳过。
- 数据库候选限定 home 根的 state_*.sqlite、home/sqlite 下 .db/.sqlite/.sqlite3 以及显式 CODEX_SQLITE_HOME 根。只探测 `threads.model_provider`、`local_thread_catalog.model_provider`；未知结构/身份键不猜测写入。表上若存在会在 `UPDATE model_provider` 时触发的触发器（无列限定的 `UPDATE`，或 `UPDATE OF` 包含 `model_provider`，或 `INSTEAD OF`）仍失败关闭。仅维护 `created_at` / `updated_at` / recency 等其它列的当前 Codex 时间戳触发器允许，因为它们不会因本次 Provider 写入而执行，也不会改时间戳。
- 本迭代更新现有目录行，不补建目录、不改标题、用户事件、工作目录、工作区状态、时间戳、session_index 或会话内容。避免将 Cockpit 的可见性重建策略误作为名称修复的必需写入。
- 配置声明、schema 和待修改数据纳入预览指纹；应用前必须再次匹配，拒绝过期预览。
- 本机目录表只更新已识别本机 host 的行；不能确认 host 时阻止目录表写入，避免改远端缓存。恢复时重新确认 journal 的 host 仍为本机，主机身份变化则拒绝恢复。

## 一致性、备份与恢复

- 预览只读，返回分布、环境路径、目标定义状态、元数据及数据库行数、跳过/阻止原因，绝不返回正文。
- 应用/恢复要求用户确认停止相关 App/CLI，后端再检测 Codex 写入进程；无法核验时拒绝，不自动关闭用户 App。
- 同一 3R 进程中修复及配置/启动/实例删除/退出清理入口互斥；状态轮询在维护期间仍可读，但跳过会恢复配置的清理。文件变更前再校验摘要；全部待改数据库取得 IMMEDIATE 事务锁后才写会话文件，失败按事务回滚。
- 应用前创建私有恢复目录：会话文件备份和 SQLite 行级变更日志（主键、原值、目标值）；Unix 采用 0600 文件/0700 目录并同步新目录的父目录。应用数据根先规范化，避免 Windows 扩展路径前缀或 macOS 路径别名导致备份不可读。备份可能包含历史正文，只作为本机恢复文件，绝不进入 3R 数据库、日志或前端。不备份 auth.json 或 config.toml。
- 所有备份完成且源未变化才开始写入。逐文件原子替换及逐库事务无法形成跨文件数据库事务，使用持久 manifest + 补偿恢复；不宣称跨文件强原子。
- 写入完成重新扫描验证零残留。普通失败自动恢复；进程中断留下 pending 清单，界面允许恢复。恢复前匹配当前目标摘要/允许状态，外部修改则拒绝覆盖，保留备份供人工处理。
- SQLite 不复制整库；行级变更日志的预览结果在持有全部数据库写事务期间复验并持久化，恢复只回写对应主键的 Provider，保留后来新增的记录及其它列。
- 首份 manifest 之前中断留下的空目录或仅临时 manifest 目录不阻塞启动，不删除正在准备的目录；有真实备份但缺失 manifest 时仍拒绝猜测恢复。成功修复后的恢复预检遇到冲突，保持原 `applied` 状态；未开始写入不会被错误标为待恢复并永久阻塞启动。
- 备份有 ID、状态和本地路径；恢复可在重启 3R 后发起。成功后提示重新打开 App，不自动替用户发模型请求。

## 命令与界面

- `get_history_repair_scopes()`：登记环境列表。
- `inspect_history_providers(scope_id)`：Provider 分布与 config 候选。
- `preview_history_provider_repair(input)`：源集合、目标、作用域；返回 token/摘要及计数。
- `apply_history_provider_repair(input, preview_token, confirmed_stopped)`：预览必须匹配，返回备份 ID/计数。
- `list_history_repair_backups()` / `restore_history_provider_repair(backup_id, confirmed_stopped)`：可恢复任务列表及恢复。

界面有 loading/空态/校验失败/忙碌/预览过期/成功/恢复冲突状态；编辑输入失效旧预览。应用失败清除预览，不能显示“修复完成”。同步按钮不提供伪功能。

## 验收

- 普通、归档、多 session_meta、多源合并、现有自定义 Provider 成功；非元数据行/未知字段语义不变。
- SQLite 两张表更新，忽略其它表，NULL/空/已目标值不误改；远端目录行不误改。
- 缺失目标、非法名称、损坏 JSONL、未知 schema、会改 `model_provider` 的触发器、符号链接、数据库锁、进程占用、过期预览均阻止写入。当前 Codex `threads` 时间戳/recency 触发器允许扫描和应用。
- 预览不写源；重复执行幂等；备份恢复保留原数据，部分失败可恢复；后续外部变动时恢复拒绝覆盖。
- Rust fixture 端到端验证、编译/格式检查、前端构建、浏览器 fixture 成功及失败路径。当前会话正在 Codex 中工作，不对真实 ~/.codex 应用修复；真实恢复验收单独记录。

## 后续同步（待办，用户明确暂缓）

默认 App 与隔离 App 双向手动同步；选择源/目标及会话，预览冲突后复制目标副本，目标使用目标环境有效 Provider。源保持不变；覆盖/同 ID 冲突必须明确；附件、归档、分支/子代理关系、session_index、threads/local_thread_catalog 的重建作为整体合同验证。禁止复制 auth、Cookie、Electron 登录库或上游凭据。先实现单次复制，自动双向同步另行设计。

## 执行状态

- [x] 核对 Cockpit 四类 Provider 持久化位置及额外同步入口。
- [x] 修正旧别名方案、定义当前写入边界和同步待办。
- [x] 后端扫描/预览/备份/应用/恢复。
- [x] 界面和错误呈现。
- [x] 回归验证与文档收尾。

## 实施与验证记录（2026-09-16）

实现位置：`src-tauri/src/history_provider_repair.rs`（单一服务与恢复状态）、`history_provider_repair/sqlite.rs`（数据库适配）、`history_provider_repair/tests.rs`（回归）、`src-tauri/src/lib.rs`（命令及维护互斥）；前端为 `src/HistoryProviderRepairPanel.tsx`、`App.tsx`、`codex-control.ts`、`dev-control-fixture.ts`、`styles.css`。JSON 原始值支持启用 serde_json 已有依赖的 raw_value feature，无新增包。

| 验证 | 实际结果 |
| --- | --- |
| `cargo fmt --all -- --check`、`cargo check --all-targets` | 通过 |
| `cargo test --all-targets` | 256 项通过：225 库测试、1 gateway、14 launcher、16 集成测试；其中修复专项 18 项。首次沙箱运行的 11 个既有本机端口用例报 EPERM，获准沙箱外重跑后全部通过。真实 Provider 的 opt-in 合同未启用。 |
| `pnpm run check`、`pnpm run build` | 通过 |
| 浏览器模拟数据 | 1280×900 与 390×844；默认页/诊断抽屉、名称不存在时禁用、custom-team 预览、停机勾选门、修复完成、备份恢复、预览过期报错及清除应用按钮均通过；窄屏无横向溢出，无框架错误覆盖或运行时异常。控制台仅有现有 `/favicon.ico` 缺失导致的 404，已定位并从应用错误断言中单独排除。 |

浏览器入口 `http://127.0.0.1:5173/?ui-fixture=1`；过期预览复现加 `&history-error=conflict`，占用复现加 `&history-error=busy`。本地临时 Playwright 脚本 `/private/tmp/3rcodex-history-ui-check.cjs` 及截图位于 `/private/tmp/3rcodex-history-{preview,restored,mobile-error}.png`，不提交生成物。初次模拟错误使用 JS Error 与 Tauri 字符串拒绝不一致，已修正 fixture；另补齐原 fixture 缺少的请求历史和版本状态，避免抽屉白屏。独立浏览器脚本自动审批曾超时，后经脚本范围核对获准运行；没有将未启动的检查计为通过。

当前边界与后续验收：

- 未应用于用户真实历史、未发送真实模型请求。真实修复须从 3R 运行，退出所有 Codex App/CLI 后扫描、预览、应用，再重新打开原任务验证加载与续聊；退出后可用备份撤销。
- 当前在 macOS 验证；Windows 原子替换、访问控制、进程检查与打包需实机复验。Windows 备份继承用户应用数据目录权限，未声称完成 Unix 模式位或父目录 fsync 等价保障。
- 进程名检查采取保守策略，任何已识别 Codex/ChatGPT 写入进程均阻止维护；无法阻止用户或其它工具在核验后从 3R 以外重新启动，维护期间必须保持停机。
- 扫描限制 100,000 个 rollout、单行 16 MiB、配置 4 MiB；未知数据库结构、会在 `UPDATE model_provider` 时触发的触发器、符号链接/硬链接及无法解析记录失败关闭。当前 Codex 为 `threads` 维护毫秒时间戳和 recency 的触发器允许。SQLite 只读扫描不改业务行，底层 WAL/SHM 管理不承诺字节不变。
- 目标定义检查不是完整的 Codex 配置语义或上游兼容验证。名称修复不能自动解决第三方旧会话的消息/推理/工具协议差异。
- 回滚功能代码时不得删除 `history-provider-repairs/`：先在当前版本恢复未完成任务，保留备份，再移除功能。会话同步继续按上文待办单独设计。

## 实施与验证记录（2026-09-21）

默认 App 当前 `state_5.sqlite` 的 `threads` 表带有 `created_at_ms` / `updated_at_ms` / `recency_at` 维护触发器。旧实现把任意触发器视为未知 schema，扫描直接失败，会话修复入口表现为不可用；打开仍指向 `custom`、`codex_local_access` 等已从 config.toml 消失的 Provider 的旧会话时，Codex App 会因 Provider 加载失败反复重建界面。现改为只拒绝会在 `UPDATE model_provider` 时执行的触发器。

验证命令与结果见执行计划同日记录。不对正在使用的真实 `~/.codex` 自动应用修复。
