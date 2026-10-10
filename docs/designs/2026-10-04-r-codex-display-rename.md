# R Codex 品牌与技术命名空间改造

> 状态：显示名与技术命名改造、全新工作区启动调整已完成；真实 Tauri 开发启动已通过。详见[全新工作区启动方案](2026-10-04-fresh-start-no-legacy-migration.md)。
>
> 日期：2026-10-04
>
> 取代范围：取代 [项目与桌面应用名称改造](../product-and-app-renaming-plan.md) 的产品显示名和当前技术 slug 基线。`3r-gpt`、`3rcodex` 与 `io.3r.codex` 仅保留在历史记录中，不作为新版本运行时输入。

## 目标

用户能看到的产品名从「3R Codex」改为「R Codex」。句子里的简称「3R」也写成「R Codex」，不留单独的「R」。启动字标同样使用「R Codex」。

新写入的技术命名空间统一为 `rcodex`。它不等同于显示名：显示名可以含空格和大写；技术命名空间只用 ASCII 小写，供包、目录、数据库、Provider 和启动器协议稳定引用。

| 层级 | 新值 | `3rcodex` / `io.3r.codex` 的处理 |
| --- | --- | --- |
| 产品、窗口、安装包、文案 | `R Codex` | 仅用户可见内容替换 |
| npm 包 | `rcodex` | 新包名直接切换 |
| Rust package / library | `r-codex` / `r_codex_lib` | 新构建目标直接切换；sidecar 文件名保持不变 |
| Tauri identifier / 数据目录 | `io.rcodex` | 只创建和读取新目录；`io.3r.codex` 旧目录忽略 |
| SQLite 文件 / 迁移标记 | `rcodex.sqlite3` | 新目录中直接创建/打开；不再使用跨目录迁移 marker |
| 专用 Codex Provider ID | `rcodex` | 新 Profile、模型目录和配置从空工作区开始；不导入旧 Provider 或会话 |
| macOS 凭据服务 / bundle account | `io.rcodex` / `rcodex-secrets` | 只读取新条目；旧条目不读取、不删除 |
| 实例准入协议 | `X-RCodex-Instance-Key` / `RCODEX_INSTANCE_KEY` | 新启动只写新值；Gateway 在过渡期只读接受旧 header，且继续按同一实例密钥哈希核验 |

钥匙串条目的显示标签改为「R Codex」。旧标签和旧 service/account 不是新工作区的输入；新工作区没有凭据时由用户重新添加。旧条目保留但不读取、不删除。

## 2026-10-04 新决策：全新工作区启动

commander 已确认不再迁移或读取之前 `3rcodex` / `io.3r.codex` 留存的数据。下方原“迁移合同”仅保留为历史决策记录，当前实施以[全新工作区启动方案](2026-10-04-fresh-start-no-legacy-migration.md)为准：

- `Database::open` 不扫描旧目录、不复制旧 SQLite/实例目录、不读取迁移 marker；只创建或打开当前 `io.rcodex/rcodex.sqlite3`。
- 主进程、launcher、Gateway 只读取当前 `io.rcodex` / `rcodex-secrets` 凭据边界；旧 Keychain fallback 移除。
- 前端只读取 `rcodex.*` localStorage key；旧主题 key 不再恢复。
- 旧目录、旧凭据、旧 Gateway 不被自动删除、停止或接管；恢复旧数据需要未来另行设计显式导入。

## 历史迁移合同（已废弃）

以下内容记录此前的跨 identifier 迁移设计，不再是当前启动合同。当前合同见上方“2026-10-04 新决策：全新工作区启动”以及[全新工作区启动方案](2026-10-04-fresh-start-no-legacy-migration.md)。

### 所有权与写入路径

- **数据库与目录**：只有 `Database::open` 可以从 `io.3r.codex` 复制到 `io.rcodex`。它在新数据库不存在时运行，先拒绝检测到 `gateway-sidecar.json` 的旧目录，随后 checkpoint WAL、复制到同目录 staging、原子发布数据库和受管理目录、最后写入 marker。重复启动只读取已发布的新库。
- **Provider 与会话副本**：新 Profile 与导入副本只写 `rcodex`；不改用户原 Codex 历史。数据库 schema migration 在同一 SQLite transaction 中只将精确的系统启动方案名称 `3R Codex` 改为 `R Codex`，用户自定义名称不变。现有活动实例阻止迁移，终止后才能以新 namespace 重新启动。
- **凭据**：控制面是唯一迁移写入者。它从 `io.3r.codex` / `3r-secrets` 读取 bundle，写入 `io.rcodex` / `rcodex-secrets`，再由同一读路径回读确认；Gateway 与 launcher 只读取新条目并可在迁移窗口回退只读旧条目。任何 Keychain 失败都保留旧条目且不删除或覆盖配置。
- **启动协议**：配置生成器唯一写入新 header/environment variable。Gateway 在迁移窗口同时提取新、旧 header；仅旧 header 可用，两个 header 值相同也可用，两个值不一致则拒绝请求；转发时删除两种准入 header，防止泄漏上游。

### 状态、失败与回滚

- 迁移状态为“未开始 → staging → 已发布”。marker 存在而新数据库缺失时阻止启动；旧数据库、旧目录和旧 Keychain 条目都保留，因而回滚可重新使用旧构建。
- 不自动迁移正在运行的 Gateway、CLI 或 App。检测到旧 ready file、WAL 锁或未终止实例时提示用户先正常退出。
- 不更改 `3r-gpt` 迁移来源、`codex-launcher` / `codex-gateway` 文件名，或用户默认 Codex 目录。仓库根目录不在运行中的会话里移动；新的 clone/checkout 使用 `rcodex` 目录名。

## 执行计划

- [x] 记录 `R Codex` 与 `rcodex` 的命名决定、读旧写新规则和回滚边界。
- [x] 更新桌面、前端、官网与当前产品文档的显示名。
- [x] 切换 npm、Rust、Tauri 与新写入的内部 namespace。
- [x] （历史）实现并测试 `io.3r.codex` → `io.rcodex` 的目录/SQLite/Provider 迁移。
- [x] （历史）实现并测试 Keychain 与启动协议的兼容迁移。
- [x] 移除旧应用目录、旧 Keychain 和旧 localStorage 的读取/fallback，改为全新工作区启动。
- [x] 跑类型检查、Rust focused tests、构建和残余搜索；把实际结果写回本计划。

## 执行位置

本计划原先要求只在默认 Codex App 执行。commander 于 2026-10-04 明确授权在当前工作区开始改造；仍不得重启隔离 App 或接管隔离 `CODEX_HOME`。

隔离 App 正在使用这份仓库和 io.3r.codex 数据目录。代码修改不重启、不停止或重编运行中的隔离 App；新 identifier 的迁移仅在未来新版本启动时发生。

执行时不要运行 pnpm run tauri:dev，不要停止或重启隔离 App，不要运行 pnpm run tauri:build，除非 commander 另要求出包。

## 锁定决定

1. 完整产品名、窗口标题、安装包显示名、托盘提示、错误文案、启动方案显示名，都改为 R Codex。
2. 启动画面字标改为 R Codex。`src/boot-loading.tsx` 的画布文字与占位元素同步更新；`.boot-loading-mark` 用受限字号和最大宽度保证完整字标单行显示，粒子仍只在字形里流动。
3. 句子里的「3R」改成「R Codex」，或把句子改通顺。例如「导入到 3R」「退出 3R 后」「3R Gateway」分别成为「导入到 R Codex」「退出 R Codex 后」「R Codex Gateway」。不要机械删掉数字后留下「R」「导入到 R」「R：」。
4. Gateway 显示名改为 R Codex Gateway；直连显示名改为 R Codex Direct Provider；新 Provider ID 是 `rcodex`。
5. 钥匙串 ITEM_LABEL 改为 R Codex。新条目 service/account 为 `io.rcodex` / `rcodex-secrets`，且测试断言旧标签、旧服务和空标签都不是新共享身份。
6. `3rcodex`、`io.3r.codex` 与 `3r-secrets` 不再作为运行时输入；新工作区没有旧 Provider 或凭据，需要用户重新配置。
7. 新内部标识采用 `rcodex`；旧数据 identifier、service、数据库和 localStorage key 只保留在历史文档和已明确废弃的迁移记录中，不保留可达的旧数据 fallback。仍有独立协议兼容要求的旧 header 不受本决定影响。

## 不要改

这些稳定边界不随命名迁移改变：

- sidecar 文件名 codex-launcher、codex-gateway。
- 旧名 3r-gpt 和主题键 3r-gpt.theme。它们是迁移来源。
- 当前会话的仓库目录和 Git remote。后续新 clone/checkout 使用 `rcodex` 目录名。
- 未挂载的静态演示 src/demo-console.tsx 与 src/demo-console.css 已于 2026-10-08 删除，不随命名迁移保留。
- src-tauri/target/ 和 website/dist/。下次打包会跟着 productName 变。

## 要改的文件

按目录分三片。三片不要同时改同一个文件。

### 桌面包装

- src-tauri/tauri.conf.json：productName、窗口 title 和 identifier `io.rcodex`。
- index.html：页面 title。
- src/components/title-bar.tsx：非 macOS 标题文字。macOS 标题栏继续不画产品名。
- src/boot-loading.tsx 和 src/surface.css 的 .boot-loading-mark：字标和字号。
- src-tauri/src/lib.rs：托盘 tooltip、用户可见错误与版本检查 User-Agent。

### 界面和会写进配置的显示名

- src/App.tsx：未安装 Codex 的说明。
- src/use-app-controller.ts：退出确认与新启动方案显示名。已有数据库中的精确系统方案名由 SQLite schema v20 迁移为 `R Codex`，用户自定义名称不变。
- src/SessionImportPanel.tsx、src/ConversationArchivePanel.tsx、src/components/utility-drawer.tsx、src/components/runtime-panels.tsx、src/components/provider-panels.tsx、src/codex-control.ts、src/app-model.ts。
- src-tauri/src/managed_codex_config.rs：MANAGED_GATEWAY_DISPLAY_NAME 和 Direct Provider 显示名。
- src-tauri/src/managed_gateway.rs：失败文案前缀。
- src-tauri/src/database.rs：迁移失败时给人看的句子。
- src-tauri/src/official_accounts.rs：ClientInfo::new 的显示名与技术名参数。
- src-tauri/src/bin/codex-launcher.rs：给人看的启动错误。不要写成「R CLI」。
- src-tauri/src/{macos_keychain.rs,secrets.rs}：ITEM_LABEL、新凭据 service/account 与旧条目迁移。
- src-tauri/src/{database.rs,managed_codex_config.rs,managed_gateway.rs,managed_launch.rs,session_import.rs,lib.rs}：`rcodex` 的新写入、旧数据库迁移与旧启动协议兼容。
- src-tauri/Cargo.toml、Cargo.lock、src-tauri/src/{main.rs,bin/codex-launcher.rs,bin/codex-gateway.rs}、scripts/tauri.mjs：Rust package/library 新命名与构建引用。

### 文档和官网

当前入口要改，历史设计稿和执行记录不必逐篇替换：

- README.md
- AGENTS.md 的产品显示名与技术 slug。
- docs/3rcodex-product-principles.md
- docs/product-and-app-renaming-plan.md 第 2 节的显示名一行，改为 R Codex，并指向本计划。
- docs/designs/2026-09-21-isolated-only-product-plan.md 里「显示名为 3R Codex Gateway」那句。
- website/astro.config.mjs 的 title 和 logo alt。
- website/src/content/docs/index.mdx、getting-started/、guides/、reference/privacy.md、website/src/components/FlowSwitcher.tsx。
- .github/workflows/website.yml 只有工作流名，可改可不改，不影响页面。

src/dev-control-fixture.ts 的显示名和 fixture 路径跟着新 namespace 改；旧路径只留给迁移 fixture。

## 验收

在仓库根目录：

    pnpm run check

再用搜索确认 src、src-tauri/src、index.html、website/src、README.md、AGENTS.md、docs/3rcodex-product-principles.md、docs/product-and-app-renaming-plan.md 里不再把 3R Codex 当作用户可见产品名，也不在新写入路径使用 `3rcodex` 或 `io.3r.codex`。只允许迁移常量、兼容测试和明确不改的历史设计稿命中旧值。

在 src-tauri/：

    cargo test --lib macos_keychain::tests::shared_label_skips_acl_rewrite -- --nocapture
    cargo test --lib database::tests::legacy_app_data_migrates_database_and_instance_directories_without_deleting_source -- --nocapture
    cargo test --lib managed_codex_config::tests -- --nocapture

实际自动验证（2026-10-04）：`pnpm run check`、`pnpm run build`、`pnpm --dir website build`、`cargo fmt --all -- --check`、`cargo check --all-targets` 与 `cargo test --lib` 均通过；Rust 单元测试为 297 passed, 0 failed。官网构建仍报告既有的 `docs → 404` 内容入口警告，但生成 10 个页面并成功完成。

不启动 Tauri 开发窗口。改完后由 commander 自己重启隔离 App，确认启动字标是 R Codex、窗口和菜单显示 R Codex，并且渠道 Key 仍能读到。若钥匙串弹出授权，那是从旧 `io.3r.codex` 条目写入 `io.rcodex` 条目的迁移，不是换了一条密钥。

## 回滚

显示名部分可通过还原字符串回滚。命名空间迁移不删除 `io.3r.codex`、`3rcodex.sqlite3` 或旧 Keychain 条目；回滚到旧构建即可继续使用旧副本。不要通过删除新或旧凭据来回滚。

## Continuation

- Status: 代码、自动验证和 Tauri 开发启动完成；窗口可继续由 commander 手动检查。
- Important files: 本文件；src-tauri/tauri.conf.json；src/boot-loading.tsx；src-tauri/src/{database.rs,secrets.rs,macos_keychain.rs,managed_codex_config.rs}。
- Decisions made: 显示名和字标是 R Codex；新技术 namespace 是 rcodex；旧 3rcodex/io.3r.codex 不作为新版本输入；sidecar 文件名不改。
- Remaining steps: commander 确认新窗口/启动字标和空工作区；之后可按需重新添加渠道和 API Key 并进行打包验收。
- Known blockers: 当前工作区不是 Git checkout，无法用 Git 状态或 diff 作为变更基线；本次未重启隔离 App，也没有进行 Tauri 打包。
- Verification already run: `pnpm run check`、前端构建、Rust 格式检查、all-targets 检查和 `cargo test --lib`（295 passed, 0 failed）通过；新增旧目录/socket 隔离回归测试通过；`pnpm run tauri:dev` 已进入运行态。
- Suggested next command: commander 检查已打开的 R Codex 窗口；若需停止开发服务，在对应终端按 Ctrl-C。
