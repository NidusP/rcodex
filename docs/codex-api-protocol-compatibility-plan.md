# Codex API 协议兼容与 Gateway 转发改造方案

> 状态：方案记录；协议适配已落在 Managed Gateway，历史单 Provider Gateway 已删除  
> 日期：2026-09-14  
> 相关设计：[Codex 混合模型路由详细设计](codex-hybrid-routing-design.md)、[Codex 混合模型路由执行计划](codex-hybrid-routing-execution-plan.md)

## 目标

当前 R Codex 的客户端入口统一使用 Responses API（响应生成接口），第三方 Provider（模型渠道）可能只提供 Chat Completions API（传统聊天生成接口）。本方案记录两类工作：

1. 梳理当前 Responses 转发链路，并列出需要优先修正的正确性、兼容性和可维护性问题。
2. 增加 Chat Completions 与 Responses 之间的协议适配，使 Chat-only Provider 可以服务当前 Codex，同时为外部 Chat Completions 客户端保留兼容入口。

本方案只讨论 API 协议和 Gateway 数据面，不改变 Provider 凭据边界、RouteSet（一次启动时固化的模型路由集合）、实例准入密钥或默认 App 的回环代理策略。

## 1. 协议边界

Responses 和 Chat Completions 都是模型生成合同，分别定义请求输入、响应结构、流式事件和可用参数。`/v1/models` 是独立的模型发现接口，只提供模型 ID 和基础元数据，不能证明某个模型支持哪种生成合同或推理、工具、图像能力。

```text
/v1                         API 版本命名空间
├── GET  /models             模型发现与基础元数据
├── POST /responses          Responses 生成合同
└── POST /chat/completions   Chat Completions 生成合同
```

协议应拆成三个层次：

| 层次 | 责任 | 典型数据 |
|---|---|---|
| 发现层 | 得到候选模型 ID | `GET /v1/models` |
| 生成层 | 发送输入并接收模型输出 | `POST /v1/responses`、`POST /v1/chat/completions` |
| 能力层 | 判断工具、图像、推理、结构化输出等能力 | RouteSet 能力、Provider 文档、受控探针 |

`/v1` 只是版本路径，不代表后面的接口都属于同一个生成合同。R Codex 的模型目录文档也明确说明 `/v1/models` 不承载推理档位：[第三方模型推理档位与 Codex 目录生成](third-party-model-reasoning-levels.md)。

## 2. 当前实现基线

### 2.1 本地 Gateway 入口

Managed Gateway 当前公开：

```text
GET  /health
GET  /v1/models
POST /v1/responses
```

路由定义位于 [`managed_gateway.rs`](../src-tauri/src/managed_gateway.rs)。本地没有 `POST /v1/chat/completions`，因此 Chat Completions 客户端直接访问本地 Gateway 会得到 404。

### 2.2 当前请求路径

```text
Codex
  -> POST /v1/responses
  -> binding 准入密钥校验
  -> JSON 与 model 校验
  -> RouteSet 精确匹配公开模型 ID
  -> 能力过滤与预检
  -> 公开模型 ID 改为上游模型 ID
  -> Provider 上游协议分派
```

对应代码入口是 [`responses`](../src-tauri/src/managed_gateway.rs)、[`forward_provider`](../src-tauri/src/managed_gateway.rs) 和 [`forward_responses_response`](../src-tauri/src/managed_gateway.rs)。

### 2.3 当前两种上游分支

#### Responses 上游

```text
本地 Responses 请求
  -> POST {provider_base}/responses
  -> Responses 响应或 SSE 透传
  -> 上游模型 ID 改回公开模型 ID
```

当前已经处理：

- Provider API Key 注入；
- 连接、首字节、流式空闲和非流式总超时；
- 非 2xx 上游错误的结构化返回；
- 非流式 JSON 解析；
- Responses SSE 转发与模型字段重写；
- 上游同步 JSON 在客户端要求流式时合成为 Responses SSE；
- usage 提取、请求诊断、流式中断和客户端取消记录。

#### Chat Completions 上游

当前已有一条受限的内部适配：

```text
本地 Responses 请求
  -> Responses 请求转换为 Chat 请求
  -> POST {provider_base}/chat/completions
  -> Chat 非流式响应或 SSE
  -> 合成为 Responses 响应
```

主要实现是 [`response_request_to_chat`](../src-tauri/src/managed_gateway.rs)、[`forward_chat_completions`](../src-tauri/src/managed_gateway.rs) 和 [`chat_sse_to_responses`](../src-tauri/src/managed_gateway.rs)。

这不是“本地 Chat 请求转 Responses 请求”的入口适配，而是“Codex Responses 请求适配到 Chat-only Provider”的上游适配。2026-10-13 起，工具按协议映射，不按模型名分支。细则见 [Chat Completions 函数工具](designs/2026-10-12-chat-completions-function-tools.md)。

- 支持字符串输入和多消息文本输入；
- `developer` 可映射为 `system`；
- 可映射 `max_output_tokens`、`temperature` 和 `stream`；
- `reasoning` 不会发送到 Chat 上游，也不补 `reasoning_content`；
- function、custom、namespace 和 tool_search 按 Chat tools / tool_calls 互转；web_search 仍不转发；
- `previous_response_id` 和压缩触发器仍拒绝；
- 非流式响应读取第一条 choice 的文本和 tool_calls；
- 流式响应读取 `delta.content`，并按 index 拼装 `delta.tool_calls`。

历史版本的嵌入式单 Provider Gateway 曾有一套相似转换逻辑。它已经不再是运行时入口；协议适配以 Managed Gateway 为唯一实现，避免两条数据面长期产生行为漂移。

## 3. 第一部分：Responses 转发优化

### 3.1 保留的设计

以下行为应继续保留：

1. Gateway 先按 binding 做准入校验，再按 RouteSet 精确路由，不按模糊模型名猜 Provider。
2. 公开模型 ID 与上游模型 ID 分离，请求下游改为上游 ID，响应再改回公开 ID。
3. Provider API Key 只在 Gateway 内存和上游请求中出现，不进入 Codex 配置或客户端响应。
4. 流式请求不经过 UI，不做无界缓冲，并保留上游取消、客户端断开和背压行为。
5. 默认不自动重试生成请求，避免重复计费和重复副作用。
6. usage 和诊断写入失败不能影响模型请求本身。

### 3.2 优化清单

| 优先级 | 当前问题 | 建议 | 用户影响 |
|---|---|---|---|
| P0 | 本地只公开 `/v1/responses` | 增加协议入口分派，明确支持的客户端合同 | Chat 客户端不再直接 404 |
| P0 | Chat 适配静默丢弃部分字段 | 建立字段兼容策略；无损转发、可选删除、不可表示字段分别处理 | 避免请求成功但语义被悄悄改变 |
| P0 | 工具不支持时可能直接删除 `tools` | 自动附带的可选工具可删除；用户显式工具应返回结构化 400 | 用户知道工具为什么不可用 |
| P0 | 上游只返回任意 JSON 就继续处理 | 校验 Responses `object`、必要字段和响应事件类型 | Chat 响应误当 Responses 响应时能明确报错 |
| P0 | 流开始后错误只能表现为断流 | 输出 `response.failed` 或对应的协议级错误事件，再关闭流 | Codex 能区分失败和正常结束 |
| P1 | 转发处理函数重新创建计时起点 | 从发送前统一记录 request、first byte、stream、completed 时间 | 诊断中的延迟与真实请求一致 |
| P1 | SSE 解析按行分散实现 | 抽取共享 `SseFramer`，支持 LF、CRLF、多行 data、跨 chunk JSON 和 `[DONE]` | 减少特定中继的流式兼容问题 |
| P1 | 上游响应头没有白名单转发 | 保留安全的 request ID、retry hint 等，过滤 hop-by-hop 头 | 更容易排查 Provider 错误，不破坏 HTTP 传输 |
| P1 | 非 GPT 删除 `metadata`；GPT 原样转发 | 若某家 GPT 中继仍拒收，再做成该渠道的兼容策略 | GPT 不再丢用户元数据 |
| P1 | 模型字段递归重写范围较宽 | 优先修改 Responses 规范中的已知模型字段 | 降低嵌套业务对象被误改的风险 |
| P2 | 协议适配、SSE 解析、usage 和错误映射需要持续收敛 | 以 Managed Gateway 为唯一数据面，抽取共享适配器 | 所有 Provider 使用同一套转发语义 |
| P2 | Responses 能力预检不完整 | 增加 reasoning、response format、输入模态、流式 usage 等能力字段 | 模型目录与真实行为更一致 |

### 3.3 Responses 请求处理建议

请求过滤应从无条件函数升级为显式策略：

```text
request
  -> parse and validate
  -> resolve route
  -> apply adapter policy
  -> reject unsupported semantic fields
  -> remove only documented optional fields
  -> rewrite public model ID to upstream model ID
  -> send upstream
```

建议默认采用保守策略：

- GPT 族保留 `metadata`；非 GPT 删除该字段。若某家 GPT 中继仍拒收，再做成该渠道的兼容策略；
- 工具、图片、推理和结构化输出无法证明支持时返回稳定错误；
- 只有明确属于可选自动能力的字段才允许被删除；
- 不从 `/models` 响应推断 reasoning 或工具能力；
- 不把响应 JSON 中不存在的 usage 填成零值。

远程压缩也遵循同一条边界：`supports_experimental_context` 只属于 Codex 客户端模型元数据，不能证明第三方上游接受 Codex 的 `compaction_trigger`。目录物化器会从第三方条目删除 GPT-6 Astra 继承的该字段。隔离 / `local_client` 见到 trigger 仍 `unsupported_compaction`，请求不到上游。默认 App `remote_v2` 按路由 `compaction_support` 分派：兼容回退透传、伪造 `gpt-*` fail-closed、其余三方走 GatewaySummarizer。这样既不把 Codex 私有压缩项原样发给不兼容的 Provider，也不静默删除历史内容。

### 3.4 多 Provider 下的压缩能力合同

R Codex 的 `ApiProvider` 是控制面保存的真实上游渠道；Codex 配置中的 `model_provider = "rcodex"` 是一个统一的客户端 Provider 门面，Gateway 再按公共模型 ID 选择多个内部 `ApiProvider`。因此当前 RouteSet 可以按模型区分 URL、Key、协议、工具能力和 `compaction_support`，但 Codex 仍不能在同一个活动 Provider 内按模型选择 `NativeRemote` 或 `LocalClient`；进程级 `compactMode` 只能二选一。

压缩能力不能继续由“GPT/非 GPT”名称推断，也不能靠把托管 Provider 显示名改成 `OpenAI`。进程级模式在启动时写入 RouteSet 快照：隔离 `rcodex` 为 `local_client`（2026-09-18 对本机 `codex-cli 0.155.0-alpha.2.6` 已确认无 `compaction_trigger`，该路径不做 Summarizer）；默认 App 保留内建 `openai` 身份，快照为 `remote_v2`。**当前**名称门按 Codex 合同视为已确定，不再为默认 App 做 trigger 探针。若目标版本日后提供真正生效的官方本地压缩配置（统称 `supports_local_compact`，实际键名以该版本为准），启动物化器探针通过后跟随写入并重算 `compactMode`；今天不得伪造仍被忽略或会打到退役 V1 的键。远程路径上 RouteSet 的 `compaction_support` 才按模型分流：兼容回退/已验证官方合同为 `native_remote`；中继 `gpt-*` 为 fail-closed；其余三方为 `gateway_summarizer`。两者都无法保证时才返回 `unsupported_compaction`。禁止用 `features.remote_compaction_v2 = false` 在 OpenAI 名 Provider 上冒充本地压缩。

统一改名为 `OpenAI` 再按 GPT/非 GPT 分流，是 OpenCodex Design B / CPA compact-bridge 已经验证过的网关形状，只适合默认 App 这条已经保留 `openai` 身份的入口。隔离实例不得改名并入。透传键必须是 RouteSet 上已验证的 `NativeRemote`（官方合同探针通过，或该中继显式实现了 v2 compact），不是模型 ID 像 `gpt-*`。API Key 中继上的 GPT 名默认 fail-closed（`not_native_gpt_compaction`），不得静默 Summarizer 或当 NativeRemote。一次压缩回合内不得因失败改路。旧会话中的 `reasoning.encrypted_content`、工具调用/结果配对、`previous_response_id` 和 Provider 私有 item 仍需独立做历史兼容检查，不能因为压缩成功就宣称跨 Provider 会话完全兼容。对照与评估见[混合路由执行计划 2026-09-18](codex-hybrid-routing-execution-plan.md)。

## 4. 第二部分：Chat Completions 转 Responses

### 4.1 方向说明

需要区分两个方向：

#### 已有方向：Responses 客户端到 Chat 上游

```text
Responses client -> Responses-to-Chat request -> Chat Provider
Chat response -> Chat-to-Responses response -> Responses client
```

这是当前 `gateway_chat_completions` 的用途。它应该被抽取、加固，而不是再复制一份。

#### 缺失方向：Chat 客户端到 Responses 上游

```text
Chat client -> POST /v1/chat/completions
           -> Chat-to-Responses request
           -> Responses Provider
           -> Responses-to-Chat response
           -> Chat client
```

如果用户的目标只是让 Codex 使用 Chat-only Provider，那么现有上游适配器是基础；如果还要让外部 Chat 客户端使用 R Codex Gateway，则必须新增本地 `/v1/chat/completions` 入口。

推荐同时支持两种方向，但按协议是否一致决定是否转换，避免重复转换：

| 客户端合同 | 上游合同 | 处理方式 |
|---|---|---|
| Responses | Responses | 直接透传 |
| Responses | Chat Completions | Responses → Chat |
| Chat Completions | Responses | Chat → Responses |
| Chat Completions | Chat Completions | 优先直接透传 |

### 4.2 推荐模块边界

新增共享协议模块，避免继续把字段转换放在两个 Gateway 文件中：

```text
src-tauri/src/protocol/
├── mod.rs
├── types.rs
├── responses.rs
├── chat_completions.rs
├── adapters.rs
└── sse.rs
```

建议职责如下：

- `types.rs`：客户端合同、上游合同、标准化请求和响应元数据；
- `responses.rs`：Responses 请求/响应提取和合法性检查；
- `chat_completions.rs`：Chat 请求/响应提取和合法性检查；
- `adapters.rs`：四种协议组合的请求与响应转换；
- `sse.rs`：共享 SSE frame 解析、事件缓冲、错误和结束处理；
- Gateway：鉴权、路由、凭据、超时、诊断和调用适配器。

Gateway 层只保留以下流程：

```text
HTTP handler
  -> authenticate
  -> resolve model route
  -> determine ClientApi and WireApi
  -> ProtocolDispatcher
  -> upstream request
  -> ProtocolDispatcher
  -> client response
```

### 4.3 Chat 请求到 Responses 请求

首版只承诺纯文本、历史消息、非流式和流式：

| Chat 字段 | Responses 字段 | 处理规则 |
|---|---|---|
| `model` | `model` | 使用公开 ID 路由，发送前改为上游 ID |
| `messages[]` | `input[]` message item | 保留消息顺序和角色 |
| 文本 `content` | `input_text` | 字符串或文本 part 可转换 |
| `stream` | `stream` | 保持请求模式 |
| `max_tokens` | `max_output_tokens` | 有值时转换 |
| `max_completion_tokens` | `max_output_tokens` | 与 `max_tokens` 冲突时返回 400 |
| `temperature` | `temperature` | Provider 支持时转发 |
| `top_p` | `top_p` | Provider 支持时转发 |

消息应使用有序输入项，而不是把所有 system 消息简单拼成顶层 `instructions`，以免改变多轮消息顺序：

```json
{
  "model": "public-model",
  "input": [
    {
      "type": "message",
      "role": "system",
      "content": [{"type": "input_text", "text": "..."}]
    },
    {
      "type": "message",
      "role": "user",
      "content": [{"type": "input_text", "text": "..."}]
    }
  ],
  "stream": true
}
```

首版暂不承诺以下字段：

- `tools`、`tool_choice` 和工具调用历史；
- 图片、音频和其他多模态内容；
- `response_format`、JSON Schema 和自定义 grammar；
- `stop`、惩罚参数、`seed` 等没有稳定等价字段的参数；
- Chat 客户端自有的服务端会话 ID。

这些字段应根据实际能力矩阵逐项增加，不能在转换器中静默丢弃。

### 4.4 Responses 响应到 Chat 响应

非流式响应的首版映射：

| Responses 字段 | Chat 字段 |
|---|---|
| `response.id` | `id` |
| `response.model` | `model` |
| 第一个 assistant 文本输出 | `choices[0].message.content` |
| 正常完成 | `choices[0].finish_reason = "stop"` |
| `input_tokens` | `prompt_tokens` |
| `output_tokens` | `completion_tokens` |
| `total_tokens` | `total_tokens` |

如果 Responses 输出包含工具调用、reasoning item、多个 output item 或非文本内容，首版应返回结构化兼容错误，不能只取第一个文本后伪装成完整 Chat 响应。

### 4.5 流式转换

需要统一的事件适配：

```text
Responses response.output_text.delta
  -> Chat chat.completion.chunk / choices[0].delta.content

Responses response.completed
  -> Chat finish_reason + usage + [DONE]

Responses response.failed
  -> Chat 风格错误事件并关闭连接
```

反向的 Chat-only Provider 仍然需要：

```text
Chat choices[0].delta.content
  -> Responses response.output_text.delta
Chat stream end
  -> Responses output_text.done
  -> content_part.done
  -> output_item.done
  -> response.completed
```

SSE 适配器必须支持：

- LF 和 CRLF；
- 一个事件中的多条 `data:`；
- JSON 跨网络 chunk；
- `[DONE]`；
- usage 事件；
- 上游错误事件；
- 客户端取消和上游断流；
- 流结束时的残余缓冲。

### 4.6 错误与能力策略

建议统一错误码：

| 错误码 | 含义 |
|---|---|
| `model_required` | 请求没有模型 ID |
| `model_not_in_route_set` | 模型不属于当前 binding |
| `unsupported_capability` | 请求包含当前协议或路由无法表达的能力 |
| `upstream_protocol_mismatch` | 上游响应合同与配置不一致 |
| `invalid_upstream_response` | 上游返回无法解析或不完整 |
| `upstream_stream_idle_timeout` | 流式空闲超时 |
| `upstream_interrupted` | 上游流被中断 |

所有转换都要在诊断中记录：客户端协议、上游协议、Adapter 版本、实际 Provider、公开模型 ID、上游模型 ID、被删除或拒绝的字段以及最终状态。诊断不保存 API Key、请求正文或完整响应正文。

## 5. 分阶段实施计划

### Phase 0：合同确认

- 确认是否需要公开本地 `/v1/chat/completions`；推荐需要。
- 确认首版只承诺纯文本、历史消息、非流式和流式。
- 固定不支持字段的错误语义，禁止静默丢弃。
- 把 `ClientApi`、`WireApi` 和 `AdapterKind` 纳入路由设计。

### Phase 1：抽取并加固现有适配器

- 将历史旧 Gateway 的仍需保留能力收敛到 Managed Gateway；旧单 Provider 运行时不再保留。
- 抽取共享 SSE parser。
- 修正首字节与总耗时统计。
- 增加 Responses 和 Chat 响应合同校验。
- 保留当前 Responses 上游的透传行为。

### Phase 2：增加 Chat 客户端入口

- 新增 `POST /v1/chat/completions`。
- 复用 binding 鉴权、RouteSet 匹配、模型重写、Provider 凭据和诊断。
- Chat → Responses 上游时使用请求适配器。
- Chat → Chat 上游时直接透传，避免双重转换。
- Responses → Chat 客户端时实现非流式和流式响应适配。

### Phase 3：能力扩展

按模型和 Provider 的实际合同逐项增加：

1. 结构化输出和 JSON Schema；
2. 工具定义与工具调用结果；
3. 图片和其他多模态输入；
4. reasoning effort 与 reasoning 输出；
5. 多 choice、finish reason 和完整 usage。

每项能力都必须进入 `CapabilitySet` 和兼容探针结果，不能仅凭模型名称或 `/models` 推断。

## 6. 验收标准

### 协议路径

- Responses 客户端调用原生 Responses Provider 成功。
- Responses 客户端调用 Chat-only Provider 成功。
- Chat 客户端调用原生 Responses Provider 成功。
- Chat 客户端调用 Chat Provider 时不发生重复转换。
- 未配置支持的入口返回结构化 404 或 405，而不是伪造成功响应。

### 请求和响应

- 公开模型 ID 和上游模型 ID 在请求、非流式响应、流式事件中都正确。
- 多轮文本消息顺序保持不变。
- 非流式 usage 正确映射；缺失 usage 时保持未知，不填零。
- 流式事件能处理分块、CRLF、多行 data、`[DONE]` 和错误事件。
- 上游同步 JSON 在客户端请求流式时能生成合法 SSE，但诊断标记为降级。

### 失败路径

- 未知模型、缺少模型、错误 JSON、无效响应、协议不匹配都返回稳定错误码。
- 不支持的工具、图片、推理和结构化输出不会静默改变语义。
- 首字节超时、流式空闲超时、上游断流、客户端取消均有诊断记录。
- Provider API Key、实例准入密钥和请求/响应正文不会进入诊断。

## 7. 验证计划

实现时至少运行：

```text
cargo fmt --all -- --check
cargo check --all-targets
cargo test --all-targets
```

需要补充的回归测试：

- Responses → Responses 非流式和流式透传；
- Responses → Chat 请求字段映射、非流式响应和流式响应；
- Chat → Responses 请求字段映射、非流式响应和流式响应；
- Chat → Chat 直通不会发生二次转换；
- CRLF、跨 chunk JSON、多行 data 和 `[DONE]`；
- usage 缺失、usage 在最终事件、上游错误事件；
- 工具、图片、reasoning、response format 的拒绝或明确降级；
- 模型 ID 重写、Provider 凭据隔离和 binding 隔离；
- 首字节、总耗时、流式空闲超时和客户端取消。

外部真实 Provider 验证应使用受控 mock 或已授权的测试渠道，覆盖至少一个 Responses-only、一个 Chat-only 和一个同时支持两者的服务。

## 8. 设计取舍

- 采用双入口和按协议矩阵选择适配器，避免所有请求强制经过一次无意义转换。
- 保留 Responses 作为 Codex 的主入口和内部路由基准，同时对 Chat 客户端提供兼容入口。
- 纯文本兼容先行，工具、多模态和推理能力按显式合同逐项增加。
- 不把 `/v1/models` 当作生成能力或 reasoning 档位证明。
- 不用模型名称猜协议，不因 404 自动切换另一个上游端点，不在失败时自动换 Provider。
- 不把有损转换伪装成完整兼容；无法表示的请求应返回可解释错误。
