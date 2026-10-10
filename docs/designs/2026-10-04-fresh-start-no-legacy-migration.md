# R Codex 全新工作区启动方案

> 状态：已实施并完成开发环境验证。
>
> 日期：2026-10-04
>
> 决策：不再迁移或读取旧版 `3rcodex` / `io.3r.codex` 留存的数据。R Codex 以全新工作区启动，旧数据保留在原位置但不再属于新版本的输入。

## 目标

让新版本 R Codex 在存在旧版数据、旧会话目录、符号链接或运行时 socket 时仍能正常启动，并从空的 R Codex 工作区开始。用户需要重新添加 Provider、模型选择和 API 登录渠道。

## 范围

- 停止 `io.3r.codex`、`io.3r-gpt.app` 到 `io.rcodex` 的应用目录、SQLite、实例目录和 ready 文件迁移。
- 停止从旧 Keychain service/account 读取或复制 Provider/API Key。
- 停止从旧 localStorage key 恢复主题或其它控制面状态。
- 保留 `io.rcodex/rcodex.sqlite3` 自有数据库的 schema migration；这不是跨应用数据迁移。
- 保持默认 Codex 的 `CODEX_HOME`、`auth.json`、原配置、原会话和原凭据完全不触碰。

## 非目标

- 不自动删除旧应用目录、旧数据库、旧 Keychain 条目或旧会话。
- 不提供旧 Provider、RouteSet、运行实例、诊断、请求档案或会话的导入入口。
- 不通过创建空库覆盖已经存在的有效 `io.rcodex/rcodex.sqlite3`。
- 不停止、接管或修改旧 Gateway；新旧控制面是否同时运行由各自生命周期和端口错误独立处理。

## 启动合同

### 数据库

`Database::open` 只执行以下步骤：

1. 创建当前 Tauri `app_data_dir`（`io.rcodex`）；
2. 打开 `rcodex.sqlite3`；
3. 对该数据库执行现有 schema migration、连接配置和过期数据清理。

它不得扫描父目录中的旧 identifier，不得创建或读取迁移 marker，不得复制目录或递归遍历旧会话树。因此旧目录中的 socket、锁文件、符号链接和损坏文件不会影响新版本启动。

### 凭据

控制面、launcher 和 Gateway 只使用当前 `io.rcodex` / `rcodex-secrets` 凭据边界。旧 `io.3r.codex`、`3r-secrets` 和其它历史 service/account 不再作为 fallback 或迁移来源。新工作区没有 Provider 凭据时显示未配置，用户重新录入 API Key。

### 首次启动空工作区

- `ManagedLaunchStore` 是唯一可写入受管理工作区的启动所有者。它在控制面启动时以单一事务确保恰有一个未删除的 `managed_app` 工作区；新工作区没有 Provider、没有模型，状态为 `needs_model_selection`。
- 该确保操作幂等：已有工作区绝不被替换、合并、改名或改写；重启不生成第二个工作区。
- 前端首屏保留空工作区，并明确提示用户选择已有模型渠道或新建模型渠道。创建 Provider 只保存渠道和凭据，绝不自动把它加入工作区；用户仍须确认加入并选择模型。
- 不能从空工作区启动 App 或 CLI；缺少模型、渠道或 API 登录渠道继续给出对应的禁用原因。

### 前端存储

只读取 `rcodex.*` key。首次运行没有新主题设置时使用默认 `system`，不再读取 `3rcodex.*` 或 `3r-gpt.*`。

### 失败与一致性

- 当前 `io.rcodex/rcodex.sqlite3` 损坏、schema 超前或无法打开时仍明确失败；不以空库静默替代。
- 旧目录存在与否不改变启动结果。
- 新数据库由单一 `Database::open` 路径创建和迁移；旧目录不参与任何写入。

## 实施切片

1. **文档合同**：已完成；品牌/重命名文档已标记旧迁移章节为历史，当前状态改为全新工作区。
2. **数据库边界**：已完成；删除旧应用目录迁移调用、来源常量、marker、staging 复制、旧 Gateway 探测和跨目录迁移测试；保留当前数据库 schema migration 测试。
3. **凭据边界**：已完成；移除 `legacy_service`、旧 bundle account 和旧 Keychain fallback；主进程、launcher、Gateway 只读取当前 service。
4. **前端边界**：已完成；删除旧主题 key fallback，保留当前 `rcodex.theme` 的读写。
5. **残余清理**：已完成；旧数据路径只剩历史/协议兼容测试命中，不再作为启动输入。
6. **首次启动准备**：已完成；`io.rcodex` 由当前版本直接创建，旧 `io.3r.codex` 不触碰。
7. **空工作区配置**：已完成；启动时创建空的 `needs_model_selection` 工作区，首屏引导用户选择或新建模型渠道。
8. **旧数据清理**：已完成；旧数据已按经审查的一次性白名单清理，脚本执行完毕后从仓库删除。

## 验收标准

- 旧 `io.3r.codex` 中包含数据库、符号链接和 `SingletonSocket` 时，`pnpm run tauri:dev` 能完成启动，不再出现“无法复制迁移文件”或 panic。
- 新工作区没有导入 Provider、运行实例、诊断或会话；首次启动持久化一个空工作区，并显示选择或新建模型渠道的提示。
- 在新 UI 中重新添加 Provider 和 API Key 后，渠道不自动加入工作区；用户确认加入并选择模型后才能启动，重启仍能读取新库和新凭据。
- 旧目录、旧 Keychain 条目和默认 Codex 配置的字节内容不被修改。
- `pnpm run check`、`pnpm run build`、`cargo fmt --all -- --check`、`cargo check --all-targets` 和定向 Rust 测试通过。
- 完成一次真实 macOS 开发启动，记录窗口出现、空工作区显示和无迁移错误。

## 回滚与残余风险

- 回滚只涉及仓库代码和新版本自有 `io.rcodex` 数据；旧 `io.3r.codex` 保留，因此旧版本仍可单独使用旧数据。
- 新版本不会自动恢复旧数据；若未来需要恢复，必须另行设计显式、只读、受控的导入流程。
- 旧 Keychain 条目会成为未使用的遗留项；清理它们是独立的、需明确授权的后续任务。

## 验证记录

- 2026-10-04：清理 `src-tauri/target` 后，sidecar 编译和 Tauri 编译通过；启动 setup 因旧目录递归复制 `SingletonSocket` 失败。该实验证明跨 identifier 迁移是当前启动阻塞点。
- 2026-10-04：确认 `/Users/populus/Library/Application Support/io.rcodex` 当前为空，适合在移除迁移代码后进行全新启动验收。
- 2026-10-04：`cargo test --lib` 通过，295 passed / 0 failed。
- 2026-10-04：`pnpm run check` 与 `pnpm run build` 通过。
- 2026-10-04：新增回归测试 `open_path_ignores_legacy_app_data_and_runtime_sockets` 通过，证明旧目录中的数据库与 Unix socket 不影响新工作区。
- 2026-10-04：`cargo fmt --all -- --check` 与 `cargo check --all-targets` 通过；`pnpm run tauri:dev` 成功进入运行态，Vite 在 `http://localhost:5173/` 就绪，`target/debug/r-codex` 保持运行。新目录已创建 `rcodex.sqlite3`、会话档案库和 `codex-instances`，未读取旧目录。
- 2026-10-04：先以 dry-run 检查旧数据清理脚本列出的 15 项约 3.6 GiB，再由执行代理在交互确认后删除；删除后 dry-run 返回无候选项，当前 `io.rcodex` 仍为约 1.0 MiB，Keychain 未触碰。该一次性脚本随后删除，不作为产品功能保留。
- 2026-10-04：`ManagedLaunchStore::from_database` 在控制面第一次读取快照前，以即时 SQLite transaction 创建唯一的 `managed_app` 空工作区。回归测试 `initial_workspace_seed_is_idempotent_and_preserves_configured_workspace` 覆盖首次空状态、重复执行不重复创建，以及不覆盖已配置的渠道/模型。
- 2026-10-04：保存 Provider 不再创建或自动加入工作区；空工作区首屏展示“选择或新建模型渠道”入口，用户确认加入渠道并选择模型后才可启动。`cargo test --lib` 297 passed；`pnpm run check`、`pnpm run build` 与 `cargo fmt --all -- --check` 通过。
- 2026-10-04：运行中的 `pnpm run tauri:dev` 已重载新代码；只读查询当前 `io.rcodex/rcodex.sqlite3` 得到 `managed_app | R Codex | none | needs_model_selection | 0 渠道 | 0 模型`。Browser 插件和项目 Playwright 均不可用，未将 Tauri IPC 流程伪装成普通浏览器验收。
