# Codex 多账户与受管理启动执行计划

## 2026-08-29 CLI/App 并行入口

### 产品决定

- 用户只面对一个“Codex 启动方案”概念；方案提供两个独立入口：`启动 Codex CLI` 与 `启动受管理 Codex App`。
- 当前产品把这条启动方案作为唯一“主工作区”；Provider/模型的增减更新同一方案并产生新的路由版本，暂不在界面创建第二条方案记录。底层多方案表结构保留，便于历史数据和未来多工作区扩展。
- 不提供“同时启动”按钮；两个入口可以先后启动且互不排斥。
- CLI 启动可指定本次工作目录；受管理 App 启动不接受前置工作目录，App 打开后由用户在 App 内选择目录。
- 受管理 App 的同一启动方案复用稳定运行实例、`CODEX_HOME`、SQLite 会话根和 Electron 数据目录；每次重新打开仍生成新的不可变方案快照、RouteSet 与实例密钥。默认 App 兼容模式不创建运行实例，改由独立兼容绑定保存路由状态；`compat_app` 不占用 `managed_app` 的单实例绑定。
- 默认 Codex App 不纳入受管理实例限制；受管理 App 仍保持全局最多一个。

### 实现边界与验收

- 前端在方案行和启动对话框显示两个入口；App 入口只在兼容性探针通过且方案使用 Gateway 时可用。
- 后端保持实例级生命周期与回执，允许同一方案的 CLI 与 App 同时处于活动状态；停止、失败和重试互不影响。
- 运行依赖可被多个活动实例共享只读引用；编辑、删除、换钥和重授权仍在所有引用实例结束前返回 `configuration_in_use`。
- 回归覆盖：CLI→App 连续启动成功、App→CLI 连续启动成功、两个 App 被拒绝、单独停止不影响另一实例，以及活动实例期间资源写操作继续被锁定。

### 2026-09-04 默认 App 兼容启动收口（已由 2026-09-07 决策 supersede）

- 历史方案曾通过安装检查和后端启动提交检测默认 App 进程；该检测因跨平台身份不稳定已移除，当前以 2026-09-07 的 best-effort 派发契约为准。
- `compat_app` 的独立兼容绑定仅用于默认 home 的 Gateway 绑定和重启恢复，不进入运行实例列表；失败派发会释放绑定并在校验通过时恢复原始配置。
- 兼容模型目录从原生/ bundled 目录合并第三方路由，避免重启默认 App 后原始模型列表被 route-only 目录替换。

> 状态：V3.1 官方账户信息与交互已实现并完成自动化验收；真实双账户交互及发布签名/DMG 封装待执行  
> 更新：2026-08-12  
> 设计基线：[Codex 多账户、API Key 与启动隔离方案](./codex-official-auth-authorization-design.md)
>
> 后续范围：本文只验收当前 V3.1，不包含“官方 GPT + 多个第三方模型同实例”的混合路由。该目标及安全边界见[产品原理：显式启动、多账户与混合模型路由](3rcodex-product-principles.md)，需另建详细设计与执行计划后实施。

## 交付目标

实现多个 API Key Provider 和多个官方 Codex 账户的管理、逻辑启动预设、受管理 CLI 启动、配置依赖锁与实例核验。只有从 R Codex 点击启动的新 CLI 使用所选配置；直接执行 `codex` 或打开 Codex App 始终保持原始默认环境。

## 不可妥协验收条件

- 公开 IPC 只有受管理 CLI 启动和无凭据参数的 App 打开入口，不提供 Profile 导出或全局应用配置。
- 实例在 `launching/running/stopping/orphaned/degraded` 时，其预设、凭据、Provider、Profile 与 Gateway 依赖均不能修改、重授权、断开或删除。
- 启动与修改竞态由 SQLite `BEGIN IMMEDIATE`、规范化依赖和资源操作租约裁决；Keychain、认证目录与数据库失败不能留下可启动的半状态。
- CLI 真实 PID、进程启动身份和启动/退出回执可核验；未知状态保持锁定，无轮询心跳。
- 官方认证只走目标 Codex 的 app-server 认证方法；R Codex 不读取 `auth.json`，不接受 token 注入，不请求私有 usage 接口。
- 数据库、日志、命令行、配置快照和持久前端状态无 API Key、token、Cookie、设备码或授权 URL。浏览器授权 URL 由后端消费，不进入前端状态；设备码前端只在当前授权提示的内存状态中保留一次性代码，完成、取消、失败或页面重载后立即清除。授权 URL 与设备码均不得进入缓存、浏览器存储或审计。

## 工作分解与所有权

| 阶段 | 交付物 | 文件所有权 | 状态 |
| --- | --- | --- | --- |
| D1 | V3 架构、迁移、协议、失败与验收契约 | `docs/` | 已完成 |
| B1 | schema v5 迁移、修订、依赖、锁与操作租约 | `src-tauri/src/database.rs`, `resource_guard.rs` | 已完成 |
| B2 | app-server 认证适配器、版本门与离线协议 fixture | `official_auth.rs`, `official_accounts.rs` | 已完成 |
| B3 | 统一启动服务、启动器回执、按需核验与 IPC 集成 | `lib.rs`, `codex_control.rs`, `codex-launcher.rs` | 已完成 |
| F1 | 账户、预设、实例、资源锁 UI；移除 Profile 逃逸入口 | `src/` | 已完成 |
| F2 | 官方账户差异化读模型、连接方式选择、详情与准确文案 | `codex_control.rs`, `official_accounts.rs`, `lib.rs`, `src/` | 已完成 |
| V1 | Rust/迁移/并发/启动器/前端/浏览器验收 | 测试与验收证据 | 自动化完成；真实登录待人工验收 |

编辑约束：小弟只修改分配到的文件；共享入口 `lib.rs`、依赖清单和最终集成由主代理处理。任何新增命令先提交结构化输入、输出和错误契约，再接 UI。

## 后端命令契约

- `get_codex_control_snapshot()`：一次返回账户、Provider、预设、实例、操作状态和每个资源的 `locked_by_instances`。
- 官方账户投影额外返回当前凭据修订的 `auth_checked_at`、`auth_error_code`、`codex_version`、`schema_fingerprint`；不得返回 email 原文、认证目录或任何 token。`workspace_label` 的产品语义固定为本地账户备注。
- `create/update/delete_launch_preset`：使用 `expected_version` 做乐观并发控制；删除采用逻辑删除，保留被历史配置与实例审计引用的预设。
- `launch_codex({ launch_preset_id, expected_version, workspace_path?, launch_target? })`：默认启动受管理 CLI；在兼容性门开启且使用 Gateway 的多 Provider 预设时，可选择受管理 App；请求中无 secret。
- `stop_codex_instance({ instance_id })`、`reconcile_codex_instance({ instance_id })`：显式停止和按需核验。
- `open_codex_app({ workspace_path? })`：不接受凭据或预设 ID，不改变 preferred 预设。
- `start/cancel_codex_account_authorization`、`refresh_codex_account_status`、`disconnect/delete_codex_account`：均走账户服务、锁与租约。
- Provider 创建/更新同时提交 Gateway 选项，禁止 UI 两阶段保存。

官方账户与资源锁错误使用 `{ code, message, conflicting_instances }`；既有 Provider/启动命令在迁移期保留稳定的 `code[:instance_ids]` 字符串，前端通过同一适配层转成结构化错误，不使用正则或展示原始内部错误。稳定错误至少包含 `configuration_in_use`、`resource_operation_in_progress`、`version_conflict`、`official_credential_in_use`、`protocol_incompatible`、`credential_missing`、`launch_unconfirmed`。

## 验证矩阵

1. 迁移：全新库、v1-v4 fixture、失败回滚、旧 `secret_ref` 保留且零 Keychain 读取。
2. 锁与竞态：启动对修改、删除、重新授权、Gateway 重绑；只有一方成功且无外部半写。
3. 监督：启动成功、Codex 不存在、终端成功但 CLI 失败、回执缺失/篡改、正常退出、停止、应用重启、PID 复用。
4. 隔离：两个 API Key、两个官方账户、直接 CLI、直接 App；比较 `CODEX_HOME`、依赖修订和默认目录哈希。
5. 官方协议：初始化/home 核验、登录完成/取消/失败/通知乱序、账户读取、可选限额读取、附加兼容字段、缺失必需字段、禁止 token 注入、stderr 分流和运行期官方原始异常。
6. 前端：资源级锁、操作 pending、结构化错误、官方单实例门、App 独立入口、无 Profile 导出、桌面/移动布局和敏感信息扫描。
7. 账户差异：浏览器与设备码授权入口、本地备注、官方账户详情、诊断字段空值、授权提示瞬时清理；API Key Provider 的余额和本地用量不得出现在官方账户详情。

## 恢复与完成记录

- 每个阶段完成后更新上表状态，并把命令、测试数、fixture 与人工步骤记录在本节。
- 若真实官方登录需要用户浏览器交互，自动化以脱敏 app-server fixture 完成；真实登录只作为发布前人工验收，不得用测试 token 代替。
- 完成定义：所有自动化验收通过，真实交互限制明确，主设计、执行计划和实际命令/UI 保持一致。

### 2026-08-11 实施结果

- 数据库升级为 schema v5，包含不可变 Provider/官方凭据修订、启动预设、启动配置依赖、运行锁、操作租约、实例回执和配额预留表；v4 回填不读取 Keychain。
- 官方认证只通过稳定 app-server 认证子集；使用独立 `CODEX_HOME`，清除继承认证环境，拒绝 external-token 方法，授权 URL 与设备码只存在于瞬时返回值。
- `launch_codex` 创建受管理 CLI，或在显式兼容性门开启时创建受管理 App；`open_codex_app` 无凭据参数，并清除 Codex/认证环境后使用 App 自身默认状态。公开 Profile 生成与直接 Profile IPC 已移除。
- Provider、账户、预设和 Gateway 依赖按活动实例精确锁定；运行中禁止修改、重新授权、断开或删除。API Key 可并行启动；同一官方凭据修订首版限制一个活动 CLI。
- Provider 更新提交后回收已退役修订的 Keychain secret，删除时回收全部修订 secret；清理失败保留恢复租约，重启后继续补偿，同时保留不含密钥的历史修订审计。
- 前端控制面一次展示账户、Provider、预设、实例及资源锁；操作 pending 与运行锁分离。开发 fixture 仅在 Vite 开发模式且显式 `?ui-fixture=1` 时加载，生产构建不可启用。
- 可选 `account/rateLimits/read` 已有类型化适配器和协议测试；V3.1 在授权完成和手动刷新账户时附带读取并持久化短期快照，失败不影响授权。手动刷新账户状态会向官方 `account/read` 传 `refreshToken=true`，先让官方 Codex 刷新本地 OAuth 凭据，再更新账户投影；缺失或失败时明确显示当前未获取，不阻止核心认证与启动。后台定时刷新和阈值通知仍留作后续增强。

### 2026-08-12 V3.1 实施结果

- Provider 产品语义收紧为 API Key 接口服务；官方 Codex 账户继续作为独立资源，仅在启动预设中与 Provider 统一选择。
- 官方账户控制面快照新增最近认证检查、稳定错误码、Codex 版本、协议指纹、可选官方限额和精确凭据修订；不返回认证目录、email 原文或 token。
- 连接/重新授权对话框支持浏览器与设备码两种官方方式，并允许填写本地账户备注。浏览器授权 URL 由后端消费；设备码只在当前内存提示中展示，取消后后端不再提供提示地址。
- 官方账户详情区分本地备注、官方套餐、认证诊断和官方限额；移除“个人工作区”推断，也不混入 API Key Provider 的余额与本地 Token 统计。
- 运行实例补充凭据修订显示，便于核验不可变启动绑定。

### V3.1 验收证据

- `cargo fmt --all -- --check` 与 `cargo check --all-targets`：通过；仅保留已有预留接口的未使用警告。
- `cargo test --all-targets`：118 项通过、0 失败（库 98、启动器 5、官方协议集成 15）。沙箱内回环端口测试会被操作系统拒绝；允许本机回环绑定后完整套件全绿。
- `pnpm run check` 与 `pnpm run build`：通过，生产构建包含 1804 个模块。
- 应用内浏览器使用开发夹具完成“账户与密钥 → 连接官方账户 → 本地备注/设备码 → 取消 → 官方账户详情”交互；桌面与 390×844 窄屏均无横向滚动、控制重叠、框架错误覆盖或应用控制台错误。

### 验收证据

- `cargo fmt --all -- --check`：通过。
- `cargo check --all-targets`：通过；仅有预留配额接口与通用锁辅助函数的未使用警告。
- `cargo test --all-targets`：116 项通过、0 失败（库与服务 96、启动器 5、官方协议集成 15），覆盖迁移回滚、资源竞态、凭据补偿、启动回执、进程身份、协议乱序/超时/取消、路径所有权和环境隔离。
- `CODEX_REAL_APP_SERVER=/Applications/ChatGPT.app/Contents/Resources/codex cargo test ...installed_app_server_contract...`：本机 `codex-cli 0.147.0-alpha.6.5` 离线契约探测通过；只使用新临时目录完成 schema、initialize 和空账户读取，未触发登录或读取现有认证文件。
- `pnpm run check` 与 `pnpm run build`：通过，生产前端成功生成。
- `pnpm run tauri build --bundles app`：通过，生成 arm64 应用包 `src-tauri/target/release/bundle/macos/R Codex.app`；主程序和 `codex-launcher` sidecar 均为 arm64 Mach-O 可执行文件。
- `pnpm run tauri:build`：应用包阶段通过，Tauri `create-dmg` 美化脚本在当前机器失败，未生成最终 DMG。正式分发仍需配置 Apple Developer 签名/公证并解决 DMG 封装；这不影响本地 `.app` 功能验收，但不能宣称安装包发布完成。
- 浏览器 UI：1440/1280 桌面和 390x844 窄屏检查通过；无横向滚动、按钮文本溢出、控制重叠或控制台错误。账户授权、资源级锁、官方单实例门、CLI 启动确认和独立 App 对话框均已走查。

### 2026-08-26 一次性凭据交接修订

- 现场日志确认一次性凭据读取失败的根因是 FIFO（命名管道）生命周期竞态，而不是 Keychain 权限或凭据引用错误：主应用已完成 `instance_key_parent_loaded`，但 Terminal 中的启动器尚未打开管道时，macOS 返回 `ENXIO`，旧实现将其误判为不可恢复错误，立即执行了 `secret_handoff_timeout` 并删除管道，启动器随后得到 `ENOENT`。
- 交接协议的所有权固定为：主应用创建 FIFO 并只负责写入；启动器成功读取后负责删除；等待超时只由主应用清理。成功写入后不得立即 unlink，否则延迟启动器会失去交接入口。FIFO 不调用 `sync_all`，写入完成并关闭即可完成一次性传递。
- macOS 在 FIFO 尚未有读端时可能返回 `ENXIO`（而不是 Rust 的 `WouldBlock`）；父进程将该结果视为“继续等待读端”，不会误判为不可恢复错误。
- 启动器延迟应覆盖 Terminal 的启动耗时（当前等待窗口 20 秒）；失败仍写入脱敏诊断日志并清理管道。回归测试覆盖“启动器延迟读取仍能拿到凭据”和“读取后管道被删除”。
- 本次修订验证：交接回归测试通过；`cargo fmt --all -- --check`、`cargo check --all-targets`、前端 `pnpm run check`/`pnpm run build` 通过；完整 Rust 套件在沙箱内 131 项通过，9 项既有回环网络测试因 `Operation not permitted` 未能绑定本机端口。以打包 `codex-launcher` 做 FIFO 冒烟时，启动器成功读取并删除管道；其后续 `/usr/bin/true` 进程过快退出导致的身份核验失败与交接无关。

### 功能发布前外部门槛

自动化不能替代真实 OpenAI 账户与浏览器回调。发布官方账户功能前，必须用两个受控测试账户完成浏览器登录和设备码登录、分别启动 CLI、验证两个目录不串号、让一个账户失效并确认官方异常原样呈现、退出一个账户并确认另一个账户与默认 `~/.codex` 不受影响。未完成该组测试时，只能判定实现机械与隔离契约通过，不能宣称真实官方账户生产验收完成。
