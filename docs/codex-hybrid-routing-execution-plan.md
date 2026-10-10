# Codex 混合模型路由执行计划

> 状态：2026-09-22 独立工作区改造已实施，自动化与浏览器夹具验证通过；真实 Codex 桌面/API 登录、会话导入恢复及 Windows 合同仍为发布前实机验收。历史默认 App 兼容入口已移除。

### 2026-10-10：Gateway 随应用启动，退出时停实例

- 变更目的：所有隔离空间继续共用一个 Gateway。应用打开就在 `127.0.0.1:43123` 等待绑定，不再等第一次启动实例。标题栏不再显示空闲或转发。5 秒没有启动回执时保留原绑定。真正退出先停掉活动隔离实例，再停止没有绑定的 Gateway。
- 合同：`setup` 调用 `ensure_serving()`，失败只记录，窗口仍打开。`launch_unconfirmed` 不拆绑定，前端刷新后沿用现有 2 秒实例核对。关窗口只隐藏。菜单栏 `exit(0)` 走停止路径。只有 `RESTART_EXIT_CODE` 保留实例和 sidecar。`unavailable` 时标题栏保留「重启」。 窗口改为系统标题栏；失联时的「重启」在页面「R Codex」右侧。
- 修改文件：`src-tauri/src/lib.rs`、`src-tauri/src/commands/instances.rs`、`src/components/title-bar/`、`src/App.tsx`、`src/use-app-controller.ts`、`src/app-model.ts`、产品原理、混合路由设计和隔离启动核对计划。
- 回滚：恢复标题栏状态、退出时有实例就阻止退出，以及 `launch_unconfirmed` 时拆绑定。Gateway 改回第一次启动实例时才拉起。
- 验证：`cargo fmt --all`、`cargo check --lib`、`pnpm exec tsc --noEmit` 通过。`cargo test --lib tests::only_the_restart_exit_code_preserves_instances -- --exact` 1 passed。未启动 Tauri，未对真实 iTerm 授权或菜单栏退出做实机验收。

### 2026-10-09：不可用 Gateway 提供标题栏手动清理

- 变更目的：控制面丢失 `gateway-sidecar.json` 或旧 CLI 退出异常后，用户可以从标题栏清理仍占用 `127.0.0.1:43123` 的旧 R Codex Gateway，恢复新隔离实例启动能力。
- 合同：新增 `cleanup_unavailable_gateway` IPC。仅在 `unavailable` 状态且无活动隔离实例时执行；按监听端口枚举 PID 并确认可执行文件名为 `codex-gateway` 后才停止，未知进程占用端口时 fail-closed。成功后删除残留 ready 文件并返回 `stopped`。标题栏只在不可用状态显示清理按钮，有活动实例时禁用。
- 代理边界：控制面请求固定回环地址时显式绕过系统代理，以免代理返回非 JSON 内容并导致误报不可用。
- 修改文件：`src-tauri/src/managed_gateway/sidecar.rs` 增加 fail-closed 清理、进程识别及控制面回环代理绕过；`src-tauri/src/codex_control/lifecycle.rs` 和 `src-tauri/src/commands/gateway.rs` 增加活动实例保护与 IPC；`src-tauri/src/lib.rs` 注册命令；`src/codex-control.ts`、`src/use-app-controller.ts`、`src/App.tsx`、`src/components/title-bar/` 接入按钮、禁用状态与通知；孤儿 Gateway 设计说明同步为标题栏优先的恢复路径。
- 验证：`cargo fmt --all -- --check`、`cargo check --all-targets`、`pnpm run check`、`pnpm run build` 均通过。`cargo test --lib managed_gateway::sidecar::tests -- --nocapture` 7 passed。初次全量 Rust 测试受本机代理拦截回环 mock 影响，291 passed / 20 failed；以 `NO_PROXY/no_proxy=127.0.0.1,localhost,::1` 且清空代理变量重跑 `cargo test --all-targets` 后，311 个库测试、13 个 launcher 测试和 16 个认证集成测试全部通过。
- 未完成/阻塞：没有启动 Tauri 开发窗口或执行真实清理，以免未经用户点击终止当前占用 `43123` 的 Gateway。commander 在加载本次代码的 R Codex 后，停掉全部隔离实例，失联时标题栏右侧是「重启」，不再显示「Gateway 不可用」文案，活动实例也不再禁用这个按钮。点击后应能重新启动隔离 CLI/App。
- 回滚：移除清理 IPC、标题栏按钮和对应错误映射；保留原有状态判定，不自动杀进程。
- 下一步命令：`pnpm run tauri:dev`（由 commander 执行）后按上一条完成实机恢复验收。

### 2026-10-09：Gateway 控制面丢失 ready 文件时不再误报已停止

- 变更目的：控制面重启或 start_sidecar 抢端口失败后，标题栏把仍在转发的 sidecar 显示成「Gateway 已停止」，并抛出 managed_gateway_sidecar_start_timeout。隔离 CLI 此时仍走 127.0.0.1:43123。
- 根因：start_sidecar 先删除 gateway-sidecar.json 再 spawn；旧 sidecar 仍占用数据口，新进程 bind 失败，5 秒超时只杀掉新 child。status() 在没有 control endpoint 时直接返回 stopped，不探数据口、不回收 ready 文件。
- 修改：ManagedGatewayClient 在 spawn 前先 serving_status / 收养 ready 文件；数据口占用且无法收养控制面时返回 managed_gateway_port_in_use，不再抢 43123。status() 无 endpoint 时若数据口仍开则返回 unavailable。前端补齐超时与端口占用文案。
- 验证：cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check 通过。cargo test --manifest-path src-tauri/Cargo.toml --lib managed_gateway::sidecar 6 passed。其中数据口占用/关闭与 port_in_use 三条实际执行；收养 ready 文件和旧 sidecar 升级两条因沙箱拒绝 bind 127.0.0.1 跳过；setsid 测例因 /bin/ps 被拒跳过。pnpm run check 通过。未启动 Tauri，未打包。
- 未完成项：丢失 control token 后无法把状态恢复成 serving，只能显示不可用。关 CLI 后若数据口仍被占用，按 [孤儿 Gateway](designs/2026-10-09-orphan-gateway-sidecar.md) 在默认环境手动结束 sidecar，再重启 R Codex 启动新 CLI。
- 回滚：恢复 status() 在无 endpoint 时返回 stopped，以及 start_sidecar 无条件删除 ready 文件并 spawn。

### 2026-10-08：隔离实例上下文压缩只作记录

- 变更目的：记下隔离实例的长对话压缩是 Codex 本地检查点。压缩后续跑拿起旧任务时，R Codex 不改写请求，也不做兜底。说明可供以后官网引用，当前不发布页面。
- 修改：新增 `docs/designs/2026-10-08-isolated-context-checkpoint.md`。混合路由设计、官网计划和 AGENTS.md 各加一条指针。没有改 Gateway 或启动代码。
- 证据：会话 `01a10ad4-55df-7f61-985e-406c856a447c` 的请求 `7d01e13d-c798-4e15-8f2f-5db6ded15023` 含 `CONTEXT CHECKPOINT COMPACTION`，无 `compaction_trigger`；续跑 `2660b120-a668-4e41-8796-d479d83997be` 重放了旧的禁用批注。
- 未完成项：官网页面未加。
- 回滚：删除该说明和三处指针。不影响运行中的实例。


### 2026-10-07：渠道名称可在实例运行时修改

- 变更目的：模型渠道列表去掉「运行中，配置已锁定」。渠道名称可以在实例使用该渠道时修改。列表不再显示「本地首选」和「API Key 渠道」；改名在弹窗里取消或保存。
- 修改：rename_api_key_provider 只更新渠道显示名，不新建修订，也不受运行锁阻挡。正在运行的实例继续使用启动时写入模型目录的名称。删除、更换密钥和地址仍受运行锁限制。
- 验证：`cargo test --manifest-path src-tauri/Cargo.toml --lib rename_is_allowed` 1 passed；`pnpm run check` 通过。未启动 Tauri。
- 回滚：去掉改名命令和编辑按钮，恢复渠道锁定提示。已改的名称不会自动还原。

### 2026-10-07：隔离实例各自的 Electron 数据，允许多 App 同时运行

- 变更目的：隔离实例不再共用 Electron 桌面数据，多个受管理 App 可以同时运行。首页「新增实例」暂时隐藏。
- 修改：Electron 目录改为实例生命周期目录下的 electron-data。同一条实例未退出时仍不能再次启动。旧的共享 managed-app-data 由下一个启动的实例收养一次。
- 数据：只保留名为「实例 4」的启动方案及其运行目录；其余隔离实例的方案记录和专属目录清除。渠道和 API Key 保留。
- 验证：`cargo test --manifest-path src-tauri/Cargo.toml --lib electron_data` 3 passed；`pnpm run check` 通过。未启动 Tauri。
- 回滚：恢复全局单 App 检查和共享 managed-app-data。已删除的实例目录不会自动恢复。

### 2026-10-07：删除结束实例不再被其他 App 记录挡住

- 变更目的：结束的隔离 App 实例可以在另一个受管理 App 仍在运行时删除。会话、配置和 CODEX_HOME 按实例分开；当时仍共用 Electron 桌面数据目录 managed-app-data。同日后续记录改为按实例分开。
- 修改：delete_instance_storage 不再因还存在其他 managed_app 记录返回 managed_app_data_shared。它仍删除该实例的生命周期目录、专属 home 和旧 preset sqlite；只有这是最后一条受管理 App 记录时才删除共享 Electron 根。
- 验证：`cargo test --manifest-path src-tauri/Cargo.toml --lib electron_data` 2 passed；`pnpm run check` 通过。未启动 Tauri。
- 回滚：恢复删除前的整单拒绝。已删除的实例目录不会自动恢复。

### 2026-10-05：Gateway 空闲/转发状态语义

- 变更目的：产品侧不再把“Gateway 进程可用”误显示成“正在对话”。无在途模型请求时显示「Gateway 空闲中」；至少一个已准入请求仍在处理时显示「Gateway 转发中」。
- 修改：`src-tauri/src/managed_gateway/runtime_types.rs`、`managed_gateway.rs`、`managed_gateway/sidecar.rs` 增加在途请求计数和响应流生命周期守卫；`src-tauri/src/lib.rs` 透传 Tauri 状态；`src/app-model.ts`、`src/use-app-controller.ts`、`src/App.tsx` 和 `src/components/title-bar/`、`src/components/isolated-app/`、`src/components/runtime-panels/` 更新文案、1 秒状态轮询及空闲/转发视觉态；标题栏样式让空闲点静止、转发点脉冲。旧 sidecar 缺少新增字段时按 0 兼容。
- 验证：`cargo fmt --all -- --check`、`cargo check --all-targets`、`cargo test managed_gateway::tests -- --nocapture`（43 passed）、`pnpm run check`、`pnpm run build` 均通过。仅有既有构建体积和 connectivity 未使用代码警告；未启动 Tauri 开发服务，未访问真实 Provider。
- 未完成项：真实 Codex App/CLI 与真实流式上游下的视觉手动验收仍需 commander 在目标平台执行；旧 sidecar 在升级前无法报告非零在途数。
- 回滚：移除 `in_flight_requests` 状态字段、响应流 guard、前端轮询及活动态样式，恢复仅按 `state` 显示 Gateway 运行状态；不删除已有实例或路由数据。
- 下一步：在 macOS 与 Windows 发布候选环境分别验证并发流式请求开始/结束、客户端取消和 sidecar 重启后的状态恢复，不发送真实敏感凭据到日志或档案。

### 2026-10-04：多隔离实例

- 变更目的：首页「新建空间」改为「新增实例」并允许创建多条 managed_app 启动方案。每条方案有独立渠道/模型配置和首次启动后的 CODEX_HOME；同时仍只能运行一个受管理 App。
- 修改：schema v21 删除 launch_presets_entry_kind 唯一索引。create_managed_launch_preset 增加 createNew / launchPresetId；新增 select_managed_launch_preset。前端标签列出全部实例，新增时复制当前渠道和模型。
- 验证：`cargo fmt --all -- --check` 通过；`cargo test --lib` 299 passed；`pnpm run check` 通过。未启动 Tauri 开发窗口。
- 回滚：恢复 schema 20 与单方案 upsert，以及禁用「新建空间」。已创建的额外方案不会自动删除。
- 下一步：commander 实机新增实例、切换标签、启动受单 App 限制。

### 2026-10-04：终态实例回收 Gateway binding

- 变更目的：修复隔离 CLI/App 自然退出或崩溃后，SQLite 已进入 `exited/failed` 但 Gateway 仍保留活动 binding、sidecar 继续运行的问题。
- 修改：`src-tauri/src/lib.rs` 的 `reconcile_codex_instance` 改为异步后端收口；发现终态后统一执行注销 binding、撤销持久记录和无绑定 Gateway 停止。清理步骤即使前一步失败也会继续尝试停止；没有 endpoint、或 Gateway 仍被其他实例使用时按幂等/并发正常状态处理。前端现有轮询无需新增写入逻辑。
- 验证：`cargo fmt --all -- --check`、`cargo check --all-targets`、`cargo test --lib` 均通过；Rust 单元测试 298 passed, 0 failed。`pnpm run check` 通过。未启动 Tauri 开发窗口。
- 残余边界：控制面被强制终止时，独立 sidecar 可能暂时存活；下一次控制面启动的健康检查和数据库终态回收负责清理。Windows 进程存活探测与租约/心跳仍是发布前增强项。
- 回滚：恢复 `reconcile_codex_instance` 为同步只读核验，并撤回本节对应的 cleanup 调用；不删除任何数据库或实例目录。

### 2026-10-04：隔离配置打开 multi_agent_v2

- 决定：新建和已有隔离目录都把 `features.multi_agent_v2` 保持为布尔值 `true`。不写 `features.multi_agent_v2.non_code_mode_only = false`，也不保留这个嵌套表。Codex 0.160 上布尔开关仍挂着 `collaboration` 命名空间；嵌套表会把 `spawn_agent` 从请求里拿掉。
- 版本：从 Codex CLI `0.160.0` 起，隔离 App 要保住直接调用的协作工具，这项必须写成布尔值 `true`。`0.159.0-alpha.12.1` 的隔离会话 `01a0fcbe`（grok-4.7）配置里只有 `goals = true`，rollout 仍有 `function_call`、命名空间 `collaboration`、名字 `spawn_agent`。`0.160.0` 用同一隔离目录做过不出网上游的 app-server 探针：键缺省，或写成布尔 `true`，请求工具表都含 `collaboration`（`spawn_agent`、`send_message`、`followup_task`、`wait_agent`、`interrupt_agent`、`list_agents`）。写成 `[features.multi_agent_v2]` 且 `non_code_mode_only = false` 时，这个命名空间消失。键缺省在这次探针里也还在，但默认目录复制可能带入嵌套表，所以合同不是“可以不写”，而是从 `0.160.0` 起每次启动写成布尔 `true`。更早的小版本没有逐个复测。
- 修改：`materialize_managed_app_config`、首次偏好复制，以及显式同步 general 之后都执行同一条修正。当前隔离 `config.toml` 已写成 `goals = true` 和 `multi_agent_v2 = true`。
- 验证：`cargo test --manifest-path src-tauri/Cargo.toml --lib launch_replaces`、`relaunch_refreshes_owned`、`seed_copies_preferences`、`explicit_sync_only` 均通过。未启动 Tauri。
- 回滚：去掉 `ensure_collaboration_feature`，并删掉隔离配置里的 `multi_agent_v2 = true`。

### 2026-10-03：标题栏只留 Gateway 状态

- 标题栏内容只留真实 Gateway 状态点与文案（运行中、不可用、已停止）。品牌、版本、主题和刷新已从标题栏去掉。Windows 最小化、最大化和关闭仍在，因为系统标题栏是关的。macOS 三色按钮未动。
- Gateway 状态是应用级共享数据：`AppStateProvider` 在根上调用一次 `useAppController`，放进 React Context。标题栏通过 `useAppState` 的调用方拿到同一份 `gateway`，不是自己再请求一份。
- 验证：`pnpm run check` 通过。未启动 Tauri。

### 2026-10-03：红色关闭键收到菜单栏

- 目的：接上「设计模型渠道探针弹窗」里被小图 400 打断的关闭行为。点击红色关闭键只隐藏窗口；macOS 上程序坞图标消失，菜单栏留下 `src-tauri/icons/tray-icon.png`。2026-10-03 晚些时候这张图换成 Didot Bold 的艺术字 R，仍是 22×22 的黑色模板，没有再把小图发给模型。左键点它，或菜单里的「显示」，再打开窗口并恢复程序坞图标。菜单「退出」仍走原来的退出阻止。
- 不做：不再查看或重画这张 22×22 图标。没有这个文件时跳过菜单栏图标，关闭仍只隐藏窗口。
- 修改：`src-tauri/Cargo.toml` 打开 `tray-icon` 与 `image-png`；`src-tauri/src/lib.rs` 安装菜单栏图标并在关闭时切到 Accessory。
- 验证：`cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check` 通过；`cargo check --offline --manifest-path src-tauri/Cargo.toml --bins --lib` 通过。未启动 Tauri。

### 2026-10-03（收口）：Grok 大图可用，小图暂不修

- 决定：这轮不再改 Gateway，也不再改目录。非 GPT 继续删除 `supports_image_detail_original`。小图 400 先接受为现状。
- Codex + Grok 的大图是通的。3rxai 的 `grok-4.7` 上，64×64 的 `high` 和 `original` 都是 HTTP 200，模型回复颜色；512×512 的 `icon.png` 用 `original` 也是 200。请求形状是 Responses 的 `input_image`，`image_url` 为 data URL 字符串。
- 小图：同一张 22×22 菜单栏图标（484 像素，`detail: high`）在 3rxai 与 uuAI 的 `grok-4.6`、`grok-4.7` 上都是 HTTP 400，正文只有 `Upstream rejected the request`。两边纯文本都是 200，上游名分别是 `grok-4.6-build`、`grok-4.7-build`。3rxai 上小于 512 像素的合成图当时也是 400，刚好 512 是 200；这是对照观察，不是厂商合同。uuAI 没有做大小阶梯。
- 不是 Codex 请求格式整体不兼容。同一地址 `code.3rcd.com` 上的 `gpt-5.6-terra` 接受了这张 22×22 图并回复尺寸。`original` 不是这次 400 的原因。
- 未打到官方 `api.x.ai`。`docs.x.ai` 连不上。官方 Python SDK 只写 PNG/JPG、最大 10 MiB、`detail` 为 `auto`/`low`/`high`，没有最小像素数。GitHub 与 Hacker News 没搜到对得上的 Grok 社区报法。不能写成 Grok 产品通病。
- 旧线程里若已经存了这张小图，续聊仍会 400。不改历史。
- 回滚：无代码改动。

### 2026-10-03（锁定）：非 GPT 继续删除 original

- 决定：commander 锁定继续删除非 GPT 系列的 `supports_image_detail_original`。`original` 是 Codex 自己的图片档，不是 xAI 或其他三方模型族的档。某条渠道探针返回 200，不能当成该模型族支持这个档。
- 现状，不是根因：3rxai 的 Grok 4.6 / 4.7 上，小图目前会 HTTP 400，正文只有 `Upstream rejected the request`。同一请求形状的大图可以 200，`detail: original` 也包括在内。上游没有说明原因。512 像素只是这次对照里的分界，不是厂商合同。同一地址上的 `gpt-5.6-terra` 接受了 22×22 的图。
- 代码：行为不变，仍是非 GPT 删除字段，不是写成 false。注释改为上述决定。Gateway 不改写出站正文。
- 官网：以后在 3R Codex 网站说明这类为兼容 Codex 做的目录处理。本轮不写页面，只记在官网计划。
- 回滚：无行为回滚。不要因为 Grok 探针通过就把字段加回非 GPT 系列。

### 2026-10-03（更正）：会话 400 是 22×22 图，不是 original 档

- 变更目的：纠正同日「非 GPT 不再广告 original」里的原因判断。目录改动和测试仍然有效，但不是这条会话死掉的原因。
- 证据：失败线程在调用 `view_image` 查看 `src-tauri/icons/tray-icon.png`（22×22，484 像素）后的下一跳，Gateway 转到 `https://code.3rcd.com/v1/responses`，上游 HTTP 400，正文只有 `Upstream rejected the request`。同一文件用 `detail: high` 写入「回复问候」和随后的接管线程，续聊同样 400，助手没有文字。新开且不含这张图的线程可以 200。
- 同日直接探针（`grok-4.7`，不经旧会话历史）：纯文本 200；22×22 实文件、合成 RGB/RGBA，`high` 和 `original` 都是 400。16×16、22×23（506 像素）、18×28（504 像素）为 400。16×32（512 像素）、23×23、24×24、64×64（含 `detail: original`）和现有 512×512 `icon.png` 的 `high` 为 200。当前这条渠道拒绝的是总像素数小于 512 的图。
- 未改代码。旧线程里这张图还在，续聊会再 400。不要用 Gateway 改写历史来绕过。看这张菜单栏图标前先放大到至少 512 像素。
- 回滚：无代码改动。

### 2026-10-03：非 GPT 系列不再广告 original

- 变更目的：Grok 会话在 `view_image` 使用 `detail: "original"` 后，3rxai 对 `/v1/responses` 返回 HTTP 400。`high` 已在同一会话通过。`original` 是 GPT 目录骨架的图片档，不是推理档 `xhigh`。
- 合同：生成目录时只按上游模型 ID 的系列名处理 `supports_image_detail_original`。`gpt-` 或含 `openai` 的系列保留；Grok、DeepSeek、Claude 和未知族删除，看图打开也删除。不按 `RouteSource` 区分官方账户和 API 渠道。Gateway 不改写出站正文，旧会话里已经保存的 original 不修补。用户自备的三方 GPT 中转若拒绝 original，不在 3R 范围。
- 实际修改：`src-tauri/src/model_family_profile.rs`。推理档文档和 Grok followups 已同步。Gateway 未改。
- 实际验证：`cargo test --manifest-path src-tauri/Cargo.toml --lib model_family_profile::tests::` 16 passed；`cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check` 通过。未启动 Tauri。
- 未完成项：已启动的 Codex 要重新应用配置并重启后，新目录才生效。旧会话不会被改写。
- 回滚：恢复「supports_images=true 时非 GPT 系列也保留 supports_image_detail_original」。

### 2026-10-02：macOS 钥匙串与实例密钥

- 变更目的：开发态 ad-hoc 签名导致多渠道、多进程反复申请钥匙串。渠道 API Key 合并为一条 `3r-secrets`；实例准入密钥不是用户凭据，不再写入钥匙串。
- 合同：见 [macOS 钥匙串授权与存储](designs/2026-10-01-macos-keychain-access.md)。明文实例密钥只走内存和一次性 FIFO；SQLite 只保存哈希；sidecar 恢复只比对哈希；launcher 管道缺失时直接失败，不回退钥匙串。
- 实际修改：`src-tauri/src/secrets.rs`、`macos_keychain.rs`、`managed_gateway.rs`、`bin/codex-launcher.rs`、`lib.rs`、`managed_launch.rs`；产品原理、混合路由设计、I1j 与 AGENTS 已同步现行合同。
- 实际验证：`cargo test --offline --lib secrets::tests` 6 passed；`cargo test --offline --bin codex-launcher` 13 passed；`cargo check --offline --all-targets` 无 warning。未启动 `pnpm run tauri:dev`。
- 未完成项：Windows Credential Manager 不在本次范围；生产包仍需同一稳定签名身份覆盖主程序与 sidecar。
- 回滚：恢复按 `secret_ref` 分散钥匙串条目，并把实例密钥重新写入钥匙串；这会重新导致多渠道多次授权。

### 2026-10-05：数据库凭据与实例密钥 Keychain 移除

- 变更目的：按产品决定停止使用 Keychain，Provider API Key 改存 R Codex 自有 SQLite，实例准入密钥只保存哈希并通过一次性 FIFO 交接，消除启动授权弹窗与 `-25293` 旁路。
- 实际修改：`database.rs` 新增 schema v22 `provider_credentials`；`secrets.rs` 使用 `DatabaseApiKeyVault`；移除 `macos_keychain.rs`、`keyring` 依赖、Provider/sidecar/launcher 的 Keychain 参数和 fallback；`managed_launch.rs` 不再持有实例密钥 vault。
- 凭据行为：旧 Keychain 内容不迁移；升级后已有 Provider 标记为 `credential_missing`，用户需重新录入。实例准入密钥明文不进入 SQLite、日志或 IPC。
- 验证：`cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`、`cargo check --manifest-path src-tauri/Cargo.toml --all-targets`、`cargo test --manifest-path src-tauri/Cargo.toml --all-targets` 通过（297 个库测试、1 个 Gateway 测试、13 个 launcher 测试、16 个官方认证集成测试）；`pnpm run check` 与 `pnpm run build` 通过。运行源码静态扫描未发现 Keychain API、`keyring` 依赖或 fallback；R Codex 专属旧条目已定向删除。
- 未完成项：未启动 Tauri 开发窗口、未打包或发送真实 Provider 请求；commander 需重新录入 API Key，并在 macOS/Windows 目标环境手动验收启动、转发、停止和重启恢复。Unix 数据库文件权限为 `0600`，Windows ACL 仍需发布环境验证。
- 回滚：代码回滚可能恢复旧实现，但旧 Keychain 条目将按本次用户要求定向清理，不保证旧版本免录入；数据库 schema v22 保持可读。


### 2026-10-02（验收更新）：手动归档已通过；会话工具仍未装配

- 手动归档：已通过。隔离实例的短真实 `CODEX_HOME` 包含 `state_5.sqlite`、`sessions/` 和 `archived_sessions/`；线程 `01a0f501-9636-7c50-9e1b-ef251b70678a` 已归档，JSONL 位于 `archived_sessions/`，SQLite 中 `archived=1`，路径使用短 home 前缀。
- 结论：分离 SQLite、短路径 symlink 与长 `rollout_path` 的归档问题均已修复。不要再把手动归档失败归因于 JSONL 或线程库布局。
- 剩余问题：模型通过本会话归档仍不可用。当前会话没有桌面 `set_thread_archived`、`codex_app` 或 `tool_search`；从 Grok 切换到 GPT 后工具集不变。模型无法调用桌面归档，而不是归档操作本身失败。
- 后续范围：单独排查隔离 app-server 的 `codex-app-tools` MCP 实际启动与动态 namespace 装配，并验证 deferred 的 `set_thread_archived` 可见。不得在 Gateway 伪造工具、直接移动 JSONL，或直接改 `threads.archived` 作为替代。
- 纠正：曾观察到 app-server 直连 `thread/archive` 在 writer lock 存在时返回 `already has an active writer`；这不代表桌面侧栏/Recents 手动归档一定失败，实机成功已推翻该泛化。

### 2026-10-02（续 2）：短路径改为真目录（代码已实施，手动归档已验收）

- 变更目的：拆库收回后隔离 App 归档仍失败。Electron `ThreadArchiveService` 用 `path.relative` 判断 rollout 是否在 `CODEX_HOME/sessions` 下，不跟随 `.../r/<short-id>` 符号链接；sqlite 里是长真实路径。无 writer lock 的会话也会失败。
- 合同：启动用的短 `CODEX_HOME` 改为真目录（文件住在这里），`CODEX_SQLITE_HOME` 仍等于它；旧长目录迁过去并把 `rollout_path` 改成短前缀。长路径只可留作指向短目录的链接。不代 Codex 释放 writer lock。隔离 App 运行中不迁树。
- 方案：[隔离 App 会话归档修复](designs/2026-10-02-isolated-app-thread-archive.md) Step 5。
- 审阅结论：根因判断成立，但实施前必须明确短真实目录为唯一事实源，补齐 `managed_codex_home` 的所有权/回执/重启复用、长兼容 symlink 的删除路径、整树含 WAL/SHM/journal 迁移顺序、SQLite 路径字段的边界改写和崩溃恢复；详见专项设计 Step 5「2026-10-02 审阅结论」。
- 已改：`src-tauri/src/managed_launch.rs`。macOS managed App 的 `managed_codex_home`、启动环境和导入读路径统一为短真实目录；新实例直接建短目录与长兼容 symlink，旧实例通过 `home-layout.json` 的 `legacy → renaming → linking → rewriting_paths → updating_db` 记录恢复。结束 binding 不删短 home；删除实例按长链接、短目录、lifecycle 的顺序清理。Windows 与 CLI 保留原布局。
- 锁定：`managed_codex_home` 存短真实目录；长 `codex-home` 只作兼容 symlink；Windows 不迁。迁移顺序：rename 长树 → 旧处建链接 → 每个 `state_*.sqlite` 按边界改 `rollout_path` → 控制库改短路径。
- 实际验证：`cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`、`cargo test --manifest-path src-tauri/Cargo.toml --lib managed_launch::`（34 passed）、`cargo test --manifest-path src-tauri/Cargo.toml --all-targets`（287 lib + 1 Gateway + 13 Launcher passed）、`pnpm run check` 均通过。未启动、停止或重启 Tauri 开发服务，未触碰 `~/.codex`。
- 残余验证：Windows 分支保持原布局；本机只安装 macOS Rust target，未做 Windows 交叉编译或安装工具链，仍需 Windows 发布环境验收。
- 实机结果：短真实目录迁移后，手动归档已成功；详见本计划文首「验收更新」。下一步改为排查本地对话的 `codex_app` 动态工具装配。不要把 Windows/CLI 套进短目录迁移。

### 2026-10-02（续，历史诊断）：迁库已生效时曾观察到 writer lock

- 变更目的：记录真实归档失败不是目录合同未落地。
- 证据：实例 home 根已有 `state_5.sqlite`；旧拆开 sqlite 目录已空。`Failed to archive conversation` 来自 Codex `localTaskRow.archiveError`。`lsof` 显示 app-server `codex` 同时持有全部三条 `thread-writer-locks/*.lock`。
- 合同：3R 不代 Codex 释放 writer。该观察只适用于 app-server 直连路径；后续手动归档成功已经证明它不能推导出侧栏/Recents 必然失败。
- 未改代码。

### 2026-10-02：隔离 App 会话归档修复（Step 2 已实施，Step 3 待实施）

- 变更目的：让隔离 Codex App 的侧栏归档可用。Codex Desktop 的 Electron 归档打开 `CODEX_HOME/state_5.sqlite`，当前活动库在 `managed-app-sqlite/<preset>`，会话 JSONL 在实例 home，归档失败。
- 合同：受管理 App 的 `CODEX_SQLITE_HOME` 等于启动 `CODEX_HOME`。启动前把旧拆开目录一次性迁入实例 home；两边都有库则 fail-closed。删除实例时对话库随 home 走，残留拆开目录 best-effort 清理。不改 ChatGPT.app，不把已归档会话纳入导入。当前仍打开的会话可能被 Codex 报 active writer，先切走再归档。
- 方案：[隔离 App 会话归档修复](designs/2026-10-02-isolated-app-thread-archive.md)。
- 已改：`managed_launch.rs`（启动赋值、迁库、删除）、`managed_launch/models.rs` 注释；launcher/lib.rs/导入调用面与错误文案留给 Step 3。
- 实际验证：`cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`、`cargo check --manifest-path src-tauri/Cargo.toml --all-targets`、`cargo test --manifest-path src-tauri/Cargo.toml --all-targets`（282 lib + 1 gateway + 12 launcher + 16 official_auth passed）、`cargo test --manifest-path src-tauri/Cargo.toml --lib session_import::`（10 passed）、`pnpm run check` 均通过。生产路径回归确认旧库/WAL 迁入实例 home，`sqlite_home == launch_codex_home`，运行中不迁移。
- 未完成项：Step 3 的 launcher/lib.rs/导入调用面与错误文案；commander 需退出隔离 App 后重启 3R，再启动隔离 App，切离当前会话后归档一条空闲会话。真实 App 的签名图形会话授权仍按既有实机项执行。
- 回滚：未迁库前可恢复拆开 `managed-app-sqlite` 赋值。迁库后只能向前修。
- 接手：Step 2 已完成。后续会话按专项设计的 Step 3 接手提示继续；单测通过后退出隔离 App，再重启 3R 验收归档。
- 下一步命令：读取 `docs/designs/2026-10-02-isolated-app-thread-archive.md`，继续 launcher/lib.rs/导入调用面与错误文案。


### 2026-09-30：隔离 App 从桌面应用包安装 Computer Use helper

- 变更目的：补上 2026-09-23 合同留下的安装缺口。隔离 `CODEX_HOME/computer-use/` 原先只做 fail-closed 检查，没有把签名 helper 装进去，导致点击「启动」在物化 MCP 时返回 `managed_computer_use_client_missing`，App 从未真正拉起。
- 合同：只从已解析的桌面应用包（`ChatGPT.app` / `Codex.app`）复制签名 `Codex Computer Use.app` 到隔离 `computer-use/`。`node` / `node_repl` / `cua_repl` / 内嵌 CLI 仍引用应用包内只读路径。已有隔离 helper 不覆盖。禁止以 `~/.codex/computer-use` 为复制源或回退。复制后仍缺 helper 则 fail-closed。偏好种子带入的默认 `notify` 路径仍改写为隔离客户端。
- 实际修改：`src-tauri/src/managed_codex_config.rs` 在 MCP 物化前安装 helper，并用 `codex_installation` 解析桌面包；`src/codex-control.ts` 映射 helper 缺失/安装失败文案；独立工作区计划同步合同。
- 实际验证：`cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check` 通过；`cargo test --manifest-path src-tauri/Cargo.toml --lib managed_codex_config::tests` 19 passed（含从桌面包复制、不覆盖已有 helper、禁止默认目录源、拒绝链到默认目录、替换不完整残留、缺 helper fail-closed）；`cargo check --manifest-path src-tauri/Cargo.toml --all-targets`、`pnpm run check` 通过。未启动、停止或重启 `pnpm run tauri:dev`；未打包；未读取或写入 `~/.codex`。
- 未完成项：commander 需重启自己的 `pnpm run tauri:dev` 后点击隔离 App「启动」，确认实例目录出现 `computer-use/Codex Computer Use.app`、`notify` 不再指向 `~/.codex`，以及真实 App 窗口拉起。签名图形会话中的辅助功能/屏幕录制授权仍按 2026-09-23 实机项执行。
- 回滚：删除安装器调用，恢复“隔离 helper 已存在才写 MCP”的旧路径；不触碰用户默认 Codex 文件。这会重新暴露本次启动失败。

### 2026-09-28：Codex Desktop 内嵌 CLI 路径兼容

- 变更目的：修复本机已安装 Codex Desktop 却被误报“未找到 Codex CLI”，并让隔离 App 的主进程预检和受管理启动器使用同一套安装解析。
- 合同：优先尊重显式 `CODEX_BIN` 与 `PATH`；否则识别 macOS Codex/ChatGPT 应用包中的旧 `Contents/Resources/codex` 以及新版嵌套 `Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex`。受管理 App 必须从 CLI 向上定位最外层桌面应用包，不能把内嵌的 `CodexCLI.app` 当作桌面 App。不会读取默认配置、认证、Cookie 或会话。
- 实际修改：新增 `src-tauri/src/codex_installation.rs` 作为唯一安装解析；`lib.rs` 的安装状态、版本/目录预检与“打开默认 Codex”改用它；`codex-launcher.rs` 的 CLI 和桌面 App 解析也改用它。新版嵌套 CLI 向上解析到外层 `/Applications/ChatGPT.app`，再启动 `Contents/MacOS/ChatGPT`；不再把内层 `CodexCLI.app` 当作 Electron App。`managed_codex_config.rs` 的隔离 Computer Use 配置也优先写入新版 CLI 路径，只有新版路径不存在才回退旧布局，并验证写入的是所选的隔离运行时路径。
- 实际验证：本机发现 `com.openai.codex` 的应用包为 `/Applications/ChatGPT.app`（版本 26.924.22138），新版内嵌 CLI 与桌面可执行文件均存在。`cargo fmt --all -- --check`、`cargo test --lib codex_installation::tests`（2 passed）、`cargo test --bin codex-launcher`（12 passed，包含本机新版路径到桌面 App 的解析）、`cargo test --lib managed_codex_config::tests`（13 passed）、`cargo check --all-targets`、`pnpm run check` 均通过。额外运行 `cargo run --quiet --bin codex-launcher -- --target verification-only-invalid-target`，启动器已越过 CLI 解析并仅因刻意无效目标退出，证明本机新版内嵌路径可被实际启动器解析；未启动 Codex App。
- 未完成项：未启动 Tauri，也未执行真实 Provider 请求；commander 需重启自己运行的 `pnpm run tauri:dev`，确认状态不再显示“未找到 Codex CLI”，然后点击隔离 App 启动并停止后点击“重启实例”，核对新的 PID、Gateway binding 与启动回执。
- 回滚：移除共享 `codex_installation` 模块并恢复两个旧解析器；这会重新暴露新版 Codex Desktop 的误报与 App 启动失败，不涉及迁移、默认目录或凭据。

### 2026-09-24：工作区交互切片

- 变更目的：让主界面直接回答当前 Gateway、渠道、默认模型、API 登录和活动实例；渠道、实例、诊断改为同级页面，加入渠道需确认。
- 修改文件：`src/App.tsx`、`src/workspace-surface.tsx`、`src/surface.css`、`src/main.tsx`、`src/dev-control-fixture.ts`、`docs/designs/3rcodex-ui-quality-upgrade-plan.md`。未改 Tauri 命令或凭据边界。
- 实际验证：`pnpm run check`、`pnpm run build` 通过。本机 Vite `127.0.0.1:5191/?ui-fixture=1` 用无头 Chrome 检查：状态条、认证失败行的编辑密钥、直连未观测、今日 3 次、启动按钮 title 为「启动」且无文字、加入确认三按钮、诊断恢复按钮与三个页签、实例技术详情折叠、390px 无横向溢出。
- 未完成：设计 token、组件拆分和完整状态矩阵仍按质量方案后续做。真实 Tauri 窗口未在本次启动。
- 回滚：还原上述前端文件即可，没有数据迁移。
- 下一步：commander 在自己的 `pnpm run tauri:dev` 里看深色/浅色和真实渠道行。

### 2026-09-28：修复 UI 改造后的隔离 App 启动回归

- 变更目的：恢复隔离 App/CLI 的真实控制面，确保启动、停止、核验和实例诊断仍通过 Tauri IPC；修复昨天视觉稿接入时 `App` 固定返回静态 `DemoConsole`，导致“重启实例”只修改演示 PID、无法实际启动隔离 App。
- 实际修改：`src/App.tsx` 恢复真实 app shell、工作区/渠道/实例/诊断导航和启动/实例确认流程；`src/main.tsx` 移除未挂载 demo CSS；`docs/designs/3rcodex-ui-quality-upgrade-plan.md` 更新回归记录。未修改 Rust 启动合同、数据库或凭据边界。
- 实际验证：`pnpm run check`、`pnpm run build` 通过；Vite `http://127.0.0.1:5173/?ui-fixture=1` 夹具页面显示真实工作区、状态条、启动 Codex 按钮与实例计数，无框架错误；启动按钮触发夹具 `launch_codex` IPC 路径并返回成功提示。Playwright 未安装，使用 Codex in-app browser 做 DOM/交互验证；未启动 Tauri，未声称真实 macOS App 启动已通过。
- 未完成项：commander 仍需在自己的 `pnpm run tauri:dev` 中验证真实隔离 App 启动、退出后再次启动（即重启恢复）、Gateway binding 与真实 PID 回执。
- 回滚：恢复 `App.tsx` 的原真实 app shell 返回值或回退本次前端提交；不需要数据迁移，也不会触碰用户默认 Codex 文件。

### 2026-09-23：文档整理后的执行顺序（当前路线图）

这条记录是当前下一步的排序依据；更早的默认 App 快速发版、双入口和旧兼容模式条目保留为历史证据，不再作为现行产品合同。现行合同以 `AGENTS.md`、[独立工作区改造](designs/2026-09-21-isolated-only-product-plan.md)、本计划最新条目和对应专项设计为准。

#### A. 先完成自动化收口（不需要真实桌面）

- [x] 为隔离 Browser/Computer Use 物化增加专门回归：`node_repl`、`cua_repl` 的命令/环境、隔离 `CODEX_HOME`、隔离 `SKY_CUA_SERVICE_PATH`、隔离 `notify`，以及默认目录 MCP/路径不泄漏。
- [x] 为 schema v18 增加迁移、读写、非法档位和 Provider × 模型独立保存的回归；确认模型目录的优先级是“官方族合同 → 未知族 `low/medium/high/xhigh` 兜底”。
- [x] 修正 helper 缺失行为：真实 managed App 现在返回明确错误；测试启动夹具只跳过外部 helper 调用，路径物化本身用 fake helper 回归覆盖。
- [ ] 清理文档中的旧状态描述：`docs/codex-default-app-release-plan.md` 保持历史标记；官网计划中的“隔离 App 暂缓”改成“未完成真实发布验收”，避免把已实现的启动骨架写成未实现。

#### B. 再做 commander 的 macOS 实机验收

- [ ] 重启 `pnpm run tauri:dev`，确认隔离配置不读取默认认证、MCP、Cookie 或 `notify` 路径。
- [ ] 完成受管理 App 的独立 API 登录、模型目录/推理档选择、连续对话、重启恢复和 Gateway binding 生命周期。
- [ ] 在签名图形会话中重新授权 Browser/Computer Use，确认本地 `node_repl`/`cua_repl` 工具出现；Settings 仍显示官方组织/地区禁用时，记录为 entitlement 门控而不是 3R 配置失败。
- [ ] 验证默认 App 入口、权限选择、单 App 限制、Gateway PID/端口复用与停止；再验收活动项目导入、未变化跳过、冲突副本和结束实例清理。

#### C. 真实 Provider 与历史兼容矩阵

- [ ] 用受控测试渠道覆盖 Responses-only、Chat-only 和双协议 Provider：新会话、连续会话、切换模型、恢复、回滚、工具/图片/推理档和上游 4xx/429/5xx。
- [ ] 继续按字段级合同排查跨 Provider 旧历史：`reasoning.encrypted_content`、`previous_response_id`、工具调用关系、item ID/status/phase 和 Provider 私有状态；不得通过改源历史或自动换路修复。
- [ ] 继续收敛协议层 P0/P1：Responses/Chat 分派、显式工具拒绝、流中错误事件、共享 SSE framer、白名单响应头和能力预检。

#### D. 发布与后续产品线

- [ ] 完成 macOS 签名/公证、Keychain 权限、升级/回滚和干净目录验收；ad-hoc `.app` 不能作为发布证据。
- [ ] 单独补齐 Windows `codex-launcher`、`codex-gateway` sidecar、启动目录、ACL、签名和升级链路。
- [ ] Provider 预设作为后续独立切片：先为 GLM、Kimi 等补官方模型族合同和字段映射，再接入 Cockpit/CCSwitch 风格预设；不把中转 `/models` 当能力真相。
- [ ] 官方账户/订阅模型继续保持关闭，直到两个受控账户完成真实授权、隔离、失效和回收矩阵。

### 2026-09-23：隔离 Computer Use 与每模型推理档

- 变更目的：让隔离 App 使用自身 `computer-use/` 下的本地签名客户端，且让每个 Provider × 模型保存独立的已验证推理档与默认档。
- 实际修改：`managed_codex_config.rs` 只在隔离 helper 存在时写入启用的 `node_repl`/`cua_repl`，把 `CODEX_HOME`、`SKY_CUA_SERVICE_PATH` 和 `notify` 指向隔离目录，并删除直接/default Computer Use MCP；不复制默认 MCP。schema v18 为模型选择增加可空 JSON 档位和默认档；启动 RouteSet、目录 overlay、IPC 组合摘要和高级映射 UI 使用该选择。模型族决定可选数组，所有支持推理的族默认 `high`。
- 实际验证：`cargo test --lib managed_launch::tests -- --nocapture` 28 passed；`cargo fmt --all -- --check`、`pnpm run check`、`pnpm run build` 通过。
- 未完成项：需要 commander 重启自己的 `pnpm run tauri:dev` 做首次 macOS 辅助功能/屏幕录制授权，并以 Computer Use 打开 Safari；还需补本地 MCP 生成与 schema v18 的专门回归测试。
- 回滚：删除 v18 两个可空列的读取/写入和本地 MCP 物化调用；旧数据库行与隔离目录均不受破坏。

### 2026-09-23：模型族官方档位优先，未知族兜底

- 决定修正：官方模型族能力合同优先；仅未知族使用 `low`、`medium`、`high`、`xhigh` 兜底，默认 `high`。中转是否支持不作为目录判断条件。GPT 族恢复其官方档位合同。
- 实际修改：`model_family_profile.rs` 为 DeepSeek 增加 `high` 单档合同，Grok 依各版本档位；Claude 与未知族使用通用兜底。`App.tsx` 编辑器同样按模型族展示对应档位。更新第三方目录合同与回归测试。
- 实际验证：`cargo fmt --all -- --check`、`cargo test --lib model_family_profile::tests -- --nocapture`（15 passed）、`pnpm run check`、`pnpm run build` 均通过。

### 2026-09-23：隔离 Browser / Computer Use MCP 依赖修正

- 诊断：隔离配置的插件开关均为 `enabled = true`，但只写入了直接 `computer-use` 服务；Browser 插件 hook 依赖 `node_repl`，统一 Computer Use 插件依赖 `cua_repl`。同时偏好继承把默认 `notify` 的 `~/.codex/computer-use` 路径带进了隔离配置。Electron 的“Disabled by your organization or unavailable in your region”仍是官方能力门控/工作区路由不可用提示，不是插件 enabled 字段。
- 实际修改：隔离物化器生成带隔离 `CODEX_HOME`、`SKY_CUA_SERVICE_PATH` 和 `NODE_REPL_TRUSTED_SERVICES` 的 `node_repl` 与 `cua_repl`，删除直接/default Computer Use MCP，并把 `notify` 改为隔离 helper 路径；不复制官方远程 MCP。
- 实际验证：`cargo fmt --all`、`cargo check --all-targets`、`pnpm run check` 通过。
- 未完成项：commander 需重启自己的 `pnpm run tauri:dev` 后重新启动隔离 App；若 Settings 仍显示组织/地区禁用，但会话里出现本地 `node_repl`/`cua_repl` 工具，则剩余是服务端 entitlement 门控，不能靠默认目录继承修复。

### 2026-09-23：补齐新行为回归测试

- 纠正：前一轮实现只通过了编译、格式、既有启动测试和前端类型检查，没有覆盖本次新增的 Computer Use 物化与 schema v18 行为。
- 实际修改：`managed_codex_config.rs` 增加隔离 MCP 成功/缺 helper 失败测试；`database.rs` 增加 v17→v18 空列迁移测试；`managed_launch.rs` 增加每个 Provider × 模型推理档位的非法值、默认值和持久化测试。真实 managed App helper 缺失不再静默跳过。
- 实际验证：Computer Use 定向测试 2 passed；schema v18 迁移测试 1 passed；managed launch 定向测试 29 passed；Rust library 全量测试 267 passed / 0 failed；`cargo fmt --all -- --check`、`cargo check --all-targets`、`pnpm run check` 通过。

> 产品边界：当前版本只实现 3R 本地 Gateway 路由接入第三方模型；“直接连接”单一 Provider 并在 3R Codex 退出后继续使用的能力暂不实现，仅保留为后续设计项。

> Provider 边界：不读取或导入用户手动维护的 Codex 外部 Provider；3R 中重新配置的同一渠道视为独立的额外路由来源，不做自动去重或凭据合并。
> 日期：2026-08-25  
> 设计依据：[Codex 混合模型路由详细设计](codex-hybrid-routing-design.md)  
> 当前基线：SQLite schema v18 / 单一 3R 工作区 / 多渠道模型与独立 API 登录 / 单向项目会话导入 / 实例请求账本和完整请求档案。

### 2026-09-23：第三方 Responses 的 agent_message 兼容（方案 A 已实现）

- 变更目的：隔离 App 直发 Grok 成功，子任务与父会话回放 `agent_message` 后 502。根因是 Codex 私有 input 项，不是 RouteSet。对照 OpenCodex 3911 与 Cockpit Responses 透传后给出 A/B/C/D。
- 合同：[第三方 Responses：Codex agent_message 兼容](designs/2026-09-23-responses-agent-message-compat.md)。方案 A：出站把 `agent_message` 收成干净 `message`，明文 `encrypted_content` 部件展成 `input_text`；入站不改 collaboration 工具名。Chat 转换、官方透传、压缩信封不在本切片。
- 实际修改：`src-tauri/src/managed_gateway.rs` 增加 API Provider 普通 Responses 请求的 agent-message 规范化和 HTTP 400 `unreadable_agent_message`；失败文案明确为「子任务转发失败」。父任务收到 Codex `Agent errored:` 回传时，Gateway 将追加“结果不可用、可自行继续”的标准文本提示。新增四项相关回归测试。设计与本计划同步。
- 实际验证：`cargo fmt --all -- --check` 通过；`cargo test managed_gateway::tests::routed_agent_message -- --nocapture` 通过（4 项）；`cargo test managed_gateway::tests::compaction_request_keeps_its_existing_fail_closed_path -- --nocapture` 通过。mock 上游确认实际收到了干净的 `message/user`，模型仍改写为绑定 upstream。
- 未完成项：Cockpit 代码未复制。方案 B（collaboration 改名与还原）未做；本次实机未触发二次委派工具名错误。
- 下一步：本票可归档。若以后子任务能开聊但二次 spawn_agent 因 namespace 工具名失败，再评估方案 B。
- 实机（2026-09-23，commander 重启隔离 App 后）：会话「排查 subagent 路由失败」并行 spawn 两个 fork_turns=none 只读子任务 probe_docs、probe_code。两者都完成并回写父会话，没有出现子任务首包 502 连打或父会话被 agent_message 拖死。probe_docs 确认隔离压缩为 LocalClient；probe_code 确认 agent_message 收成 message/user，失败码为 unreadable_agent_message。

### 2026-09-22：打开默认 Codex 与隔离 CODEX_HOME

- 变更目的：隔离 App 占住同一个应用包时，Finder 只能聚焦隔离窗口；同时让用户在首次继承后能直接打开隔离配置目录，自行修改且不被下次启动的默认目录覆盖。
- 合同：偏好和引导状态仍只在隔离目录首次启动时从 `~/.codex` 复制一次，标记为 `.3rcodex-preference-seed`。登录、认证和历史对话不复制。「打开默认 Codex」使用 `open -n` 启动对应应用包，不带隔离环境、`--user-data-dir` 或工作区路径；重复点击依赖默认 Electron 单实例锁回到已有默认窗口。「打开CODEX_HOME目录」打开当前工作区复用的隔离 App 目录，运行中也可打开；未启动过则返回 `isolated_home_not_ready`。
- 实际修改：`src-tauri/src/lib.rs`、`src-tauri/src/managed_launch.rs`、`src/App.tsx`、`src/codex-control.ts`、`src/styles.css`、独立工作区计划。
- 实际验证：`cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check` 通过；`cargo test --manifest-path src-tauri/Cargo.toml --lib tests::default_codex_open_starts_a_new_instance_without_isolation_args -- --exact` 通过；`pnpm run check` 通过。未打包，未启动 Tauri。
- 未完成项：真实点击需 commander 重启自己的 `pnpm run tauri:dev` 后验收。未打包。
- 回滚：恢复 `open_codex_app` 的 `codex app` 启动器路径，移除目录打开命令和两个按钮。

### 2026-09-23：首次继承个人 skills 与全局规则

- 变更目的：让隔离 App 在首次建立工作区时继承用户维护的个人技能和全局指导文件。
- 合同：只复制默认 `~/.codex` 根目录的 `AGENTS.md`、`rules/` 和 `skills/` 中的用户内容；隔离目录已有的 `.system` skills 保留，不复制链接、非普通文件或超过 16 MiB 的单文件。复制由 `.3rcodex-preference-seed` 标记保护，只发生一次；之后默认目录变化不会覆盖隔离目录内的修改。MCP、marketplace、shell 环境、凭据、会话和历史仍不继承。
- 实际修改：`src-tauri/src/managed_codex_config.rs` 增加安全的首次指导文件种子和回归测试；独立工作区计划同步。
- 实际验证：`cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`、定向 `managed_codex_config` 测试待本轮完成；未启动 Tauri、未打包。
- 回滚：移除 `seed_user_guidance` 调用和复制 helper；已有隔离目录不会被自动清理。

### 2026-09-23：设置继承清单与安全偏好白名单

- 变更目的：解释 Settings 中 Plugins、Browser、Computer Use 以及 Personal 偏好为何出现差异，并建立官网/未来同步设置可复用的继承分类。
- 结论：插件启用状态本来就在 `config.toml` `[plugins]` 中继承；本轮补齐 `.codex-global-state.json` 中稳定的 General、Appearance、Onboarding、模型选择器、侧栏和编辑器偏好。Browser/Computer Use 的 UI 开关可继承，但 Cookie、登录、历史、站点权限、设备绑定、运行时 socket、缓存、打开标签页和绝对路径不复制。MCP、marketplace、模型路由、账户和历史继续不继承。
- 实际修改：`src-tauri/src/managed_codex_config.rs` 扩展原子状态白名单与测试；独立工作区计划加入详细设置继承表。
- 实际验证：定向 `managed_codex_config` 种子测试通过；`pnpm run check` 待本轮完成。未启动 Tauri、未打包。
- 已完成：工作区卡片提供用户可选的按清单同步设置入口，支持 General、Appearance、Onboarding/Permissions、Plugins、个人指导文件和项目信任级别；同步要求隔离 App 已停止，且不复制整个 Electron 用户数据目录。后续可继续增加更细粒度的预览和版本冲突提示。

### 2026-09-23：项目会话导入冲突生成隔离副本

- 变更目的：把项目会话导入从“同 ID 一律跳过”扩展为可重复执行的安全增量导入。
- 合同：活动会话仍按项目 `cwd` 读取，归档会话不参与。源 rollout 与已有隔离 rollout 做规范化内容比较；未变化的同 ID 会话计入 skipped。内容变化时生成新的隔离 thread ID 和 rollout 文件，保留旧隔离副本，不覆盖原记录。新副本首行身份改为新 ID，Provider 仍为 `3rcodex`，源文件和源索引保持只读。
- 实际修改：`src-tauri/src/session_import.rs` 增加规范化比较、冲突副本生成和回归测试；`src/SessionImportPanel.tsx` 展示冲突副本数量；独立工作区计划同步交互合同。
- 实际验证：`cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`、`cargo test --manifest-path src-tauri/Cargo.toml --lib session_import::tests`（10 项通过）、`pnpm run check` 通过。未启动 Tauri、未打包。
- 未完成项：真实 UI 仍需 commander 重启自己的 `pnpm run tauri:dev` 验收；冲突副本会增加隔离历史条目，后续可增加来源 ID/冲突关系展示。

### 2026-09-22：结束实例清理与项目会话导入修复

- 变更目的：让「清除结束实例」真正清除 3R 隔离实例的专属运行数据，并修复项目扫描包含归档会话、导入无法定位最近隔离 App 数据的问题。
- 合同：受管理 App 删除其生命周期目录（专属 `CODEX_HOME`、其中已对齐的对话索引、配置、认证文件、模型目录和启动记录）及 3R 专属 Electron 数据，并 best-effort 删除旧 `managed-app-sqlite/<preset>` 残留；CLI 只删除其生命周期目录。模型渠道/API Key、启动方案、工作区级请求档案不属于单个实例，保留。绝不触碰用户默认 Codex 文件。
- 导入：项目列表和导入候选只读取 `sessions` 下的活动会话，`archived_sessions` 不再形成项目或被复制；目标从指定启动方案最近一次已初始化且已退出的 managed App 实例读取。
- 实际修改：`src-tauri/src/managed_launch.rs` 增加实例准入密钥同步撤销、目录归属校验和 App/CLI 分层清理；`src-tauri/src/lib.rs` 与 `codex_control.rs` 使用同一删除清理路径并不再静默忽略生命周期目录删除；`src-tauri/src/session_import.rs` 排除归档会话并保留异常路径安全校验；`src/App.tsx`、`src/codex-control.ts` 和 `src/SessionImportPanel.tsx` 增加不可恢复范围与失败提示；产品原理、详细设计和独立工作区计划同步。
- 实际验证：`cargo fmt --all -- --check` 通过；`cargo test --all-targets` 通过，252 个库测试、Gateway 1、launcher 12、官方认证 16 项全部通过；`cargo check --all-targets`、`pnpm run check`、`pnpm run build` 通过。未打包、未启动 Tauri。
- 未完成项：仍需 commander 用自己运行的 `pnpm run tauri:dev` 做真实 App 删除后数据残留检查，以及退出 3R App 后扫描/导入活动项目会话的实机验收；未读取用户默认配置、认证或真实会话正文。
- 回滚：恢复本条涉及的删除清理与会话筛选改动，不需要迁移用户默认目录；恢复 UI 文案即可撤回确认提示行为。
- 下一步：commander 重启自己的 `pnpm run tauri:dev`，先创建并退出一次 3R App 会话，再扫描确认归档项目不出现、活动项目可导入；随后删除结束实例并确认 App 对话、专属目录和 Electron 数据均不再残留。

### 2026-09-22：开发热更新与 Gateway sidecar 进程隔离

- 目标：开发期间修改 3R 控制面或 renderer 不影响正在运行的隔离 App；Gateway sidecar 保持原进程和原代码，只有显式停止后重新启动才应用新 sidecar 二进制。
- 合同：Vite renderer 热更新只刷新 UI；Tauri 主进程重载从 `gateway-sidecar.json` 复用已服务端点；sidecar 以独立进程会话启动，不随控制面父进程会话退出而被动终止。`pnpm run tauri:dev` 只在命令启动阶段构建 sidecar，开发过程中的 Rust 热重载不替换运行中的 sidecar。
- 实际修改：`src-tauri/src/managed_gateway/sidecar.rs` 在 Unix 使用 `setsid` 创建独立 session（进程会话），Windows 使用独立进程组和 detached process 标志；保留现有 ready 文件复用和 active binding 恢复；补充 Unix session 隔离回归测试。`src-tauri/src/lib.rs` 的正常退出分支在无活动实例时同步调用 `stop_if_unused()`，停止失败或仍有 binding 时阻止退出；Tauri 受控重启分支继续保留 sidecar。
- 非目标：不在 renderer 中实现 Gateway 热替换；不自动停止有活动 binding 的 Gateway；不改变实例路由、Keychain 或用户默认 Codex 文件。
- 生命周期补充：3R Codex 正常退出且无活动实例时先停止 Gateway；Tauri 受控开发重启、编译失败或强制终止时保留独立 Gateway，避免开发中的隔离 App 被控制面变化影响。退出停止失败或仍有 binding 时阻止退出并通过 `app-exit-blocked` 发送原因。
- 实际验证：`cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`、`cargo check --manifest-path src-tauri/Cargo.toml --all-targets`、`cargo test --manifest-path src-tauri/Cargo.toml --lib managed_gateway::sidecar`、`cargo test --manifest-path src-tauri/Cargo.toml --all-targets`、`pnpm run check` 和 `pnpm run build` 均通过；Rust 全量测试 282 项通过。未运行 `pnpm run tauri:dev`，未打包或签名。
- 未完成项：真实 Tauri/Vite 开发窗口中的 HMR、编译失败恢复、正常退出后的进程消失和 Gateway 连续请求仍需 commander 使用自己的 `pnpm run tauri:dev` 手动验收。
- 回滚：移除 `detach_sidecar_process` 的启动调用与 helper/test；保留 ready 文件复用逻辑即可恢复旧行为，不涉及用户默认 Codex 文件或实例数据迁移。
- 下一步：commander 用自己的 `pnpm run tauri:dev` 验收两条路径：正常退出 3R Codex 后确认 Gateway 进程结束；修改 Rust 触发开发重载或制造编译失败时确认 Gateway 与隔离 Codex 请求继续运行。若要让新的 `codex-gateway` 代码生效，仍需先停止所有隔离 App/CLI，使 Gateway 无活动 binding，再在 UI 中停止 Gateway，最后重新运行 `pnpm run tauri:dev`。

### 2026-09-22：单受管理 App 与 Gateway 按需生命周期

- 目标：当前产品只允许一个隔离 Codex App 活动运行；多 Provider 聚合成一份模型目录和不可变 RouteSet，运行期间不创建第二个 App 或替换当前 Gateway。
- 合同：启动首个需要 Gateway 的隔离目标时才调用 `ensure_serving()`；已有健康 Gateway 只复用，不因第二次 App 启动、控制面重载或配置文件变更而重启。第二个受管理 App 在启动预检和最终 SQLite 写事务均返回 `managed_app_in_use`。实例失败清理、实例删除和最后一个 binding 注销后都尝试 `stop_if_unused()`。
- 实际修改：保留并明确既有 `managed_app_in_use` 两阶段单实例检查；`src-tauri/src/lib.rs` 在 Gateway 启动前增加 App 可用性预检，避免第二次 App 启动先触碰 Gateway；实例启动准备失败、实例清理和实例删除后统一尝试停止无 binding 的 Gateway；设计文档同步区分底层多 binding 能力与当前单 App 产品限制。
- 非目标：本次不删除 Gateway 的底层多 binding 能力，不改变受管理 CLI 的既有并行策略，不做运行中的模型目录热更新或 RouteSet 改写。
- 验收：第二个 App 不启动新 Gateway、不改变现有 Gateway PID/端口；首个 App 启动才出现 Gateway；App 停止或启动失败后无 binding 时 Gateway 退出；已有 App 运行时控制面开发重载不影响请求。
- 实际验证：`cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`、`cargo check --manifest-path src-tauri/Cargo.toml --all-targets`、`cargo test --manifest-path src-tauri/Cargo.toml --all-targets`、`pnpm run check` 和 `pnpm run build` 均通过；Rust 全量测试 282 项通过。未运行 `pnpm run tauri:dev`，未打包或签名。
- 未完成项：真实第二次 App 启动拒绝、首个 App 失败后 Gateway 进程退出、App 停止后 Gateway 退出及开发重载期间 PID/端口稳定性仍需 commander 手动验收。
- 下一步：commander 使用自己的 `pnpm run tauri:dev`，先启动一个隔离 App，再重复点击启动确认返回“已有一个受管理 Codex App…”且 Gateway PID 不变；随后停止 App 确认 Gateway 退出，再制造一次启动失败确认无残留 Gateway。

### 2026-09-22：模型目录必填推理字段修复

- 变更目的：最新实机预检日志显示当前 Codex 拒绝缺少 `supported_reasoning_levels` 的生成目录。无推理档位时旧覆盖逻辑删除该字段，误把“不能广告推理能力”实现成了无效的目录结构。
- 合同：第三方条目始终保留 `supported_reasoning_levels`。有已验证档位时写精确档位与默认值；无档位时写 `[]` 并移除 `default_reasoning_level`，不回填 GPT 的推理档位。
- 实际修改：`src-tauri/src/model_family_profile.rs` 在无推理档位分支写入 `supported_reasoning_levels: []`；新增覆盖层断言。`src-tauri/src/managed_codex_config.rs` 的目录生成断言确认该字段和默认值的最终形状；本计划、独立工作区计划同步。
- 实际验证：`cargo fmt --all -- --check` 通过；`cargo test --lib` 249 通过；`cargo check --all-targets` 通过。未打包、签名或启动 Tauri，未读取默认 Codex 配置/认证。
- 未完成项：真实 Codex 解析由 commander 重启 `pnpm run tauri:dev` 后复验。若仍失败，读取最新 `startup.log` 的 `catalog_probe` 行；预期结果为 `catalog_probe result=success`。
- 回滚：将空数组写入恢复为移除字段并删除两条回归断言；这会重新暴露当前 Codex 的目录解析失败，不能作为发布修复。
- 下一步：commander 重启自己的开发服务后再次点击启动，确认预检通过；不需要打包。

### 2026-09-22：模型目录按渠道和模型排序

- 变更目的：实机生成目录的模型数组已按渠道/模型排列，但 Codex 条目的 `priority` 仍使用保存顺序，导致桌面端可能显示为旧顺序；3R 工作区的若干模型列表也直接显示保存顺序。
- 合同：目录按渠道名称、模型名称、公共模型 ID 稳定排序；`priority` 与目录位置一致。默认模型单独写入 3R 私有 Profile 的 `model` 字段，排序不能改变默认模型、路由 ID 或保存顺序。首次隔离 App 引导不读取原 Codex 设置，跳过后的 UI 状态只保存在 3R 的 Electron 数据根。
- 修改文件：`src-tauri/src/managed_codex_config.rs`（目录优先级、显式默认模型、排序回归）；`src-tauri/src/managed_launch.rs`（向 Profile 传入已保存的默认模型）；`src/App.tsx`（路由预览、工作区下拉、渠道和检测目录统一排序）；独立工作区计划。
- 实际验证：`cargo fmt --all -- --check` 通过；`cargo test --lib managed_codex_config::tests` 8 通过；`cargo check --all-targets` 通过；`pnpm run check` 与 `pnpm run build` 通过。不打包、不启动 Tauri。
- 未完成项：commander 需用自己运行的 `pnpm run tauri:dev` 重启后，确认 Codex App 的模型目录顺序和当前默认模型均符合预期。
- 回滚：恢复 `priority=route.position`、移除 Profile 的 `model` 字段和前端排序 helper；不涉及 3R 数据迁移或用户默认 Codex 文件。

### 2026-09-22：生成模型目录预检日志

- 变更目的：`codex_catalog_validation_failed` 原先丢弃 Codex 子进程的标准错误，无法区分命令不可执行、配置解析失败、目录输出格式改变等原因。现在在启动记录写入前的预检阶段保留可定位的脱敏证据。
- 修改文件：`src-tauri/src/managed_codex_config.rs`（模型目录与 Profile 探针的分阶段日志、URL/凭据脱敏、回归测试）；`src-tauri/src/managed_launch.rs`（将本次生命周期 `startup.log` 传给预检）；[独立工作区计划](designs/2026-09-21-isolated-only-product-plan.md)（日志隐私合同）；本计划。
- 合同：每次预检记录成功或失败阶段、退出状态、stdout/stderr 字节数和 SHA-256；失败 stderr 只记录最多 768 字符的摘要。写入前会移除 URL、认证头和常见 Key/Token 值；不记录请求正文、完整 URL 或凭据。日志写入失败不改变原有阻止启动的行为。
- 实际验证：`cargo fmt --all -- --check` 通过；`cargo test managed_codex_config::tests` 7 通过（包含非零退出写日志，以及 URL/Bearer 脱敏）；`cargo check --all-targets` 通过；`pnpm run tauri:build --bundles app` 通过。包内确认包含 `catalog_probe` 诊断标识；执行 `codesign --force --deep --sign -` 后 `codesign --verify --deep --strict` 通过。
- 未完成项：真实 Codex 预检失败尚待 commander 用新包复现。预检在实例记录写入之前执行，失败日志位于 `~/Library/Application Support/io.3r.codex/codex-instances/<attempt-id>/startup.log`，尚未作为单独 UI 记录展示。
- 回滚：移除两个探针的 `startup.log` 参数与诊断辅助函数；不影响用户 Codex 或任何 Provider 凭据。
- 下一步：退出旧 3R 进程后运行新包复现；读取最近修改的 `startup.log` 中 `catalog_probe` 行，按 `stage`、`status` 和 `stderr_summary` 定位根因。

### 2026-09-22：独立工作区改造实施与验证

- 已确认并实现：单工作区，主按钮只有图标且 hover「启动」；App/CLI 使用独立配置与会话目录；支持多渠道模型及工作区内 API 登录选择；首版仅「我的 Codex → 3R」按项目复制；不做清理向导。
- 已移除：原配置快照、默认配置投影/恢复、旧兼容启动、原地历史 Provider 修复、默认上游 fallback、旧 UI/命令及无效样式。旧数据库结构只用于迁移识别；schema v18 只更新自有库的兼容绑定/运行锁和模型选择推理档位，不操作用户文件或进程。
- 升级补充：sidecar ready 加协议版本。遇到已运行的旧网关，只经 3R 控制接口注销历史兼容绑定；无独立活动绑定时正常关闭旧网关，下次启动新网关；仍有独立实例时保留其转发。注销失败显式阻止复用，不发送默认 Codex 进程信号。测试覆盖有/无独立活动绑定和重复状态查询。
- 启动回归：登录渠道限定工作区，乐观版本与运行锁校验，移除渠道清除旧选择；Key 只物化到 3R auth.json；重启合并 3R 自有偏好；链接和越界目录拒绝；失败清理新准入绑定和运行锁。
- 导入回归：仅显式扫描；源索引/WAL 复制后查询，不修改原 SHM；预览元数据、停机复核、初始化/schema 检查、同 ID 跳过、目标副本 Provider 改为 3rcodex、事务失败回滚。后台线程执行，启动与导入互斥。
- 保留请求档案：迁至运行诊断，保留按环境/会话分组、四阶段分页、阶段对照、部分/待关联状态、显式记录与清空保护。
- 修改文件按模块见[专项计划 §6](designs/2026-09-21-isolated-only-product-plan.md#6-修改范围)，另包括 resource_guard.rs、providers.rs、diagnostics.rs 和 sidecar 升级读写结构；README、AGENTS、产品原理、详细设计和 UI 专项均已同步。

实际验证（macOS，本机临时夹具；未访问真实默认配置/认证/会话正文，未请求收费模型）：

- pnpm run check：通过；pnpm run build：通过，Vite 生产资源生成成功。
- cargo fmt --all -- --check：通过；cargo check --all-targets：通过，无编译警告。
- cargo test --all-targets：通过，275 项（lib 246、gateway 1、launcher 12、official_auth fixture 16），0 失败。真实安装合同测试未显式开启，不能算作真实认证验收。
- 浏览器：内置浏览器运行 ui-fixture；核验图标/hover、API 登录选择与缺失禁用、显式扫描、预览、成功、进程占用失败、空项目状态、启动成功反馈；档案分组、阶段对照、记录中禁止清空和正文预览通过，控制台无错误。
- pnpm run tauri:build --bundles app：通过；产物为 src-tauri/target/release/bundle/macos/3R Codex.app（macOS arm64）。首次包验证发现仅有链接器签名；对生成包执行 codesign --force --deep --sign - 后，codesign --verify --deep --strict 通过。当前为本机 ad-hoc 签名，非 Developer ID 公证发布包；未安装或启动该包，避免打断现有会话。

已知范围：项目附件保持原路径，不搬运；源自定义 CODEX_SQLITE_HOME 不自动发现；异常中止可留下未索引的目标副本。以上首版限制已写入产品合同，均不覆盖或写回源历史。Windows 打包/ACL、真实 Codex App 登录、多渠道连续与恢复、导入后列表显示仍需实机验收。

回滚：仅代码、生成包与 3R 自有数据；schema v18 后不能直接用只支持旧 schema 的二进制打开新库。保留旧版内部备份后才可回滚；不恢复用户默认配置。没有清理向导，重启不表示过去版本的默认配置投影已经还原。

下一步：commander 按自己的安排重启新包，选择渠道并启动 Codex 创建一次会话；退出双方 Codex 后导入项目，再重启 3R Codex 检查列表与续聊。未执行真实停机或修改真实用户数据。

## 历史执行记录（已取消的默认 App / 修复步骤不再执行）

### 2026-09-21：提案 — 取消双入口，3R 只启动隔离 Codex（已获确认，历史记录）

- 变更目的：commander 要求评估「去掉默认 App / 隔离 App 对举，3R 启动的 Codex 与用户原配置完全隔离；多渠道保留；其中一个渠道作 API 登录；不碰默认 config.toml/auth.json；允许按项目把会话拷进 3R」。
- 结论：赞成。兼容投影是近期故障和概念负担的主要来源；隔离启动本来就是完整产品。
- 修改文件：[改造计划](designs/2026-09-21-isolated-only-product-plan.md)。**本轮不改代码。**
- 未完成项：确认对外名称、导入是否单向、撤卡前是否做 marker 清理向导；确认后先改产品原理再实施。阶段 0 需废弃未完成的默认 auth.json 覆盖草稿。
- 阻塞：等确认。不要在合同未改时删除 compat_app。
- 回滚：删除该计划文件与本段。
- 下一步：按计划第 10 节确认点回复。

### 2026-09-21：冻结默认 App「开启本地压缩」

- 变更目的：远程 Gateway 压缩已可用；初版不上默认 App 本地压缩。隐藏卡片开关，并忽略已保存的 opt-in，避免用户被留在无入口的本地链上。
- 修改文件：`managed_codex_config.rs`（`COMPAT_LOCAL_COMPACT_OPT_IN_ENABLED=false`，启动模式与显示名强制远程）；`managed_launch.rs`（读取 opt-in 时钳制）；`App.tsx`（去掉开关与 handler）；产品原理、详细设计 §8.2.1、AGENTS 未完成项；本计划。
- 合同：默认 App / 兼容 CLI 固定 `remote_v2` + 显示名 `OpenAI`。隔离入口仍 `3rcodex` / `3R Codex Gateway` / LocalClient。`set_compat_local_compact` 可写 JSON，物化不生效。解冻只翻该常量为 true 并恢复 UI。
- 实际验证：`cargo fmt --all --manifest-path src-tauri/Cargo.toml -- --check` 通过；`cargo test --lib compact` 21 passed（含 `frozen_compat_local_compact_opt_in_stays_remote_v2`）；`catalog_compatibility_uses_known_version_profile` 与 `compat_projection_*` 通过；`pnpm run check` 通过。未改正在跑的默认 App；若曾勾选过，需再应用配置一次把显示名写回 `OpenAI`。
- 未完成项：解冻本地压缩仍是独立票。隔离路径不做 Summarizer。
- 阻塞：无。
- 回滚：常量改回 true，恢复卡片开关与 LocalClient 测试期望。
- 下一步：应用配置后确认默认 App 长会话走 Gateway 摘要。不要和 F/C 混票。

### 2026-09-21：会话修复允许当前 Codex 时间戳触发器

- 变更目的：恢复显式会话修复。当前默认 App 的 `state_5.sqlite` `threads` 表带有毫秒时间戳/recency 触发器，旧实现把任意触发器当未知 schema，扫描失败；`custom` / `codex_local_access` 等旧 `model_provider` 因此无法改写，打开这些会话时 Codex App 会因 Provider 不存在反复重建界面。
- 修改文件：`history_provider_repair/sqlite.rs`（只拒绝会在 `UPDATE model_provider` 时触发的触发器）；`history_provider_repair/tests.rs`；[会话修复设计](designs/codex-history-provider-migration-design.md)；本计划。
- 合同：`AFTER INSERT` / `AFTER UPDATE OF` 其它列允许；无列限定的 `UPDATE`、`UPDATE OF model_provider`、`INSTEAD OF` 仍失败关闭。本次写入仍只改 Provider 名称，不改标题或时间戳。
- 实际验证：`cargo fmt --all --manifest-path src-tauri/Cargo.toml -- --check` 通过；`cargo test --manifest-path src-tauri/Cargo.toml --lib history_provider_repair` 22 passed / 0 failed（含当前 Codex 时间戳触发器允许修复、无列限定 UPDATE 仍失败关闭）。未对真实 `~/.codex` 应用修复，未改正在跑的默认 App。
- 未完成项：commander 退出 Codex 后在「历史与测试记录 / 会话修复」扫描，把 `custom`、`codex_local_access`、`3rcodex`、`3r_gpt_*` 预览并应用到 `OpenAI`，再打开旧会话确认不再闪烁。Windows 实机仍待验。
- 阻塞：应用修复前必须退出 Codex App/CLI。
- 回滚：还原 sqlite.rs 触发器策略。
- 下一步：用户停机后执行修复；不要和 F/C/D5 混票。

### 2026-09-21：F3 当前模型族-only 落地

- 变更目的：未选 GPT 族时默认 App 改名/侧栏不得再打官方 `gpt-5.6-luna`，改走工作区默认 Responses 站桩。
- 修改文件：`routing.rs`（族判断/别名目标）；`managed_codex_config.rs`（merge 滤 GPT、隐藏 luna 目录行）；`managed_launch.rs`（快照 `ephemeralFamilyAliases`、fallback 过滤）；`managed_gateway.rs`（请求按快照别名转发，`/models` 不列出）；`model_family_profile.rs`（别名不写 auto_review）；F 设计 / followups / E11；本计划；详细设计 2026-09-21 段。
- 合同：`gpt_family_is_selected` 为工作区勾选。未选则 merge/回退去掉 GPT slug（内部角色保留）。luna 不进 RouteSet 身份表（public_model_id 全局唯一）；目录隐藏行 + Gateway 快照映射到站桩。选了 GPT 保持 native 回退。无 Responses 站桩则 `model_not_in_route_set`。
- 实际验证：`cargo fmt --check` 通过；`cargo test --lib` 311 passed（含 merge 剥 GPT、fallback 滤 GPT、luna 转发 grok-4.6 + channel key、无别名 404、compat_app 假 Codex 夹具）。未改正在跑的默认 App。
- 未完成项：用户手验（应用配置后改名/view_image；下次 compact 看 D5 504）。E10-1/E10-4。C 停放。F3 代码已完成。
- 阻塞：无。用户须应用配置。
- 回滚：还原上述文件；再应用一次配置。
- 下一步：用户应用配置后改 Grok 会话名，确认 3rcd 不再出现 luna。

### 2026-09-21：F 锁定当前模型族-only；C 停放

- 变更目的：commander 不能容忍未选 GPT 时 luna 出现；只接入三方时必须当前模型族-only。C 生图产品化改为极低优先级（日后或可做成技能管理），暂不做。**不改代码。**
- 修改文件：[F 设计](designs/2026-09-21-f-family-only-ephemeral.md)；followups C/F；错误待办 E11；本计划；详细设计 2026-09-21 兼容回退例外；AGENTS 未完成项；产品原理第 15 节补充句；overlay 诊断表。
- 已确认现状：默认 App 兼容目录 merge 保留全部原生 bundled 行；`compatibility_fallback_from_config` 把未选中原生 slug 放进官方回退。App 写死 `gpt-5.6-luna` 的 ephemeral 请求因此走 `compatibility_native`。删 GPT 勾选不会改用 Grok。
- 合同：未选 GPT 族（`looks_like_gpt_model_id`）则 merge/回退去掉 GPT slug；luna 隐藏别名到工作区默认 Responses；选了 GPT 族保持 native 回退。残留官方登录不算选了 GPT。不因 401/502 换路。
- 实际验证：只读 `managed_codex_config.rs` merge、`managed_launch.rs` fallback、Gateway 精确匹配失败后的 native 转发。未跑 cargo test。未改正在跑的默认 App。
- 未完成项：F3 实现；D5 实机 504；默认 App view_image 手验；E10-1/E10-4。C 停放。
- 阻塞：无。
- 回滚：删除 F 设计并还原本段与 followups/E11 状态句。
- 下一步：独立轮次实现 F3；不要做 C，不要和 D5/B 混票。

### 2026-09-21：B3 Grok 4.x Responses 打开看图

- 变更目的：标准 Responses `input_image` 探针已看见像素；family/CapabilitySet/目录对 Grok 4.x Responses 宣称 `[text, image]`。Chat 与 grok-3 仍关。日后接入探针按各协议标准形，本票不改 connectivity.rs 向导。
- 修改文件：`model_family_profile.rs`；`managed_launch.rs` 断言；`managed_codex_config.rs` 目录夹具；followups B3；本计划。
- 合同：`grok_version_is_4x` → supports_images；`family_image_support_for_route` 仍要求 Responses。Gateway 预检放行 input_image。
- 实际验证：`cargo fmt --check` 通过；`cargo test --lib` 304 passed。未改正在跑的默认 App。复测 HTTP 一次超时，不挡 B3。
- 未完成项：C、F、默认 App 图形 view_image 手验。
- 阻塞：无。用户须应用配置。
- 回滚：grok_profile.supports_images 改回 false。
- 下一步：应用配置后用 Grok 任务 view_image；或 C/F。

### 2026-09-21：B1 3rxai grok-4.6 看图探针失败，B2 保持 false，A3 写入路径接上

- 变更目的：按产品原理第 8 条验证 3rxai Grok 看图；失败则不准宣称。接上 family→CapabilitySet 图像写入，当前值仍为 false。
- 修改文件：[B 设计](designs/2026-09-21-b-grok-vision-probe.md)；fixture `src-tauri/tests/fixtures/grok-vision-probe-red-square.png`；`scripts/probe_grok_vision.py`；`model_family_profile.rs` `supports_images` / `family_image_support_for_route`；`managed_launch.rs` A3；followups B1/B2/A3。
- 探针：3rxai `ai.3rcd.com` `/v1/responses` grok-4.6。文本「Reply with the single word ok」→ 200，上游模型 grok-4.6-build。`input_image` data URL / https 红图以及 Chat `image_url` → 400 Upstream rejected 或 502 Upstream request failed。不经 Gateway。
- 合同：family.supports_images 默认 false；Responses 且 family true 才写 [text,image]。Chat 永不因本字段开图。搜索仍 false。
- 实际验证：`cargo fmt --check` 通过；`cargo test --lib` 303 passed（含 grok_image_support_stays_off_after_failed_3rxai_probe、prepare_launch 仍纯文本）。未改正在跑的默认 App。D5 实机 commander 暂不验收。
- 未完成项：B3 等渠道看图合同；C；F。
- 阻塞：3rxai 拒图。
- 回滚：去掉 family 字段与 launch 分支。
- 下一步：不要把 supports_images 改 true。C/F 或等渠道。

### 2026-09-21：A1 剥图像/搜索泄漏；D5 Summarizer SSE + medium

- 变更目的：目录不再广告未验证看图/搜索；Summarizer 出站改 SSE，避免 30 秒首字节误杀，摘要推理降到 medium。不打开 supports_images，不改 3rxai 超时表。
- 修改文件：`model_family_profile.rs` overlay；`managed_codex_config.rs` 目录夹具；`managed_gateway/compaction.rs`；`managed_gateway.rs` mock；§8.2.1 第 2/5 条；overlay §6/7/9；followups A1/A4/A5/D5；错误待办 E8-1/E8-3/E10-2/E10-3；[设计](designs/2026-09-21-a1-d5-parallel-design.md)；本计划。
- 合同：ApiProvider 且 supports_images=false 删除 supports_image_detail_original；supports_web_search=false 删除 web_search_tool_type。Summarizer 出站 stream=true，高推理档降 medium；收齐 SSE/JSON 后仍给 Codex 一个 3r-compaction-v1 项。失败不回退、不换路。
- 实际验证：`cargo fmt --all --manifest-path src-tauri/Cargo.toml -- --check` 通过；`cargo test --manifest-path src-tauri/Cargo.toml --lib` 302 passed / 0 failed（含 overlay 剥图像/搜索广告、官方行保留、catalog 夹具、Summarizer stream=true、推理降 medium、上游 SSE 收齐成一个信封）。未跑付费矩阵；未改正在跑的默认 App。
- 未完成项：B 看图探针；C 生图产品化；F/E11 luna；E10-1 渠道超时表；E10-4 用 UI 改小窗。D5 实机 504 需下次 compact 观察。
- 阻塞：无代码阻塞。用户须应用配置 / 重启 Gateway。
- 回滚：还原 overlay 删除、prepare_summarizer_request 的 stream=false 与推理处理。
- 下一步：B 或观察 D5 实机；不要和 C/F 混票。

### 2026-09-21：A1 / D5 并行设计，待确认后实现

- 变更目的：回答 A1 与 D5 能否同步；写出可确认的合同，**不改代码**。
- 修改文件：[A1/D5 设计](designs/2026-09-21-a1-d5-parallel-design.md)；followups / 错误待办状态句；本计划。
- 并行：可以。A1 只动 overlay 目录字段；D5 只动 Summarizer 出站 stream 与摘要推理档。共享 Markdown 实现时由主会话合并。
- 拟议合同（未锁进 §8.2.1 / overlay 正文，确认后才写）：A1 按 CapabilitySet 删除 supports_image_detail_original 与 web_search_tool_type；D5 摘要出站 stream=true 收齐 SSE，过高推理降到 medium；不改 3rxai 超时表，不改 500k family 窗。
- 实际验证：只读 model_family_profile.rs overlay、compaction.rs prepare/collect、§8.2.1。未跑 cargo test。未改正在跑的默认 App。
- 未完成项：确认后实现 A1/D5；B/C/F 仍分开。
- 阻塞：等 commander 确认设计。
- 回滚：删除该设计文件并还原本段与 followups 状态句。
- 下一步：确认后按设计 Execution 切片实现；不要和 B/C/F 混票。

### 2026-09-20（晚）：A2 每模型官方推荐窗 + 接入 UI 覆盖

- 变更目的：停止把 GPT 骨架的 272000/872000 抄进三方目录；Grok 写官方推荐 500k，工作区高级映射可覆盖。只动窗口，不打开看图、不改 Summarizer 超时。
- 修改文件：followups A2、错误待办 E8-2、overlay §6/7、本计划、AGENTS.md、详细设计 schema v16；`database.rs` v16；`model_family_profile.rs`；`CapabilitySet.auto_compact_token_limit`；`managed_launch.rs` / `preset_validation.rs` / `models.rs`；`managed_codex_config.rs` 目录写入；`src/App.tsx` / `codex-control.ts` 高级映射两栏。
- 合同：UI 覆盖 → family 默认 → 官方 slug → 仍无则剥骨架三键。Grok 500000 / 475000；其它族 None。校验 `1..=999_999`；两 UI 值都有时 compact ≤ window。启动物化 compact = UI or（family 且 ≤ 已解析窗）or window×95%。不写进程级窗。原生 GPT 行不 overlay。空列 = family 默认；保存并应用 / 重启后生效。
- 实际验证：`cargo fmt --all --manifest-path src-tauri/Cargo.toml -- --check` 通过；`cargo test --manifest-path src-tauri/Cargo.toml --lib` 296 passed / 0 failed（含 Grok 500k/475k family、未知族剥 GPT 窗、UI 覆盖、仅窗覆盖默认 95% compact、拒绝 0/1000000、原生 GPT 兼容行不 overlay、schema v16 空列、v15 部分夹具仍能补 `error_detail`）；`pnpm run check` 通过。未跑付费矩阵，未改正在跑的默认 App。
- 未完成项：A1 图像/搜索泄漏；B/C；D5/E10 超时；F/E11 旁路 luna。
- 阻塞：无代码阻塞。正在跑的默认 App 仍用旧目录，直到用户保存并应用。
- 回滚：还原 schema v16 列与上述文件；旧行空列仍可读。
- 下一步：D5 或 A1（只剩图像/搜索泄漏，窗口已不在 A1）；不要和 B/C 混票。本会话结束。

### 2026-09-20（下午）：实机 compact 成功；超时 / 窗口 / 旁路 luna 只落档

- 变更目的：把今日默认 App Grok 实机 compact 证据、504 首字节超时、窗口自定义要求和 Codex 旁路短模型写进待办。**不改代码、不改正在跑的默认 App 配置。**
- 修改文件：[followups](designs/2026-09-19-grok-session-followups.md) D3/D5/A2/F；[错误待办](designs/2026-09-19-grok-session-error-todos.md) E1-3、E10、E11；本计划；AGENTS.md 未完成项。
- 已确认：
  - 会话 `01a0bcf6`：GatewaySummarizer 触发且 `code=ok`；input 约 228622 → 30654。外国 compaction 信封未再拒绝。
  - 同一压缩链路上 5 次 `upstream_first_byte_timeout`（3rxai first_byte=30s，非流式 send 等头），Codex 客户端重试后成功。候选优化见 E10，待 commander 确认。
  - 窗口：当前 Grok 目录仍抄 GPT 272000/872000，会话有效窗 258400。A2 按官方推荐窗 + UI 覆盖继续做，未探针不写 1000k。
  - luna：ChatGPT.app 标题/说明写死 `gpt-5.6-luna`+low；不是 3R 把 Grok 改道。删 GPT 勾选不够，兼容目录仍 merge bundled。是否 Grok 兜底待确认（F1/E11-1）。
- 实际验证：只读 sidecar、`gateway_request_diagnostics`、ChatGPT.app asar（`var yj=gpt-5.6-luna`，`H9=low`）、cockpit 会话 rollout（正文 sol）。未改 Gateway/目录。
- 未完成项：A/B/C；A2 窗口实现（当晚已做）；D5/E10 超时优化；F/E11 旁路短模型产品决定。
- 阻塞：无代码阻塞。优化项等确认。
- 回滚：还原上述 Markdown。
- 下一步：A2 已在当晚交接落地；剩余 D5 / 旁路目录策略。不要和 B/C 混票。

### 2026-09-20：E1-2 选项 1 锁定，Summarizer 折叠 reasoning 密文

- 变更目的：默认 App Grok 长会话 compact 不再把本路由 `type=reasoning` 密文误报成压缩信封；GatewaySummarizer 折叠密文、保留可见文本后再摘要。外国 compaction 信封仍 fail-closed。
- 修改文件：`docs/codex-hybrid-routing-design.md` §8.2.1 第 3 条（合同锁定）；`src-tauri/src/managed_gateway/compaction.rs`（`prepare_summarizer_request` / `fold_summarizer_encrypted_content`）；`src-tauri/src/managed_gateway.rs`（`incompatible_reasoning_envelope` 文案与 HTTP mock）；本计划、产品原理第 15 节、第三方 overlay 诊断表、AGENTS.md、[错误待办](designs/2026-09-19-grok-session-error-todos.md)、[followups D](designs/2026-09-19-grok-session-followups.md)。
- 合同：Summarizer 出站前展开 `3r-compaction-v1:`；外国 `type=compaction` 信封 → `incompatible_compaction_envelope`；`type=reasoning` 删除密文、可见 `summary`/`content` 写成 assistant 文本（Chat 适配需要），无可见文本则丢弃；其它非 compaction 项剥密文，剥不干净 → `incompatible_reasoning_envelope`。普通转发仍只展开 3R 信封，不折叠 reasoning。
- 实际验证：`cargo fmt --all -- --check` 通过；`cargo test --lib` 280 passed / 0 failed（含 `prepare_summarizer_folds_reasoning_ciphertext_and_rejects_foreign_compaction`、`remote_v2_summarizer_folds_reasoning_ciphertext_before_upstream`、`remote_v2_foreign_envelope_never_reaches_upstream`）。未跑付费 Provider 矩阵，未在真实默认 App 上再 compact 一次。
- 未完成项：真实默认 App Grok 长会话 compact 一次（E1-3 实机）；A/B/C 仍分开。
- 阻塞：无代码阻塞。实机 compact 需要用户默认 App + Grok 长会话。
- 回滚：还原 §8.2.1 第 3 条与 `compaction.rs` / 错误文案，普通转发不受影响。
- 下一步：有真实默认 App 时用新建短 Grok 会话把上下文撑到 compact；不要往原 thread `01a0b8da-0e36-7bf2-9344-81f272b14558` 续写。不要和 A/B/C 混票。

### 2026-09-19（下班）：E1 `encrypted_content` 调研落档，未改代码

- 变更目的：确认 GatewaySummarizer 为何把真实默认 App Grok 长会话 compact 打成 `incompatible_compaction_envelope`；分类 `encrypted_content`；对照 OpenCodex / CC Switch / Cockpit / Codex++ 的 Summarizer，给明天改合同用。用户明确只要 E1，不改代码。
- 修改文件：[Grok 会话错误分析 E1 调研](designs/2026-09-19-grok-session-error-todos.md)、[问题与下一步 D 段](designs/2026-09-19-grok-session-followups.md)；本计划、详细设计 §8.2.1 实现偏差注、产品原理第 15 节、第三方 overlay 诊断表、AGENTS.md。
- 已确认：
  - `encrypted_content` 是槽位，不是一种密文。三类：`type=reasoning`（签发方续接状态）、官方 `type=compaction`、网关合成信封（`3r-compaction-v1:` / `ocx1:` / `cc-switch-compaction-v1:`）。
  - 本会话失败物是 Grok reasoning 密文，不是已证明的官方 compact blob。普通回合已经把同类 reasoning 项转给同一 3rxai；Summarizer 的 `reject_foreign_encrypted_content` 比 §8.2.1 第 3 条字面更严（展开 3R 信封后拒绝任何剩余密文）。
  - 官方 compact 信封仍不得发给三方。本路由自己签发的 reasoning 密文不是密码学上的绝对禁令，但 Summarizer 没有理由把它当摘要输入（无信息量、体积、Chat 无此槽、xAI 可能因 `content: null` 误报 compact blob）。
  - 同行：发给三方的是文本；官方密文保持不透明；剥/折 reasoning 密文后续接，不整次 compact 失败。Codex++ 无 Summarizer。
- 推荐（**未锁进合同**）：E1-2 选项 1——Summarizer 折叠 reasoning 密文、保留可见 `summary`/`content`，外国 compaction 信封继续 `incompatible_compaction_envelope`。选项 2/3 仍写在待办里。
- 实际验证：只读核对 `compaction.rs`、OpenAI Responses 文档、OpenCodex raw 源码（`compaction.ts`、`parser.ts`、`reasoning-envelope.ts`、`sanitizeReasoningInputContent`）、CC Switch PR #5536 API、Cockpit 发行说明。未跑 cargo test（无代码变更）。未跑付费矩阵。GitHub HTML 抓取曾被拦，源码走 raw/API。
- 未完成项：把选项 1（或改选）写入 §8.2.1 第 3 条；改 `prepare_summarizer_request` 与错误码（E1-1/E9）；E1-3 mock；真实默认 App Grok 长会话 compact 一次。A/B/C（目录泄漏、看图、生图）仍分开。
- 阻塞：E1-2 未写入详细设计前不改 Summarizer。不要第三条静默历史改写。
- 回滚：还原上述 Markdown。无代码/schema 变更。
- 下一步命令：按下方「明天阅读与执行顺序」做。临时绕开：隔离入口（默认 App 本地压缩初版已冻结）。原 thread `01a0b8da-0e36-7bf2-9344-81f272b14558` 保持归档。

#### 明天阅读与执行顺序（E1 压缩，不要和 A/B/C 混票）

新会话不要从零再调研 `encrypted_content`。按这个顺序读完再动合同；合同未改前不改 `compaction.rs`。

**读（够用即可停，不要整本翻设计）**

1. [AGENTS.md](../AGENTS.md)「当前未完成项」里 Grok 压缩那条：安全边界、不改 Summarizer 除非先写 §8.2.1。
2. 本段交接：已确认事实、推荐未锁、未完成项。
3. [错误待办 E1 调研](designs/2026-09-19-grok-session-error-todos.md)：槽位三类、发/不发表、同行表、选项 1/2/3。E2–E8 跳过。
4. [问题与下一步 D 段 + Active Status](designs/2026-09-19-grok-session-followups.md)：D 是压缩票；A/B/C 是目录/看图/生图，本轮不做。
5. [详细设计 §8.2.1 GatewaySummarizer 第 3 条](codex-hybrid-routing-design.md)：先读合同原文，再读紧随其后的「实现偏差」注。偏差注不是合同。
6. 代码只在动手时打开 [`compaction.rs`](../src-tauri/src/managed_gateway/compaction.rs)：`expand_compaction_history`、`reject_foreign_encrypted_content`、`prepare_summarizer_request`，以及单测 `prepare_summarizer_strips_trigger_expands_envelope_and_rejects_foreign_blobs`。

**做**

1. 把 E1-2 **选项 1** 写进 §8.2.1 第 3 条（折叠 `type=reasoning` 密文、保留可见 `summary`/`content`；外国 **compaction** 信封仍 `incompatible_compaction_envelope`）。用户改选 2/3 才改写别的。写完才算锁定。
2. 改 `prepare_summarizer_request`：Summarizer 路径折叠 reasoning 密文后再摘要；普通转发不要突然变得和今天一样严。
3. E1-1 / E9：拆错误码和用户文案（reasoning 不要再说成「压缩信封」）。
4. E1-3 mock：仅 reasoning 密文 + trigger → 应摘要；仅外国 compaction 信封 + trigger → 仍拒绝；`3r-compaction-v1:` + reasoning 密文 → 展开信封并折叠 reasoning 后摘要。
5. 验证：`cargo fmt`、`cargo test --lib` 针对 compaction / summarizer 相关测试。真实默认 App Grok 长会话 compact 一次可做，但**不要**跑付费 Provider 矩阵，除非用户再要。

**不要做**

- 未写 §8.2.1 就改 Summarizer。
- A overlay 泄漏、B 看图探针、C 生图产品化。
- 静默改写历史、把官方 compact blob 发给三方、把隔离 Provider 改名为 `OpenAI`。
- 往原 thread `01a0b8da-0e36-7bf2-9344-81f272b14558` 续写。

### 2026-09-19：接管「确认当前模型是否为 Grok」并落档待办

- 变更目的：原会话在用户要求把问题与方案落档时因远程压缩失败（systemError / incompatible_compaction_envelope），本仓库接管该对话，把问答结论和下一步写成专项待办，并把错误拆成可复现条目。本轮只改文档，不改代码。
- 修改文件：[Grok 会话问题与下一步待办](designs/2026-09-19-grok-session-followups.md)、[Grok 会话错误分析与待办](designs/2026-09-19-grok-session-error-todos.md)；本计划、产品原理第 15 节、第三方目录 overlay 诊断表、AGENTS.md 未完成项增加入口。
- 已确认：聊天模型是 3rxai/grok-4.6，不是 Grok 5.6，也不是 gpt-5.6-luna。luna 只出现在误用 image2 Responses 宿主的那次生图。grok-imagine 应走当前 Grok 渠道的 /v1/images/generations；本机 Gateway 只有聊天 /v1/responses。supportsImages=false 是看图开关、故意保守，不是生图能力。目录仍泄漏 GPT 的 supports_image_detail_original、web_search_tool_type=text_and_image 和 272000/872000 窗口。
- 已落地（个人技能，不在本仓库）：~/.codex/skills/grok-imagine 用 3rxai 凭证出图成功；image2 在 Grok 会话让路。
- 关键错误：默认 App Grok 长会话 compact 时，GatewaySummarizer 把 122 条 reasoning.encrypted_content 当成外国压缩信封拒绝。普通回合能继续，一压缩就 systemError。这是真实默认 App + Grok 矩阵证据，不是 mock。
- 实际验证：只读核对原会话 rollout、兼容目录 grok-4.6 条目、managed_launch CapabilitySet 写死点、compaction.rs prepare_summarizer_request 与单测。未跑 cargo test（无代码变更）。
- 未完成项：见两份专项待办。E1 调研已落档（见本文件更上方 2026-09-19 下班交接）；代码仍未改。其次 A/E8（目录泄漏），再 B（3rxai 看图探针），再 C（生图产品化）。
- 阻塞：看图不能在未探针时标 true；Summarizer 在 E1-2 写入 §8.2.1 前不改。不要第三条历史改写路径。
- 回滚：删除上述两份 designs 文档并还原本计划/AGENTS/原理/overlay 入口即可。
- 下一步命令：以本文件更上方「2026-09-19（下班）」交接为准。原 thread 保持归档，不要再往 01a0b8da-0e36-7bf2-9344-81f272b14558 续写。

### 2026-09-17：第三方目录身份 overlay 产品决策

- 目标：Responses 接入发挥三方模型自己的执行力，而不是让它扮演 GPT-6 驾驶 Codex。
- 合同：一个目录生成器 + 族 profile。身份句为「你是 {Family}，在 Codex 工作区里用本地工具。」Grok / DeepSeek / Claude 依此类推。必须替换 bundled 的 GPT-6 开场白，并剥除 `max`/`ultra`、未声明的 Fast/priority 档；保留 `code_mode_only` 与本地 `exec` / `apply_patch`。
- 实现：`model_family_profile` 提供共享族数据与 overlay；`model_entry_from_template` 在克隆骨架后统一应用两个指令入口的身份、推理档、速度档、强制子 agent 推理档、Node REPL 默认值和审核字段，同时保留 Codex 通用多 agent 与代码工具合同。Grok 4.6 写入 `low/medium/high/xhigh` 并默认 `high`，deepseek/Claude/未知族不凭空声明推理档。
- 自动审核：每个第三方 Responses 条目的 `auto_review_model_override` 指向自身公共模型 ID。隔离 Profile 不写 `approvals_reviewer`，合并时删除旧值，沙箱提权询问；Chat-only 路由不启用审核覆盖，前端不提供第二个审核模型选择。
- 验证：Rust profile/catalog/启动测试与 `pnpm run check` 已通过；完整回归结果记录在本次任务交接段。
- 旧会话：跨 Provider 不兼容时返回 `route_history_incompatible`，引导新会话用文本交接继续；不把旧 GPT 会话字段级续成 Grok。

### 2026-09-18：第三方目录 overlay 实现交接

- 变更目的：让 Grok、deepseek、Claude 和未知第三方模型以自己的身份使用 Codex 本地工具；移除 GPT-6 推理/速度能力泄漏，并让自动审核随当前 Responses 模型路由。
- 修改文件：`src-tauri/src/model_family_profile.rs` 新增族 profile 与 overlay；`managed_codex_config.rs` 在版本骨架末尾应用 overlay；`managed_launch.rs` 固化推理能力、自动启用审核并保证经 Gateway 精确改写公共 ID；`lib.rs` 注册模块；`src/App.tsx` 和 `src/codex-control.ts` 移除手工审核选择并展示自动跟随；产品原理、详细设计、专项说明和本计划同步更新。`managed_gateway.rs` 的同步响应 fixture 补成合法消息输出，使既有 SSE 生命周期断言与 fixture 一致。
- 实际验证：`cargo fmt --all -- --check`、`cargo check --all-targets`、`cargo test --all-targets` 全部通过（250 个 library、1 个 gateway、14 个 launcher、16 个 official-auth 测试）；`pnpm run check` 与 `pnpm run build` 通过。使用本机 `codex-cli 0.154.0-alpha.6.2` 执行 `CODEX_REAL_CATALOG_BIN=/Applications/ChatGPT.app/Contents/Resources/codex cargo test managed_codex_config::tests::installed_codex_accepts_its_version_matched_managed_catalog_when_requested -- --nocapture`，Grok 4.6、deepseek、两个指令入口、自动审核字段和 Profile 均通过真实解析探针。本地浏览器预览确认旧审核模型下拉框已消失，启动入口显示“Responses 模型审核自动跟随”说明。
- 未完成项：尚未向真实收费 Grok/deepseek 上游发送审核请求；DeepSeek、Claude 与未知族暂不声明未经确认的推理阶梯，后续只需扩充同一 profile 数据。
- 阻塞：无。
- 回滚点：移除 `model_family_profile` 调用并恢复目录模板原字段即可回到旧行为；数据库 schema 和旧 `launch_preset_role_models` 字段未删除，回滚不需要数据迁移。
- 下一步命令：配置测试 Provider 后运行 `CODEX_REAL_GROK_CONTRACT_BIN=<codex-path> cargo test managed_gateway::tests::installed_codex_can_call_a_grok_route_through_base_url_and_api_key_when_requested -- --nocapture`，再从默认 App 检查生成目录与一次真实自动审核请求。

### 2026-09-15：三方模型旧会话无法继续的诊断结论

> **待办状态：首版已接入，待持续验收**。完整请求档案和转发边界已修改；仍需使用真实默认 App 会话矩阵对照三方模型旧/新会话字段，确认兼容根因。

- [ ] **待办：三方模型切换后的旧会话兼容性**：在显式完整模式下关联原生会话，保存请求/响应四阶段内容与脱敏摘要，以档案副本完成跨 Provider 受控对照，定位具体不兼容字段，并在可确定时返回 `route_history_incompatible`；验收要求是旧会话、三方新会话和切回原 Provider 的结果可区分，且不删除或静默改写用户历史。

- **现象**：切换到第三方 Grok 4.6 后，某个已有会话的连续请求在 2026-09-14 16:55–17:08 持续返回 `502 upstream_error`；同一路由在 17:42–17:43、19:01–19:02 的新请求随后成功。错误详情为 `https://ai.3rcd.com/v1/responses · Upstream request failed`。
- **已确定**：sidecar 日志对这些失败请求均有 `event=forward`，且 `loopback=0`。请求已经进入 Gateway 并被转发到上游，因此本机回环代理拦截不是这组失败的原因。失败请求体长度约 491 KB；同一路由后续成功请求达到约 599 KB，单纯请求大小上限也不成立。失败与成功请求的工具类型集合相同（`custom`、`function`、`namespace`），工具定义集合本身不足以解释差异。
- **当前实现边界**：Responses API（Codex 使用的响应生成协议）请求会保留大部分 `input` 历史和服务端续接字段；Gateway 当前只删除中继明确不接受的 `metadata`，并按能力过滤可选工具。`previous_response_id`（上游服务端响应的续接指针）不会被自动清除，历史中的工具调用、推理/加密项目、图片和 Provider 私有状态也不会被伪造迁移。Chat 适配器则明确拒绝非空 `previous_response_id` 和 `mcp_servers`。
- **结论等级**：当前证据可以确定“失败发生在上游接收已转发请求之后”，并高度指向“旧会话携带的续接指针或历史项目与目标三方 Provider 的 Responses 合同不兼容”。但现有请求账本只保存长度、哈希、工具类型和脱敏错误摘要，不保存原始正文、顶层字段集合或 `input` 项目类型，所以**尚不能仅凭现有数据断言具体是 `previous_response_id`，也不能断言某一个历史项目字段**。同一工具集合和可变请求大小已经排除了两个较弱解释，但不等于完成字段级归因。
- **本地会话侧补充证据**：本机 Grok 归档中，长历史会话包含多条带 `encrypted_content` 的 `reasoning` 项和工具调用；新建的短 Grok 会话只有普通消息项。由于 Codex 的归档文件不是 Gateway 原始请求，且请求账本当前没有可用 `session_id`，这能解释“旧/新会话形状不同”，但不能把某条归档 item 与 491 KB 失败请求逐一对应。
- **不采用**：不在运行中的 SQLite、rollout 或会话历史上删字段/改 Provider；不把旧会话静默降级到原模型；不把上游 502 重试到另一地址。这些做法会破坏用户历史或掩盖路由边界。

#### 同类工具对照

- **Cockpit Tools**：公开 issue [#2012](https://github.com/jlcodes99/cockpit-tools/issues/2012) 与本问题最接近：从一个模型切换到另一个模型后，旧线程因重放旧模型的 `reasoning` item 而收到 `input[n].content` schema 错误；同一目标模型在新线程正常。其建议是按目标模型做 Responses item 归一化，无法无损转换时用文本摘要开启新 response chain。
- **CC Switch**：公开 issue [#6385](https://github.com/farion1231/cc-switch/issues/6385) 证明“当前 Provider + 旧会话模型名”会让旧线程在切换渠道后失败；[#6629](https://github.com/farion1231/cc-switch/issues/6629) 证明旧线程可因非法 response item id 失败而新线程正常；[#5197](https://github.com/farion1231/cc-switch/issues/5197) 与 [#6260](https://github.com/farion1231/cc-switch/issues/6260) 分别展示损坏的 `function_call.arguments`、空 `thinking` 被写入历史后造成永久 400。共同点是坏 item 会在每次继续时被重放，因而线程持续失败。
- **OpenCodex**：公开 issue [#334](https://github.com/lidge-jun/opencodex/issues/334) 记录本地 `previous_response_id` 状态展开丢失 assistant/reasoning/tool-call 项，导致工具循环；[#702](https://github.com/lidge-jun/opencodex/issues/702) 记录本地续接状态过期后仍继续发送缺失历史的请求；OpenCodeX 同类 issue [#43557](https://github.com/anomalyco/opencode/issues/43557) 则直接指向旧会话中的 `reasoning.encrypted_content` / `itemId` 与 Provider 不兼容。它们都把失败边界放在续接状态或历史 item，而不是新会话的基础模型可用性。

对照结论：本项目应优先检查 `reasoning`（尤其 `encrypted_content`）、`function_call`/`custom_tool_call` 成对关系、item `id`/`status`/`phase`、`previous_response_id` 归属，以及切换后请求中的 `model` 是否已改写为目标上游模型。只有这些字段都通过目标 Provider 的能力合同，才适合继续复用旧历史。

其中，CC Switch #6385 的“旧模型名未改写”在本项目中不是首要假设：Gateway 在能力预检后会把请求的 `model` 明确替换为当前 RouteSet 的 `upstream_model_id`（见 [`managed_gateway.rs`](../src-tauri/src/managed_gateway.rs#L1127-L1130)）。因此仍需优先查历史 item 和续接状态；只有抓到上游实际收到的 model 后，才能完全排除模型名问题。

#### 下一步验证与产品行为

1. 按[完整请求档案设计](designs/conversation-request-archive-design.md)核实原生会话关联、修正转发缺陷并采集四阶段内容；普通诊断继续只保存脱敏形状，完整正文仅进入显式开启的私有档案。
2. 做受控 A/B：同一旧会话原请求重试；切回原 Provider 后用同一提示继续；在目标三方 Provider 新建会话后用同一提示请求。若仅跨 Provider 的旧会话失败，可确认是会话状态/历史兼容边界。若具备脱敏 fixture，再逐项移除续接指针、Provider 私有历史项目并重放，定位具体字段。
3. 能确定不兼容时，在请求到达上游前返回结构化 `route_history_incompatible`（路由历史不兼容）错误，说明需要新会话或切回原模型；只有在具备明确能力合同的字段上才做过滤，禁止静默丢弃历史。

### 2026-09-16：历史 Provider 标识修复已实现

- [x] 显式扫描、预览、自定义目标名称、停机修复、私有备份和恢复已实现；详见[会话 Provider 修复设计](designs/codex-history-provider-migration-design.md)。仅接受环境中已定义或内建的目标名称，不自动创建配置定义。
- 原“Gateway 运行时别名解析”方案已撤销：Codex 在请求进入 Gateway 之前就加载 Provider 定义，网关内部映射无法解决配置加载失败。允许的写入范围为经验证的 Provider 元数据，不修改消息/工具/推理内容。
- 验证沿用专项设计记录：此前 256 项 Rust 测试及前端/浏览器验收通过；本轮仅修正文档状态，没有重复执行或宣称真实旧会话恢复已验证。真实 Codex 恢复与 Windows 实机仍待验，跨 App 同步暂缓。

### 2026-09-17：按会话保存完整请求及转发代码审查

- [x] 完成[实施设计与代码审查](designs/conversation-request-archive-design.md)，同步产品原则、架构与 AI 编程入口。
- [x] 接入独立归档 SQLite、四阶段后台采集、原生会话身份核验、跨分块凭据脱敏、崩溃/清理恢复和对话历史 UI。
- [x] 修正 Responses/Chat SSE 分块、协议终态、转换拒绝和上游错误正文采集；`cargo test --lib conversation_archive` 18 项通过，前端类型检查和生产构建通过。
- [ ] 在真实默认 App 与三方新建/恢复会话上完成字段对照，确认旧会话无法继续的具体根因。
- [x] 确认当前会话关联只 hash 顶层候选 ID，默认 App 会话汇总缺失；计划以独立历史环境 scope + 已验证原生会话 ID 关联，不复用实例外键。
- [x] 审查覆盖四阶段采集点、流式解析/终态、Chat 与同步转流式适配、计时/usage、数据库阻塞和内存上限。最小 Rust 操作探针已确认跨块中文损坏和 CRLF 无法分帧；其他为静态证据，完整回归待实施。
- [x] A：用临时隔离会话/本地模拟上游验证目标 Codex CLI 的原生会话标识来源，冻结 Scope/RequestTrace/ArchivePolicy 合同；默认 App 图形合同仍需实机复验。
- [x] B：修复流式解码、协议完成判断、历史工具项拒绝、同步转流式和有界响应解析；补充成功/失败/取消路径的协议回归。
- [x] C：实现四阶段采集、按会话文件档案、独立 SQLite 索引、有界写入队列、崩溃恢复和清理隔离。保留期自动 prune 与旧摘要/用量异步幂等投影仍待后续。
- [x] D：实现默认 App 的会话/请求列表、阶段详情、阶段对照、未知关联、记录缺失和清理入口；导出与完整统计仍待后续。
- [ ] E：采集受控旧/新会话对照，定位此前 Grok 502 的具体字段；不因发现转发缺陷就宣称旧问题已解决。
- 继续入口：先实施 A/B，再接 C；完整档案是普通无正文诊断之外的显式模式，原生历史不改写，认证凭据不落盘。回滚先停采集并保留档案，不能删除原生历史或修复备份。

### 2026-09-18：第三方远程压缩边界

- 目的：接管“目录改造2（压缩问题）”中发现的第三方压缩失败路径。目录层不再让第三方条目继承 GPT-6 Astra 的 `supports_experimental_context`；Gateway 层不把 Codex 私有 `compaction_trigger` 转发给第三方上游。
- 修改文件：`src-tauri/src/model_family_profile.rs` 删除第三方上下文实验标记并补回归测试；`src-tauri/src/managed_gateway.rs` 增加第三方压缩预检、结构化错误文案和失败路径测试；同步第三方目录、协议兼容、混合路由设计文档。
- 验收：第三方含 `compaction_trigger` 的请求在本地返回 `unsupported_compaction` 且不触发上游；官方路由不被该预检拦截；Grok 目录没有 `supports_experimental_context`，原有身份、推理档位和审核自指仍通过。
- 实际验证：`cargo fmt --all -- --check`、`cargo check --all-targets`、`cargo test --all-targets` 通过（253 个 library、1 个 Gateway、14 个 launcher、16 个 official-auth 测试）；`pnpm run check` 与 `pnpm run build` 通过；本机 `codex-cli 0.154.0-alpha.6.2` 的版本匹配目录解析探针通过。新增 Gateway 集成测试确认第三方压缩请求返回 400 `unsupported_compaction` 且 mock 上游没有收到请求，官方压缩触发项仍通过预检。
- 回滚：删除本节 Gateway 预检和目录字段移除即可恢复上一行为；不触碰原生会话历史、请求档案或凭据。
- 未完成：真实付费上游的压缩合同仍不执行；若未来 Provider 明确支持压缩，应为该 Provider 增加显式能力合同和单独验收，不能仅靠目录字段放行。

### 2026-09-18：目录改造2（压缩问题）会话对照审计

本节把旧任务与当前接管任务的结论合并，作为后续继续入口。审计范围包括旧任务在远程压缩失败前完成的请求形状分析、当前目录 overlay 实现、Gateway 预检，以及随后确认的多 Provider / Codex Provider 边界。

#### 已覆盖

- [x] 确认 `supports_experimental_context` 是 Codex 客户端实验性上下文资格标记，不是第三方 Provider 远程压缩能力开关；第三方目录会删除从 GPT-6 Astra 骨架继承的字段。
- [x] 确认旧失败请求包含 `input` 中的 `compaction_trigger`，普通 Grok 请求可以成功，失败发生在已转发到上游之后；这不是单纯的本机回环地址或请求大小问题。
- [x] Gateway 在第三方路由转发前返回 `unsupported_compaction`，不注入 Provider Key、不发送上游；官方路由不受该预检影响。
- [x] 3R 的 `ApiProvider`、`ModelRoute`、统一 Codex `3rcodex` Provider 和 Gateway 精确路由边界已写入设计文档。
- [x] 新增目录字段移除、第三方拒绝、官方放行和 mock 上游未收到请求的回归测试；完整 Rust 与前端验证结果见上一节。

#### 当前仍有的遗漏

- [x] **隔离实例本地压缩探针（2026-09-18，`codex-cli 0.155.0-alpha.2.6`）**：隔离 `3rcodex` / `name = "3R Codex Gateway"` 在自动压缩时不发 `compaction_trigger`，走客户端本地摘要（普通 `/v1/responses`，可无 tools）。证据见本节后的探针交接。
- [x] **默认 App 与隔离实例的压缩路径分叉（合同）**：用户在启动时用现有双入口选择。隔离 = `local_client`；默认 App = `remote_v2`。不再把「名称门是否仍发 trigger」当作实现阻塞；产品按 Codex 合同视为已确定。详见后文「启动时 CompactMode 实现方案」。
- [x] **目标 Codex 版本探针（默认 App 半边）**：按 2026-09-18 产品决定**跳过**。`name = "OpenAI"` → 远程 v2 为硬合同；隔离半边已用可执行文件证实。实现按该合同落地，不为默认 App 再做 trigger 探针。
- [x] **RouteSet 压缩能力合同（2026-09-19）**：`CapabilitySet.compaction_support` 缺省 `unsupported`；启动写入 `compactMode`；兼容回退标 `native_remote`。无 schema bump。
- [x] **LocalClient 隔离闭环（该 Codex 版本）**：隔离 `3rcodex` 长会话压缩已能结束当前回合且不触发远程 v2。未覆盖真实付费上游质量或默认 App。
- [x] **GatewaySummarizer（2026-09-19 mock 回归）**：仅 `remote_v2` + `gateway_summarizer`。去掉 trigger、空 tools、展开 `3r-compaction-v1:`、缓冲后返回恰好一个 compaction 项。空摘要 / 伪造 GPT / 外源信封 fail-closed；隔离人为 trigger 仍拒绝。未覆盖付费上游。
- [x] **混合目录的模式选择（快照已落地）**：进程级仍只能二选一（双入口）。远程路径上按 RouteSet 分流透传 / Summarizer / fail-closed。没有第三套改名开关。
- [x] **失败回退与幂等（合同）**：一次压缩回合不换路、不重试计费。LocalClient 失败不得升级远程；Summarizer 失败不得改透传或改本地。无需「只回退一次」状态机。
- [ ] **历史兼容**：压缩之外的 `reasoning.encrypted_content`、工具调用/结果配对、item ID/status/phase、`previous_response_id` 和 Provider 私有状态仍未完成字段级跨 Provider 归一化；压缩成功不等于旧会话完全兼容。
- [ ] **SessionHandoff**：尚未实现摘要预览、用户确认、新 response chain 建立和旧会话只读保留的连续性方案；本压缩切片不做。
- [ ] **真实矩阵**：尚未执行付费 Grok/DeepSeek 的真实压缩合同、混合 GPT + 第三方同一实例矩阵，以及 Codex App 图形路径验收。

#### 审计结论

2026-09-19：切片 0–3 已落地。隔离 LocalClient 对该 Codex 版本仍成立；默认 App `remote_v2` 按路由 NativeRemote / 伪造 GPT fail-closed / GatewaySummarizer。旧 binding 缺 `compactMode` 仍 fail-closed，需重新启动或重新应用配置。3R 以**当前安装的 Codex** 为准跟随升级：版本字典 `CompactContract` 今天仍是名称门，未知版本保守不写草案键。下一步是真实 Provider 矩阵，不是再改名称门。SessionHandoff 与默认 App 改名为本地优先仍是独立票。不得把隔离 Provider 显示名改成 `OpenAI`。

### 2026-09-18：同类工具远程压缩对照与方案校正

- 目的：核对 OpenCodex、Cockpit Tools、CC Switch、Codex++ 以及 CPA 压缩桥如何处理三方模型远程压缩，判断当前 fail-closed 是否还要立刻做 GatewaySummarizer。
- 证据等级：公开 issue、changelog、PR 说明和二次检索到的源码摘录；本轮未能拉取 GitHub raw。结论用于校正顺序，不替代目标 Codex 可执行文件探针。
- 修改文件：本计划、混合路由详细设计 §8.2.1、第三方目录 overlay 的压缩边界。

#### Codex 客户端合同（各工具共用前提）

- 远程压缩 v2 不再以 `/responses/compact` 为主路径，而是普通 `/v1/responses` 流，最后一项 `{"type":"compaction_trigger"}`，客户端要求**恰好一个** `{"type":"compaction","encrypted_content":"..."}` 输出，否则致命错误 `expected exactly one compaction output item`。见 [openai/codex#22809](https://github.com/openai/codex/pull/22809)。
- 选远程还是本地，主要看 **Provider 显示名/能力**，不是模型 ID。内建 `name = "OpenAI"`（以及后来的 Azure Responses）走远程；自定义名默认本地。见 [openai/codex#17773](https://github.com/openai/codex/issues/17773)、[PR #17958](https://github.com/openai/codex/pull/17958)。
- `[features] remote_compaction_v2 = false` 在已具备 V2 的 OpenAI 名 Provider 上会退回已退役的 V1 `/responses/compact` 并 404，**不是**改走本地压缩。见 [openai/codex#42468](https://github.com/openai/codex/issues/42468)。
- 3R 当前两条写入路径因此会分叉：
  - 默认 App 兼容模式保留用户的内建 `openai` 身份，只改 `base_url` 到回环 Gateway（见 `managed_codex_config.rs` 的 `compat_requires_builtin_openai_provider`）。历史线程继续认原 Provider，但 Codex 会按 `OpenAI` 发远程压缩。
  - 隔离实例 Profile 写入 `model_provider = "3rcodex"`，`name = "3R Codex Gateway"`。按公开合同应走本地压缩，Gateway 不应再收到 `compaction_trigger`。

#### 各工具实际做法

| 工具 | 做法 | 对 3R 的含义 |
| --- | --- | --- |
| [Cockpit Tools](https://github.com/jlcodes99/cockpit-tools) 1.3.53–1.3.55 | 1.3.53 把托管 Provider 显示名写成 `OpenAI` 以打开官方远程压缩；DeepSeek 无服务端 compact（`/responses/compact` 404，`compaction_trigger` 只回普通 message），长会话失败。1.3.54 对 DeepSeek 池改 `remote_compaction_v2=false` + `token_budget=true`。1.3.55 **撤销 `OpenAI` 名**，三方与经网关的官方账户都回到本地压缩；Grok-only 池强制本地，让**当前模型**做摘要。混合路由设计禁止把 API 路由会话的压缩发到 OAuth/ChatGPT。 | 不要把混合网关命名为 `OpenAI`。3R 隔离实例已经没用这个名字。Cockpit 用 feature 旗标逼本地压缩，在当前 Codex 上可能把 OpenAI 名 Provider 打到 V1 404，复制前必须按版本探针。 |
| [CC Switch](https://github.com/farion1231/cc-switch) v3.16 / [PR #5536](https://github.com/farion1231/cc-switch/pull/5536) | 三方 Provider 可选“Remote Compaction”，打开后把显示名写成 `OpenAI`。Chat 适配器若把 trigger 当普通回合，会得到 0 个 compaction item。PR 检测 v1 `/responses/compact` 与 v2 trigger，去掉工具做摘要，JSON/SSE 都返回恰好一个 compaction；摘要放进 `cc-switch-compaction-v1:` + URL-safe Base64，后续回合还原成 assistant 历史。空摘要或 `finish_reason: length` 拒绝。SSE 先缓冲再一次性发出。 | 这是完整的 GatewaySummarizer。它之所以必要，是因为先把名字改成了 `OpenAI`。3R 不应先抄改名，再被迫合成。若默认 App **必须**保留 OpenAI 名，才需要同类合成。 |
| [OpenCodex](https://github.com/lidge-jun/opencodex/blob/main/src/responses/compaction.ts) | Design B 代理挡在名为 `OpenAI` 的 Provider 前。路由模型不能生成官方加密 blob：无工具摘要 → `ocx1:` + base64(utf8) 写入 `encrypted_content`；回放时解码。真 OpenAI blob 保持不透明，只留短说明。只对规范 ChatGPT 转发做原生透传；API Key `openai-responses` 网关若原样转发 trigger，线程会因 0 个 compaction item 死亡（[#422](https://github.com/lidge-jun/opencodex/issues/422)）。用 OpenCodex 压缩过的线程回到原生 Codex 前要 `ocx recover-history`。 | 合成信封是代理私有状态，不是官方合同。3R 若做 Summarizer，必须用自己的版本化前缀，并说明这些线程不能当官方加密摘要恢复。 |
| [cpa-codex-compact-bridge](https://github.com/patrick-fu/cpa-codex-compact-bridge) | **按模型** `passthrough`（原生 compact）vs `bridge`（普通摘要 → `cpa_compact_*`）。同一 endpoint 混官方 GPT 与 DeepSeek/GLM 时，不能用 Provider 级总开关。未匹配默认透传，不自动 bridge。混入非法原生加密状态则 fail-closed。 | 直接对应 3R 混合 RouteSet。默认 App 一个 OpenAI Provider 挂多种模型时，压缩决策必须按公共模型 ID 在 Gateway 内分流。 |
| [codex-localcompact](https://github.com/JHXSMatthew/codex-localcompact) | 拦截 trigger，调用摘要模型，用 AES-256-GCM 包一层再吐出 compaction item。默认关闭，模型名单需显式加入。 | 加密包装是为了看起来像官方 blob。OpenCodex/CC Switch 用透明信封即可，因为客户端目前只要求字段形状。3R 不要先做无验证的加密剧场。 |
| [Codex++](https://github.com/BigPizzaV3/CodexPlusPlus) | 按渠道/模型写 `model_context_window` 与 `model_auto_compact_token_limit`，生成 `model_catalog_json`。**不**实现远程压缩桥。用户仍会在三方中继上遇到 remote compact 502（[#1414](https://github.com/BigPizzaV3/CodexPlusPlus/issues/1414)）；窗口配置还经常被上游 `/v1/models` 的 258K 元数据盖掉。 | 窗口与自动压缩阈值是互补优化，不能代替协议桥。3R 目前没有为三方条目写入可靠的 `auto_compact_token_limit`。 |

#### 对当前 3R 方案的校正

1. **fail-closed 仍然正确，且不应撤销。** Cockpit 1.3.53、OpenCodex #422、CC Switch #5320 都证明：把 `compaction_trigger` 原样发给 DeepSeek/Grok/Chat 中继，得到的是普通 message 或 502，客户端随后致命失败。3R 在凭据注入前返回 `unsupported_compaction`，比含糊上游错误更安全。
2. **最大的顺序错误是把 LocalClient 当成默认 App 的第一刀。** 隔离实例已经不用 `OpenAI` 名，探针若确认本地压缩可用，长会话应已能在隔离路径继续，无需 Summarizer。默认 App 为了旧线程身份不能改名，所以才会稳定发出远程压缩；这才是 Gateway 预检真正保护的路径。
3. **不要抄 CC Switch/Cockpit 的 `name = "OpenAI"`。** Cockpit 已经回滚；CC Switch 做成 opt-in 后仍必须自建合成器。3R 隔离实例的 `3R Codex Gateway` 是正确的本地压缩信号。
4. **不要在 OpenAI 名 Provider 上写 `remote_compaction_v2 = false` 来“改本地”。** 在当前 Codex 上这会打到退役 V1。Cockpit 1.3.54 的旗标只在特定旧版本上可能有效。
5. **若默认 App 必须无感续接，GatewaySummarizer 是同类工具里唯一验证过的静默方案**，但合同要收紧：按 RouteSet 公共 ID 分流；摘要走当前第三方路由，禁止发到官方账户；返回恰好一个 compaction item；透明版本化信封（建议 `3r-compaction-v1:`），回放时还原为文本；空摘要/截断 fail-closed；SSE 缓冲成一次性完成事件；一次压缩回合不换 Provider、不重试计费。
6. **合成压缩 ≠ 跨 Provider 历史兼容。** 所有对照工具都把 `encrypted_content`、工具成对、`previous_response_id` 留作独立问题。OpenCodex 的信封甚至会阻止线程回到原生 Codex。
7. **低成本互补项：** 为三方目录写入经探针的上下文窗口和 `auto_compact_token_limit`，让本地压缩在正确阈值触发；这不能修复默认 App 的远程 v2。

#### 修订后的下一步

1. ~~隔离 `3rcodex` 是否发出 `compaction_trigger`。~~ 2026-09-18 本机 `codex-cli 0.155.0-alpha.2.6` 已确认不发；隔离 LocalClient 对该版本成立，不再为该路径做 Summarizer。
2. ~~默认 App trigger 探针。~~ 2026-09-18 产品决定名称门无需再验；`name = "OpenAI"` 按合同走远程 v2。实现直接按启动时 CompactMode 方案落地。
3. 实现切片：快照 `compactMode` + 路由 `compaction_support` + `remote_v2` 上的 GatewaySummarizer / 伪造 GPT fail-closed。旧快照与隔离路径保持 fail-closed。
4. 真实 Grok/DeepSeek 矩阵仍在 Summarizer 之后，不能用 mock 代替。

#### 评估：统一 `name = "OpenAI"` + Gateway 按路由分流

用户提出的统一路径：所有入口把 Provider 显示名写成 `OpenAI`，让 Codex 一律发远程压缩 v2；Gateway 再判断当前转发路由是否为 GPT 系列，是则透传，否则对本路由做 GatewaySummarizer。

**结论：协议上可行，且已被 OpenCodex Design B 与 CPA per-model bridge 验证；不能作为 3R 的默认统一方案。** 它只覆盖「已经会发 `compaction_trigger`」的入口（当前主要是默认 App）。把隔离实例也改成 `OpenAI` 名，是主动放弃已更稳的 LocalClient。

同类工具对照：

| 工具 | 是否做过“统一 OpenAI 名 + 网关分流” | 结果 |
| --- | --- | --- |
| OpenCodex Design B | 是。挡在名为 `OpenAI` 的 Provider 前；规范 ChatGPT 转发透传，路由模型本地摘要后塞 `ocx1:` 信封。 | 能续接，但信封是代理私有状态；线程回到原生 Codex 前要 `ocx recover-history`。API Key `openai-responses` 若未分流会因 0 个 compaction item 死亡（[#422](https://github.com/lidge-jun/opencodex/issues/422)）。 |
| CPA compact-bridge | 是。同一 endpoint 上按模型 glob：`passthrough` vs `bridge`。未匹配默认透传。 | 这是混合目录的正确形状。分流键是模型规则，不是“名字像 GPT”。 |
| CC Switch Remote Compaction | 半套。打开后改名为 `OpenAI`，Chat 路径必须自建合成器（[PR #5536](https://github.com/farion1231/cc-switch/pull/5536)）。 | 验证了改名会强制远程 v2；没有按 GPT/非 GPT 自动透传官方合同。 |
| Cockpit 1.3.53 | 试过统一 `OpenAI` 名，**没有** Summarizer。 | DeepSeek/Grok 长会话失败，1.3.55 回滚改名，改走本地压缩。 |

3R 特有约束：

1. **默认 App 已经是这条路径的左半边。** 兼容模式保留内建 `openai` 身份，不能为了压缩再改一次名。这里缺的只是右半边：按 RouteSet 公共 ID 分流透传 vs Summarizer。
2. **隔离实例不要并入。** 受管理任务必须保持 `model_provider_id = 3rcodex`（产品原理 / AGENTS）。改成 `OpenAI` 会让新线程与恢复路径认错 Provider，并迫使所有压缩都走远程 v2 + 合成信封。
3. **“GPT 系列”不能靠 slug/显示名。** RouteSet 里官方订阅是 `RouteSource::Official`（发布门仍关）；API Key 中继上的 `gpt-*` 只是上游模型 ID，通常**没有**官方 `compaction` blob 合同。透传条件必须是：当前路由已验证具备 `NativeRemote`（官方合同探针通过，或显式标记该中继实现了 v2 compact），不是 `upstream_model_id` 含 `gpt`。
4. **透传仍要防串台。** 官方路由透传不得把第三方会话正文或 API Key 带到 ChatGPT；三方 Summarizer 不得把官方 Authorization 注入中继。CPA/Cockpit 都把这条写成硬边界。
5. **合成输出是 3R 私有状态。** 后续普通回合必须能解开自己的信封；官方加密 blob 不得交给三方模型解密。混入无法识别的 `encrypted_content` 应 fail-closed，不要当普通历史转发。
6. **失败不能重试换路。** 一次压缩回合内：命中 NativeRemote 失败不得改打 Grok 摘要；Summarizer 失败不得再透传官方。否则重复计费，且可能把会话内容送到错误上游。

若只做默认 App：可行切片是「保持现有 `OpenAI` 名 + RouteSet `compaction_support` + 按路由透传/Summarizer」，发布默认仍可 fail-closed，Summarizer 作显式能力。若把隔离实例一并改名：拒绝。优先探针隔离 LocalClient 是否已经够用。

#### 混合目录下 Codex 只能按 Provider 名二选一

同类工具通常一个 Codex Provider 对着一个上游（或挡在名为 `OpenAI` 的单一 Provider 前）。它们可以按显示名整段选择：改名 → 客户端本地压缩；保持 `OpenAI` → 代理侧 Summarizer / 透传。

3R 把多个真实 `ApiProvider` 的模型合成一份 Codex 目录，但客户端仍只看见一个活动 Provider（隔离 `3rcodex`，默认 App 内建 `openai`）。Codex 的远程/本地压缩开关在 Provider 显示名上，不在模型 slug 上。因此：

- **客户端层**无法按「当前转发的是 Grok 还是官方 GPT」选择本地压缩或远程 v2；一次启动里整份混合目录同一模式。
- **Gateway 层**只有在客户端已经选出远程 v2（发了 `compaction_trigger`）之后，才能按 RouteSet 分流透传 / Summarizer / fail-closed。Gateway **不能**把已经发来的远程压缩改回客户端本地压缩，也不能给同一 Provider 里的部分模型单独打开本地压缩。
- 隔离路径用非 OpenAI 名，换到的是整份目录的 LocalClient，这是混合目录下唯一能在客户端层避开 `compaction_trigger` 的办法。
- 默认 App 必须保留 `openai` 身份，客户端层锁死远程 v2；这里没有「对 Grok 用本地、对 GPT 用远程」的客户端开关。

综合安全与成本，混合目录的首选是 LocalClient（隔离实例已具备条件）。GatewaySummarizer 只补默认 App 这条无法改名的入口，不是把隔离实例也改成远程再按模型分流。本地压缩仍可能对当前模型发一次普通摘要请求（无 `compaction_trigger`），但没有官方 blob、没有合成信封、失败也不致命；比远程 v2 + 网关造包更安全、通常也更便宜。

混合目录本身不是产品概念错误。产品原理第 4 节从一开始就用专用 `3rcodex` 门面换「一个选择器、多来源、不重启」。有问题的是把「选择器里能同时看到」理解成「每个模型都有一份原生 Provider 合同」。压缩只是这个上限的一个例子；同类上限还有会话 Provider 身份和官方认证开关。正确收敛是承认进程级合同整份目录同一套，请求级合同按 RouteSet 拆，而不是改成「一个渠道一份 Codex Provider」或把隔离实例改名为 `OpenAI`。

#### 评估：按「主用 GPT / 主用三方」让用户选 OpenAI 名或 3rcodex 名

用户建议：混合目录仍然可行；产品上提供选择——主用 GPT 则 Provider 显示名为 `OpenAI`（真 GPT 透传，伪造 `gpt-*` 远程压缩失败时明确提示，不假装成功）；主用其他模型则名为 `3rcodex`，走客户端本地压缩。本地压缩应比三方 Summarizer 更可靠。

**结论：方向对，落点应叠在现有双入口上，不要再做第三套按「主用模型」改名的开关。** OpenCodex 已经验证同一形状：默认 Design B 保持内建 `openai` / 显示名 `OpenAI`，混合目录走远程 v2，路由模型合成 `ocx1:`；`ocx system settings --client-compaction on` 才把新线程改到 `model_provider = "opencodex"`、显示名 `OpenCodex Proxy`，让 Codex 自己本地压。旧 `openai` 线程靠留下的 `openai_base_url` 继续进代理，不会自动改标签。[Design B / client-compaction](https://opencodex.me/guides/codex-integration/)

映射到 3R：

| 用户意图 | 现有入口 | Provider 身份 | 压缩 |
| --- | --- | --- | --- |
| 要默认 App 旧线程 / 官方身份 | 默认 App · 兼容路由 | 内建 `openai`，名必须是 `OpenAI` | 远程 v2；今天 fail-closed；以后按 RouteSet：已验证 `NativeRemote` 才透传 |
| 要混合目录里长会话、主用三方 | 受管理 App · 隔离路由 | `3rcodex` / `3R Codex Gateway` | 本机 `0.155.0-alpha.2.6` 已确认 LocalClient；不要改名为 `OpenAI` |

不要按「主用 GPT」去改隔离实例的显示名或 ID。主用 GPT 若走的是 API Key 中继上的 `gpt-*`，并不是 `NativeRemote`；改名只会让伪造 GPT 也发 `compaction_trigger`。主用官方订阅 GPT 且必须留在默认 App 时，名字本来就是 `OpenAI`，没有可选项。Codex 也没有官方「关远程压缩改本地」开关（[openai/codex#24418](https://github.com/openai/codex/issues/24418) 仍未落地）；`remote_compaction_v2 = false` 会打退役 V1，不是本地。

远程 v2 路径上的分流（仅默认 App 或未来显式 Summarizer 能力）：

1. `RouteSource::Official` 且 compact 探针通过 → 透传。官方混合路由发布门仍关时，这条不存在。
2. 显示名或 slug 像 `gpt-*`、但是 `ApiProvider` 且未验证 `NativeRemote` → **失败关闭**，文案说明「当前模型不是具备官方远程压缩合同的 GPT，不能把 compact 请求发给中继」。不要透传，也不要静默 Summarizer 伪装成功。
3. 用户明确勾选的第三方路由（Grok/DeepSeek/Claude）→ 发布默认仍 fail-closed；若以后打开 Summarizer，这才是摘要对象，且信封必须透明版本化。
4. 一次压缩回合不因失败改路。

本地压缩对比三方 Summarizer：对混合目录里的非官方模型，**更可靠**——摘要格式由 Codex 持有（明文、可移植），没有 `ocx1:` / `3r-compaction-v1:` 锁死，也不会因 0 个 compaction item 致命。代价是：仍可能对当前模型发一轮普通摘要（无 trigger）；官方 GPT 走本地时质量可能不如原生 remote compact；默认 App 选不了这条路。

产品文案建议（实现前先写进入口空态，不改 Provider 名）：隔离入口写「长会话走 Codex 本地压缩，适合主用三方模型」；默认 App 写「保留 OpenAI 身份与旧线程；远程压缩仅官方合同可透传，中继 GPT 名会失败而不是假装压缩成功」。Summarizer 保持显式能力，不因「主用 GPT」自动打开。

#### 评估：统一两入口为「本地优先，不支持再 Gateway 透传/Summarizer」

用户建议：隔离 App 与默认 App 用同一套处理；默认优先本地压缩；Codex 按上下文需要压缩时先本地压；若本地不支持，再由 Gateway 按模型类别尝试透传或 Summarizer，以保证长会话可持续。

**结论：设想作为启动时二选一成立；作为「本地能力探测失败后再 Summarizer」不成立，因为 Codex 没有独立于 Provider 名的本地压缩能力探测。** 旧会话 `model_provider` 改名不是硬墙：Cockpit 会改，3R 已有显式会话修复。默认 App 留 `openai` 是 2026-09-03 兼容策略（`compat_requires_builtin_openai_provider`），不是物理限制；若产品决定默认 App 也走本地优先，可以改名并配合修复，但那是另一张票。

用户本意澄清：不是同一次压缩里先本地再远程重试。而是启动时若 Codex 会走本地压缩，就不需要 Summarizer；只有本地走不成时，Gateway 才对远程 v2 做透传/Summarizer 兜底。

卡点不在「会话认错 Provider」，而在 **Codex 把「支不支持本地压缩」实现成 Provider 显示名，没有第三种探测：**

```text
启动写入
  name ≠ "OpenAI"（隔离 3rcodex 已是）
    → Codex 选用本地压缩路径
    → 长会话压缩时不发 compaction_trigger
    → Gateway 无 compact 合同可兜底，也不需要 Summarizer
    → 本地摘要若打模型，只是普通 /v1/responses

  name = "OpenAI"（默认 App 兼容当前如此）
    → Codex 选用远程 v2，不会再问「能不能本地」
    → 每个压缩都带 compaction_trigger
    → Gateway 看见 trigger = 本地路径已被跳过
    → 这时只能透传 / Summarizer / fail-closed
    → 不能把这次请求改回客户端本地压缩
```

因此「如果本地不支持再 Summarizer」在实现上只能等于：

1. **启动前用名字选择模式**（隔离已经选了本地；默认 App 当前选了远程）。没有 `codex supports_local_compact?` 这种 API。[#24418](https://github.com/openai/codex/issues/24418) 要的官方关远程开关也还没落地；`remote_compaction_v2 = false` 不是本地。
2. **Gateway 看见 `compaction_trigger` 再分派**。这不是「探测到本地失败」，而是「这次启动选了远程」。Summarizer 是远程路径的实现，不是本地路径的后备。

两入口今天已经是这棵树的两片子节点，不是缺一套统一代码：

| 入口 | 启动时选的模式 | 长会话压缩时 | Gateway |
| --- | --- | --- | --- |
| 隔离 `3rcodex` | 本地（名不是 OpenAI） | 客户端压；无 trigger（`0.155.0-alpha.2.6` 已探针） | 当普通请求转发 |
| 默认 App `openai` | 远程 v2（名为 OpenAI） | 必发 trigger | 今天 fail-closed；以后才是透传/Summarizer |

若要把默认 App 也纳入「本地优先」，必须在启动时改显示名（并显式修复旧线程 Provider），让 Codex **不要发 trigger**。不能既保留 `OpenAI` 名（继续发 trigger），又指望「本地不支持才 Summarizer」——名为 OpenAI 时本地路径根本不会被选中，Gateway 收到的每一张 compact 都已经是远程合同。OpenCodex `--client-compaction` 也是启动时改名，不是 compact 到达代理后再决定本地还是摘要。

仍禁止的只有：同一次 `compaction_trigger` 失败后改打本地或换路。那才是做不到、也不该做的运行时回退。

#### `config.toml` 能否在 Provider 名之外切换本地/远程压缩

对照公开配置参考与 Codex issue/PR（[config-reference](https://learn.chatgpt.com/docs/config-file/config-reference)、[#24418](https://github.com/openai/codex/issues/24418)、[#19400](https://github.com/openai/codex/issues/19400)、[#42468](https://github.com/openai/codex/issues/42468)、[#44255](https://github.com/openai/codex/pull/44255)）。**没有受支持的第二开关。** 路径仍由活动 Provider 的显示名（及后来的 Azure Responses / 部分 Bedrock 识别）推断。

| 键 | 作用 | 能否改成本地/远程 |
| --- | --- | --- |
| `model_providers.<id>.name` | 显示名；`name == "OpenAI"`（精确、大小写敏感）走远程 v2 | **这是开关** |
| `model_provider` | 选哪个 Provider id | 间接：换到名为非 OpenAI 的 id 才变本地 |
| `openai_base_url` | 只改内建 openai 的地址 | 否。名字仍是 `OpenAI`，仍远程 |
| `features.remote_compaction` | 已删除（#10840）；OpenAI 强制远程 | 否 |
| `features.remote_compaction_v2` | 已退役（#44255 为 `Stage::Removed`） | **否。** 旧核上 `false` 打退役 V1 `/responses/compact` → 404（#42468），不是本地 |
| `compact_mode = "local"` | 非官方键 | 否。#24418 记录被忽略 |
| `supports_remote_compaction` | 仅 Rust 方法，不是 TOML 字段 | 不能写进 config |
| `compact_prompt` / `experimental_compact_prompt_file` | 覆盖**本地**摘要提示词 | 只影响已走本地的路径；远程 v2 忽略（#34428） |
| `model_auto_compact_token_limit` / `_scope` | **何时**自动压（阈值、计数范围） | 否。不选路径 |
| `features.context_management.experimental_mode` | 实验性上下文管理 | 否。不是 local/remote 开关 |
| `features.compaction_image_budget` | 远程压缩是否保留图片预算 | 否 |
| hooks `PreCompact` / `PostCompact` | 压缩前后钩子 | 否。两种路径都可能跑 |

Azure Responses 后来也走远程（`supports_remote_compaction()` / `is_azure_responses_provider(name, base_url)`，#17958）。自定义 Provider 没有 `supports_remote_compaction` 字段；不像 OpenAI/Azure 的名字默认本地。项目级 `.codex/config.toml` 不能覆盖 `model_providers`。

3R 不要用旗标或伪造 `compact_mode` 冒充本地。隔离路径继续靠 `name = "3R Codex Gateway"`；默认 App 继续靠内建 `OpenAI` 名。阈值项可以写进三方目录以让本地压缩在正确上下文触发，那是何时压，不是哪条路径。

**跟随义务（尚未到来的官方键）：** 上表描述的是 2026-09-18 仍生效的 Codex 合同。若未来版本把 `supports_local_compact`、真正落地的 `compact_mode`、或行为确为「本地而非退役 V1」的 feature 旗标做成受支持配置，3R 必须：

1. 把该键加入目标版本的兼容字典，而不是继续声称「名称是唯一开关」；
2. 启动物化时按用户入口意图写入官方键（隔离 / 希望本地的默认 App 写本地；必须远程的默认 App 不写或写远程）；
3. 用同一可执行文件确认自动压缩路径后，重算 RouteSet `compactMode`——若 OpenAI 名也可以本地，默认 App 允许 `local_client` 且 Gateway 不做 Summarizer；
4. 未探针通过前保持上表行为，不把 issue 里的草案键提前写进用户 `config.toml`。

产品合同见[产品原理 §4 跟随 Codex 官方本地压缩配置](3rcodex-product-principles.md)与[详细设计 §8.2.1](codex-hybrid-routing-design.md)。这是兼容承诺，不是当前切片的实现范围。

- 未完成：默认 App 图形压缩路径未验收；真实付费矩阵未跑。默认 App trigger 探针按产品决定不再做。
- 阻塞：无。
- 回滚：本节只改文档顺序，不改代码。

### 2026-09-18：隔离 `3rcodex` 本地压缩探针

- 目的：验证隔离入口在本机目标 Codex 上是否已经不发 `compaction_trigger`。这是「本地优先」设想的前半句；通过则隔离路径不需要 GatewaySummarizer。
- 范围：只读探针。临时 `CODEX_HOME` / `CODEX_SQLITE_HOME`，mock 上游，不读用户历史，不打付费上游。默认 App 兼容路径不在本切片。
- 修改文件：`src-tauri/src/managed_gateway.rs` 增加 opt-in 测试 `installed_codex_isolated_3rcodex_does_not_emit_compaction_trigger`；同步本计划、混合路由设计 §8.2.1、产品原理、协议兼容计划和第三方目录 overlay。
- 探针方法：物化隔离 Profile（`model_provider=3rcodex`，`name=3R Codex Gateway`）；捕获层挡在 Gateway 之前记录 Codex 发出的 POST；mock `/v1/responses` 回报高 `input_tokens`；第一回合持久化会话后 `exec resume`，用 `model_auto_compact_token_limit=100` 逼出压缩。
- 实际验证：本机 `/Applications/ChatGPT.app/Contents/Resources/codex`（`codex-cli 0.155.0-alpha.2.6`）。

```bash
CODEX_REAL_COMPACT_PROBE_BIN=/Applications/ChatGPT.app/Contents/Resources/codex \
  cargo test --manifest-path src-tauri/Cargo.toml --lib \
  installed_codex_isolated_3rcodex_does_not_emit_compaction_trigger -- --nocapture
```

结果：通过。resume 后 Codex 发出至少三轮普通 `/v1/responses`（含一轮 `tools` 为空的本地摘要），**零** `compaction_trigger`，**零** `/responses/compact`；客户端 JSONL 出现 compaction 提示；Gateway 把摘要当普通三方请求转发；用户默认 `config.toml` 未改。
- 结论：对该 Codex 版本，隔离入口的 LocalClient 半边已落地。不要为隔离路径做 Summarizer，也不要把隔离 Provider 改名为 `OpenAI`。
- 未覆盖：真实付费 Grok/DeepSeek 长会话质量；图形 App 路径。默认 App `name=OpenAI` 按 Codex 合同视为远程 v2，不再探针。
- 回滚：删除该 opt-in 测试即可；不改运行时预检或用户数据。
- 下一步：按后文「启动时 CompactMode 实现方案」写代码；不要再为默认 App 做 trigger 探针。

### 2026-09-18：启动时 CompactMode 实现方案

- 目的：把已锁定的产品选择写成可落地切片。用户在**启动 Codex 时**决定这次进程走不走本地压缩；走本地则 Gateway 不参与 compact 合同；不走本地则远程 v2 由 Gateway 按路由透传 / Summarizer / 失败关闭。不是同一次压缩里探测本地失败再换路。
- 状态：切片 0–3 已实现（2026-09-19）。mock 回归覆盖 Summarizer / 伪造 GPT / 兼容回退透传 / 隔离拒绝。未跑付费上游，未做默认 App 真实 Codex trigger 探针（按产品决定跳过）。
- 依据：[混合路由详细设计 §8.2.1](codex-hybrid-routing-design.md)、产品原理第 4 / 7 节、本节此前的同类工具对照。

#### 产品映射（不再新增第三入口）

用户说的「自己决定启动时是否本地压缩」叠在现有双入口上：

| 用户动作 | 3R 写入 | Codex 选的路径 | Gateway |
| --- | --- | --- | --- |
| 启动隔离 App / 隔离 CLI | `3rcodex` / `3R Codex Gateway` | 本地压缩 | 普通转发；禁止 Summarizer |
| 启动默认 App / 兼容 CLI，或「应用默认 App 配置」 | 保留 `openai` / `OpenAI` | 远程 v2 | NativeRemote / Summarizer / 伪造 GPT fail-closed |
| 单渠道直连 | `3R Codex Direct Provider` | 本地压缩 | 无 Gateway compact 合同 |

入口卡片文案（实现 Summarizer 时一并改 UI，本切片可先做文案）：

- 隔离：「长会话由 Codex 在本地压缩，适合主用三方模型。」
- 默认 App：「保留 OpenAI 身份与旧线程。远程压缩：真实官方模型透传；中继 GPT 名会明确失败；其它三方模型由本机 Gateway 摘要续接。」

不要按「主用 GPT」去改隔离显示名。不要为压缩新增 `launch_presets` 列或第三张入口卡。

物化器为 `compactMode` 预留「目标 Codex 官方本地压缩键」扩展点（版本字典字段即可，不必先建表）。当前切片只实现名称门投影。若日后探针通过，按详细设计「跟随官方本地压缩配置」写入官方键并重算模式，不另开产品入口。当前实现禁止写入 `supports_local_compact`、`compact_mode` 或 `remote_compaction_v2` 这类尚未生效或会打到 V1 的键。

#### 切片 0 — 领域类型与快照（无行为变化以外的默认）

文件：`src-tauri/src/routing.rs`、`managed_launch.rs`、`managed_gateway.rs` 测试夹具。

1. `CapabilitySet` 增加 `compaction_support: CompactionSupport`，serde 缺省 `unsupported`，旧 JSON 可反序列化。取值：`unsupported` / `native_remote` / `gateway_summarizer`。**不改 schema 版本**：能力已在 `capabilities_json` 里，缺字段按 unsupported。
2. `GatewayServiceRevision.config_snapshot` 增加 `compactMode`：`local_client` | `remote_v2`。LaunchService 写入：
   - `launch_target` 为 `managed_app` 或 `cli` → `local_client`；
   - `compat_app` 或 `compat_cli` → `remote_v2`；
   - 直连 Profile 不经 Gateway，快照可省略或标 `local_client`。
3. 物化路由时赋值：
   - 隔离 / 直连：全部 `unsupported`；
   - 默认 App `compatibility_fallback` 白名单：`native_remote`（今天的 `compatibility_fallback_route` 必须改，不能继续当成普通 `ApiProvider`）；
   - 官方路由：发布门关闭期间保持现有 `official_route_unavailable`，能力仍 `unsupported` 或预留 `native_remote` 但不放行；
   - `ApiProvider` 且 `looks_like_gpt_model_id(public 或 upstream)`：`unsupported`；
   - 其余 `ApiProvider`：仅当 `compactMode = remote_v2` 时 `gateway_summarizer`，否则 `unsupported`。
4. `looks_like_gpt_model_id` 只用于 **fail-closed**，从不用于放行 NativeRemote。启发式：slug 以 `gpt-` 开头、或等于常见 ChatGPT 公开名；家族 overlay 已标 Grok/DeepSeek/Claude 的不走这条。
5. 旧 binding 缺 `compactMode`：预检保持今天行为（第三方 trigger → `unsupported_compaction`）。用户重新启动或重新应用配置后才有新合同。

验收：现有 `cargo test` 全绿；新增反序列化测试证明旧 `capabilities_json` 缺字段仍加载。

#### 切片 1 — 预检分派（仍可先不摘要）

文件：`managed_gateway.rs` `preflight_capabilities` 及错误文案。

今天：所有 `ApiProvider` + trigger → `unsupported_compaction`。

改为读取 binding 的 `compactMode` 与路由 `compaction_support`：

| 条件 | 结果 |
| --- | --- |
| 无 trigger | 现有 tools/images 预检 |
| `local_client` + trigger | `unsupported_compaction`（防御） |
| `remote_v2` + `native_remote` | 放行，走现有 forwarder（兼容回退带入站 Authorization） |
| `remote_v2` + `unsupported` 且像 `gpt-*` | `not_native_gpt_compaction` |
| `remote_v2` + `gateway_summarizer` | 切片 1 可仍返回 `unsupported_compaction`，或直接进入切片 2；**不得**把 trigger 原样发给三方 |
| `remote_v2` + `unsupported` 其它 | `unsupported_compaction` |

用户可见文案：

- `unsupported_compaction`：保持现有「第三方模型不接受 Codex 远程压缩请求…」
- `not_native_gpt_compaction`：「当前模型不是具备官方远程压缩合同的 GPT，不能把 compact 请求发给中继。请换真实官方模型、改用隔离环境走本地压缩，或使用非 GPT 名称的三方模型。」
- `incompatible_compaction_envelope`：「会话含有无法在当前渠道解开的压缩信封，请新建会话。」
- `incompatible_reasoning_envelope`：「会话含有无法在当前压缩路径展开的推理密文，请新建会话或改用隔离环境。」不得把 reasoning 密文说成压缩信封。

隔离路径回归：现有 `third_party_compaction_trigger_is_rejected_before_forwarding` 在 `local_client` 快照下继续通过。新增：兼容回退 + trigger 不在预检被拦；伪造 `gpt-*` 第三方返回 `not_native_gpt_compaction` 且 mock 上游零请求。

#### 切片 2 — GatewaySummarizer

新模块建议：`src-tauri/src/managed_gateway/compaction.rs`，由 `forward_provider` 之前的 Responses 处理调用，避免把摘要逻辑堆进已有转发函数。

算法：

1. 确认 `compactMode == remote_v2`、路由 `gateway_summarizer`、input 含 trigger。
2. Clone 请求；去掉所有 `type=compaction_trigger`；`tools = []`；`model` 已是上游 ID。
3. 展开 `3r-compaction-v1:` 信封为文本；外国 **compaction** 信封 → `incompatible_compaction_envelope`，不上游。`type=reasoning` 密文在 Summarizer 里折叠为可见文本（2026-09-20 已按 E1-2 选项 1 锁定 §8.2.1 第 3 条）。普通转发不折叠 reasoning。见[错误待办 E1](designs/2026-09-19-grok-session-error-todos.md)。
4. 调当前路由既有 forwarder（Responses 透传或 Chat 适配）。Summarizer 回合强制非流式收齐（对上游仍可 stream，Gateway 缓冲）。
5. 从上游取出唯一文本摘要；空 / 截断 / 非 2xx → 对应错误，不重试。
6. 构造 Responses 完成事件：恰好一个 compaction 项，`encrypted_content = "3r-compaction-v1:" + urlsafe_base64(utf8_summary)`。若客户端要 SSE，一次性写出 created/output_item/completed，不要把上游 token 流冒充 compact 流。
7. 诊断：`event=compact_summarize`，字段限 `route_id`、`support`、`envelope=v1`、成功/失败码、tools_len、是否 trigger；无正文。

回归（mock 上游，不打付费）：

- Grok 路由 + trigger → 上游收到**无** trigger、**空** tools 的摘要请求；客户端收到恰好一个 compaction 项，前缀 `3r-compaction-v1:`。
- 空摘要 / 截断 → 400，上游可被调用一次，无第二次。
- 伪造 `gpt-4o` API 路由 + trigger → 不上游，`not_native_gpt_compaction`。
- 兼容回退 `gpt-native` + trigger → 透传到原 URL，请求仍含 trigger，响应不经 3R 信封改写。
- 隔离 binding（`local_client`）+ 人为 trigger → 仍 `unsupported_compaction`，不进入 Summarizer。
- 官方 Authorization 不得出现在 Summarizer 出站头；第三方 Key 不得出现在兼容回退出站头。

#### 切片 3 — 入口文案与控制面只读展示

文件：`src/App.tsx` 工作区卡片 copy；可选在实例/预设摘要暴露 `compactMode`（只读）。

不提供运行中切换。切换 = 停掉当前目标，从另一入口启动。默认 App 改本地优先（改名 + 历史修复）明确标为非本切片。

#### 非目标（本方案明确不做）

- 默认 App trigger 探针或任何「验证名称门」测试。
- 把隔离 / 直连 Provider 改名为 `OpenAI`。
- `features.remote_compaction_v2 = false`、伪造 `compact_mode`。
- 同回合重试、换 Provider、换摘要模型、失败后改本地。
- SessionHandoff、字段级旧会话兼容、官方订阅 NativeRemote 发布。
- 为三方目录写 `auto_compact_token_limit`（互补优化，可另开；不阻塞 Summarizer）。
- schema v16；压缩能力走 JSON 缺省即可。
- 把 Summarizer 接到隔离路径「以防万一」。
- 把尚未生效的 `supports_local_compact` / `compact_mode` / `remote_compaction_v2` 写入用户配置。跟随官方键是产品承诺，等目标 Codex 版本探针通过后再做独立切片。

#### 风险与回滚

- 信封是 3R 私有状态：用 Summarizer 压过的默认 App 线程不能当官方 compact blob 回到原生 OpenAI；用户文案必须写明。回放只认 `3r-compaction-v1:`。
- 默认 App 旧 binding 在用户重新应用配置前仍 fail-closed，这是可接受的过渡。
- 回滚：预检恢复「所有 ApiProvider + trigger → unsupported_compaction」；删除 `compaction.rs`；快照多一个未读字段无害。

#### 建议开工顺序

切片 0 → 1 → 2 → 3 已按此顺序落地。伪造 GPT 现为明确 `not_native_gpt_compaction`；切片 2 给默认 App 三方长会话 mock 续接。切片 3 控制面只读 `compactMode` 已投影到 `LaunchPresetSummary`。默认 App 可用显示名开关测两条链（见下节）；付费真实 Provider 矩阵仍不是名称门工作。

### 2026-09-19：跟随最新 Codex 版本字典

- 目的：用户确认压缩合同以**当前安装的 Codex** 为准，3R 随 Codex 升级跟随，而不是维护历史版本矩阵。
- 修改文件：`managed_codex_config.rs` 增加 `CompactContract`（今天仅 `NameGate`）并挂到 `CodexCatalogCompatibility`；启动物化器按该合同选 `compactMode`、把合同传入 Profile 渲染；`NameGate` 不写 `compact_mode` / `supports_local_compact` / `remote_compaction_v2`。`LaunchPresetSummary.compactMode` 只读展示入口投影。
- 验收：未知版本与 `0.150.` 档案均为 `NameGate`；隔离 Profile 不含草案压缩键；兼容入口仍 `remote_v2`，隔离仍 `local_client`。
- 未完成：真实付费压缩矩阵；官方本地压缩键探针通过前不得写新键。日后 Codex 提供生效官方键时，在 `CompactContract` 增加变体、用目标可执行文件探针后再写入。
- 回滚：删除 `CompactContract` 字段并恢复按 `for_launch_target` 硬编码即可；不改 schema。

### 2026-09-19：默认 App「开启本地压缩」显示名开关

- 目的：在默认环境卡片上用开关真实切换本地压缩与远程 v2 / GatewaySummarizer，方便验收两条链。隔离卡不加开关。
- 机制：只改 builtin openai **显示名**（开：`3R Codex Compat Local`；关：显式 `OpenAI`）。`model_provider` id 仍是 `openai`。不修历史、不升 schema、不写草案压缩键。
- 持久化：`launch_presets.profile_options.localCompact`。命令 `set_compat_local_compact` 只 merge JSON 并 bump version，不物化 config。真正生效仍走「应用配置」+ 重启默认 App。`compat_cli` 共用同一预设。
- 修改文件：`managed_codex_config.rs`（NameGate 写 compat 显示名、marker 跟踪 `name`）；`managed_launch.rs`（读 options、fingerprint、setter）；`codex_control` 投影 `localCompact` / `compactMode`；`lib.rs` 注册命令；`App.tsx` 默认卡 checkbox；本计划、详细设计 §8.2.1、产品原理、AGENTS.md。
- 验收（2026-09-19）：`cargo fmt --manifest-path src-tauri/Cargo.toml --all`；`cargo test --lib managed_codex_config::` 36 passed；`cargo test --lib managed_launch::` 28 passed（含 setter 拒绝隔离、JSON merge、opt-in 后 snapshot `local_client` / 显示名 `3R Codex Compat Local`）；`cargo test --lib compact_` 5 passed；`pnpm run check` 通过。手动：开 → 应用 → 看 `~/.codex/config.toml` 的 name + marker → 重启 → 长会话应无 `compaction_trigger`；关 → 应用 → 重启 → Summarizer/远程 v2。
- 未完成：付费矩阵；把默认 App 永久改成本地优先（改 id + 历史修复）仍是独立票。
- 回滚：去掉 setter/UI；compat 投影始终写 `OpenAI`；删除 `profile_options.localCompact` 键即可，不改 schema。

## 2026-08-31 Rust 模块化重构

- 目标：在不改变 Tauri 命令、数据库结构、Gateway 协议和启动行为的前提下，按职责拆分 Rust 大文件，便于维护和学习。
- 策略：先在同一个 crate 内进行模块化，不提前引入 Cargo workspace 或跨 crate API。
- 顺序：`managed_gateway` → `managed_launch` → `codex_control` → `providers` / `official_auth` / `official_accounts` / `database`。
- 验收：每个模块拆分后执行 `cargo fmt --all -- --check`、`cargo check --all-targets` 和 `cargo test -q`；只接受行为等价的结构变更。
- 当前进度：`managed_gateway` 已将请求诊断、脱敏摘要和用量记录抽离到 `managed_gateway/diagnostics.rs`，将状态、绑定、凭据、运行时容器及 Sidecar 状态抽离到 `managed_gateway/runtime_types.rs`，并将 Sidecar 控制客户端生命周期抽离到 `managed_gateway/sidecar.rs`；`managed_launch` 已将 Store 容器、启动方案输入/摘要、准备结果、Provider 快照、方案校验/资源可用性检查以及数据库依赖写入/查询辅助函数抽离到 `managed_launch/models.rs`、`managed_launch/preset_validation.rs` 与 `managed_launch/persistence.rs`；`codex_control` 已将控制面快照、账户/Provider/实例摘要、启动输入和准备结果抽离到 `codex_control/models.rs`，其余职责保持原入口不变。

## 目标

- 用户结果：从 3R Codex 选择一个可选官方账户、多个 API Provider 和模型后启动 Codex CLI；同一模型选择器显示固化模型集合，并按模型使用正确凭据。
- 首个交付：纯 API Key 多 Provider 的端到端 CLI 路径，以及官方登录 + 第三方 API Key 路径的实验能力。
- 条件交付：官方订阅 GPT 经过 Gateway 的路径，仅在真实兼容探针通过后启用。
- 范围边界：本执行计划首期实现受管理 App 启动骨架与默认 App 兼容入口；不规划 Codex App 多开。账号池、LAN、公网和运行期热更新同样不在范围内。默认 App 兼容模式允许对原始 Provider 目录中未被选中的原生模型做受限本地 Gateway 回退，以保持单 Provider 配置下的模型可用性；这不是 Provider 或账号的自动切换。App 启动目标的单独预研见 [Codex App 支持预研](codex-app-support-research.md)。

### 2026-09-10：默认模型与第三方渠道的地址边界修复

- 目标：第三方请求只使用所选渠道修订的 URL 与 Key；默认模型使用接入前的有效地址和当前请求认证。裸域名与 `/v1` 形式都能访问标准 OpenAI-compatible 端点。
- 已核对：当前 Grok 4.6 路由绑定 uuAI 的 `https://uuapi.io/v1`；旧 reset-backup 中的 Grok/ai.3rcd.com 记录不能证明本次请求的目的地址。当前请求账本为空，尚不能声称复现真实上游 502。
- 修复边界：先在内存中按 marker 还原受管理字段，再选择原始 Provider 地址与原始目录；不得把上一轮生成目录中的第三方路由当成原生模型。Gateway 的精确公共 ID 匹配与渠道凭据隔离保持不变；不按模糊模型名切换渠道。
- URL 合同：无路径或仅 `/` 的 API Base URL 规范化为 `/v1`；已有 `/v1` 与自定义非空路径原样保留（去尾斜杠），然后追加协议端点。不因 401/502 重发到另一地址，也不改写用户原始配置。
- [x] 在配置读取与目录生成边界修复重复应用的来源污染、原始地址选择顺序与回环自转发。
- [x] 统一受管理渠道与默认模型的 Base URL 处理。
- [ ] 用不同本地上游验证第三方/默认模型各自地址与认证，覆盖重复应用、移除模型、目录残留、空路径与自定义路径。
- [x] 运行相关 Rust 回归与编译检查，并同步实际验证结果（网络测试仍受环境阻断，见下方）。
- 当前状态：实现完成；用户已明确批准本地网络回归，但执行审批服务持续超时/限流，网络验证仍被环境阻断。真实上游验收待用户测试。未修改用户本机配置、认证或渠道数据库。
- 验证：目录污染回归在修复前失败、修复后通过。`cargo test --all-targets` 在沙箱内为 library 202/217 通过，剩余 15 项全部在监听回环端口时被 `Operation not permitted` 阻断；另行执行 3 个 binary target 的 15 项测试全部通过。两次沙箱外运行请求分别遇到审批超时和审批服务 429 限流，尚未执行。
- 验证：`cargo fmt --all -- --check`、`cargo check --all-targets` 与 `pnpm run tauri:build --bundles app`（含前端类型检查/生产构建）通过。已生成 `src-tauri/target/release/bundle/macos/3R Codex.app`，对生成包做本地 ad-hoc 签名后 `codesign --verify --deep --strict` 通过；不是公证发布包。
- 用户批准后的验证：再次请求全量测试，以及缩小为 `cargo test --lib managed_gateway::tests::compatibility_routes_keep_channel_urls_and_credentials_separate -- --exact` 的单项本地测试，均被自动审批超时拒绝，命令未启动；不将这些请求计为测试通过。授权已提供，无需再次向用户请求相同权限。
- 下一步：执行环境恢复后运行 `cd src-tauri && cargo test --all-targets`，尤其确认双上游地址/凭据隔离测试；随后打开本次构建，应用默认 App 配置并由用户重启默认 App，分别测试原始模型与 uuAI/Grok。真实 502 根因需新请求账本佐证，不再引用旧 reset-backup 记录推断当前路由。

### 2026-09-03：运行目标优先的主工作区交互

### 2026-09-03：双入口独立配置与渠道保留重置

> 旧版“两个入口共享同一份工作区配置”的交互决策已由上方“双入口独立配置与渠道保留重置”取代；后端 Provider、Gateway 和 RouteSet 边界保持不变。

## Decision

- Change: 产品只保留两个固定启动入口：`默认 App · 兼容路由` 与 `受管理 App · 隔离路由`。两者各自拥有一份入口配置，可分别设置名称、工作目录、传输策略、已加入的模型渠道和模型；不再把一份主工作区配置同时套用到两个入口。
- Contract or boundary: Provider（模型渠道）是全局资源，入口配置只保存对 Provider/模型的引用和本入口的运行参数。启动时仍为每个入口生成独立不可变 RouteSet；运行中的实例继续使用启动快照。
- Reset contract: 重置保留 API Key Provider、Provider 修订、模型目录快照、连接超时设置、连接检测结果以及系统凭据库中的 secret reference；清除官方账户与授权、入口配置、路由/启动快照、会话摘要、请求/网关诊断、用量/余额、实例生命周期记录、受管理 App 数据目录和兼容接管状态。
- State or consistency model: 重置是一次性、事务化、可恢复的本地操作；执行前生成带时间戳的本地备份（不导出 Keychain 密钥原文）。重置后两个入口均为“尚未配置”，必须分别完成配置才能启动。
- Failure behavior: 有活动/未知实例、兼容配置无法安全恢复、数据库事务或目录迁移失败时拒绝重置并保留原数据；不做部分删除。
- Rejected options: 不复制 Provider/API Key，不同步两个入口的会话数据库，不用软链共享 App 状态；共享渠道引用但独立入口配置，避免会话和路由状态漂移。
- Verification: Schema/迁移测试、重置事务保留/删除表断言、空状态 UI 两入口分别配置与启动、兼容配置恢复和失败回滚检查。

### 2026-09-03：Codex 版本检查与升级建议

## Decision

- Change: 启动时检查本机 Codex CLI（命令行程序）的可执行文件与版本；在设置/启动入口提供“当前版本、最新稳定版本、官方变更摘要和发布说明”卡片。官方桌面 App 版本单独读取安装包元数据，不与 CLI 版本混比。
- Contract or boundary: 3R Codex 只读取官方公开发布源（GitHub Releases API），不上传用户数据、不读取认证/会话内容，不自动替换 Codex 可执行文件。
- Owner and write path: Rust 控制面负责 CLI 版本解析、官方请求、响应校验与短期缓存；官方请求使用 GitHub Releases 的 `/releases/latest` 小响应端点，降低大响应在慢网络/代理下被截断的概率；renderer 只展示状态并提供刷新/打开发布页操作。
- State or consistency model: 更新检查是 best-effort（尽力而为）；网络失败、限流或发布源结构变化不影响渠道管理、入口配置和已有 Codex 启动。
- Failure behavior: 无网络时展示最近一次成功结果或“暂时无法检查”；只接受版本号、发布日期、摘要和官方 URL，不显示未经校验的远程任意内容。
- Rejected options: 不强制升级、不静默下载、不自动修改用户安装目录；后续如增加安装，必须另设用户确认和签名校验流程。
- Verification: 单元测试覆盖版本比较、非法响应、缓存过期；浏览器检查更新卡片的加载/失败/已是最新/有新版本状态。

### 2026-09-03：主工作区产品版本与认证摘要

## Decision

- Change: 主工作区固定展示 Codex CLI 与 Codex App 两块；CLI 提供直接启动，App 在卡片内选择默认 App 或受管理 App。两者共用的本机认证环境以脱敏摘要展示。
- Contract or boundary: 只读取本机 Codex 可执行文件版本、桌面 App 包元数据和认证状态摘要；不读取或展示 access token、refresh token、API Key 或会话正文。
- Owner and write path: Rust 控制面读取并序列化版本/认证摘要，renderer 仅展示；认证数据仍由 Codex 官方运行时维护。
- State or consistency model: 摘要是启动时本地快照，刷新后重新读取；读取失败不阻塞 CLI/App 启动。
- Failure behavior: 找不到认证文件或字段时显示“未检测到/信息不可用”，不把失败解释为登出，也不触碰认证文件。
- Rejected options: 不复制认证信息到 3R 数据库，不在工作区提供修改或退出官方账户操作。

### 2026-09-03：主工作区按运行环境组织

## Decision

- Change: 主工作区以“默认环境”和“隔离环境”为一级分区；默认环境提供 Codex CLI 与默认 App，隔离环境提供受管理 App，并预留高级入口启动隔离 CLI。CLI/App 的本地版本信息和共用认证摘要置于分区上方。
- Contract or boundary: 默认环境继续使用本机默认 Codex 配置、认证和历史；隔离环境使用独立 CODEX_HOME 与路由快照，不复制默认认证/会话数据。现有 `launchTarget` 命令含义不变。
- Owner and write path: 每个环境对应一个工作区入口配置；模型渠道仍是全局资产，环境只维护模型选择。旧版传输方式和工作目录字段仍由后端兼容读取，但新建/编辑入口统一写入 Gateway，CLI 工作目录改为启动时按次填写。启动动作选择目标，不创建额外配置副本。
- State or consistency model: 默认 CLI 与默认 App 读取同一入口配置；隔离 CLI/App 读取同一隔离入口配置。运行中的实例继续使用启动时快照。
- Failure behavior: 未配置环境或没有模型时禁用对应启动动作；活动实例锁定环境时禁止编辑和移除渠道；隔离 CLI 作为高级动作展示，不影响默认环境。
- Rejected options: 不将 CLI、默认 App、隔离 App 做成三个平级配置块，避免把产品类型与配置边界混淆；不为 CLI 和默认 App 保存两份默认配置。

### 2026-09-03：默认环境兼容路由保留原生 provider

## Decision

- Change: 兼容模式不再把默认 `model_provider` 替换为 `3rcodex`；保留原生 `openai`（含用户原有大小写）身份，仅注入 Gateway 地址、模型目录和可撤销的实例 Header。默认 CLI 与默认 App 共用该兼容投影，隔离 CLI 继续使用独立 Profile。
- Contract or boundary: 历史线程继续按原 provider 标识解析；第三方模型通过模型目录中的路由 ID 进入 Gateway，由 Gateway 决定真实 Provider，不伪造新的默认 provider。
- Owner and write path: `materialize_managed_app_config_for_target` 只更新兼容所需根键和原生 provider 的 Header，生成配置仍由 3R 控制面持有；退出时按 marker 哈希恢复原文件。
- State or consistency model: 兼容接管期间默认 CLI/App 共享同一临时 Gateway 路由（`compat_cli`/`compat_app`）；Gateway 停止或配置冲突时请求失败而不覆盖用户修改。
- Failure behavior: 已选择非原生 `openai` provider、用户自定义 `openai_base_url` 与 Gateway 冲突或 marker 哈希不匹配时拒绝启动并要求恢复/人工处理。
- Verification: 兼容配置测试断言原 `model_provider` 保留、`openai_base_url` 与实例 Header 注入、历史文件不变、恢复后字节级还原；完整 Rust 测试继续通过。

## Decision

- Change: 主工作区首屏由“先展示模型渠道、再展示启动按钮”调整为两个同级运行目标：`默认 App · 兼容路由` 与 `受管理 App · 隔离路由`。每个目标卡片内提供“配置模型渠道”和对应启动动作；CLI 保留为次级入口。
- Contract or boundary: 这只是 renderer（桌面界面）信息架构调整。`LaunchPreset`、Provider、模型、RouteSet、Gateway、配置锁和 `launch_codex` IPC（桌面界面与 Rust 后端之间的命令接口）不新增字段、不改变含义。
- Owner and write path: 仍由唯一主工作区的编辑器保存 API Key 渠道、模型和传输策略；启动时 `ManagedLaunchStore` 生成本次不可变 RouteSet。两个 App 入口只向同一 `launch_codex` 写入不同 `launchTarget`（`compat_app` 或 `managed_app`）。
- State or consistency model: 两个入口共享同一份已保存工作区配置，且对下一次启动一致可见；已运行实例继续持有启动时快照，不受之后的界面编辑影响。
- Failure behavior: 没有模型、传输策略不支持 App、兼容性探针不可用或工作区被活动实例锁定时，在对应目标卡片禁用操作并说明原因；不会降级为修改默认 App，也不会暗中切换 Provider。
- Rejected options: 不为默认 App 与受管理 App 分别持久化 Provider 副本，也不把 Provider 编辑页变成启动首页；前者会制造状态漂移，后者仍要求用户先理解实现概念。
- Verification: TypeScript 检查与生产构建；`ui-fixture=1` 验证两个入口、无渠道空状态、禁用状态及窄屏布局；确认启动调用仍传递既有 `launchTarget`。

### 2026-09-01：默认 App 兼容模式与安全模式历史边界

- 受管理 App 不再同步、链接、复制或修复默认 App 的会话、归档、附件、线程/目标/记忆索引；只可一次性种子化白名单内的非敏感偏好。跨模式历史不能承诺继续对话。
- 新增 `compat_app` 启动/路由模式。它以字段级 marker 接管默认 `CODEX_HOME/config.toml`，让默认 App 保留原生历史；`auth.json`、Electron 用户数据和历史文件始终不写入。后端、启动器和前端入口已接入并通过自动化验证；真实签名桌面验收属于发布前环境验收。
- 混合路由状态显式暴露为 `official_hybrid_available=false`：方案可以保存官方账户与 API Provider 的模型来源映射，但在官方凭据代理合同通过前，所有启动目标均返回 `official_hybrid_route_experimental_disabled`，不得伪造官方 Authorization 或自动回退到 API Provider。
- 兼容配置在用户显式停用前持续生效，直接重新打开默认 App 时仍使用已选择 Provider，语义接近 CC Switch 的活动配置，不是一次性环境注入。
- 兼容 Gateway 的生命周期与 3R Codex 进程一致，不作为常驻后台服务。3R Codex 退出、崩溃或机器重启后，默认 App 的后续请求失败是预期行为；重新打开 3R Codex 后从兼容绑定和系统凭据库重新注册路由。该恢复使用固定的当前用户回环端口；端口不可绑定时保持路由不可用。Gateway 不得持久化明文 Provider API Key；只从系统凭据库读取。默认 `config.toml` 中若必须存在准入值，只允许独立可撤销的本地准入密钥。
- 主动退出 3R Codex 时，若有受管理 App 或兼容模式，应显示影响提示并阻止退出；用户先停止实例后才允许退出，依赖 Gateway 的新请求随后会失败。停用与恢复要求默认 App/CLI 已停止；配置冲突、未知 App 状态或哈希不匹配一律进入 `orphaned`，不自动覆盖。退出、崩溃、进程重开、固定端口占用、手动停用和异常恢复均是发布门。

### 2026-08-29：显式自动审核模型（历史方案）

- 运行方案可把一个已选择的普通 API 模型指定为“自动审核模型”；不创建、展示或转发 `codex-auto-review` / `gpt-code-review` 等内部角色 ID。
- 未指定时保持当前行为：受管理配置继续移除继承的审核设置，不隐式猜测或回退模型。
- 指定后，版本匹配目录中每个可选主模型的 `auto_review_model_override` 都指向该公共路由 ID。受管理 Profile 不写 `approvals_reviewer`。
- 自动审核角色只接受 Responses 传输的 API 模型。为保证精确路由、凭据隔离和请求诊断，启用该角色的方案始终经过实例 Gateway，即使方案只有一个 Provider。
- 角色绑定与模型选择在同一事务保存；角色只能引用同一运行方案已经选择的模型。删除或变更模型时不得留下悬空角色。
- 目标 Codex 版本若不接受目录字段或 Profile 开关，启动前兼容性验证失败并返回明确错误，不静默改用官方内部模型或当前模型。

2026-09-17 起改为按当前对话模型自动跟随：保留旧字段用于数据库和 IPC 向后兼容，但产品 UI 不再让用户指定审核模型；生成目录以条目自身 ID 覆盖旧的统一目标。

验收状态：持久化、物化、启动路由、UI 与迁移测试已完成。2026-08-29 使用本机 `codex-cli 0.150.0-alpha.8` 在临时隔离目录验证：自定义目录可以加载 `auto_review_model_override`，且同时设置 `approvals_reviewer = "auto_review"` 能通过配置解析；探针未登录、未发送模型请求。

### 最终特殊模型方案（2026-08-29）

- 特殊模型不进入普通模型选择器、不进入受管理实例公开 `/models`；控制面展示其调用者（例如自动审核、子代理编排）和当前状态。
- 普通模型使用 Provider `selectedModels` 白名单与用户 `disabledModels` 黑名单；特殊模型由目标版本目录/探针分类器强制隐藏，不能被用户重新启用为普通路由。
- 角色替代是独立的版本化合同。只有探针确认可替代并满足能力要求的角色才显示替代入口；替代模型必须是同一运行方案已选普通模型。
- 目标版本缺少原生特殊模型时，若无显式替代则该角色保持未启用；有显式替代则通过公共路由 ID 经 Gateway 调用。任何角色失败均不回退到当前主模型或其他 Provider。
- `/models` 只返回普通路由；角色信息通过控制面 IPC 返回，避免将内部 Codex ID 泄露给用户选择器或第三方上游。

### Codex 版本字段兼容配置（2026-08-29）

- 生成器维护小型版本能力配置（例如 `visibility` 隐藏值及可选字段支持），不复制一份固定模型目录。
- 目标 Codex 的 bundled catalog 仍是字段来源；未知版本从本地模板推断字段能力，并使用保守默认值。
- 启动时把实际 Codex 版本传入生成器，随后继续使用同一可执行文件执行目录、模型列表和 Profile 探针。
- 基础目录探针失败时阻止启动；特殊角色合同探针失败时应独立关闭角色替代，不得静默回退到其他模型。
- 2026-08-29 修复：当前 Codex `0.150.x` 的隐藏枚举为 `hide`，不再生成不兼容的 `hidden` 枚举；并按模板决定是否写入 `hidden`、`supported_in_api`、`auto_review_model_override` 字段。目标版本不支持自动审核覆盖字段时，显式返回不支持错误。

## 硬性验收

- 直接 CLI、直接 App 和默认 `~/.codex` 不变；
- v6 受管理启动全部通过固定 `3rcodex` Provider 和共享 Gateway；
- Provider API Key 不进入 Codex 子进程；实例密钥不进入 UI 或 SQLite 明文；
- 第三方 upstream 永不收到官方 Authorization 或实例密钥；
- 官方 upstream 永不收到 Provider Key；
- 一个实例只解析一个不可变 RouteSet；
- 多 Provider 实例的每一条请求都记录实际 Provider 修订、Token 用量、时间、状态和时延；
- 活动依赖不能修改、替换、重授权或删除；
- 无模型、错模型、能力不兼容和凭据失效均不回退；
- 旧 v5 数据可读，旧预设因无模型选择而明确提示补充，不猜测；
- 新建、恢复、回滚和重启使用同一 Provider ID 与公共模型 ID。

## 工作分解

| Slice | 交付 | 文件所有权 | 依赖 | 状态 |
| --- | --- | --- | --- | --- |
| P0 | 真实 Codex custom-provider 兼容探针与版本门 | `docs/`, test harness | 无 | 进行中；无凭据机械探针可自动化，真实登录待人工 |
| A | schema v6、routing domain、RouteStore、资源种类与迁移测试 | `database.rs`, `resource_guard.rs`, 新 `routing.rs`, `route_store.rs` | 设计冻结 | 完成 |
| B | 实例密钥、launcher 参数、不可变 Profile/目录物化、现有配置安全快照 | `secrets.rs`, `launcher_protocol.rs`, `bin/codex-launcher.rs`, 新 `managed_codex_config.rs` | A 类型合同 | 完成；真实 Codex Profile/目录探针通过 |
| C | 多 RouteSet Gateway、逐路由 Header allowlist、Provider 修订读取 | 新 `managed_gateway.rs`, `providers.rs` | A 类型合同 | API Provider 路径完成；官方 forwarder 保持关闭 |
| D | LaunchService 集成、补偿、生命周期、Tauri IPC | 新 `managed_launch.rs`, `codex_control.rs`, `lib.rs` | A/B/C | CLI/隔离 App/默认 App 路由垂直链路已接入；配置恢复、退出确认和真实桌面验收待续 |
| E | 可选账户、多 Provider、模型选择 UI 与快照适配 | `src/codex-control.ts`, `src/App.tsx`, `src/styles.css` | IPC 冻结 | 完成：完整 `/models` 目录选择与公共显示名映射 |
| V | 迁移、安全、并发、CLI、UI、真实账户验收 | tests/docs | A-E | 自动化与浏览器 smoke 完成；真实 Codex CLI 已通过模拟 Responses 与 Chat-only Grok 上游完整契约，真实用户上游/官方账户待人工 |

共享入口 `lib.rs`、Cargo 依赖和最终集成只由主代理修改。并行执行者不得同时编辑 schema、共享 IPC 类型或同一文件。

## Phase 0：可行性门

- [x] OpenAI Docs 校准自定义 Provider、环境 Header、模型目录和 app-server 账户合同。
- [x] 现有代码与参考实现审查。
- [x] Rust 基线测试：136 个库测试、7 个 launcher 测试、15 个 official-auth 集成测试通过；真实 Codex Grok 契约另有 Responses/Chat-only 两条测试。
- [x] 无真实账户 fixture：验证实例 Header、Provider Header 隔离、精确路由、模型改写和官方 route gate。
- [ ] 受控真实账户：验证官方认证是否到达 custom provider。
- [ ] 受控真实账户：验证官方订阅模型固定上游合同；未通过则保持官方路由关闭。

## Slice A：持久化与纯领域

- [x] `CURRENT_SCHEMA_VERSION` 已升级到 16，并完成运行方案角色绑定、启动方案逻辑删除、启动入口字段、受管理 App 方案—实例稳定绑定、默认 App 兼容 binding、请求错误摘要，以及工作区模型选择行的可空窗口覆盖；
- [x] schema v8 为请求级诊断增加可空输入/输出/总 Token 字段，并覆盖旧库迁移；
- [x] 新增 composition、model identity/catalog、RouteSet、model routes、binding 表；
- [x] 扩展资源 kind CHECK；
- [x] v5 回填为 `needs_model_selection`；
- [x] 新 `routing.rs` 定义并验证 RouteKind、CapabilitySet、ModelRoute、RouteSet；
- [x] 新 `route_store.rs` 提供 ready RouteSet 写入/读取；
- [x] 测试全新/v4/v5/失败回滚/重复迁移/future version；
- [x] 测试公共 ID、精确路由、来源归属和非法状态。

验收命令：

```bash
cd src-tauri
cargo test database::tests
cargo test resource_guard::tests
cargo test routing::tests
cargo test route_store::tests
```

## Slice B：实例密钥与配置

- [x] 每实例 Keychain reference 和高熵密钥；
- [x] 使用 SHA-256 摘要与常量时间比较；
- [x] launcher 改用 `--instance-key-ref`；
- [x] 子进程只设置 `THREER_INSTANCE_KEY`，移除所有继承 API Key；
- [x] 固定 `3rcodex` 配置、auth flag 和命名 Profile；
- [x] 按目标 Codex 版本克隆 bundled catalog，并原子写目录与 Profile；
- [x] 配置、命令行、回执和目录不承载 Provider secret；UI/IPC 不再返回本地 Client Key。

## Slice C：Gateway

- [x] 一个 listener + 多 active binding registry；
- [x] `X-3R-Instance-Key` 准入；
- [x] exact public model lookup 和 request/response model rewrite；
- [x] `/models` 使用固化 RouteSet；
- [x] Provider revision 固定读取，禁止 current 漂移；
- [x] 第三方从空 HeaderMap 构造，只注入固定 Provider Key；
- [x] 官方 route 默认返回稳定不可用错误，不猜测私有上游；
- [x] 两实例、错 Key、错模型、无回退和 Header 隔离测试。
- [x] 实例请求账本采集 Responses/Chat 的 usage；未知 usage 保持为空，不按字节数估算；Provider 汇总继续独立写入。

## Slice D：启动与生命周期

- [x] v6 API-only `prepare_launch` 接入统一 `launch_codex`；官方组合由显式 gate 阻止；
- [ ] 完整 materializing/ready/failed、operation lease 与崩溃恢复；当前保留无实例引用的不可变 RouteSet 作为安全残留；
- [x] 共享 Gateway register/unregister 已完成；独立 `codex-gateway` sidecar、loopback 控制面、健康复用和启动时 active binding restore 已完成；流式响应中断后的续接仍不支持；
- [x] 活动依赖资源锁；
- [ ] official credential 单活动实例数据库保护；
- [x] exited/failed 撤销 binding 和 Key；orphaned/degraded 保留；
- [x] 移除新建路径的 manual Gateway Provider 绑定和全部 UI/IPC client key 返回；
- [x] 旧 v5 预设保留并明确标记需要模型选择。

## Slice E：前端

- [x] 官方账户选择改为可选；
- [x] API Provider 改为多选；
- [x] 按来源手工声明上游模型 ID、公共模型 ID 和显示名；
- [x] Provider `/models` 完整目录读取、修订级快照和 UI 勾选/映射；
- [ ] Provider 模型能力编辑和目录陈旧状态展示；
- [x] 预设列表展示来源组合和公共模型摘要；
- [ ] 活动实例显示 RouteSet 修订和模型数；
- [x] 删除 Client Key 显示/复制/状态与 IPC；
- [x] 后端使用活动依赖资源锁；前端 mutation 使用资源级 pending；
- [x] TypeScript、生产构建、桌面与 430px 窄屏 smoke 通过；数据态多模型抽屉仍需原生 Tauri 验收。

## 最终验证矩阵

- Rust：fmt、check、all-targets、迁移、route domain、Gateway、launcher、official auth；
- Frontend：TypeScript check、production build、真实浏览器 happy/error path；
- Security：官方/Provider/实例三种 sentinel 全链路扫描；
- Isolation：两个 Provider、两个实例、两个 RouteSet；
- Lifecycle：启动、退出、失败、orphaned、degraded、Gateway restart；
- Codex：新建、连续对话、恢复、回滚、模型切换；
- Default safety：默认目录摘要和直接启动环境不变；
- External：真实官方账户与条款门。

## 活动状态

- 当前：纯 API Key 多 Provider 从预设、RouteSet、Profile、实例密钥、Gateway 到 launcher 的垂直链路已落地；官方组合可保存但启动保持实验关闭。
- 已验证：全量 Rust 测试、前端检查/构建、真实本机 Codex bundled catalog + 命名 Profile 探针、浏览器桌面/窄屏 smoke。
- 待续：官方订阅模型透传、托盘入口，以及真实用户 DeepSeek/Grok 上游会话。
- 当前 Codex bundled catalog 暴露隐藏的 `codex-auto-review`（旧讨论中的 `gpt-code-review` 未在公开模型目录中确认）；API Key 受管理 Profile 不继承 `review_model` 或 `[agents]` 下的子代理模型设置。第三方 Responses 路由保留 `auto_review_model_override`，但不写 `approvals_reviewer`；沙箱提权询问。普通子代理和 `/review` 继续沿用当前公共模型。
- 模型目录实现：`codex-auto-review`、`gpt-code-review` 及同类审核角色 ID 不进入用户可选模型列表；历史路由在受管理 App 的 catalog 中标记为 hidden / unsupported in API。每个第三方 Responses 条目把自身公共路由 ID 写入 `auto_review_model_override`，不伪造或转发内部角色 ID。
- 下一步：真实中转站进行新建/连续/恢复/回滚；补齐托盘入口；最后执行双官方测试账户探针。

## 2026-09-13 Gateway binding 合同收敛执行记录

### Goal

- 用户可见结果：默认 App 继续作为无运行实例的兼容绑定，受管理 CLI/App 继续按真实运行实例工作；Gateway 注册代码不再用实例字段承载默认 App 语义。
- 范围：Gateway 控制/运行时注册类型、sidecar 注册与恢复、LaunchService 适配、相关测试和设计文档。
- 非目标：不改变 `codex_instances` 或 `compatibility_gateway_bindings` schema，不轮换现有密钥，不修改 `X-3R-Instance-Key`/`THREER_INSTANCE_KEY` 的持久化协议，不调整 UI。

### Acceptance

- `GatewayBinding` 只包含绑定标识、RouteSet 和准入密钥摘要；fallback 从 RouteSet Gateway 快照派生。
- 默认 App 注册和恢复不创建/查询运行实例记录；受管理实例注册、注销、请求诊断和清理行为保持不变。
- 旧 sidecar 控制 JSON 在升级期间仍可被读取；新 sidecar 不把 fallback 重复字段作为领域状态，旧字段仅由控制面 shim 兼容。
- 覆盖注册成功、缺字段/坏快照拒绝、重复绑定冲突、默认兼容绑定恢复和两个独立绑定路由隔离。

### Execution

- [x] 更新 Gateway domain/control types，并保留 JSON 过渡别名。
- [x] 从 RouteSet Gateway 快照派生 compatibility fallback，删除注册 payload 和 PreparedLaunch 的重复字段；旧 sidecar 复用时仅附带 deprecated wire shim。
- [x] 更新 LaunchService、sidecar client/server、请求诊断字段映射和测试 fixture。
- [x] 运行 Rust 格式化、check、Gateway/launch focused tests；同步设计文档中的旧命名。

验证结果：`cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`、`cargo check --manifest-path src-tauri/Cargo.toml --all-targets` 通过；允许本机回环监听后执行 `cargo test --manifest-path src-tauri/Cargo.toml --all-targets`，库测试 229 项、Gateway/launcher/official-auth 目标全部通过。受限沙箱中的首次网络测试仅因禁止绑定 `127.0.0.1` 返回 `Operation not permitted`，不是断言失败。

### Risks and rollback

- 风险：旧 sidecar 仍在运行时的控制协议字段不兼容；通过 serde alias 和仅线协议保留的 `compatibilityFallback` shim 保持字段与 fallback 语义，直到未来协议版本迁移。
- 风险：人为构造的历史 RouteSet 缺少 fallback 快照；注册应把格式错误报告为明确失败，普通无 fallback RouteSet 仍允许注册。
- 回滚：代码改动可回退，不涉及 schema 或 Keychain 写入；RouteSet 和 binding 持久化记录保持可读。

### 2026-08-29 UI 精简切片

- 移除首次使用引导卡、顶栏帮助入口和直接打开默认 Codex App 按钮；默认 App 不再作为 3R Codex 的主路径入口展示，受管理 App 仍从启动方案进入。
- 添加模型渠道收敛到“账户与密钥”页面；Gateway 停止与批量实例清除移动到“运行实例”的“高级操作”。
- Provider 用量/余额和启动方案手工模型映射默认折叠，降低首屏信息密度；保留原有诊断、清除和高级映射能力。
- 模型渠道提供“刷新模型目录”入口，刷新后覆盖该渠道的最新 `/models` 快照；特殊角色模型继续只在系统角色说明中提示，不进入普通目录、路由摘要或计数。
- 当前产品只维护一条主工作区；名称、工作目录、Provider 和模型均在工作区编辑器中更新，并产生新的路由版本。底层旧版创建/更新命令保留用于兼容和迁移，不代表界面开放多工作区。
- 将传输策略设置移入启动方案；模型渠道界面不再编辑传输方式，旧 Provider 字段仅作兼容读取。启动方案列表和启动确认弹窗收敛为摘要，模型详情通过详情抽屉查看。
- 2026-08-29 启动阻断修复：当前 Codex `visibility` 枚举要求特殊模型使用 `hide`，生成器此前误写为 `hidden`，导致模型目录在 App/CLI 启动前解析失败；已修正并通过本机 `0.150.0-alpha.8` 真实目录校验。

## 2026-08-25 验收记录

- `cargo test -q`：136 个库测试、7 个 launcher 测试、15 个 official-auth 集成测试，共 158 个通过；回环 mock 在允许绑定 `127.0.0.1` 的环境执行。
- `cargo fmt --all -- --check` 与 `cargo check --all-targets`：通过；仅保留既有未使用内部 API 警告。
- 本机真实 `/Applications/ChatGPT.app/Contents/Resources/codex`：版本匹配 bundled catalog、生成目录、`$CODEX_HOME/<name>.config.toml` 和 `--profile <name> debug prompt-input` 契约通过；未登录、未读取认证文件。
- `pnpm run check` 与 `pnpm run build`：通过，Vite 生产包生成成功。
- in-app Browser：桌面和 430×780 窄屏 smoke 通过，无相关 console error、无横向溢出；普通浏览器没有 Tauri IPC，因此数据态的多 Provider 模型编辑仍需原生应用人工验收。
- `pnpm run tauri:build`：Rust release、前端生产构建和 `.app` bundle 均通过；默认 `bundle_dmg.sh` 在受限环境的挂载阶段失败，未生成最终 DMG。当前 `.app` 产物为 `src-tauri/target/release/bundle/macos/3R Codex.app`，仍为 ad-hoc 签名，未做 Developer ID 公证。

## 2026-08-26 Grok CLI 启动收口

- 受管理 CLI 的 `.command` 脚本现在显式交给 macOS Terminal 执行，不依赖用户的文件关联；避免 `open` 提前返回成功但 launcher 未运行、实例长期停留在 `orphaned` 的情况。
- 对 `orphaned`、`degraded` 或 `stopping` 且没有可核验 PID 的实例，停止/重新核验会确认其已无进程并释放配置锁；旧的失败派发不会永久阻塞同一 Provider 的下一次启动。
- 回归验证：完整 Rust library tests 136/136，Codex launcher tests 7/7，Tauri `.app` release bundle 构建成功。

## 2026-08-26 启动失败诊断契约

- 每个受管理实例在其私有生命周期目录中维护有上限的 `startup.log`，由 Tauri 启动请求和 `codex-launcher` 共同追加启动阶段、凭据库读取结果、Codex spawn、回执写入和退出状态；不记录 API Key、实例密钥或 Authorization 原文。
- `get_codex_control_snapshot` 将读取脱敏后的最近日志片段作为 `startupLog` 返回；运行实例页面对失败、未确认和降级实例提供展开查看。读取失败或日志超过上限只显示稳定诊断码，不阻塞实例状态刷新。
- 启动异常后前端先刷新实例快照再显示错误，避免只显示 `launch_unconfirmed` 而隐藏具体 launcher 阶段。

## 2026-08-26 启动失败根因与失败实例清理

- 当前 Grok 受管理 CLI 的实际失败阶段已从 `startup.log` 确认：Terminal 已派发，launcher 已解析 `/Applications/ChatGPT.app/Contents/Resources/codex`，工作目录已完成，但在 `instance_key_read_requested` 后无法由 Terminal 派生的 launcher 读取 macOS Keychain。现有策略把实例密钥读取放在 launcher 中，受 macOS Keychain 调用方/权限上下文影响，导致实例随后被清理为 `failed`。
- 新策略由 Tauri 主进程（已持有受信任的 Keychain 访问上下文）读取本次短期实例密钥，再通过实例私有目录中的一次性 FIFO（命名管道）交给 Terminal 派生的 launcher；密钥不出现在命令行、脚本正文、SQLite、日志或 UI，launcher 读完即删除管道。Keychain 仍是唯一持久化来源。
- 运行实例提供“删除记录”操作，但后端只接受 `exited` 或 `failed` 且没有活动锁的实例；`running`、`launching`、`stopping`、`orphaned`、`degraded` 必须先核验/停止，防止删除仍可能运行的进程和配置锁。

## 2026-08-26 最新构建交付

- 本轮 `.app` 已重新构建，直接可打开的产物固定为 `src-tauri/target/release/bundle/macos/3R Codex.app`。
- 后续交付只提供该直接可打开的 macOS 应用目录，不再把压缩包或磁盘映像作为交付入口。
- 已验证：`pnpm check`、前端生产构建、`cargo fmt --all -- --check`、`cargo check --all-targets`、启动日志/脱敏测试、终止/未核验实例删除边界测试、Codex launcher 8 项测试、真实 FIFO 密钥交接探针和 Tauri `.app` bundle 构建。

## Continuation

- 重要文件：`docs/codex-hybrid-routing-design.md`、`database.rs`、`resource_guard.rs`、`managed_gateway.rs`、`codex_control.rs`、`bin/codex-launcher.rs`、`src/codex-control.ts`、`src/App.tsx`。历史单 Provider Gateway 文件已删除。
- 决策：新 v6 启动统一 Gateway；官方账户可选；官方 route 默认关闭；v5 overlay 迁移。
- 已有证据：`cargo test -q` 全绿，`pnpm run check && pnpm run build` 全绿，显式真实 Codex Profile/目录契约测试通过。
- 已增加真实 CLI→本机 Gateway→模拟 Responses/Chat-only Grok 上游契约；真实上游验收仍使用测试 Key 和隔离工作区，不触碰默认 `~/.codex`。

## 2026-08-26 受管理 Grok 路由与实例批量清理收口计划

### 目标与硬约束

- 受管理启动必须同时固定 `model_provider = "3rcodex"`、本次 RouteSet 的公共模型和 Gateway 地址；启动参数、Profile、模型目录和 Gateway 诊断要能互相核对，不能静默回到官方默认 Provider。
- Grok 选择必须在真实 CLI 请求层验证：请求进入本机 Gateway 后，Gateway 按不可变 RouteSet 将公共模型精确映射到固定 Grok Provider revision、固定 base URL，并只注入该 revision 的 API Key。
- 诊断日志只记录稳定标识、哈希/摘要、模型 ID、Provider revision、URL 主机和阶段；请求级账本额外保存限长脱敏错误摘要，不记录 API Key、实例密钥、Authorization、请求正文或完整响应正文。
- 运行实例“一键清除”只处理 `exited`/`failed` 且无活动锁的实例；`launching`、`running`、`stopping`、`orphaned`、`degraded` 一律保留。批量入口逐实例复用单实例 reconcile、绑定撤销、资源锁检查和生命周期目录校验。
- 修复无 PID `degraded` 实例的停止状态转换：显式停止后进入 `exited` 并释放锁，随后可删除；无 PID `orphaned` 仍进入 `failed`，避免把未核验实例误判为已退出。

### 边界与写路径

- `managed_codex_config`/launcher：唯一生成并执行受管理 CLI 的 Provider/模型启动合同；Profile 是持久配置，显式 CLI 参数是最终覆盖层。
- `ManagedLaunchStore`：唯一生成不可变 RouteSet 和启动快照的写入者；启动诊断由 Tauri 与 launcher 共同追加。
- `ManagedGatewayRuntime`：只读取 ready RouteSet；`forward_provider` 是 API Key 注入和上游地址选择的唯一写路径。旧单 Provider Gateway 不再作为回退数据面。
- `lib.rs`：单实例与批量删除共享同一个安全删除 helper；任何跳过/失败按实例返回结果，不因一个实例失败而误删其他实例。

### 执行与验收

- [x] 补齐显式 `--model`/Provider 覆盖和脱敏启动/路由诊断；保留 FIFO 一次性凭据交接。
- [x] 增加真实 CLI→Gateway→模拟 Grok upstream 回归：断言 `model_provider`、公共模型、上游模型、Gateway base URL、Provider revision、API Key 隔离及官方默认未被调用。
- [x] 增加批量清除 IPC/UI、确认/结果提示和安全状态矩阵测试。
- [x] 运行 Rust targeted/all、`cargo check --all-targets`、前端 check/build，并构建直接可打开的 `src-tauri/target/release/bundle/macos/3R Codex.app`。

## 2026-08-26 OpenCodex 风格 MCP/sidecar 兼容收口计划

### 目标与验收标准

- 无官方认证的 Grok/API Key 实例不得继承 `openaiDeveloperDocs` 等 OpenAI 专用 MCP；普通可移植 MCP 可以继续继承。
- 有官方认证的兼容 Profile 保留专用 MCP；官方认证状态由官方 Codex 登录态决定，绝不复用第三方 API Key。
- 不支持的 MCP 在配置物化阶段被过滤并写入脱敏诊断；Codex 首轮不会因 403 官方 MCP 阻塞主对话。
- 回归测试断言：无认证时专用 MCP 不在托管 `config.toml`、普通 MCP 仍在；官方兼容策略保留两者；日志不含 URL、Token、API Key 或 Authorization。

### 设计与写路径

1. `managed_codex_config` 增加显式 `ManagedMcpPolicy`，按认证边界过滤已知 OpenAI/ChatGPT 专用服务器，返回被过滤名称供启动诊断使用。
2. `ManagedLaunchStore` 按 RouteSet 的官方认证要求选择策略；当前纯 API Key Grok 路径使用外部模型策略，官方兼容路径使用保留策略。既有 FIFO 一次性凭据交接不改动。
3. MCP 过滤只发生在隔离实例配置快照，不修改默认 `~/.codex`；Gateway 仍只负责模型请求的 Provider/模型映射。
4. sidecar 作为后续独立能力边界记录：它可以把搜索/视觉故障变成有界工具结果，但本轮不把官方 MCP 认证代理给 Grok。

### 验证
- Rust：MCP 分类/过滤单测、启动快照诊断测试、现有 Gateway/launcher/官方认证回归。
- 配置安全：无认证实例不能出现 OpenAI 官方 MCP URL 或 ChatGPT 专用命令；第三方 MCP 与 FIFO 凭据路径保持不变。
- 发布：`cargo fmt`、`cargo check --all-targets`、针对性 `cargo test`、`pnpm run check`、`pnpm run build`，最后只生成可直接打开的 `.app`。

### 当前状态

- 根因已由现有代码与请求级回归确认：真实 CLI 必须在最终命令边界显式覆盖 Provider/公共模型；现已固定并由模拟 Grok upstream 契约测试核验。批量清除已实现并复用单实例安全删除路径。
- 回滚点：保持现有 Profile/RouteSet schema 与 FIFO 协议不变；若目标 Codex 版本拒绝显式覆盖，则启动前失败并保留脱敏诊断，不回退到官方默认路由。

### 2026-08-26 收口证据

- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`：通过。
- `cargo check --manifest-path src-tauri/Cargo.toml --all-targets`：通过；仅保留既有未使用内部 API 警告。
- `cargo test --manifest-path src-tauri/Cargo.toml --all-targets`：库 140、launcher 9、official-auth 15 全部通过；包含 Provider Header/model rewrite、Responses/Chat-only Grok 契约、启动 Profile/catalog、FIFO 凭据交接、终端/锁定实例删除边界。
- `pnpm run check && pnpm run build`：通过，Vite 生产构建 1804 modules。
- `pnpm run tauri -- build --bundles app`：通过，直接可打开的 arm64 `.app` 已生成；未生成 DMG/zip。
- `.app` 校验：主程序和 `codex-launcher` 均为 arm64 Mach-O；bundle 约 19 MB。当前为本机 ad-hoc 签名，`codesign --verify` 仅提示无资源但签名声明资源，未做 Developer ID 公证。
- 外部 Codex `exec` 探针：显式 `-c model_provider="3rcodex"` 的测试命令会在真实 CLI 进程中等待超过本地模拟响应窗口，已终止两枚本次探针 PID；不会将该探针误报为通过。无凭据的 Gateway 单测与请求级 Grok mapping 已通过，真实用户上游仍需在桌面端用测试 Key 手动验收。

### 2026-08-27 当前会话接手验收

- `cargo test --manifest-path src-tauri/Cargo.toml --all-targets`：库 145、launcher 9、official-auth 15 全部通过；新增同步 JSON 流式降级为 Responses SSE 的回归。
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`、`cargo check --manifest-path src-tauri/Cargo.toml --all-targets`：通过；仅保留既有 dead-code 警告。
- `pnpm run check`、`pnpm run build`：通过，Vite 生产构建生成 1804 modules。
- `node_modules/.bin/tauri build --bundles app`：通过，生成直接可打开的 `src-tauri/target/release/bundle/macos/3R Codex.app`；未生成 DMG/ZIP。应用主程序和 `codex-launcher` 均为 arm64 Mach-O。
- 旧计划中的显式 `--model`/`model_provider` 覆盖、请求级脱敏诊断、模拟 Grok upstream 契约和批量安全清除已在现有代码与回归测试中落地；真实用户 Provider/官方账户验收仍需桌面端凭据。

## 2026-08-27 CLI 流式响应兼容收口

### 问题与决策

- 现象：Codex CLI 请求 `stream=true`，但部分 OpenAI-compatible Grok 上游以 `application/json` 返回同步 Responses JSON。旧 Gateway 会把该正文标记为 SSE 并原样转发，CLI 无法收到 `response.completed`，表现为持续思考；部分失败路径也把原始流式请求错误记为非流式。
- 决策：仅在受管理 Gateway 观察到“请求流式、成功响应不是 `text/event-stream`”时，在本地读取受已有总超时约束的同步 JSON，重写公共模型 ID 并合成完整 Responses SSE 生命周期。该兼容层不重发上游请求、不持久化正文，诊断记录真实上游 Content-Type 和 `upstream_stream_downgraded`。
- 失败行为：同步正文超时、中断或格式错误仍返回稳定 Gateway 错误；诊断保留 `stream=true`，不会再误报同步请求。

### 验收

- [x] 流式请求收到同步 JSON 时，响应包含 `response.created`、输出项完成事件和 `response.completed`，并保持公共模型 ID。
- [x] 流式请求收到同步 JSON 时，Gateway 请求级诊断会保留真实 `stream=true`、上游 `application/json` 和 `upstream_stream_downgraded` 原因；首字节超时、网络失败与非 2xx 路径也已按相同规则修正。该字段在完整桌面实例生命周期中落库，路由单测仅验证协议响应。
- [ ] 使用真实 Grok 中转站从桌面端进行新建和连续 CLI 对话验收。

## 2026-08-27 受管理单 Provider 直连收口

### 目标与边界

- 单 Provider、无官方混合、Provider 传输模式为 `direct_responses` 时，仍使用受管理实例的隔离 `CODEX_HOME`、生命周期回执、启动日志和一次性密钥交接，但 Codex Profile 的 `base_url` 必须是该 Provider 的真实地址。
- 多 Provider、`gateway_chat_completions`、需要公共模型精确映射或其他协议转换的配置继续使用本机 Gateway；本机 Gateway 可以因其他实例运行而存在，但直连实例的数据请求不得经过它。
- 直连模式仍不把 API Key 写入 Profile、命令行、SQLite 或日志；只通过现有启动凭据管道交给 Terminal 派生的 launcher，并注入隔离子进程的 `OPENAI_API_KEY`。

### 写路径与失败行为

- `ManagedLaunchStore` 根据不可变 Provider revision 判断启动数据面：只有单一 `DirectResponses` Provider 才生成 direct-managed Profile；该判断与 RouteSet/实例快照一起固化。
- `launch_codex` 仅为 Gateway-managed 实例建立本机 Gateway 请求绑定；direct-managed 实例不注册可用的本机数据路由。
- 直连 Provider 地址不可用、返回非 2xx 或 Codex 启动失败时，保留现有实例失败/回执/诊断路径，不自动降级到本机 Gateway。

### 验收

- [x] 单 Provider `DirectResponses` 受管理启动生成真实 Provider `base_url`，请求不命中 `127.0.0.1`。
- [x] Gateway 透传与 Chat 转换路径保持本机 Gateway 地址和实例密钥行为。
- [x] 直连 API Key 只通过一次性启动凭据管道进入隔离 CLI，Profile 与日志不含密钥。
- [x] 运行 Rust 全量测试、前端检查/构建并重新生成 macOS `.app`；真实 Provider 连续 CLI 对话仍需凭据手动验收。

## 2026-08-27 多 Provider 模型选择可辨识性

- 多 Provider 的 `/models` 目录在启动配置抽屉中合并展示；每条候选保留 Provider 配置名和上游模型 ID。
- 当不同 Provider 的上游模型 ID 或显示名相同时，候选显示为“Provider 配置名 / 模型名”，并自动生成带稳定 Provider 标识前缀的公共模型 ID；公共 ID 仍是 Codex/Gateway 的内部唯一路由键。
- 选择全部或逐条勾选时，已选模型不会因另一个 Provider 的同名模型出现而静默覆盖用户手工修改；自动生成的默认名称会在需要时补上渠道前缀。

## 2026-08-27 Gateway 回环代理与实例退出收敛

### 问题与边界

- macOS 系统代理可能被 Codex 的 HTTP 客户端用于访问 `127.0.0.1`，即使系统代理排除列表已经包含回环地址；此时空正文 `502` 来自系统代理而非 3R Codex Gateway，请求不会产生 Gateway 诊断。
- 3R 拉起的 Gateway-managed CLI / 兼容 CLI / 受管理 App / 从 3R 打开的默认 App 需要强制绕过本机回环代理。Finder / Dock 直开拿不到进程 `NO_PROXY`。单 Provider direct-managed CLI 仍按用户环境访问真实 Provider Base URL，不得关闭其系统代理路径。详见[默认 App 回环 Gateway 与系统代理](compat-app-loopback-proxy.md)。
- Terminal 窗口或进程组被直接关闭时，launcher 可能与 Codex 一起退出，来不及写入 exited 回执；后端必须以回执记录的 PID 和进程启动身份为准完成兜底收敛。

### 写路径与失败行为

- `codex-launcher` 在 Gateway-managed CLI **以及**受管理 App / 兼容 App / 兼容 CLI 的子进程环境中合并父进程现有 `NO_PROXY`/`no_proxy`，保留用户条目并补齐 `127.0.0.1`、`localhost`、`::1`；两种大小写变量写入相同结果。Finder / Dock 直开的默认 App 不是该子进程，拿不到这份覆盖。
- direct-managed CLI 不注入上述覆盖，避免改变真实上游的代理行为。
- running 回执对应的原 PID 不存在或启动身份不匹配时，实例转为 `exited`、退出码保持未知并释放配置锁；不得因旧状态是 `running` 而停留在活动态。
- 运行实例页面仅在可见且存在活动实例时定期调用单实例核验；状态进入终态后停止轮询，并同步 Gateway 状态。

### 验收

- [x] 既有代理排除项被保留，回环条目去重，大小写环境变量一致；direct-managed 命令不受影响。
- [x] running 回执的 PID 消失和 PID 身份不匹配均自动转为 `exited` 并释放锁；进程仍存在但身份读取暂时失败时保持活动，避免误判。
- [x] 运行实例页面无需手工刷新即可显示 CLI 退出；没有活动实例或不在该页面时不轮询。
- [x] Rust 全量测试、前端检查/构建、macOS `.app` 打包和本机旧实例恢复验证通过。

### 验收证据

- `cargo test --manifest-path src-tauri/Cargo.toml --all-targets`：库 150、launcher 10、official-auth 15 全部通过；覆盖消失 PID、复用 PID 和回环代理条目合并。
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`、`cargo check --manifest-path src-tauri/Cargo.toml --all-targets`、`pnpm run check`、`pnpm run build`：通过，仅保留既有 dead-code 警告。
- in-app Browser：`http://localhost:5173/` 页面身份、非空渲染、运行实例入口、目标文案、控制台和截图检查通过。
- 原生 `.app`：历史 running 回执在启动核验后自动转为 `exited` 并释放锁；新建混合 Provider 实例的真实 Codex 子进程同时包含 `NO_PROXY`/`no_proxy=127.0.0.1,localhost,::1`。
- 原生生命周期：停掉测试 Codex 子进程后，运行实例页面约两秒内从 1 个活动实例自动更新为 0 个，记录显示“已退出”，最后一个绑定清理后 Gateway 自动停止。
- `node_modules/.bin/tauri build --bundles app`：通过，最终应用位于 `src-tauri/target/release/bundle/macos/3R Codex.app`。

## 2026-08-27 P0 CLI 多渠道可用性版

### 目标与范围

- 在不改变现有 RouteSet、实例锁和 Gateway 写路径的前提下，让用户在启动前看懂本次会话的 Provider、模型、传输方式和工作目录。
- 将 Provider → 公共模型 → Gateway/真实 Base URL 的关系显式展示；重复模型继续使用渠道前缀，不在 UI 层重新推断路由。
- 将诊断分为“请求未进入 Gateway”“Gateway/路由层”“上游服务”三类，并保留原始稳定错误码。
- 扩充运行实例可见信息和 Gateway 联动状态；保留现有自动核验与终态清理。
- （已由 2026-08-29 UI 精简切片取代）不再提供阻塞式首次使用引导卡；空状态直接提供下一步操作，不上传遥测、不改变默认 Codex 环境。

### 非目标

- 不实现官方账号生产验收。
- 受管理运行实例的 Codex App 启动骨架已实现；默认 App 入口不再从 3R Codex 顶栏展示，受管理 App 仅从启动方案进入。App 目标与 CLI 共用运行实例 Provider/RouteSet/Gateway 合同，但不规划 Codex App 多开。
- 不让 UI 成为路由状态的第二写入者；默认 App 兼容边界允许新增独立绑定表，但不扩展运行实例表。

### 验收

- [x] 启动对话框显示完整启动预览、模型路由和“确认后生成隔离快照”提示。
- [x] 概览和启动方案页显示 Provider/模型数量、传输路径和 Gateway 状态；重复模型标识清晰。
- [x] 诊断页面按故障层级显示用户可执行的下一步，并能关联 Provider、模型和实例。
- [x] 运行实例显示启动方案、模型摘要、Gateway 路径、进程与退出信息；状态变化后不需手工刷新。
- [x] 首页不再显示首次使用引导卡；无 Provider/启动方案时直接显示上下文空状态操作。
- [x] 前端类型检查、生产构建、浏览器桌面/窄屏 smoke 与原生 `.app` 验收通过。

### 实施证据

- `npm run build`：TypeScript 检查和 Vite 生产构建通过。
- `cargo test --manifest-path src-tauri/Cargo.toml`：175 个 Rust 测试通过（150 library、10 launcher、15 official-auth）。
- in-app Browser：默认桌面视口和 390×844 窄屏通过页面身份、非空、无错误日志、导航切换和首次引导关闭/重新打开 smoke；窄屏 `scrollWidth === clientWidth`，无横向溢出。
- UI 变化集中在 `src/App.tsx`、`src/styles.css`；未新增持久化写路径，首次引导仅使用本地 `localStorage` 记忆关闭状态。
- `npm run tauri:build`：原生 release 二进制和 `src-tauri/target/release/bundle/macos/3R Codex.app` 已生成；DMG 封装仍受既有 `bundle_dmg.sh` 挂载环境限制，直接可打开的 `.app` 不受影响。

## 2026-08-28 P1 Gateway sidecar MVP

### 产品承诺

Gateway 现在由独立的 `codex-gateway` 伴随进程承载。3R Codex 是控制面，负责拉起/复用、健康检查、实例 binding 注册与注销；Codex CLI 直接访问 sidecar 的回环数据端口。这样 3R Codex UI 退出不会直接终止仍被运行实例使用的转发服务，也不会让每个模型请求额外绕行 UI。

### 已实现

- [x] 新增 `codex-gateway` 二进制，复用现有 Axum Gateway 数据面并提供 loopback 控制端点：`/control/status`、`/control/register`、`/control/unregister`、`/control/stop`。
- [x] 主应用通过随机控制令牌调用控制端点；令牌仅写入当前用户可读写的 `0600` ready 文件。ready 文件不包含 Provider API Key、实例密钥或 Authorization 原文。
- [x] 启动 Gateway 方案时自动确保 sidecar serving，并注册实例 ID、实例密钥哈希和不可变 RouteSet；实例退出/删除时注销 binding，最后一个 binding 清理后可显式停止 sidecar。
- [x] 应用重启优先复用健康 sidecar；失效端点会被清理并重新拉起。并发启动请求使用单独启动锁，避免竞争 ready 文件而生成重复 sidecar。
- [x] 打包脚本与 Tauri `externalBin` 同时包含 `codex-launcher` 与 `codex-gateway`；README 已补充 sidecar 概念、生命周期和单 Provider 直连/多 Provider 强制 Gateway 的用户说明。

### 尚未承诺

- [x] sidecar 进程自身崩溃或被系统杀死后，从数据库恢复仍处于活动态且密钥/RouteSet 校验通过的 active binding；无效记录安全跳过并保留数据库状态供实例核验。
- [x] 窗口关闭隐藏主窗口；真正退出在仍有活动实例时被阻止，并通过 UI 提示实例数量和停止入口。托盘驻留仍属于后续桌面体验增强。
- [ ] 正在传输的 SSE 流在 sidecar 重启后从中间续接；该场景仍按断流处理。

### 验收证据

- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`、`cargo check --manifest-path src-tauri/Cargo.toml --all-targets`：通过。
- `cargo test --manifest-path src-tauri/Cargo.toml --all-targets`：通过，覆盖 Gateway 数据面、控制端点参数解析、launcher 与 official-auth 回归。
- 手工 sidecar 契约：ready 文件生成、正确令牌 `/control/status` 返回 `serving`、错误令牌返回 `401 invalid_control_token`、`/control/stop` 后进程正常退出并删除 ready 文件。
- `pnpm run check`、`pnpm run build`：通过；`codex-launcher` 与 `codex-gateway` 均纳入 Tauri app bundle 构建路径。

## 2026-08-28 Codex App 机械兼容性探针记录

- [x] 当前 Codex CLI `0.150.0-alpha.8` 与桌面 App `26.820.60940` 已记录；`codex app --help`、`codex app-server --help` 均通过。
- [x] 在临时 `CODEX_HOME` 下生成 App Server v2 JSON schema，未读取默认认证或修改用户默认配置。
- [x] 只读验证临时 `CODEX_HOME`、Electron 用户数据目录和默认 App 共存；当前版本未把第二个 App 启动转发到默认 App。
- [x] 只读验证隔离模型目录读取，并静态核验当前 renderer 的 `hidden` 可见性规则；运行时探针会在打包资产齐备时开放受管理 App，失败时可用环境开关关闭。
- [x] 单运行实例 App 行为探针：核验真实 PID、派生 app-server、配置/模型生效、App Server 模型列表、Gateway 请求和退出回执；renderer 已完成静态规则与入口可见性核验，真实模型 picker 点击仍需签名桌面会话验收。不做 App 多开编排。Keychain 无授权时按有界失败处理，不能静默回退。

上述机械探针与发布版运行时探针共同证明当前安装具备受管理 App 的可调用入口；完整上游请求仍受签名桌面会话和 Keychain 授权条件约束。详细决策和官方合同见 [Codex App 支持预研](codex-app-support-research.md)。

隔离启动探针补充结论：默认 App 与一个临时数据目录 App 可同时运行；临时 `CODEX_HOME` 有实际 Codex 状态写入，App Server initialize 握手成功，未读取默认认证。临时 App Server 的 `model/list` 已返回测试模型；同一隔离配置下 `thread/start`/`turn/start` 已请求到本机 Gateway 并收到完整 `response.completed`；Gateway 的两条无敏感 Codex→模拟 Grok upstream 回归也通过。运行时探针现在会在打包的 launcher、Gateway 和桌面可执行文件齐备时将 `managedAppAvailable` 置为 `true`，并支持 `THREER_CODEX_DISABLE_MANAGED_APP=1` 紧急关闭；Keychain 授权失败仍会在启动阶段有界失败，不会静默回退。

桌面控制通道补充探针确认：Computer Use 服务接受的是签名授权 Apple Event；外部只读调用返回 `Sender process is not authenticated`。这不是受管理 App 本身的失败，而是当前执行环境没有可授权的图形会话；发布验收仍须在授权桌面会话中完成，不能用未授权外部进程替代。

授权桌面重试补充：renderer 已显示受管理 App 按钮；当前发布版探针在打包 sidecar 与桌面可执行文件齐备时将 `managedAppAvailable` 置为 `true`。实际启动仍可能在 macOS Keychain（系统凭据库）读取 Provider API Key 或实例密钥时失败，失败会有界返回并保持默认 App 不受影响；正式请求链路需在能完成 Keychain 授权的签名桌面会话复验。改名迁移同时补充了残留 SQLite WAL/SHM 旁车文件 checkpoint 后复制的回归，避免旧库旁车文件阻断首次启动。

补充证据：`installed_codex_accepts_its_version_matched_managed_catalog_when_requested` 使用本机 `0.150.0-alpha.8` 通过，CLI 侧版本匹配模型目录可读；桌面 App 的 renderer 模型选择器仍需单独验证。

目录语义补充：CLI 启动可指定本次工作目录；受管理 App 不预先绑定工作目录，启动后由 App 自己选择任意目录，后端忽略 `managed_app` 请求中的目录参数。

## 2026-09-03：默认 App 启动前检查现有进程（已废弃）

### Decision

- **Change**: 启动默认 App 兼容路由前检测本机 Codex 桌面进程；检测到默认 App 正在运行时，禁用启动按钮，用户退出 App 后点击“重新检测”才能继续。
- **Contract or boundary**: 进程检测只作为启动前安全门，不接管、强制退出或重启用户正在使用的默认 App；通过后仍由 `compat_app` 负责可逆配置接管、Gateway 绑定和启动回执。
- **Owner and write path**: Rust 安装状态命令负责读取桌面进程状态，renderer 只负责展示禁用原因和触发重新读取；不新增第二条配置写入路径。
- **State or consistency model**: 进程状态是重新检测时的一次快照；按钮状态以最近一次检测结果为准，启动提交时后端仍需保留兼容配置/实例校验。
- **Failure behavior**（历史）：无法可靠识别进程时保持保守禁用并提示重新检测。该策略已由 2026-09-07 的 best-effort 派发决策替代。
- **Rejected options**: 不直接替用户关闭并重启默认 App，避免丢失未保存内容和破坏用户会话；不把系统直接打开的默认 App 伪装为 3R 运行实例。
- **Verification**: 安装状态返回运行/未运行两种结果；前端分别验证默认 App 启动按钮禁用与重新检测后恢复；兼容启动仍传递既有 `compat_app` 目标。

## 2026-09-04：默认 App 兼容会话与失败回滚收口

### Decision

- `compat_app` 是用户拥有的默认 Codex App 路由投影，不是 `managed_app` 的实例，也不参与受管理 App 的全局单实例检查；重复点击默认 App 只更新独立兼容绑定，不再提示“已有受管理 App 实例”。
- 默认 App 进程由用户和操作系统拥有，3R 不读取、关闭、聚焦或重启它；默认 App 不属于运行实例。
- 兼容启动以配置投影与启动器派发成功为准，不依赖 App 进程回执；字段级 marker 仍负责后续精准清理。
- 兼容模型目录以原始/ bundled 目录为基底合并 3R 路由，3R 路由只覆盖同 slug 条目；因此默认 App 重启后仍能看到原生模型和新增第三方模型。
- 默认 App 的兼容绑定单独保存在控制数据库中用于 Gateway 重启恢复；它不出现在运行实例列表，也不参与运行实例锁，但经过 Gateway 的请求会进入全局请求诊断账本。

### Verification

- [x] 兼容目标不触发 managed-App singleton；默认 App 使用固定独立绑定，不累积运行实例。
- [x] 原生模型目录与第三方路由合并，目标 Codex `debug models` 解析并返回两类模型。
- [x] 默认 App 启动器显式传入默认 `CODEX_HOME`/SQLite home，仅注入实例准入 Header，不继承受管理 App 的 Electron 数据目录。
- [x] Rust targeted tests、`cargo check --all-targets`、前端类型检查/生产构建通过；全量 Rust 测试为 198 通过。
- [x] 已生成并校验 arm64 测试包：`src-tauri/target/release/bundle/macos/3R Codex_latest_default-app-independent-binding_aarch64.app.zip`；应用为本机 ad-hoc 签名（未做 Developer ID 公证）。

## 2026-09-04：默认 App 兼容会话停止/删除状态机修正

### Decision

- 根因：旧版本错误地把默认 App 兼容投影写进 `codex_instances`，并用受管理实例的停止/删除状态机处理用户拥有的 App，导致脏行与实际进程脱节。
- 修正：默认 App 改用独立兼容绑定；停止/恢复只撤销 Gateway 路由并按字段级 marker 清理，永不向桌面 App PID 发送终止信号。默认 App 进程不参与 3R 状态判断。
- 默认 App 安全边界：`compat_app` 停止操作永不向桌面 App PID 发送终止信号；用户负责在需要时退出或重新打开默认 App 以加载新配置。
- 遗留清理：旧 `compat_app` 行从运行实例快照隐藏；移除命令在 App 已退出后清理遗留行、运行锁和旧 binding。字段级 marker 的匹配字段直接清理，不匹配字段保留而不覆盖。

### Verification

- [x] 新增默认 App 独立绑定无运行实例行、Gateway 重启恢复，以及旧兼容行安全清理的回归覆盖；受管理实例的 PID 停止与删除测试继续通过。

## 2026-09-04 默认 App 所有权边界纠正

默认 App 是用户拥有的进程，不是 3R Codex 的运行实例。因此
`compat_app` 不得创建、更新或出现在 `codex_instances`（运行实例表）中，
也不得取得运行实例锁。兼容路由改由独立的
`compatibility_gateway_bindings` 记录保存；其中的固定 subject 仅用于
Gateway（网关）准入密钥命名，不代表进程身份或运行实例。

兼容生命周期为：准备默认目录投影并写入独立绑定；只观察真实桌面进程确认
启动；停用时撤销独立绑定并恢复默认配置，绝不向默认 App 发送信号。旧版本
遗留的 `compat_app` 行在运行实例列表中隐藏，并且只有默认 App 已退出且
配置哈希校验通过后才清理。

验收标准：重复启动默认 App 不新增 `codex_instances`、运行实例锁或
managed-App 单例错误；运行实例列表只显示 3R 拥有的 CLI/managed App；
Gateway 重启可恢复独立绑定；遗留脏行在真实进程退出前不能伪报“已停止”。

## 2026-09-04：工作区移除最后模型渠道

- **Change**：允许工作区移除最后一个 API Provider，提交后保留工作区记录并将组合标记为 `needs_model_selection`（待重新选择模型），而不是返回“凭据组合无效”。
- **Boundary**：空组合只能作为编辑中的未完成状态；启动路径仍拒绝非 `ready` 组合，避免生成没有路由的 RouteSet（路由集合）。
- **Verification**：覆盖同一默认 App 工作区从单 Provider/单模型更新为空组合，并确认 Provider、模型均清空且工作区不可启动。

## 2026-09-04：工作区渠道快捷操作与初始化反馈

- **Change**：工作区卡片内直接选择并加入未绑定的模型渠道，或通过渠道 chip 的图标移除；不再把这类操作导航到全局渠道管理页面。
- **Interaction**：操作按钮只显示图标，悬停提示和无障碍标签提供完整语义；首次读取本地状态时显示初始化 loading（加载）反馈。

## 2026-09-04：统一工作区组件布局

- 默认环境与隔离环境统一由 `WorkspaceEnvironmentCard`（工作区环境卡片）渲染，差异仅通过目标、状态和回调参数传入。
- 卡片操作区在桌面端统一右对齐；移动端继续采用纵向布局以保持可点击区域宽度。

## 2026-09-04：默认 App 配置快照安全与恢复体验（历史方案，已由字段级记录替代）

- [x] 将固定兼容备份升级为带 manifest 的版本化快照包，恢复后保留历史。
- [x] 为控制面增加默认配置状态（clean/active/conflict/missing_backup/corrupt）。
- [x] 默认环境卡片按状态提供启用、停用并恢复、冲突提示，减少全局恢复操作。
- [x] 覆盖快照篡改和外部编辑的回归测试，并实现 marker 写入失败时的补偿恢复路径。

## 2026-09-05：默认 App 投影去重与滚动恢复历史（历史方案，已由字段级记录替代）

- [x] 明确原始快照是停用/还原来源，后续写入前快照只作为最近历史，二者不互相覆盖。
- [x] 复用默认 App 兼容 binding 的本地准入密钥，并将兼容 Profile/目录命名收敛为投影内容稳定的名称。
- [x] 仅在当前 `config.toml` 与新生成投影不一致时创建写入前快照并原子写入；保留最近十份写入前快照。
- [x] 更新 marker 的原始快照字段兼容、状态/还原读取与回归测试，覆盖重复启动无写入、投影变更有备份、外部修改后重新投影和密钥轮换。
- [x] 验证 Rust 全量测试、前端检查与生产构建。

## 2026-09-05：已迁移 Provider 启动迁移回归

- [x] 确认开发服务启动失败来自 schema v5 的重复 Provider 回填，而非 Tauri 或前端服务。
- [x] 仅在 schema v5 前执行 Provider 修订/当前指针回填，保留 v5+ 已删除渠道的空当前修订。
- [x] 增加已退役 Provider 重新打开数据库的回归测试，并验证 `pnpm run tauri:dev` 能启动。

## 2026-09-05：模型渠道目录查看

- [x] 在每个模型渠道增加仅图标的模型目录入口，并提供可访问名称与悬停说明。
- [x] 目录抽屉只读取最近一次 `/models` 连通性快照，不触发上游请求、凭据库读取或工作区配置写入；未检测渠道明确引导用户使用刷新操作。

## 2026-09-05：默认 App 字段级兼容投影

- [x] 将默认 App marker 收敛为 3R 管理字段路径和值，不再创建新的完整用户配置备份。
- [x] 启用/更新只写入发生变化的 3R 管理字段；重复配置保持幂等。
- [x] 取消接入逐字段比对并删除仍匹配的 3R 值，保留用户手动修改字段；同时撤销兼容 binding 和清理 3R 生成文件。
- [x] 覆盖字段匹配、用户修改冲突、空字段清理、配置文件已被外部删除和重复取消的回归测试。

## 2026-09-07：移除默认 App 旧配置备份路径

- [x] 将默认 App 接入记录升级为仅字段级 marker：移除全文件哈希、原始备份路径与旧备份读取/恢复分支。
- [x] 将控制面状态收敛为 `clean`、`active`、`corrupt`，不再向界面暴露“用户修改 3R 管理字段”或旧备份缺失状态。
- [x] 保留逐字段精确清理：只有当前值仍匹配 marker 的字段才能删除；不匹配字段不覆盖、保留 marker 以支持后续重试。
- [x] 覆盖字段记录升级、字段差异清理、旧 marker 移除失败提示以及前端构建检查（Rust 205 项、前端类型与生产构建均通过）。

## 2026-09-06：Rust 冗余代码清理

- [x] 清理仅由旧测试辅助路径使用、生产代码不可达的 ResourceGuard 包装方法与辅助函数；测试专用辅助保留在 test 构建中。
- [x] 移除未接入运行路径的兼容 API，保留当前启动、网关、认证和路由写入路径。
- [x] 通过 Rust 全量测试和前端检查，确认清理不改变公开命令与持久化行为。

## 2026-09-04：默认 App 模型目录保留官方条目（完成）

- [x] 兼容目录以原始/官方 bundled catalog 为基底增量合并，避免官方 GPT 模型在重复启动后消失。
- [x] 第三方目录显示名统一为 `渠道名/模型名`，保持 slug 与精确路由身份稳定。

## 2026-09-06：双工作区主界面与辅助抽屉

- [x] 主界面主体只保留默认 App 与隔离 App 两张工作区卡片，移除左右导航驱动的主内容布局。
- [x] 模型渠道、受管理运行实例、历史/测试诊断改为按需打开的辅助抽屉，并保留原有操作、空态、错误态和 loading 状态。
- [x] 在主界面提供清晰的附加功能入口，运行实例入口仅展示隔离 App 相关运行记录。
- [x] 完成生产构建与浏览器交互验证。

## 2026-09-07：移除默认 App 进程检测

- [x] 移除默认 App 的跨平台进程采样、重新检测按钮和启动后进程确认，避免把当前 Codex/ChatGPT 宿主或辅助进程误判为默认 App 会话。
- [x] 默认 App 启动改为配置投影成功后的 best-effort 派发，不再因无法确认外部进程而返回 `launch_unconfirmed` 或回滚本次接入。
- [x] 增加独立的“应用默认 App 模型配置”命令与卡片图标入口：只写入 3R 管理字段并注册兼容 Gateway binding，不打开 App，提示用户重启默认 App 后生效。
- [x] 保留隔离 App 的实例回执、PID 身份和隔离目录生命周期管理；默认 App 不属于运行实例。

## 2026-09-08：工作区编辑入口收敛

- [x] 编辑启动入口移除用户可编辑的名称、默认工作目录和传输方式；固定工作区名称由“默认 App/隔离 App”卡片提供，CLI 工作目录在启动对话框按次填写，新配置统一使用 Gateway。
- [x] 默认 App 兼容目标在启动边界将旧直连策略规范化为 Gateway，避免历史配置因 UI 收敛而无法再次应用。
- [x] 工作区卡片增加快捷默认模型选择器；切换只重排已选模型并复用现有组合写入事务，模型展示统一采用“渠道名/模型名”。
- [x] 工作区保存前按渠道与上游模型复用全局模型身份；发现历史公共模型 ID 冲突时自动生成渠道限定 ID，避免 SQLite 唯一约束错误。


## 2026-10-09 审批询问

变更目的：`approvals_reviewer = "auto_review"` 在 Codex 0.162 上会把沙箱提权改成自动拒绝，不是产品要求的权限策略。隔离 Profile 停止写入该键，合并 `config.toml` 时删除旧值。`auto_review_model_override` 保留。

修改文件：`src-tauri/src/managed_codex_config/materialize.rs`、`src-tauri/src/managed_launch/prepare.rs`、对应测试，以及产品原理、混合路由设计和第三方目录说明。

验证：`cargo test --lib relaunch_refreshes_owned_config_and_keeps_isolated_preferences` 与 `cargo test --lib tests_prepare` 通过。`cargo fmt --all -- --check` 通过。

未完成项：已经打开的隔离 CLI 要下次启动才吃到合并结果。补绑和标题栏转发不再做。5 秒未确认保留绑定，见 [隔离启动核对计划](designs/2026-10-09-isolated-launch-review.md)。

回滚：恢复 Profile 写入 `approvals_reviewer = "auto_review"`，并停止在合并时删除该键。
