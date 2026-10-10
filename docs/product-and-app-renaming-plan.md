# R Codex 项目与桌面应用名称改造方案

> 状态：命名基线已确认；阶段 0/1、前端/运行时改造、应用数据迁移保护与工作区目录整理已完成，Release `.app` 已验收
> 日期：2026-08-28
> 旧名称：`3r-gpt`
> 新技术名称：`rcodex`

> 2026-10-04：产品显示名与技术 namespace 均以 [R Codex 品牌与技术命名空间改造](designs/2026-10-04-r-codex-display-rename.md) 为准。commander 后续决定放弃旧 `3rcodex` / `io.3r.codex` 数据迁移，当前启动合同见[全新工作区启动方案](designs/2026-10-04-fresh-start-no-legacy-migration.md)；本文件第 4–8 节的跨目录迁移内容仅作历史记录。

## 1. 目标与原则

本方案把项目、桌面应用、运行时、打包产物和文档统一到 `rcodex` 命名体系。当前全新工作区方案不读取旧 Provider、运行实例、Gateway 状态或凭据；默认 Codex 环境仍完全不受接管。

这不是一次简单的页面标题替换。当前名称同时存在于前端包、Tauri 元数据、Rust crate、数据库文件、Keychain 服务、临时目录、诊断字段、协议兼容常量和文档中；这些引用必须区分“可以直接改名的显示标识”和“需要迁移/兼容的持久化或运行时标识”。

核心原则：

- **品牌与技术分层**：用户看到 `R Codex`，代码、文件和命名空间使用 `rcodex`；Rust 标识遵守 Rust 的命名规则。
- **全新工作区**：新版本只读取 `io.rcodex` 自有数据；旧目录、旧数据库和旧凭据保留但不作为输入。
- **不改变 Codex 边界**：默认 `CODEX_HOME`、默认 Codex App、用户自行执行的 `codex` 命令不受接管；只有 R Codex 显式启动的运行实例使用受管理配置。
- **不改变 sidecar 合同**：`codex-launcher` 与 `codex-gateway` 是职责型可执行文件名，继续保留，避免影响 Tauri `externalBin`、运行中的实例和升级恢复。
- **可回滚**：旧数据不被删除，回滚到旧版本仍可使用；新版本的当前数据库损坏时明确失败，不静默覆盖。

## 2. 正式命名基线

| 层级 | 目标值 | 处理方式 | 备注 |
| --- | --- | --- | --- |
| 产品/应用显示名 | `R Codex` | 全面替换用户可见文案、窗口和安装包名称 | 允许空格和大小写 |
| 技术 slug | `rcodex` | 用于 npm 包、目录、临时前缀和新的持久化文件名 | 小写 ASCII，无分隔符 |
| 仓库目录 | `rcodex` | 新 clone/checkout 使用该目录名 | 当前运行中的工作区不移动 |
| npm 包名 | `rcodex` | 修改 `package.json`，重新生成锁文件元数据 | 包为 private，不改变发布 API |
| Tauri `productName` | `R Codex` | 修改安装包、窗口和系统应用列表显示名 | 用户可见 |
| Tauri `identifier` | `io.rcodex` | 只创建和读取新目录；不读取旧 `io.3r.codex` | 旧目录保留但不参与启动 |
| Rust package | `r-codex` | 修改 Cargo 包名和默认运行目标 | `r` 开头可作为 Rust 标识 |
| Rust library | `r_codex_lib` | 修改 `[lib] name` 及引用 | 生成的内部符号使用下划线 |
| 前端存储命名空间 | `rcodex.*` | 只读写新 key；默认主题为 `system` | 旧 key 不读取 |
| SQLite 文件 | `rcodex.sqlite3` | 在新目录直接创建/打开并执行 schema migration | 不导入旧文件 |
| 新凭据服务 | `io.rcodex` | 只读写新 service/account | 旧条目不读取、不删除 |
| Gateway 控制字段 | `x-rcodex-control-token` | 过渡期继续接受 `x-3r-gpt-control-token` | 新写入用 rcodex；旧字段只读兼容 |
| Sidecar 可执行文件 | `codex-launcher`、`codex-gateway` | 保留文件名，更新显示名称和诊断标签 | 这是稳定的职责型接口 |

### 2.1 为什么显示名和技术 slug 分开

`R Codex` 适合窗口、菜单、帮助文案和安装包；`rcodex` 适合目录、包名、文件前缀和自动化脚本。这样可以避免用户界面出现全小写技术串，也避免脚本因空格、大小写或本地化文本而不稳定。

### 2.2 Rust 命名

Rust 的 crate/library 标识需要可作为代码标识符使用。`rcodex` 可直接用于 library，Cargo package 使用可读的 `r-codex`；用户可见的 sidecar 文件仍是 `codex-launcher` 和 `codex-gateway`，桌面产品为 `R Codex`。

## 3. 改造范围

### 3.1 用户可见层

- `index.html` 页面标题、React 顶栏、空状态、错误提示、诊断标题、帮助和启动说明；
- Tauri 窗口标题、macOS/Windows 应用列表名称、安装包名称；
- “R Codex Gateway”“R Codex 运行实例”等产品文案；
- README 和面向用户的 Markdown 标题、截图说明和命令示例。

### 3.2 工程与构建层

- `package.json` 的 `name`，以及 `pnpm-lock.yaml` 中由包名生成的 importer 元数据；
- `src-tauri/tauri.conf.json` 的 `productName`、`identifier`、窗口标题和 bundle metadata；
- `src-tauri/Cargo.toml` 的 package、default-run、library 名称、description、authors；
- `src-tauri/Cargo.lock` 的生成包名；
- `scripts/tauri.mjs`、构建说明和产物清单；
- 构建产物中保留 `codex-launcher`、`codex-gateway`，不把职责型 sidecar 重命名为带品牌的长文件名。

### 3.3 Rust 与运行时层

- `3r-gpt` 的错误文案、日志前缀、临时文件前缀、测试 fixture 和诊断 `owned_by` 标签改为 `rcodex` 或 `R Codex`；
- launcher/gateway 的用户可见错误和帮助文本改为 `R Codex`；
- `official_accounts.rs` / `official_auth.rs` 中用于客户端识别的名称改为新值，并保留协议探针回归测试；
- `managed_gateway.rs` 的品牌诊断字段改为新值，但控制请求在过渡期同时兼容旧 header/旧 sidecar；
- 数据库、ready 文件、实例生命周期目录和临时目录统一采用新的命名空间。

### 3.4 文档与文件路径层

建议重命名以下包含旧品牌的文档文件，并同步更新所有链接：

- `docs/3r-gpt-product-principles.md` → `docs/3rcodex-product-principles.md`；
- `docs/designs/3r-gpt-ui-redesign.md` → `docs/designs/3rcodex-ui-redesign.md`。

其他 `codex-*.md` 文件不包含旧品牌文件名，可保留；正文中的旧名称按“用户可见文本 / 历史兼容说明 / 内部协议兼容常量”分类处理。

## 4. 持久化与兼容迁移设计

### 4.1 应用数据目录

新 `identifier` `io.3r.codex` 会导致 Tauri 应用数据目录发生变化。启动时按以下顺序处理：

1. 加载新目录并获取迁移锁；
2. 若新目录不存在而旧目录 `io.3r-gpt.app` 存在，检查旧应用/旧 Gateway 是否仍在运行；
3. 若存在活动进程，提示用户先退出旧版本，禁止并发打开两个控制面；
4. 将旧数据库、ready 状态、非敏感生命周期元数据复制到新目录，使用临时目录和原子替换；
5. 写入 `migration_version` 和来源摘要，重启后迁移流程必须幂等；
6. 新目录验证成功后保留旧目录备份，不自动删除。

### 4.2 SQLite

- `3r-gpt.sqlite3` → `3rcodex.sqlite3`；
- 先做文件级备份，再执行现有 schema migration；
- 保留 Provider、RouteSet、运行实例、请求账本、诊断和状态记录的原 ID；
- 打开失败时明确报错并回退到旧副本，不能创建空库掩盖迁移失败；
- 提供一次性“迁移完成/迁移失败”诊断事件，便于用户确认数据是否已接管。

### 4.3 Keychain / Credential Manager

旧服务为 `io.3r-gpt.app`，新服务为 `io.3r.codex`。迁移顺序必须是：

1. 读取旧服务中的 Provider/Gateway secret reference；
2. 写入新服务；
3. 使用新服务读取并验证每个 secret；
4. 数据库引用切换到新服务；
5. 旧服务只在用户明确确认或后续清理版本中删除。

任何一步失败都保留旧服务，避免出现“数据库显示已配置但实际取不到密钥”。默认 Codex `auth.json` 不参与迁移。

### 4.4 前端 localStorage

首次启动时将 `3r-gpt.theme` 等旧 key 复制到 `3rcodex.theme`，新代码只写新 key。迁移期间若两者同时存在，以新 key 为准；旧 key 不用于恢复业务数据。

### 4.5 Sidecar 与运行实例

- 运行中的 `codex-gateway` 不因品牌迁移被静默杀死；
- 新应用启动时探测旧 ready 文件和端口，若 sidecar 健康则完成归属迁移或明确提示用户；
- 不能让新旧应用同时写同一 SQLite 或抢占同一个控制端口；
- 旧实例的绑定、实例密钥和 RouteSet 快照保持不变；新名称只影响显示与新建资源；
- 流式 Responses/SSE（服务器发送事件）不承诺跨应用升级续接，升级前应提示用户完成或重试当前请求。

## 5. 分阶段执行顺序

### 阶段 0：基线与保护

- 固化本方案和名称映射表；
- 扫描所有 `3r-gpt` 引用并标注“改名 / 保留兼容 / 历史说明”；
- 增加迁移版本、旧 identifier、旧数据库和旧 Keychain 服务常量；
- 确认没有运行中的 Gateway、CLI 或 Codex App 实例。

### 阶段 1：无数据破坏的品牌改造

- 先改用户可见显示名、README、文档标题、窗口标题和安装包名称；
- 改 npm/Cargo 的工程标识与诊断文案；
- sidecar 文件名和控制协议旧 header 暂不改变；
- 构建并验证开发版和 Release `.app`。

### 阶段 2：技术 slug 与运行时前缀

- 将新建数据库、临时文件、ready 文件和日志前缀切换到 `3rcodex`；
- localStorage 执行一次性 key 迁移；
- 运行时继续读取旧文件，写入只使用新文件；
- 添加旧/新名称混用时的回归测试。

### 阶段 3：应用 identifier 与凭据迁移

- 切换到 `io.3r.codex`；
- 实现应用数据目录和 Keychain 的幂等迁移；
- 验证 Provider、Gateway、运行实例和默认 Codex 边界；
- 迁移失败时保留旧目录、旧数据库和旧凭据并阻止继续启动。

### 阶段 4：路径和文件整理

- 代码、文档和构建验证完成后，新 clone/checkout 使用 `rcodex` 目录；当前运行中的工作区不移动；
- 重命名带旧品牌的文档文件并修复 Markdown 链接；
- 重新生成锁文件、Cargo 产物和 Tauri bundle；
- 更新开发脚本、CI/发布脚本和用户安装说明。

## 6. 明确保留的旧名称

以下内容不应因为品牌改造而直接删除：

- 旧 Tauri identifier `io.3r-gpt.app`：用于升级迁移和旧进程探测；
- 旧 Keychain 服务 `io.3r-gpt.app`：迁移验证完成前保留；
- 旧数据库 `3r-gpt.sqlite3`：作为迁移源和回滚备份；
- 旧控制 header `x-3r-gpt-control-token`：过渡期继续接受；
- 历史文档中的旧名称：可在迁移说明中保留，但不得作为当前产品名称展示；
- `codex-launcher`、`codex-gateway`：保留稳定 sidecar 文件名。

## 7. 验收标准

### 新安装

- 应用、窗口、系统应用列表和安装包显示 `R Codex`；
- 新数据目录、数据库、localStorage 和新凭据服务使用 `rcodex` / `io.rcodex`；
- 生成的 `.app` 能正确携带并启动 `codex-launcher`、`codex-gateway`。

### 从旧版本升级

- 旧 Provider、密钥、RouteSet、运行实例、请求账本和诊断记录可见；
- 迁移中断后再次启动可继续，重复执行不会产生重复 Provider 或重复凭据；
- 旧应用或旧 sidecar 活动时不会发生双实例并发写入；
- 迁移失败不会以空数据库启动，也不会删除旧凭据。

### Codex 边界

- 直接点击系统 Codex App 或执行 `codex` 仍使用默认 `CODEX_HOME`；
- R Codex 显式启动的 CLI/受管理 App 仍使用实例专属配置和正确 Gateway；受管理 App 按运行时兼容性门开放，并限制为单个活动实例；
- 默认 App 不会被系统图标劫持，也不会自动打开最近的受管理实例。

### 工程验证

- `pnpm run check`、`pnpm run build` 通过；
- `cargo fmt --all -- --check`、`cargo check --all-targets` 和 Rust 全量测试通过；
- 使用旧数据 fixture 验证数据库、Keychain、localStorage、ready 文件迁移；
- 至少完成一次 macOS `.app` 打包验收，并检查 bundle identifier、窗口名和 sidecar 安装路径；
- `rg` 残余扫描只允许命中兼容常量、迁移代码和历史说明，不允许命中新的用户可见文案。

## 8. 风险与回滚

| 风险 | 影响 | 防护与回滚 |
| --- | --- | --- |
| identifier 变化造成数据目录变化 | 用户看不到旧 Provider/实例 | 启动迁移锁、旧目录探测、原子复制、失败保留旧目录 |
| Keychain 服务切换失败 | Provider 显示已配置但请求失败 | 新服务写入后立即回读验证，旧服务不删除 |
| 旧应用/sidecar 仍运行 | 双实例、端口或 SQLite 冲突 | 进程探测与活动锁，提示先退出旧版本 |
| sidecar 文件名被改动 | 打包后无法启动 Gateway/launcher | 保留职责型文件名，只改显示标签 |
| 文档/脚本链接失效 | 开发和发布流程中断 | 文件重命名后全量链接扫描和构建验证 |
| 迁移中断或磁盘空间不足 | 部分数据复制 | 临时目录、空间预检、原子替换、可重复执行 |

## 9. 当前状态与下一步

- **已确定**：产品显示名 `R Codex`、技术 slug `rcodex`、桌面 identifier `io.rcodex`、sidecar 文件名保留策略，以及全新工作区启动原则；
- **已实现**：npm/Tauri/Rust 工程标识、前端品牌与新 namespace、SQLite schema migration、带旧品牌文档的路径整理，以及兼容残留扫描；旧应用数据/Keychain/localStorage 读取路径待移除；
- **已验证**：`pnpm run check`、`pnpm run build`、Rust fmt/check/test 和 `pnpm run tauri build --bundles app` 均通过；Release bundle 的显示名、identifier 与 sidecar 安装路径已核验；
- **未完成**：移除旧应用数据、旧 Keychain 和旧 localStorage 的 fallback，并完成全新工作区实机启动验收；
- **已完成**：仓库目录已由 `3r-gpt` 移动为 `3rcodex`。旧数据、旧凭据和旧目录不自动删除；若未来需要恢复，另行设计显式导入流程。
