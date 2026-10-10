# R Codex Codex 支持路线与 Gateway 诊断

> 状态：I1e、I1f-0、I1f-1、I2 核心与 V3.1 信息模型已完成（2026-08-12）  
> 依据：Cockpit Tools / CC Switch 的公开产品行为；实现独立完成，不复制其代码或凭据处理方式。

Codex App 支持预研已于 2026-08-28 完成：当前只保留默认 App 打开入口，不把受管理 CLI 的 Provider/Gateway 运行实例边界扩展到官方桌面 App；默认 App 兼容路由经过本机 Gateway 的请求仍写入全局请求诊断账本。详见 [Codex App 支持预研](codex-app-support-research.md)。

## 目标

- 将 R Codex 建成以安全 Provider、受管理 Profile 和回环 Gateway 为核心的 Codex 本地控制面。
- 优先让用户清楚知道“当前 Provider 是否可用、为什么失败、Gateway 实际做了什么”，再扩展官方账户与复杂协议转换。
- 保持 API Key 与 OAuth 凭据不写入 SQLite、诊断记录、请求日志或普通配置文件。

## 对比与路线

| 能力域 | Cockpit / CC Switch 已提供的参考 | R Codex 当前状态 | 建议阶段 |
| --- | --- | --- | --- |
| Provider、Profile 与 API Key | Provider 切换、预设、配置恢复 | 多 API Key、三种传输模式、Keychain、命名 Profile | 已完成基础；I1e 补连通性和能力结果 |
| Gateway | 本地服务、模型策略、请求日志、超时/重试、健康状态 | 回环 `/health`、`/models`、`/responses`、流式与基础用量 | I1e 诊断；I1f 稳定性/策略 |
| Responses 兼容 | 原生透传和复杂 Chat 转换 | 透传完整；Chat 转换支持文本多消息与完整 SSE 生命周期 | I1f 扩展 tool/MCP/多轮 |
| 用量与费用 | 请求日志、价格快照、按账户/模型统计 | 日/周/月本地请求和 Token；无价格表 | I1f 模型价格与详细日志 |
| 官方 Codex 账户 | 登录、套餐、配额、多实例 | 浏览器/设备码登录、套餐、可选限额、隔离启动与锁已实现 | I2 核心完成；真实双账户交互仍为发布门槛 |
| 配置治理 | 备份、接管、恢复、模型目录 | 受管理命名 Profile；无接管中心 | I2：预览、快照、冲突检测、恢复 |
| MCP / Skills / 项目规则 | 统一管理和同步 | 尚未实现 | I3：全局/项目分层只读扫描优先 |
| 团队能力 | 同步、预设与内部策略 | 尚未实现 | I4：加密导入导出、内部 adapter；不作为开源核心依赖 |

### 明确边界

- 不默认开放 LAN 或公网 Gateway。
- 不自动轮换 OAuth 账号，也不使用 OAuth 作为未经批准的转发上游。
- 不用复制 `auth.json`、把 token 写入 SQLite，或将第三方 API Key 明文写进 `config.toml` 的方式实现“保留官方登录”。
- 对没有公开、可配置且被明确选择的余额接口，不猜测路径或显示虚假余量。

## I1e：Gateway 诊断与 Provider 连通性/能力检测

### 用户可见结果

每张 Provider 卡可手动执行“检测连接”。检测会明确提示它会访问上游且**可能产生一次最小模型请求费用**。完成后显示：

- 当前状态：`ready`、`degraded`、`unreachable`、`unauthorized` 或 `not_checked`；
- `models`、基础文本请求两项检测结果及所用模型；
- 由传输模式推导的能力提示：原生 Responses 透传、Chat 文本多消息转换受限、工具/MCP/图像/服务端历史是否已支持；
- 最后检测时间和脱敏错误码/HTTP 状态，绝不显示 Key、请求正文或上游响应正文。

Gateway 首页显示最近诊断：时间、Provider、模型、路径、HTTP 状态、耗时、是否流式、结果分类和稳定错误码；历史与测试记录同时显示最近的请求级错误摘要。首页事件列表只展示最近 20 条；不记录请求或完整响应正文。

受管理运行实例另有请求账本：同一实例可以包含多个 Provider，因此每一条请求都保存实际 Provider/修订、记录时间、状态、HTTP 状态、首字节/总耗时和上游明确返回的 `input_tokens`、`output_tokens`、`total_tokens`。上游没有返回 usage 时显示为未知，不依据请求字节数估算 Token；这张账本与 Provider 的日/周/月汇总同时写入，但互不替代。

### 契约与边界

```text
UI -> test_api_key_provider(provider_id)
   -> ProviderStore 读取指定 Provider 的 Keychain key
   -> ConnectivityService 以固定 15 秒超时访问 Provider
   -> DiagnosticsStore 写入检测结果（单一写入者）
   -> UI 读取 ProviderConnectivityReport

Codex -> Gateway /v1/responses
   -> Gateway 转发或转换
   -> DiagnosticsStore best-effort 写入实例级脱敏 request ledger
   -> UsageStore best-effort 写入 Provider 聚合事件
```

- `DiagnosticsStore` 是诊断事件的唯一写入者；它与 `UsageStore` 分离，避免统计失败影响请求，也避免诊断 UI 反向修改用量。
- 检测只允许针对已保存 Provider；Gateway 运行时仍禁止配置变更，但允许读取和检测。
- `GET /models` 成功后保存去重后的完整模型目录，并选择第一个非空模型 ID 发起一次最小文本探测。模型列表不可读时，文本请求标记为 `not_run`，不凭空选择模型；用户后续在启动配置中明确勾选需要暴露的模型。
- 探测的成功以 HTTP 2xx 与可解析 JSON 为准。协议先试 Responses，只有路径明确不存在才再试一次 Chat Completions；401、403、429、5xx 和超时不换协议。检测失败不改变 Provider、默认项或 Gateway 状态。
- Gateway 诊断为 best-effort：SQLite 写入失败、读取失败或诊断表迁移失败不能中断模型请求；首次打开失败会作为 Tauri 启动错误，避免应用在未知持久化状态下运行。

### 数据模型

```text
gateway_diagnostic_events {
  id, provider_id, created_at, source("gateway"|"probe"),
  route, model?, transport_mode, stream,
  outcome("success"|"failed"),
  http_status?, latency_ms?, error_code?
}

gateway_request_diagnostics {
  id, instance_id, session_id, created_at,
  provider_id, provider_revision_id, public_model_id, upstream_model_id,
  wire_api, stream, http_status, outcome, error_code,
  error_detail,
  first_byte_ms, total_latency_ms,
  input_tokens?, output_tokens?, total_tokens?
}
```

普通诊断仅记录上述字段。`error_code` 是 R Codex 本地稳定码；请求账本中的 `error_detail` 只保存经过提取、脱敏和限长的非敏感错误摘要（最多 1024 个字符；当前实现先读取完整错误响应，超过 4096 字节则不提取摘要，该值不是实际读取上限），不保存原始请求/响应正文、URL 查询项、Authorization 或 API Key。HTTP 错误响应另行返回 `error.layer`、`error.gateway_code` 和 `error.upstream_status`，避免把 Gateway 自身的 502 与上游 Provider 返回的 502 混为一谈。已有流包装器记录结束/中断，但仍有 EOF 被无条件计为成功的缺口，不能把传输结束误解为模型生成成功，见文末 2026-09-16 审查。

### 异常响应包装（2026-09-10）

受管理 Gateway 的错误响应统一包含：

- `error.layer`：`gateway` 或 `upstream`；
- `error.gateway_code`：R Codex 稳定错误码；
- `error.upstream_status`：上游实际 HTTP 状态，仅在已收到上游响应时存在；
- `error.upstream_detail`：从上游错误中提取的 `code`/`message` 或限长纯文本摘要。

本地网络、路由、准入和响应解析失败归类为 `gateway`；上游返回非 2xx 归类为 `upstream`，即使上游状态也是 502。每条失败请求写入请求级诊断；诊断写入失败不能改变模型请求的错误响应。

### 验收与验证

- mock Provider 的模型列表与基础 Responses/Chat 请求成功时，报告 `ready` 并保存两条 probe 事件。
- 401/403、超时/连接失败、模型列表无模型、模型请求非 2xx 分别得到可区分但脱敏的状态。
- Gateway 成功、上游 HTTP 失败、网络失败都会记录不含 secret 或正文的诊断事件；统计写入仍不受影响。
- `gateway_recent_diagnostics` 仅返回最近 20 条，按时间倒序并按 Provider 可筛选。
- `clear_gateway_diagnostics` 清空本地 Gateway 事件和请求级异常账本；不删除 Provider 配置、用量统计或最近一次 Provider 检测报告，前端必须二次确认后才能调用。
- Rust 单测/回环集成测试、TypeScript 检查、生产构建通过；桌面端至少手动检查成功、空状态与失败状态。

## 后续排程

### I1f-0：SQLite 统一初始化与数据生命周期

**目标**：在增加 Gateway 运行策略、模型目录和价格快照前，建立单一数据库初始化/迁移入口，避免多个 Store 各自迁移同一 SQLite 文件。

- `Database` 负责连接创建、`PRAGMA`、schema version/migration、应用数据目录和超时设置表；Provider、Usage、Diagnostics 只负责各自领域读写。
- 连接统一启用 `foreign_keys`、WAL（平台可用时）和有限 busy timeout；迁移失败阻止应用启动，不以部分 schema 继续运行。
- 诊断默认保留 30 天、用量默认保留 90 天，应用启动时自动清理；Provider 删除的关联记录按隐私优先默认清除，不删除系统凭据直到元数据删除成功，失败时返回可恢复错误。
- 迁移保持向前兼容；不改写 API Key、`auth.json` 或其他 secret 文件。

**验收**：全新数据库和现有数据库均能启动；所有 Store 使用同一初始化入口；重复启动不会重复迁移；过期诊断/用量会自动清理；并发 Gateway 诊断写入不因 Store 初始化互相锁死；迁移失败有明确错误且不会静默降级。

### I1f-1：Gateway 稳定性与可控超时（下一切片）

**目标**：把当前固定的 120 秒上游超时，升级成可查看、可配置、可诊断的 Gateway 运行策略，而不在不确定条件下自动重放模型请求。该切片已完成基础设置持久化和 UI 配置入口。

- 每个 Gateway Provider 增加连接超时、非流式总超时、流式首字节/空闲超时；变更需先停止 Gateway，下一次启动绑定新的不可变快照。设置存储于 SQLite 的 `gateway_timeout_settings`。
- 健康页显示当前生效策略与回环/本机代理绕过检查；失败日志使用 `connect_timeout`、`first_byte_timeout`、`stream_idle_timeout` 等稳定错误码。
- `GET /models` 与手动连通性检测可有限重试；`POST /v1/responses` 默认不自动重试，防止网络不确定时造成重复计费或重复副作用。
- 为流式转发增加终止包装器，区分 `success`、客户端取消、上游中断和超时；不记录正文。

**验收**：mock 的连接失败、首字节超时、流空闲和客户端中断分别被分类；流式取消关闭上游连接；策略变更不会热改正在运行的 Gateway；UI 可读取和保存 Provider 策略；默认仍仅回环监听。

### I1f-2：模型目录、能力档案与价格表

**目标**：将“检测到的模型”和“用户确认的可用模型”分开，避免把上游临时模型列表或虚假的 1M 上下文配置当作保证。

- 保存 Provider 的模型目录快照、默认探测模型、人工启用/禁用模型和传输能力标签；未知模型不自动进入 Codex 默认选择。
- Provider 卡展示请求协议、工具/MCP/多轮、流式、图像和上下文窗口的“已验证 / 未验证 / 不支持”状态。
- 增加用户维护的每模型输入/输出/缓存 Token 价格；每次用量事件保存价格快照，费用报表从快照计算，历史不随价格表修改而重算。
- 将 `model_context_window`、自动压缩阈值作为受管理 Profile 可选项；明确提示其是客户端请求配置，实际受上游模型限制。

**验收**：模型目录刷新不会覆盖人工策略；价格修改不改变既有费用；无价格、无能力或无上下文上限时均显示未知而非零或“已支持”。

### I1g：Chat Completions 路由扩展（按实际 Provider 决定）

**目标**：只为已验证需要 Chat-only Provider 的团队场景扩展转换器，而不削弱原生 Responses 透传路径。

- 先建立 Responses ↔ Chat 的 fixture 矩阵；逐项加入工具调用、工具结果、多轮、SSE 事件和明确的未支持错误。
- 原始 Responses 结构、call ID 和流式事件顺序必须保留；无法无损映射时拒绝请求，不做近似伪造。

**决策门**：先由 commander 用真实 Provider 的检测/使用结果确认需要的能力，再决定是否开始此切片。

### I2：Codex 多账户、API Key 与启动隔离中心

**目标**：在官方支持范围内管理 ChatGPT/Codex 登录、API Key、套餐/配额与多实例，并确保所选配置只有在用户从 R Codex 显式启动新 CLI 时生效；直接启动 Codex 保持原始默认配置。实例运行期间锁定其启动配置及依赖，禁止变更与删除。

认证、授权、权限边界、启动修订、状态机、数据模型、失败处理和验收标准以[多账户、API Key 与启动隔离方案](codex-official-auth-authorization-design.md)为准。本阶段只接入经目标 Codex 版本 app-server schema 验证的官方能力；任何不兼容都停止在账户连接入口，不能退化为私有 HTTP 或令牌导入。

- 通过官方 Codex app-server 的浏览器/设备码流程登录；不导入浏览器 Cookie，不将 OAuth token 作为普通业务字段存储或转发。每个账户使用隔离 `CODEX_HOME`，认证文件由官方进程拥有。
- 账户读模型显示套餐、配额、刷新时间、过期和错误；刷新失败保留上次成功数据。
- 提供用户级配置/Profile 的 diff、快照、写后解析、外部变更冲突检测和恢复；`auth.json` 只由官方流程或明确隔离实例拥有。
- 多实例按不可变启动配置修订固定账户/API Key、Provider、Gateway 与 Profile，并持有运行时配置锁；相关实例退出前，编辑、替换、重新授权、断开与删除均返回 `configuration_in_use`。官方认证失效由 Codex 直接报错，R Codex 不自动换号、修复或重启；同时不修改默认 `~/.codex`、Codex App 登录或系统环境。

**实施状态**：I2 核心与 V3.1 差异化信息模型已完成，具体契约、验收证据和剩余真实账户门槛见[执行计划](codex-auth-launch-execution-plan.md)。OAuth 仍不进入 Gateway 上游或自动轮换范围。

### I3：Codex 工作区治理

- 全局、Profile、项目级 `AGENTS.md`、MCP、Skills 与项目 `.codex` 配置的只读扫描、来源层级和差异预览。
- 先做导入建议与复制补丁，再考虑对受信任项目进行受确认的原子写入。
- 增加加密导入/导出和内部 Provider adapter 边界；跨设备同步保持可选且不进入开源核心。

## 执行顺序与状态

| 阶段 | 状态 | 前置条件 | 交付判断 |
| --- | --- | --- | --- |
| I1e 诊断与检测 | 已完成 | 无 | 19 个 Rust 测试、前端构建、桌面调试已验证 |
| I1f-0 数据库治理 | 已完成 | 无 | 统一迁移、锁策略和保留边界可验证 |
| I1f-1 稳定性 | 已完成 | I1f-0 | 超时/取消分类、策略持久化与快照可验证 |
| I1f-2 模型与价格 | 待开始 | I1f-1 | 模型能力和价格快照可追溯 |
| I1g Chat 扩展 | 需确认 | 明确的 Chat-only Provider 需求 | fixture 矩阵完整且不降级原生透传 |
| I2 官方账户/恢复 | 核心与 V3.1 已完成 | 发布前双真实账户测试 | OAuth 安全边界、隔离启动、锁与可选限额已自动化验收 |
| I3 工作区治理 | 待开始 | I2 配置恢复基础 | 分层扫描、diff 和受确认写入可回滚 |

## 执行

### 2026-08-26：受管理实例请求级协议诊断与脱敏对话摘要

> 接管记录（2026-08-26）：异常会话在 Gateway 接线编译阶段中断；以下实现与验证项由当前会话继续收敛。诊断写入保持 best-effort（不阻断模型请求），实例级查询/清除走 Tauri 命令，实例删除依赖 SQLite 外键级联。

- [x] 在受管理 Gateway 请求/响应边界记录实际 `stream`、`wire_api`、公共/上游模型、Provider revision、HTTP 状态、响应 Content-Type、首字节/总耗时以及成功、超时、中断、格式错误分类。
- [x] 以实例和可稳定识别的会话键关联本地脱敏摘要：时间、输入/输出长度、不可逆摘要 hash、工具调用类型及结果摘要 hash；默认不保存提示词、回复、请求/响应体或任何凭据。
- [x] SQLite 请求诊断与摘要按实例读取；删除实例通过外键级联清除关联记录，显式清除入口保留二次确认且不触碰默认 Codex 配置。请求诊断/会话摘要的 30 天自动清理尚未接入，不能与已经清理的 Gateway 事件混为一谈（2026-09-16 核对）。
- [x] 运行实例详情展示协议诊断和对话摘要，便于定位 stream、响应类型及卡住阶段；补充 stream/非 stream、成功/失败、脱敏/sentinel、清除/级联和 UI 类型渲染回归测试。

架构决策：`ManagedGatewayRuntime` 注入可选 `DiagnosticsStore`，请求处理器是请求级事件的唯一写入者；流式响应由包装器在首字节、完成、上游中断、空闲超时和客户端取消处写入一次终态事件。摘要只接受已解析的长度/hash/工具元数据，不跨越到原始正文持久化。诊断写入 best-effort，SQLite 故障不影响上游请求，但删除实例的生命周期事务必须依赖 `ON DELETE CASCADE` 清理关联摘要。

- [x] 写入路线、契约和验收标准。
- [x] 增加诊断持久化、Tauri 查询/检测命令与 Provider 读取契约。
- [x] 在 Gateway 成功/失败路径记录诊断事件。
- [x] 在 Provider 卡和 Gateway 首页展示检测与最近诊断。
- [x] 统一 SQLite 初始化、迁移、WAL/busy timeout、过期数据清理和 Provider 关联记录清理。
- [x] 增加分层超时、流式终止诊断、无重试策略、SQLite 设置持久化和 UI 配置入口。
- [x] 运行 29 个 Rust 测试、前端检查/构建；同步文档状态。

### 接管后的活动状态

- [x] 读取异常会话进度并复现当前编译错误。
- [x] 修复 Gateway 请求级诊断的实例关联、Content-Type 接线和 SQLite 读模型。
- [x] 完成流式/非流式与 Chat 转换路径的终态记录及回归测试。
- [x] 在运行实例详情展示诊断与脱敏摘要，并提供二次确认清除入口。
- [x] 运行完整发布验证并按要求仅生成可直接打开的 `.app`。

当前状态：已完成；最后验证为 Rust 全量测试通过、`cargo check --all-targets`、`cargo fmt -- --check`、`pnpm run check`、`pnpm run build` 通过，并成功生成仅 `.app` bundle。应用仍为 macOS ad-hoc 签名，未做 Developer ID 公证。

## 活动状态

- 当前状态：I1e、I1f-0、I1f-1、I2 核心与 V3.1 已完成；I1f-2 与 I3 待开始。
- 阻塞：无；真实 Provider 探测会访问上游且可能产生一次最小文本请求费用，需由用户在桌面端手动触发。
- 上次验证：Rust 格式检查与 29 个测试通过（含迁移、超时、流式取消、模型列表/文本成功、401 不发文本探测、Gateway 诊断）；TypeScript 检查、Vite 生产构建通过；Tauri 调试窗口已启动检查新界面。
- 发布验证：Tauri Release 应用已成功编译；DMG 已使用系统 `hdiutil` 手工生成并通过镜像格式校验，当前产物仍为 ad-hoc 签名，未做 Developer ID 公证。
- 下一步：发布官方账户功能前，用两个受控真实账户完成浏览器/设备码登录、隔离启动、失效异常和默认环境不变验收；Provider 路线可并行进入 I1f-2。

### 2026-08-28：多 Provider 运行实例请求账本

- [x] schema v8 为请求诊断增加可空 `input_tokens`、`output_tokens`、`total_tokens`，旧库安全迁移且历史记录不估算 Token。
- [x] 独立 `codex-gateway` sidecar 在 Responses 非流式、Responses SSE、同步响应转 SSE 和 Chat Completions 转换路径采集上游 usage，并与实例、Provider revision、状态、时间和时延写入同一请求记录。
- [x] 运行实例详情展示渠道、上游模型、记录时间、状态、协议/流式形态和输入/输出/总 Token；同一实例的多个 Provider 可直接区分。
- [x] 保持 best-effort 与隐私边界：usage/诊断写入失败不阻断请求；请求/响应正文、API Key 和 Authorization 仍不落盘；无 usage 时显示未知。

验证：新增迁移、usage 解析、非流式、同步转 SSE 与 Responses SSE 回归；Rust 全量 153 个库测试、Gateway/launcher/official-auth 测试、TypeScript 检查、前端构建和 macOS `.app` 打包均通过。

### 2026-09-17：完整请求档案与正确性补全（首版已接入，持续验收）

已接入显式的按会话完整记录模式，独立保存 Codex 入口请求、实际上游请求、上游原始响应和实际下游响应；普通诊断的无正文合同保持有效。详见[专项设计及验收矩阵](designs/conversation-request-archive-design.md)。默认 App 优先交付，原生会话身份经版本探针验证，无法关联的请求单列；不能用当前 hash/session 摘要假装已映射原生文件。剩余工作是默认 App 图形合同复验和基于档案的三方旧会话字段差异定位。

本轮已修复跨块 UTF-8/CRLF SSE 解码、EOF 终态误判、同步转流式输出类型、Chat 历史不支持项静默跳过和有界错误正文读取，并接入流式上下游采集。普通诊断投影仍保留既有同步 best-effort 路径；归档保留期自动 prune、导出、完整统计和默认 App 图形合同仍未完成。上述改造不能反向推断此前 502 的唯一根因。

目标写入路径已改为 sidecar 专属归档工作线程与有界队列：四阶段内容存私有文件，查询索引独立 SQLite。队满、磁盘故障和中断明确标记记录缺失，不阻塞模型转发；现有普通诊断/用量仍是同步 best-effort，后续需迁入同一投影队列以消除锁等待。

采集、原生元数据关联、修复备份与普通诊断具有不同所有权和保留期；记录功能不得写入原生会话或认证数据。正文保留期/容量、分页读取、差异、未知用量、清理、导出和故障恢复一起验收，不单独上线无界正文日志。
