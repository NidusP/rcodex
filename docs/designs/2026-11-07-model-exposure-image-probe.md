# 模型透出与按模型图片探针实施

> 2026-11-07 commander 已确认推荐方向并授权实施。本文是该功能当前合同，优先于多隔离实例发布预案中 2026-11-06 的初稿；项目治理仍待启用。

## 目标与合同

- 渠道 `/models` 发现完整模型目录；渠道模型目录里明确勾选「透出」。空间只允许选择透出的普通对话模型。内部角色、生图模型不可透出。
- 「添加渠道」菜单的待分配上游仍列出未透出模型的渠道（2026-11-07 commander 确认），但状态列显示为「不可用」（红点红字），悬停提示先在模型渠道中选择要透出的模型；未配置、协议未确认、透出状态读取失败等其他阻塞同样显示不可用并悬停给出各自原因。悬停提示用 `disabled-hint` 包装层 + Mantine Tooltip（禁用按钮不派发鼠标事件，事件落到包装层）。
- 分别保存透出偏好（provider_id + upstream_model_id）和图片探针证据（provider revision + 模型 + wire_api + 探针合同版本）。刷新目录不清偏好、不执行图片请求。
- 首次透出且当前配置没有缓存时执行一次异步图片探针；取消再透出复用已完成缓存（包括失败）。显式刷新才重复请求。渠道配置版本变化后证据显示 unknown，保留勾选，等待显式刷新。
- Responses 与 Chat Completions 使用各自实际格式探测，只打当前渠道已确认的协议；不猜协议、不换地址、不重试 POST。成功必须识别图片内容，单纯 HTTP 200 不足。超时、认证、限流、服务故障、读图答案错误均记 failed；只有明确不支持图片才记 unsupported。
- 新启动的图片能力仅取当前有效 supported 证据，模型族默认不能越过该门槛。运行实例仍用原不可变路由快照。
- 升级将现有未删除空间选择的普通模型回填为透出，图片证据 unknown；不发真实请求、不改历史和目录。
- 取消透出前在事务内检查未删除空间引用；仍被使用则拒绝并提示空间名，用户先移除引用。空间保存和启动在后端校验透出，避免只靠 UI 过滤。
- 不更改版本、不推送、不打包、不启动/停止 Tauri 或现有实例；不发送真实 Provider 请求。

## 架构与并发

- `model_exposure` 独占偏好与证据写入，database 独占迁移；同一 SQLite 事务协调取消透出与空间写入。
- 网络期间不持有 SQLite 锁；同一配置/模型探针去重，全局探针并发限制。结果只能写回对应配置，过期结果不能覆盖新配置。
- UI 保存透出后可以文本使用，行级展示探测中/未检测/支持/不支持/失败、时间及刷新操作；其他行和空间不被网络等待锁住。
- 普通诊断仅记状态码与固定错误码，不保存请求/响应正文、图片或凭据。
- additive schema v24；旧二进制不支持更高 schema，回退须用升级前备份或向前修复，不能直接降版本号。升级只写 R Codex 自有数据库。

## 并行分工与共享接口

| 负责人 | 文件范围 | 状态 |
| --- | --- | --- |
| 数据任务 | `src-tauri/src/model_exposure/`、database 迁移与其测试 | 完成并验证 |
| 探针任务 | `src-tauri/src/model_image_probe.rs` 及独立测试 | 完成并验证 |
| 界面任务 | `src/` 下的类型、控制器、渠道目录和空间筛选、夹具 | 完成，类型/构建/浏览器夹具通过 |
| 主任务 | commands/lib 装配、启动/空间写入校验、路由能力、集成验证、文档 | 完成并验证 |

IPC（前后端命令）统一为：

- `list_provider_model_exposures(providerId)` → 模型状态数组。
- `set_provider_model_exposure(providerId, upstreamModelId, exposed)` → 最新模型状态；首次透出需要探测时异步执行并返回结果。
- `refresh_model_image_probe(providerId, upstreamModelId)` → 最新模型状态；显式重复探测。
- 状态字段：`providerId`、`upstreamModelId`、`exposed`、`imageProbeState`、`imageProbeCheckedAt`、`imageProbeErrorCode`、`referencedBy`（空间名数组）。探测中是行级临时状态。

## 验收与状态

- [x] 数据：旧选择回填、事务回滚、引用拒绝、配置隔离、缓存复用、过期结果防覆盖。
- [x] 探针：Responses/Chat 成功、假成功、明确不支持、认证/限流/上游失败、超时与响应大小边界；仅本机 mock。
- [x] 集成：透出后可选、无透出渠道禁用、取消引用提示、显式刷新、启动能力读取证据。变更只读取新启动能力，活动 RouteSet 不改写。
- [x] 检查：前端类型/构建、Rust 格式/编译/库测试、Chrome 真实页面夹具。
- 当前阻塞：无。既有工作树的多空间、缺凭据修复、档案面板等其他修改保留，未提交。
- 回滚：保留升级前数据库备份；不删除空间、历史、实例或运行目录。

## 2026-11-07 实施与验证记录

- 数据：`database/migrate_v24.rs` 新增两表并回填既有未删除空间的普通模型，图片证据不回填。迁移在现有事务内完成；正常数据库升级不修改空间/实例/会话。部分旧测试结构按列检查后跳过无关回填。
- 写入：`model_exposure/` 提供唯一偏好/证据服务；取消检查全部未删除空间引用。`managed_launch/workspace.rs`、`prepare.rs` 和最终 binding 事务都校验透出；新 RouteSet 的看图字段只取当前成功证据。旧的族默认授予入口已移除。
- 网络：`model_image_probe.rs` 使用固定 256×128 左蓝右黄图片；答案必须正确识别左右颜色。5 秒建连、25 秒总超时、64 KiB 回包上限、512 输出 token 上限；禁止跳转和重试。`commands/model_exposure.rs` 同配置/模型去重并限制同时执行两个探针，等待和请求期间不持有数据库锁，过期答案不写回。
- 界面：独立 `components/model-exposure` Hook 负责行级状态与错误；原模型目录抽屉挂载 `provider-model-exposure-list.tsx`，空间和添加渠道入口按透出状态筛选。状态读取失败显式提示，已有模型仍可从空间移除。
- PASS：`pnpm run check`；`pnpm run build`（前端有大于 500 KiB chunk 提示）。
- PASS：`cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`；`cargo check --manifest-path src-tauri/Cargo.toml --all-targets`（测试模块存在未使用导入提示）；`git diff --check`。
- PASS：`cargo test --manifest-path src-tauri/Cargo.toml --lib`：403 passed、0 failed。其中新增 Store 8 项、迁移 3 项、图片探针 10 项、启动集成 3 项和调度守卫 1 项。
- PASS：无头 Chrome 在真实前端夹具上完成 13 项断言：空间过滤、引用禁用、角色/生图排除、打开目录零探针、行级等待、首次一次、缓存复用、显式刷新、不支持文本可用、超时归类、读取失败、无透出渠道禁用、无页面异常。首次以 `tmp/model-exposure-ui-check.mjs` 执行，原脚本随后原样移入 `scripts/model-exposure-ui-check.mjs` 并补运行说明。
- 环境：macOS 本机、Node.js 22.17.1、Chrome；本机端口测试在沙箱内因 EPERM/PermissionDenied 拒绝，获工具审批后以本机 mock 重跑通过。没有使用真实 Provider 或用户凭据。
- 未执行：真实 Codex App/CLI 会话看图、Windows 桌面、真实模型渠道探测、应用打包和发布。自动验证不能替代这些外部合同验收；首次实际探针由用户勾选/刷新触发。
- 下一步操作：commander 自行启动/重启开发环境后，进入「模型渠道 → 模型目录 → 透出/刷新图片探针」，确认结果；成功后重启对应隔离实例以应用目录能力。开发服务保持由 commander 控制。
- 回归命令：先运行 `pnpm exec vite --host 127.0.0.1 --port 5185 --strictPort`，再运行 `node scripts/model-exposure-ui-check.mjs http://127.0.0.1:5185`。可通过 `MODEL_UI_CHROME` 指定 Chrome 可执行文件。
- 2026-11-07 补充（commander 方案）：待分配上游中未透出的渠道仍列出但显示「不可用」，悬停提示先透出模型。`ChannelAddMenu` 用 `disabled-hint` 包装层 + Mantine Tooltip 替换原生 title（禁用按钮不派发鼠标事件），状态列/圆点按 `data-tone="不可用"` 显示红色；其他阻塞（未配置、协议未确认、读取失败）同样显示不可用并给出各自原因。`scripts/model-exposure-ui-check.mjs` 该断言同步改为检查 不可用 文案与包装层 aria-label；`scripts/workspace-ui-check.mjs` 新增「已透出渠道仍显示可用、未透出显示不可用」对照断言。
- PASS：`pnpm run check`、`pnpm run build`；`node scripts/workspace-ui-check.mjs` 19 项通过；`node scripts/model-exposure-ui-check.mjs` 13 项通过（仅前端 vite，跑完已停）。未执行真实渠道悬停手感验收（commander 开发窗口复核）。
