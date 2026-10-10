# R Codex 内部版：Codex 账户管理与本地 API 服务执行计划

> 状态：I1a/I1b/I1d/I1e/I1f-0/I1f-1 已实现；官方账户与工作区治理仍未开始  
> 更新：2026-07-17
>
> 历史说明：本文保留内部版早期路径。2026-08-25 之后的 Codex 多账户、schema v6、多 Provider RouteSet 与实例密钥实现，以[混合模型路由执行计划](codex-hybrid-routing-execution-plan.md)为准。

## 前端包管理器迁移（2026-08-08）

### 目标

- 将前端依赖管理统一为 pnpm，避免 npm 与 pnpm 混用造成锁文件和 `node_modules` 结构漂移。

### 范围

- 从现有 `package-lock.json` 导入 `pnpm-lock.yaml`，重新安装前端依赖。
- 更新 README、Tauri `beforeDevCommand`/`beforeBuildCommand` 和验收命令。
- 不修改 React/Tauri 业务代码、Rust 依赖或 Gateway 数据格式。

### 回滚与验收

- 在删除 npm 锁文件前完成 `pnpm install --frozen-lockfile`、类型检查和生产构建；这些检查通过后才完成清理。
- 若未来需要回滚，应从版本库恢复 `package-lock.json` 并重新执行 npm 安装；该迁移不触碰应用数据、SQLite 或系统凭据。

### 状态

- 已完成 `pnpm import`、pnpm 依赖目录重建、Tauri 前端钩子切换和 npm 锁文件清理。
- `pnpm run check` 与 `pnpm run build` 已通过；Tauri 开发启动命令保留给本机终端进行最终窗口验收。

## 目标

- 内部版完整支持 Codex 的多账户管理、Hourly/Weekly 配额展示、套餐识别，以及本地 Codex API 服务。
- 用户可选择多个已明确授权的账户或 API-key Provider，并从 R Codex 显式启动隔离实例；配置只应用于本次新实例，直接启动 Codex 保持原始默认配置。实例运行期间，其启动配置及依赖禁止变更或删除。
- 保持可开源核心：领域模型、回环 Gateway、Profile/配置投影和本地安全存储接口不依赖内部服务。

## 范围与非目标

### 内部版范围

1. **多账户与实例**：官方登录后的账户库、API Key Provider、不可变启动配置修订、运行时配置锁和独立 `CODEX_HOME` 实例；不导入认证材料，不修改 Codex 默认账户。
2. **限额与套餐**：展示最近一次成功读取的主/次官方限额窗口、重置时间、套餐/工作区和刷新错误；仅在窗口时长可验证时才标为 Hourly/Weekly，失败绝不显示为零。
3. **API-key Provider**：保存用户明确提供的 API key、Base URL、模型/Provider 配置，测试连通性并生成受管理 Profile。
4. **本地 Codex API 服务**：仅回环监听、独立 Client API Key、`health/models/responses`、SSE 取消、请求元数据和上游健康状态。
5. **启动配置物化**：只在受管理实例的独立 `CODEX_HOME` 中原子写入非敏感配置；不再生成或修改用户默认目录中的可复用 Profile，不修改项目级敏感 Provider/认证设置。

### 明确非目标（除非后续单独批准）

- 默认 LAN/公网暴露、共享 Gateway、请求/响应正文落盘。
- 静默读取浏览器 Cookie、无提示导入认证数据、自动唤醒、配额规避或自动账户轮换。
- 复制 Cockpit Tools 的源代码或其 OAuth/账户池实现。

## 架构决策

### 凭据与配置边界

| 数据 | 唯一权威来源 | 允许写入方 | 禁止写入位置 |
| --- | --- | --- | --- |
| API key、Gateway Client Key | OS Keychain / Credential Manager | `secrets` adapter | SQLite、日志、UI 状态、普通快照、`config.toml` |
| 官方 Codex OAuth/刷新凭据 | 账户私有 `CODEX_HOME` 中的官方 Codex 认证存储 | 官方 app-server | R Codex 的 SQLite、日志、UI 状态、快照、导出与 Gateway |
| 账户/Provider/实例元数据与审计 | SQLite | `app` service | `auth.json` |
| 受管理实例的启动配置 | 不可变启动配置修订；API Key 子进程环境或官方凭据修订 `CODEX_HOME` | `LaunchService` | 默认 `~/.codex`、Shell/系统环境、Codex App 设置 |
| Provider、Base URL、模型、Profile 配置 | 受管理 Profile / 启动配置修订 | `codex_fs` adapter | 默认 Codex 登录、项目级敏感配置 |

`auth.json` 不是多账户数据库。它只由官方 Codex 在某一官方凭据修订的隔离 `CODEX_HOME` 中管理；R Codex 不读取、投影、展示、导出或复制它。用户直接启动 Codex 时仍使用自己的默认 `~/.codex`，不会继承 R Codex 上一次选择。

### 认证模式

- **API-key Provider**：用户显式输入 key；应用保存 secret reference，Profile 只引用受控的凭据 helper 或官方凭据存储。
- **ChatGPT/Codex 账户**：只走官方 Codex app-server 的浏览器/设备代码流程；由官方客户端拥有 OAuth 回调、token 刷新与撤销。R Codex 保存脱敏账户元数据和隔离目录引用，不解析、伪造、复制或共享刷新 token。
- **Gateway 上游**：首批支持 API-key 上游。基于 ChatGPT/Codex OAuth 的请求转发仅在官方协议、用户授权范围与发布审查均明确后启用；它不是 API-key 功能的前置条件。

### 单一写入路径与状态

所有账户连接、API Key 更新、启动配置生成、Gateway 绑定与实例启动都走：

```text
Draft -> Validated -> AwaitingConfirmation -> Snapshotting -> Applying
-> Verified -> Completed
                         \-> Restoring -> Failed
```

- `app` service 是 SQLite 元数据、计划与审计的唯一写入者。
- `secrets` adapter 是 secret 的唯一写入者。
- `codex_fs` adapter 是 Codex 文件的唯一写入者，并以文件 hash 做乐观并发检查。
- `ConfigurationLockService` 在实例启动事务中获取配置锁；账户、Provider、Profile、Gateway 和删除写路径必须先确认无活动锁，否则返回 `configuration_in_use`。
- 配额是可过期的读模型；刷新失败保留最后成功值和错误信息。
- Gateway 与配额刷新互相隔离：配额失败不能中断已有 API-key Gateway 请求。

## 分期与验收

### I0：安全基础与 Gateway 契约

- 修复 Tauri/Rust 构建、实现结构化 404/401/503、mock upstream 和 SSE 成功/取消集成测试。
- 验收：仅监听 `127.0.0.1`；每个 Gateway key 独立；secret 不出现在序列化状态、日志或构建产物。

### I1：API-key Provider、Profile 与隔离实例

- SQLite 元数据、Keychain/Credential Manager adapter、Provider/Account CRUD、原子 Profile 投影、API-key upstream、实例生命周期。
- 验收：两个 API-key Provider 可各自启动隔离实例；切换失败后原 Profile、端口与凭据投影不变；Gateway 可真实完成一次流式请求。

### 当前切片：OpenAI API Key 认证与 Gateway 上游（I1a）

**范围**：先交付一个活动的 OpenAI API-key Provider，而不是多账户 UI、SQLite、Profile 写入或自定义 Base URL。用户在界面显式输入 key 后，应用将 key 写入 OS 凭据库；Gateway 启动时从凭据库读取，并将 `GET /v1/models` 与 `POST /v1/responses`（含 SSE）转发到 `https://api.openai.com/v1`。

**命令契约**：

- `openai_api_key_status`：只返回是否已配置及固定 Provider 名称，绝不返回 key 或其可逆片段。
- `save_openai_api_key(apiKey)`：拒绝空白、控制字符或仅空格的值；成功后只返回脱敏配置状态。
- `clear_openai_api_key`：Gateway 运行中拒绝清除，避免已有服务继续持有不可见旧 key。
- `start_gateway`：无已配置 key 时返回 `upstream_unconfigured`；成功后仅将 key 保留在 Gateway 进程内存。

**HTTP 契约**：Gateway 验证本地 Client API Key 后，以保存的 OpenAI API Key 设置上游 `Authorization: Bearer`；保留上游成功响应和 SSE 字节流，网络/超时错误映射为带错误层级和稳定错误码的结构化 `502/504`，上游非 2xx 额外返回状态码和限长脱敏摘要。`/v1/models` 与 `/v1/responses` 均不把上游 key、请求正文或完整响应正文写入日志或状态。

**验收**：空 key 被拒绝；保存后重新读取状态仍只显示“已配置”；未配置时 Gateway 无法启动；mock upstream 收到正确 Bearer 认证，SSE 字节流可透传；本地错误 key 仍为 401；清除运行中的 key 被拒绝。

### 当前切片：多 API-key Provider 注册表与默认绑定（I1b）

**目标**：将 I1a 的单一 OpenAI key 迁移为多个明确命名的 Responses-compatible Provider。每个 Provider 有独立的名称、Base URL、Keychain/Credential Manager 密钥项和稳定 ID；Gateway 每次启动只绑定一个默认 Provider，不在运行时热切换。

**数据与写入边界**：

- SQLite 仅保存 `id`、`name`、规范化 `base_url`、`secret_ref`、`is_default` 和时间戳；`secret_ref` 是不含 key 的 Keychain 条目名。
- Keychain/Credential Manager 是每个 API Key 的唯一正文存储；Provider 删除先清除该 key，随后删除元数据。若元数据删除失败，Provider 留在库中并显示为“未配置”，以便用户重新保存 key 或重试删除。
- 所有 Provider 写入走 `ProviderStore`；Gateway 只读取一个已绑定 Provider 的快照，不持有 SQLite 连接，也不让 UI 直接访问凭据库。
- 已保存的 I1a `openai-api-key` 会在首次启动时迁移为名为 `OpenAI` 的默认 Provider，保持现有用户密钥可用。

**状态与失败行为**：Base URL 仅允许 HTTPS，或明确回环地址的 HTTP；名称不能为空且最多 80 个字符。已有 Provider 时永远保持一个默认项；删除默认项会原子选择最早创建的其余 Provider 为默认项。Gateway 运行时拒绝创建、选择、更新或删除 Provider，防止界面状态与实际运行上游不一致。

**命令契约**：`list_api_key_providers`、`create_api_key_provider`、`select_default_api_key_provider`、`delete_api_key_provider`。所有返回值为脱敏 Provider 摘要，绝不包含 API Key 或可逆片段。

**验收**：可创建两个 Provider 且密钥分别保存在不同凭据项；选择默认项后重启 Gateway 使用该 Provider；删除默认项会选择替补；运行中的 Gateway 拒绝配置写入；旧单 key 可迁移为默认 Provider。

### 当前切片：Provider 传输模式（I1c）

每个 API-key Provider 选择且只选择一种模式：

| 模式 | Codex 请求路径 | 凭据边界 | 适用上游 |
| --- | --- | --- | --- |
| `direct_responses` | Codex → Provider | 通过受管理 Profile 的凭据 helper 读取系统凭据库；禁止在 TOML 写入 API key | 原生兼容 Responses |
| `gateway_passthrough` | Codex → R Codex Gateway → Provider | 真实 API key 仅由 Gateway 从系统凭据库读取 | 原生兼容 Responses，需要本地控制面 |
| `gateway_chat_completions` | Codex → R Codex Gateway → 转换器 → Provider | 同 passthrough | 仅兼容 Chat Completions |

**Profile 投影契约**：启用或切换模式必须生成命名用户级 Profile，而不是修改项目 `.codex/config.toml`。投影先写入临时 TOML、解析校验、创建快照后原子替换；失败时删除临时文件并保留原 Profile。`direct_responses` 的 Profile 指向 Provider Base URL；两个 Gateway 模式指向回环 Gateway URL。API key 不写入 `config.toml` 或 `auth.json`。

**转换器范围**：I1c 接受 JSON `POST /v1/responses` 的文本字符串或多消息文本输入和 `stream` 标志；将其转换为 `POST /v1/chat/completions` 的 `messages`，并把非流式响应和 OpenAI-style SSE 文本 delta 转换回完整 Responses 生命周期事件。工具调用、图像、MCP、推理摘要传递和多轮 `previous_response_id` 必须明确返回 `unsupported_capability` 或从模型能力中移除，不能伪造兼容性。

**验收**：三种模式可在 Provider 卡片选择；模式字段持久化；direct Profile 无 Gateway URL 且不含 key；两种 Gateway 模式有回环 URL；Chat Completions mock 的普通与流式文本响应能转换；不支持的请求得到结构化 400。

**实施状态（2026-07-29）**：传输策略已收归启动方案；Provider UI 只维护上游连接和凭据，旧 Provider 模式字段保留兼容。`gateway_passthrough` 已实现 Responses/SSE 透传，`gateway_chat_completions` 已实现受限文本转换及结构化 `unsupported_capability`。三种模式都能创建受管理 Profile 并启动 Codex：直连 Profile 指向 Provider；Gateway Profile 指向动态回环 URL。Profile 均先解析、快照、原子替换，且只含 Base URL、Responses 协议与 `OPENAI_API_KEY` 环境变量名。macOS 的受管理启动器从系统凭据库读 Provider Key 或临时 Gateway Client Key，并仅注入其启动的 Codex 子进程；临时终端脚本仅含启动器路径、Profile 和不可逆 secret reference。Gateway 停止时会删除临时 Client Key。Tauri sidecar 已可构建。

**API Key 用量边界**：API Key 不是 ChatGPT/Codex OAuth 身份凭据，不能可靠推导 ChatGPT Plan、Hourly 或 Weekly 配额。Provider 卡片应显示安全状态和上游入口，而不是伪造余量；官方 OAuth 账号的套餐/配额显示留在 I2。对于 OpenAI API Key，用户通过 OpenAI Usage 页面查看实际 API 账单与用量。

### 当前切片：API Key 用量与余额面板（I1d）

**目标**：参照 Cockpit 的 API Key 面板，显示可审计的本地 Gateway 用量；仅在已适配的上游余额接口可用时显示真实余额，且始终标注数据来源与刷新时间。

**读写边界**：Gateway 是请求结果的唯一写入者。它为当前绑定 Provider 写入脱敏 `usage_events`：时间、模型、成功/失败、已观测 input/output token、估算成本（若未来配置模型价格）。UI 仅通过 `get_provider_usage_summary` 读取日/周/月聚合，绝不接触请求正文或 API Key。SQLite 是本地统计的权威来源；事件写入为 best-effort，写入失败不能影响模型请求。

**上游余额契约**：`upstream_balance` 与本地 `usage` 是两个独立字段。余额只通过具名、只读 adapter 查询，adapter 必须有超时、缓存时间、错误状态和来源标签；不使用面板 JWT（JSON Web Token，登录会话令牌）替代 Provider API Key，也不把任意路径探测结果当成余额。系统凭据库读取成功后只在当前 Rust 进程缓存，钥匙串仍是持久化真源；Codex 启动器是独立进程，首次读取时仍可能触发一次自己的系统凭据授权。

**统计语义**：请求数和成功率涵盖所有 Gateway 请求；Token 为上游响应中可解析的 `usage` 字段之和，未返回 usage 的流式请求仍计入请求数但不计入 Token。估算费用在没有 Provider 模型价格表时为 `null`，UI 显示“未配置价格”，不能显示为余额或零费用。

**验收**：同一 Provider 的请求在日/周/月各自聚合；错误请求影响成功率但不伪造 token；未知/流式 usage 不显示为零 token；Provider 切换后统计按 Provider 隔离；余额不可用时保持明确状态，不影响 Gateway；任何统计记录不含 API Key、请求正文或响应正文。

### Cockpit API Provider 卡片对齐（I1d-2，重新规划）

#### 目标与范围

以 Cockpit 截图中的中间“API 服务”卡片为交互参考，为 R Codex 的 API-key Provider 提供完整、可追溯的卡片读模型；不把右侧 OAuth Codex 账户卡片的套餐/Hourly/Weekly 配额混入 API-key Provider。

#### 字段对照与当前缺口

| Cockpit 卡片信息 | R Codex 目标 | 当前状态 | 主要问题/边界 |
| --- | --- | --- | --- |
| 名称、当前标记、API 类型、健康状态 | Provider 身份、默认标记、`API Key` 类型、最近检测状态 | 部分已有 | 缺少明确的 Provider 类型、最后一次活动时间和状态摘要 |
| Base URL、供应商、传输方式 | Endpoint、Provider kind、直连/透传/转换 | Base URL/传输方式已有 | `provider_kind` 尚未持久化，无法选择余额/能力适配器 |
| API Key 遮罩、查看、复制 | 遮罩状态、替换密钥、复制安全引用 | 仅遮罩状态 | 原始 API Key 不应进入前端，不能照搬“查看/复制明文” |
| OAuth 绑定 | 可选 `account_id` 关联 | 未实现 | OAuth 账户是独立资源，不能伪造为 API Key 登录 |
| 供应商切换、设为当前 | 最近使用 Provider | 已有 | 由启动图标隐式更新；不再提供单独的“设为当前”按钮 |
| 账户余额 | 余额、币种、来源、刷新时间、过期状态 | 固定 `unsupported` | 没有通用余额契约；不能猜测私有 URL 或把余额显示为 0 |
| 今日请求、今日 Token | 本地请求、输入/输出 Token、成功率 | 后端已有，卡片仅摘要 | 直连模式没有 Gateway 观测，必须显示“未观测”而不是 0 |
| 最后请求/更新时间 | 最后活动、余额刷新时间 | 未在读模型展示 | 需要从事件聚合和余额缓存分别提供时间戳 |
| 标准/渠道/模型选择 | 已验证默认模型、通道或人工策略 | 未实现 | 属于 I1f-2 模型目录/能力档案，不应凭探测结果自动覆盖用户选择 |
| 详情、终端、标签、编辑、启动、刷新、导出、删除 | 对应真实操作 | 部分已有 | 终端/标签/导出/余额刷新尚未实现；导出必须剔除密钥 |

#### 读模型与数据边界

新增面向 UI 的 `ProviderCardView`（只读投影），由 Tauri 业务层组装，不让 React 直接拼接多个 Store：

```text
ProviderCardView {
  identity: { id, name, kind, type, is_default, configured },
  endpoint: { base_url, transport_mode, upstream_label },
  auth: { state, masked, secret_ref_present },
  account_binding: { account_id?, login_method?, label? },
  health: { status, http_status?, error_code?, checked_at? },
  local_usage: { today_requests, today_tokens?, success_rate?, observed_by, last_request_at? },
  upstream_balance: { status, amount?, currency?, windows?, source?, refreshed_at?, expires_at?, error? },
  model_policy: { selected_model?, channel?, capabilities, verified_at? },
  actions: { can_test, can_edit, can_select, can_launch, can_export, can_delete }
}
```

- API Key 永远只由系统凭据库读写；`ProviderCardView` 不包含明文密钥。
- `local_usage` 与 `upstream_balance` 是两个独立来源；直连请求显示 `observed_by=none`，不是零请求。
- 所有可选上游字段必须携带来源、刷新时间和错误状态；刷新失败保留最后一次成功值并标记 stale。
- Provider 删除、常用启动配置选择、Gateway 运行中锁定等权限仍由 Provider/Gateway 业务层决定，UI 只消费 `actions` 能力，不自行推断。

#### 上游余额适配器

定义 `QuotaAdapter` 注册表，按显式 `provider_kind` 选择适配器：

```text
refresh_upstream_balance(provider_id)
  -> ProviderStore 校验 Provider 与 secret_ref
  -> QuotaRegistry 按 provider_kind 选择已启用 adapter
  -> adapter 使用固定域名、超时和只读请求
  -> BalanceStore 原子写入 value/source/refreshed_at/expires_at/error
  -> ProviderCardView 返回 fresh/stale/unsupported/error
```

适配器按固定顺序组成链：

1. 团队专用 `token-profile`：仅当主机为 `ai.3rcd.com` 或 `gpt.3rcd.com` 时请求同一 Origin 下的 `/api/cockpit-tools/token-profile`，读取 `data.usage.total_available`、`total_granted`、`total_used`、`unlimited_quota` 和 `expires_at`。
2. Sub2API `/v1/usage`：使用 Provider API Key 请求与 Base URL 相同版本前缀下的 `/v1/usage`，解析 `quota_limited` 的额度、已用、剩余与过期时间，或 `unrestricted` 的订阅/钱包剩余额度。该端点由 Sub2API 官方 Gateway 为 CC Switch 等客户端提供，余额和用量都按当前 API Key 作用域返回，不需要面板 JWT。
3. OpenAI 兼容 `credit_grants`：对 API-key Provider 使用同一 API Key Bearer 认证请求 Origin 根路径下的 `/dashboard/billing/credit_grants`，解析 `total_granted`、`total_used`、`total_available`、`currency` 和授权额度的 `expires_at`。这是兼容契约，不代表所有上游都必须提供该接口；404、HTML 登录页或字段不匹配时标记为不支持。

官方 `api.openai.com` 不启用该兼容适配器，避免把官方 API Key 的 OAuth/Usage 语义误报为中转余额。适配器必须使用 Provider 输入的 Origin，不将 `/v1` 重复拼接到根路径；不根据任意 Base URL 猜测 `/balance`、`/quota` 等私有路径，也不把 OpenAI API Key 误解释为 ChatGPT 套餐或 Codex Weekly 配额。返回的数值默认标记为“额度单位”，只有上游明确提供币种时才显示货币符号。每个 adapter 必须有脱敏测试 fixture、路径约束、超时、缓存 TTL 和失败回退。

**余额适配器契约（I1d-2b）**：余额刷新是独立于连通性检测和 Gateway 请求的异步操作；请求超时为 10 秒，成功值写入 `provider_upstream_balances`，失败时保留上次成功的数值并返回 `stale`/错误状态。适配器按顺序尝试，只有 404、非 JSON 页面或契约不匹配才继续下一个适配器；401/403 返回 `unauthorized`；其他 HTTP/网络错误返回 `error`。API Key 只在 Rust 请求作用域内短暂使用，不进入日志、SQLite 或 React 状态。

#### 分阶段实施

1. **I1d-2a：ProviderCardView 与字段投影**
   - 扩展 Provider 元数据：`provider_kind`、可选供应商标签、可选模型策略引用。
   - 增加最后请求时间、今日 Token/请求/成功率、连通性摘要和动作能力。
   - 卡片展示 Endpoint、密钥状态、来源标签、今日用量、最后活动时间；直连模式明确“本地未观测”。

2. **I1d-2b：卡片交互与操作栏**
   - 右侧抽屉展示完整卡片；列表卡片保持扫描级信息。
   - 增加独立“刷新连接”和“刷新余额”状态，不让余额刷新阻塞 Gateway 或其他 Provider。
   - 连接检测完成后明确区分余额更新成功、认证拒绝、上游不支持和刷新异常，不再只显示“已尝试刷新”。
   - 支持安全导出（不含 API Key）、备注/标签和 Provider 配置复制；不提供明文密钥复制。

3. **I1d-2c：余额/订阅适配器**
   - 先接入一个有明确公开契约的 Provider adapter，再推广到其他上游。
   - 支持金额、币种、额度窗口、重置时间、来源、刷新时间、过期和错误状态。
   - 增加 fake upstream、超时、401、字段缺失、过期缓存和并发刷新测试。

4. **I1f-2：模型/渠道/能力档案**
   - 保存模型目录快照、用户选择的默认模型/通道、Responses/SSE/工具/MCP/上下文能力及验证时间。
   - “标准”只作为用户可选策略，不把一次探测成功自动宣传为完整兼容。

5. **I2：Codex OAuth 账户卡片**
   - 单独建立 `CodexAccount` 资源，负责登录方式、Plan、Team、Hourly/Weekly 配额、刷新时间、过期和授权错误。
   - Provider 可选绑定账户，但 API Key Provider 与 OAuth 账户的凭据、刷新和失败状态完全隔离。

#### 关键问题与决策门

- **“余额”不是一个通用字段**：API Key 可能对应预付费余额、月度账单、代理套餐或无限制账户，必须显示语义和来源。
- **“今日 Token=0”容易误导**：直连和未返回 usage 的流式请求都应显示未知/未观测。
- **Cockpit 的 OAuth 错误不能复制到 API Key 卡片**：截图右侧的区域限制、套餐和 Weekly quota 属于官方账户登录链路。
- **明文密钥操作不对齐**：Cockpit 的查看/复制交互不应原样复制到本项目；本项目优先替换密钥、系统钥匙串状态和安全导出。
- **卡片信息过多会降低扫描效率**：列表卡片只放身份、状态、Endpoint、今日本地用量和关键动作；余额、模型能力、OAuth 绑定和完整错误进入右侧抽屉。

#### 验收标准

- API-key Provider 卡片可显示身份、类型、Endpoint、传输方式、密钥状态、连接状态、今日请求/Token、成功率、最后活动时间和数据来源。
- 余额不可用时明确显示 `不支持/未配置/刷新失败/已过期`，不显示虚假数值；支持 adapter 时显示金额、币种、窗口、重置时间、来源和刷新时间。
- 直连 Provider 与 Gateway Provider 的本地用量语义不同且可解释；Provider 切换后数据不串卡。
- 卡片上的刷新、编辑、导出、启动、删除全部有真实状态和失败回退；导出包不含 API Key。
- OAuth 账户的 Plan、Hourly/Weekly quota 在独立账户卡片展示，Provider 绑定只显示关联状态。
- Rust 单测覆盖读模型、迁移、Provider 隔离、适配器失败回退；Tauri 原生验收覆盖卡片、抽屉、刷新 loading、禁用提示和窄窗口布局。

### I1g：Provider 自有 Gateway 配置与卡片启动（2026-08-06）

**交互契约**：Provider 卡片不再提供显式“设为当前”。点击卡片的启动图标即选择该 Provider 作为最近使用项：直连模式直接生成隔离 Profile 并启动 Codex；Gateway 模式按该 Provider 保存的 Gateway 超时策略启动本地服务，再生成对应 Profile 并启动 Codex。

**配置归属**：传输方式和 Gateway 超时策略都属于 Provider。编辑或新增 Provider 时，选择任一 Gateway 模式后在同一右侧抽屉显示连接、首字节、流式空闲和非流式总超时；直连模式隐藏这些字段。SQLite 的 `gateway_timeout_settings` 继续以 `provider_id` 为键，不新增重复配置源。

**运行边界**：本机仍只有一个 Gateway 运行实例。`start_gateway(provider_id)` 显式绑定一个 Provider 并将绑定 ID 投影到状态；若另一个 Provider 已绑定，必须先停止当前 Gateway，不能热切换。独立 Gateway 页面不再作为配置入口，运行状态和停止操作保留在概览与侧栏状态区。

**删除与失败行为**：删除按钮打开应用内确认对话框，不依赖 WebView 的 `window.confirm`。确认后由 ProviderStore 清除系统凭据、用量、诊断、连通性报告和该 Provider 的 Gateway 设置；失败显示可见错误且不伪装成功。卡片操作统一使用带 tooltip 和 `aria-label` 的圆形图标按钮。

**验收**：每个 Provider 可保存不同 Gateway 超时策略；从任意 Provider 卡片启动时使用该项配置且更新最近使用标记；运行中的其他 Provider 禁止启动/编辑/删除并显示原因；删除确认可取消、可成功提交，删除后无孤立 Gateway 设置；直连与 Gateway 启动图标均为播放语义。

### I1h：Codex 启动目标与 CLI 工作目录（2026-08-09）

**目标**：Provider 的启动图标先让用户选择 Codex CLI 或 Codex 桌面 App；CLI 继续使用受管理终端启动器，并在本次启动时指定工作目录。

- `Codex CLI`：保留当前隔离 Profile、系统凭据库注入和终端体验；工作目录由用户输入，Rust 启动器在启动子进程前校验目录存在且为目录，并通过 Codex 的 `--cd` 参数传入。
- `Codex App`：通过 `codex app <目录>` 打开桌面应用，不创建终端窗口；工作目录作为 App 要打开的 workspace。当前 API-key Provider 的密钥/Profile 不注入桌面 App，因为 App 的官方启动契约没有 API-key/profile 参数；UI 必须明确提示 App 使用自身登录状态。
- 启动目标不写入 Provider 数据库，避免把一次性工作目录误当成账户配置；后续可单独增加“记住上次目录”设置。
- 空工作目录表示使用用户主目录；非法或不存在的目录必须在后端返回可理解错误，不能启动到 R Codex 工作区作为隐式替代。

**验收**：Provider 卡片与概览启动均能打开选择抽屉；CLI 选项显示工作目录并继续打开终端；App 选项不打开终端且传入 workspace；取消不产生进程；非法 CLI 目录被拒绝；UI 明确 App 不使用 API-key Provider 凭据。

### I1i：Provider 可靠性收尾（2026-08-10）

**目标**：消除普通页面刷新导致的上游请求和系统凭据库读取，确保 Provider 的最近使用标记、Gateway 生命周期以及 Provider 元数据和 API Key 在失败时不产生误导性状态。

**刷新契约**：`refresh` 只读取 Gateway 状态、Provider 摘要、SQLite 中的本地用量/余额缓存、连通性报告和诊断记录。它不得读取 API Key、调用余额 adapter 或访问网络。余额只由用户显式执行“检测连接”时刷新；失败保留缓存并显示既有错误状态。前端同时只允许一个状态读取请求，重复操作复用该请求，避免旧响应覆盖新状态。

**启动提交契约**：Provider 的最近使用标记在 CLI 终端或 Codex App 已成功派发后才写入。Gateway 仅负责绑定上游和监听回环端口，不单独更新最近使用标记。若本次卡片启动新建了 Gateway 而后续 Codex 派发失败，前端必须停止该 Gateway；之前已运行的 Gateway 不自动停止。

**凭据与数据库一致性**：OS 凭据库与 SQLite 没有跨存储事务，因此 `ProviderStore` 采用补偿写入：替换 API Key 前先读取旧值，SQLite 更新失败时恢复旧值；删除前读取旧值并清除凭据，SQLite 删除事务失败时恢复旧值。恢复失败必须返回明确错误，不能伪装 Provider 已保存或删除。

**启动器清理**：CLI 临时 `.command` 脚本在 Codex 退出后自删除；启动前仅清理由 R Codex 创建且超过保留期的旧脚本。受管理 Profile 的备份仍保留，作为配置恢复证据，不在此任务自动清除。

**验收**：普通刷新没有余额网络请求或凭据库读取；并发刷新不覆盖更晚的状态；错误工作目录或 Codex 启动失败不改变默认 Provider；Gateway + Codex 联合启动失败后不保留新 Gateway；模拟 SQLite 失败后 API Key 被恢复；CLI 退出后临时脚本被删除。

**实例生命周期补充（2026-08-07）**：Tauri 注册官方 Single Instance 插件作为第一个插件。重复启动会退出新进程并聚焦已有主窗口，避免多个应用进程同时持有 Gateway、SQLite 和系统凭据状态。

### I1j：系统凭据访问节流与发布签名（2026-08-10）

**目标**：避免开发热重载或常规页面刷新对每个 Provider 重复读取 macOS 钥匙串，同时保留 API Key 只由系统凭据库持久化的安全边界。

**读模型契约**：`api_key_providers.configured` 是 SQLite 中不含密钥的“最近一次凭据写入成功”状态。Provider 列表、普通刷新、用量/余额缓存和 Profile 投影只读取该状态，不读取系统凭据库。它用于界面上的遮罩状态和禁用提示，不是凭据原文或授权绕过。

**显式密钥访问**：只有连接检测、余额刷新、Gateway 启动、CLI 启动、替换密钥和删除密钥可以读取或写入凭据库。`ProviderStore` 是唯一的凭据状态写入者；若显式读取发现条目不存在，会将 `configured` 标记为未配置，后续 UI 不再尝试读取。进程内成功读取继续缓存至退出；缓存不写入 SQLite、日志或 React 状态。

**兼容与失败行为**：旧数据库新增 `configured` 列时一律迁移为已配置，避免升级时枚举所有历史密钥。旧版单一 OpenAI Key 的迁移只尝试一次并记录结果；钥匙串拒绝或错误不能阻止应用启动，也不会在每次重启重试。用户仍可在 Provider 抽屉中重新添加 API Key。

**macOS 发布约束**：生产 `.app` 和 `codex-launcher` sidecar 仍应用同一稳定 Apple 签名身份签名并在外部分发时完成公证。登录钥匙串默认 ACL 会把 ad-hoc 编译当成新调用方，因此不能靠「始终允许」实现一次授权。2026-10-02 起 macOS 将模型渠道 API Key 写入同一钥匙串条目 `3r-secrets`，迁完后一次授权即可长期使用；实例准入密钥不进钥匙串。`codex-launcher` 读取实例密钥只走 FIFO，管道缺失时不再回退钥匙串。不得把 API Key 降级保存到明文文件或 SQLite。详见 [macOS 钥匙串授权与存储](designs/2026-10-01-macos-keychain-access.md)。

**验收**：三个已保存 Provider 的启动刷新不触发三次凭据库读取；显式检测在单进程内只读取一次并复用缓存；外部删除凭据后首次显式操作将卡片标为未配置，随后不再读取；旧数据库升级不枚举 Keychain；生产构建文档明确同一签名身份覆盖主程序与 sidecar。

### I2：官方 Codex 账户、套餐与配额

- 认证、授权、账户/API Key 选择、启动配置修订和隔离实例以[多账户、API Key 与启动隔离方案](codex-official-auth-authorization-design.md)为准。官方 app-server 是唯一登录与状态适配器；OAuth 令牌、Cookie、`auth.json` 和私有 usage 接口都不进入本产品的导入、存储或 Gateway 路径。
- 当前实施和验收进度以[Codex 多账户与受管理启动执行计划](codex-auth-launch-execution-plan.md)为准；旧 I1 中公开生成用户级 Profile、把 App 作为 Provider 启动目标的行为已经移除。
- 官方浏览器/设备代码编排、账户状态读取、套餐、手动刷新与可选限额快照已实现；后台刷新、阈值通知和真实双账户发布验收仍待完成。
- 验收：官方登录可取消、失败或完成；刷新异常不覆盖上次成功配额；只有从 R Codex 点击启动的新 CLI 使用所选账户/API Key；直接启动 Codex 保持默认配置；实例处于启动、运行、停止中或未核验状态时，相关配置修改、重新授权、断开与删除均返回 `configuration_in_use`；官方认证失效由 Codex 直接报错，不自动换号、修复或重启；协议不兼容时不降级为私有接口或 token 导入。

### I3：内部版本地 API 服务完善

- 多个受管理 Client API Key、模型策略、健康状态、脱敏请求元数据、配置接管/恢复与可复现诊断。
- 验收：Key 禁用/轮换立即生效；未受支持路径为结构化 404；所有接管操作均生成可恢复快照。

### I4：内部定制层与开源切分

- 将内部特定 Provider、企业身份/审计导出和部署策略作为 adapters，不进入开源核心。
- 验收：开源构建不包含内部 secret、端点或许可证受限代码；内部 adapters 可独立装配和测试。

## 当前阻塞与下一步

- Cockpit Tools 为 CC BY-NC-SA 4.0；只能参考公开功能与交互，不能复制其代码或将其实现作为内部商业版本基础。
- ChatGPT/Codex OAuth Gateway 的可用授权和协议边界尚未确认，故仅作为受限的后续里程碑。

## 活动状态

- **I1a（OpenAI API Key 认证与 Gateway 上游）**：已实现，Rust 单元与回环 Gateway 转发测试已通过。
- **I1b（多 API-key Provider 注册表与默认绑定）**：已实现并完成 Rust、前端与 Gateway 回归验证。
- **已完成**：系统凭据库 adapter、无 secret 的状态/保存/清除命令、启动前密钥检查、OpenAI `models` 与 `responses` 转发、SSE 字节流透传、结构化 404、前端安全输入与配置状态。
- **验证证据**：TypeScript 检查与 Vite 生产构建通过；`cargo test` 通过 7 个库测试，其中包括本地错误 key、空白 key、无 secret 状态序列化、结构化 404，以及回环 mock upstream 的 Bearer 认证与 SSE 透传。
- **已处理环境项**：应用图标已补入 `src-tauri/icons/icon.png`（透明 512×512 PNG），Tauri 的 `generate_context!` 可正常编译。
- **I1b 已完成代码**：SQLite Provider 注册表、旧单 OpenAI key 迁移、每个 Provider 独立 Keychain 引用、默认 Provider 选择、删除回退、运行中禁止配置变更、Gateway 默认绑定和 Provider 列表/添加/选择/删除界面。
- **I1b 验证证据**：TypeScript 检查、Vite 生产构建与 Rust 格式检查通过；`cargo test` 通过 9 个测试，覆盖两个 Provider 的独立密钥、默认项切换、删除默认项后的回退、URL 安全边界，以及 Gateway 的 Bearer/SSE 透传。
- **I1d（API Key 用量与余额面板）**：已实现。Gateway 在本地 SQLite 的 `provider_usage_events` 中以 best-effort 方式记录绑定 Provider 的请求结果；只保存时间、模型、成功状态和上游响应中已观测的 input/output token，不保存 API Key 或请求/响应正文。Provider 卡片显示日／周／月请求、成功率、输入/输出 Token 与未配置价格状态；流式或未携带 `usage` 的请求保留 Token 未知。已适配团队 `token-profile`、Sub2API `/v1/usage` 和兼容 `credit_grants`，其他上游仍明确显示 `unsupported`，不影响请求转发。
- **I1d-2a（Provider 卡片字段投影）**：已完成首版。Provider 列表补充 API Key 类型、供应商主机、最近活动时间和最近检测时间，并提供“账户余额 / 今日请求 / 今日 Token”三格摘要；详情抽屉显示上游余额状态、来源和最近活动时间。API Key 与 OAuth 账户仍保持独立边界。
- **I1d 验证证据**：TypeScript 检查与 Vite 生产构建通过；Rust 格式检查与 `cargo test` 通过 30 个测试，包含非流式 Responses 解析 token 用量、流式用量未知、Provider 隔离聚合和最近活动时间投影；Tauri 原生应用已验证三格摘要与编辑抽屉。
- **I1e（Gateway 诊断与 Provider 连通性/能力检测）**：已完成。新增脱敏诊断事件与持久化的 Provider 检测报告；Provider 卡可手动检测模型列表和一次最小文本请求，Gateway 首页显示最近诊断。详细路线、数据边界、验收和后续阶段见 [Codex 路线与 Gateway 诊断](./codex-roadmap-and-gateway-diagnostics.md)。
- **I1f-0/I1f-1（数据库治理与 Gateway 稳定性）**：已完成。所有 Store 共享单一 SQLite 连接，启用 WAL/foreign keys/busy timeout，迁移由 `user_version` 管理；诊断保留 30 天、用量保留 90 天，Provider 删除会清理关联本地历史。Gateway 支持连接/首字节/流式空闲/非流式总超时，禁止模型 POST 自动重试，并将上游中断、流空闲超时和客户端取消写入稳定诊断码。超时设置按 Provider 持久化，Gateway 停止后可在 UI 修改，下一次启动采用不可变快照。
- **I1f 验证证据**：Rust 30 个测试通过；TypeScript 检查与 Vite 生产构建通过。完整回环测试需要本机绑定权限，已在受控环境中通过。
- **I1d-2（Cockpit API Provider 卡片对齐）**：I1d-2a 字段投影与 I1d-2b 余额适配器链已落地；后续再将当前域名匹配迁移为显式 `provider_kind`/`QuotaAdapter` 注册表。
- **I1d-2b（Provider API 余额适配器）**：团队 `token-profile`、Sub2API `/v1/usage` 与通用 OpenAI `credit_grants` 适配器已纳入实现范围；均复用 Provider API Key，支持余额/额度、已用额度、无限额、币种或单位、来源、刷新时间、过期时间和 SQLite 缓存。对 `ai.3rcd.com` 的当前诊断显示 `token-profile` 为 404、`credit_grants` 为 HTML 页面，而 `/v1/usage` 是存在的鉴权 API，因此后续刷新优先使用 `/v1/usage`。刷新失败保留最近成功值并标记缓存/错误状态；脱敏 fixture 覆盖路径匹配、字段解析和缓存回读。
- **I1d-2b 验证证据**：`cargo fmt`、TypeScript 检查与 Vite 生产构建通过；Rust 全量测试在允许本机临时端口的环境中通过 43 个测试，包含 Sub2API 余额解析、余额缓存、数据库迁移和既有 Gateway 回环测试。团队域名的线上请求只做了不带密钥的状态探测：`token-profile` 返回 404、`credit_grants` 返回 HTML 页面、`/v1/usage` 返回 401，未把真实 API Key 或外部服务纳入自动化测试。
