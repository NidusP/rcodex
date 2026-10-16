# 第三方模型目录 overlay：身份、推理档位与骨架

> 状态：已实现并通过完整 Rust、前端构建与本机 Codex 目录合同验证（2026-09-18）  
> 依据：默认 App 兼容目录实测（Grok 4.6 经 uuapi 类中继）、xAI 推理文档、本机 `GET /v1/models` 发现结果、2026-09-17 产品确认（三方模型不得自称 GPT-6）  
> 相关：[产品原理 §7 模型目录](3rcodex-product-principles.md#7-模型目录原理)、[混合路由详细设计 §6.2](codex-hybrid-routing-design.md)

产品原理第 8 条要求「能力如实展示」：模型目录只声明已验证的上下文、推理、图像、工具和传输能力，不把第三方模型伪装成具备全部官方能力。当前实现继续用官方 bundled 条目当版本安全骨架，再由 family profile（模型系列配置）覆盖身份、推理档位、速度档和审核路由。本文固定：身份从哪来、档位从哪来、`/v1/models` 能信什么、目录该怎么生成。

> 2026-09-23 产品优先级：先按上游模型 ID 匹配模型族的官方推理档位合同；已知模型族严格显示其官方梯度。GPT、Grok 等已知族使用各自合同；只有无法匹配已知官方合同的模型才使用通用兜底 `low`、`medium`、`high`、`xhigh`，默认 `high`。是否经过中转、或中转是否支持该档位，不参与目录选择。Provider 预设未来可补充字段映射和供应商差异。

## 压缩能力边界

`supports_experimental_context` 是 Codex 客户端的实验性上下文管理资格标记，主要控制官方模型的上下文管理合同；它不是第三方 Provider（模型渠道）支持远程压缩的通用声明。第三方条目从 GPT-6 Astra 骨架生成时必须删除这个继承字段，避免 Grok、DeepSeek、Claude 或未知模型被误标为 Astra 上下文能力。

目录声明不能单独阻止 Codex 核心发起压缩请求。隔离 / `local_client` 若仍收到 Codex 私有的 `compaction_trigger`，Managed Gateway 返回结构化 `unsupported_compaction`，不把该请求转发成含糊的上游 502。默认 App `remote_v2` 不再一律拒绝：兼容回退透传、伪造 GPT 明确失败、其余三方走 Summarizer。官方路由保持原有行为后仍 `official_route_unavailable`。预检不删除或改写用户历史。

2026-09-19：启动 `compactMode`、RouteSet `compaction_support` 与 GatewaySummarizer 的 mock 合同已落地。隔离 `rcodex` 在 2026-09-18 对本机 `codex-cli 0.155.0-alpha.2.6` 确认走 `LocalClient`（无 `compaction_trigger`），不要再为隔离入口做 Summarizer。默认 App 保留 `OpenAI` 名即远程 v2，由 RouteSet 分流 NativeRemote / GatewaySummarizer / fail-closed。当前名称门不再探针。若 Codex 日后提供真正生效的官方本地压缩配置（统称 `supports_local_compact`），目录/Profile 物化器按目标版本探针跟随写入，并据此重算 `compactMode`；未探针通过前不把草案键写进 `model_catalog_json` 或 `config.toml`。用户确认的 `SessionHandoff` 仍缓后。不要把托管 Provider 改名为 `OpenAI` 来启用远程压缩。每个 RouteSet 必须固定实际模式和失败原因；不在一次压缩回合中盲目重试或切换 Provider。合同见[混合路由详细设计 §8.2.1](codex-hybrid-routing-design.md)。

「统一改名为 OpenAI，Gateway 再按是否 GPT 系列透传或 Summarizer」与 OpenCodex Design B / CPA per-model bridge 同构，协议上可行，但只覆盖已经会发远程 v2 的入口（当前主要是默认 App）。隔离实例不得并入这条路径。透传条件是 RouteSet 上已验证的 `NativeRemote`，不是 `upstream_model_id` 含 `gpt`；API Key 中继上的 GPT 名默认 fail-closed（`not_native_gpt_compaction`），不得静默 Summarizer 或当 NativeRemote。评估见[混合路由执行计划](codex-hybrid-routing-execution-plan.md)。

## 1. 结论

1. Codex App / CLI 的推理档 UI 读的是 R Codex 生成的 `model_catalog_json`，不是上游 `/v1/models`，也不是 Gateway 运行时探测。
2. 第三方条目仍可能整份克隆官方 bundled 的**第一条**（GPT），但现在会在同一个生成器末尾应用 family profile overlay，覆盖身份、推理档位、速度档和自动审核路由。
3. 接入时路由能力是默认值：文本、Responses 才开 tools，`reasoningLevels: []`。发现层只存 `{"discovery":"openai_models","ownedBy":...}`。
4. `GET /v1/models` 是模型 ID 清单，**不是** Responses 能力目录，**不返回**推理档位。不能用它生成 Codex 的 `supported_reasoning_levels`。
5. Grok 4.6 **有** `xhigh`（Codex Extra high）。官方默认是 `high`，不能关闭推理。不要把 `xhigh` 映射成 `high`。
6. 仪表盘上的「grok + low」不能单独证明 Gateway 丢掉了 Extra high。Responses 转发不改写 `reasoning`。会话侧已出现 `effort: "xhigh"` 时，更应怀疑中继默认档或字段名（`reasoning.effort` vs `reasoning_effort`）。禁止把请求/响应正文写入日志或诊断。
7. 三方模型的身份句按模型族套用，不得保留 GPT-6 自称。产品句为「你是 {Family}，在 Codex 工作区里用本地工具。」目的是让 Grok / DeepSeek / Claude 发挥自己的执行力，只使用 Codex 本地工具，不扮演 GPT-6 去开官方专属功能。

## 2. 生成路径与已修复缺口

兼容 / 受管理目录生成：

1. 对目标 Codex 执行 `codex debug models --bundled`。
2. 在 bundled 中找 `slug == route.upstream_model_id` 的条目。
3. 找不到则用 **bundled 第一条**（几乎总是 GPT）。
4. 克隆整份 JSON，覆盖 slug、display_name、description、visibility、tools/search；窗口字段按 CapabilitySet 写入或从骨架删除（见第 6 节）。第三方 `base_instructions` 开场由 family profile 替换。
5. Responses 路由按 family profile 写入推理档；已知族会过滤旧快照中的 GPT-only 档位，未知族只保留显式能力且移除 `max`/`ultra`。

API Key 渠道物化路由时写入：

```text
CapabilitySet {
  input_modalities: ["text"],
  supports_tools: wire_api == Responses,
  context_window / auto_compact_token_limit: UI 覆盖 → family 默认 → None,
  reasoning_levels: family overlay
}
```

Provider 连通性探测 `GET {base_url}/models`，解析只取 `id` / `name` / `owned_by`，存库：

```json
{"discovery":"openai_models","ownedBy":"<owned_by 或空>"}
```

修复前 live Grok 路由能力为：

```json
{"reasoningLevels":[],"supportsTools":true,"supportsImages":false,"supportsWebSearch":false,...}
```

修复前生成目录后，Grok 条目会带着 GPT 的：

- `default_reasoning_level = "low"`
- `supported_reasoning_levels` 含 `xhigh` **以及** GPT 才有的 `max` / `ultra`

App 里能选出 Extra high，同时又默认显示 low，根因是这份合同，不是 Gateway 把档位丢掉。

代码位置：

- `src-tauri/src/managed_codex_config.rs`：`build_version_matched_catalog_for_version`、`model_entry_from_template`
- `src-tauri/src/managed_launch.rs`：接入时写入 `CapabilitySet`
- `src-tauri/src/connectivity.rs`：`parse_models`
- `src-tauri/src/diagnostics.rs`：`provider_model_catalog_entries.capabilities_json`

## 3. 两个接口不要混用

| 接口 | 作用 | 和档位的关系 |
| --- | --- | --- |
| `GET /v1/models` | 列出当前 key 能用的模型 ID | 发现「有哪些模型」 |
| `POST /v1/responses` | 真正推理 | 请求里由客户端带 `reasoning.effort` |

R Codex 探测走的是前者。Responses 没有「列出模型及可用档位」这种接口。档位是请求参数，也是 Codex bundled catalog 里的 UI 合同，不是 models 列表字段。

### `/v1/models` 可以信什么

- 这个 key **此刻**能看到哪些 `id`（R Codex 已用于勾选接入）。
- 可选的非标准扩展：xAI 官方可能带 `context_length`、价格、aliases；更全的列表在 `GET /v1/language-models`（模态、定价）。这些仍**没有** effort 数组。

OpenAI 兼容合同的标准形状是 `id`、`object`、`created`、`owned_by`。中继为了兼容，通常只仿这一层。把 parser 扩成「把 JSON 全吃下来」也得不到档位。

### 不能当能力真相的原因

1. **合同里没有档位。** 官方 OpenAI `/v1/models` 没有 `supported_reasoning_levels`。xAI 官方 `/v1/models` 与 `/v1/language-models` 同样没有。Grok CLI 的下拉来自本地 `default_models.json`，与 Codex bundled catalog 同一套路。
2. **中继会省略或伪造。** 同一 `grok-4.6`，uuapi 类中继可能只回四字段、乱填 `owned_by`、或抄 GPT 字段。`ownedBy: "xai"` 只说明中继愿意这么标，不证明支持 `xhigh`，也不证明认 `reasoning.effort` 而不是 `reasoning_effort`。
3. **同名模型能力会分叉。** 官方 grok-4.6 有 `xhigh`；grok-4.5 没有（`xhigh` 会被当成 `high`）；有的中继把 4.6 映射到别的后端。ID 相同 ≠ 档位相同。
4. **错误档位的代价在 Codex 侧。** 目录写出 `max` / `ultra`，App 就会画出来；上游可能静默降级、忽略或 400。

发现结果负责点名；Codex 目录负责描述这个模型在选择器里该怎么画能力。两者不能互相替代。

## 4. 已知模型族档位（文档，非 `/v1/models`）

来源：[xAI 推理能力](https://x.ai/docs/developers/model-capabilities/text/reasoning) 与对应模型卡。请求字段为 Responses 的 `reasoning.effort`（部分 SDK / 中继使用顶层 `reasoning_effort`）。

| 模型 | 可用档位 | 官方默认 | 备注 |
| --- | --- | --- | --- |
| grok-4.6 | `low` `medium` `high` `xhigh` | `high` | 不能关闭推理；Codex Extra high = `xhigh` |
| grok-4.5 | `low` `medium` `high` | `high` | `xhigh` 视为 `high` |
| grok-4.3 | `none` `low` `medium` `high` `xhigh` | `high` | R Codex 工作区统一默认档；以模型卡为准确认可选档位 |

GPT 模板里的 `max` / `ultra` 不是 Grok 档位，不得随克隆泄漏到第三方条目。官方 Codex bundled（本机 ChatGPT.app，2026-10-16）里：gpt-5.6-sol / gpt-5.6-terra / gpt-6-astra / gpt-6.1-sol 有 max 和 ultra；gpt-5.6-luna 有 max 无 ultra；gpt-5.5 只到 xhigh。R Codex 的 GPT 族目前按一整条梯子写 low/medium/high/xhigh/max/ultra，不按 slug 逐条抄。这不是 0.1.2 把 DeepSeek 的 max 抄给 GPT。

2026-10-13：DeepSeek、GLM、Qwen 改读厂商公开模型卡，不探针，也不读中继 /v1/models。别名如果会被厂商折成另一个档，目录只保留折完后仍有区别的名字。厂商没公布 Codex 档位名单的族，目录留空，不再套未知族的 low / medium / high / xhigh。Claude 还没有按同一轮卡片誊写，暂时仍用这套未知族兜底。

| 模型 | 可用档位 | 官方默认 | 上下文 | 看图 | 来源 |
| --- | --- | --- | --- | --- | --- |
| deepseek-flash，以及仍由 Flash 承接的旧 flash 名 | none、low、high、max | high | 1,000,000 | 是 | DeepSeek Thinking Mode 与 Models & Pricing，2026-10-13 |
| deepseek-v4-pro | 同上 | high | 1,000,000 | 否 | 同上。medium / xhigh 折成 high，ultra 折成 max，目录不单列 |
| deepseek-chat 等不在当前价目卡上的旧名 | 同上 | high | 不写 | 否 | 推理控制是当前 API 的参数；窗口和看图只属于价目卡上的模型 |
| glm-5.3 | low、high、max | max | 1,000,000 | 否 | Z.AI GLM-5.3。思考不能关闭 |
| glm-5.3-flash / flashx | 同上 | max | 1,000,000 | 是 | Z.AI GLM-5.3-Flash。文本参数与 GLM-5.3 相同 |
| glm-5.2 | none、high、max | max | 1,000,000 | 否 | Z.AI GLM-5.2 与 Core Parameters。low / medium 折成 high，xhigh 折成 max |
| 更早的 GLM | 不写 | 无 | 不写 | 仅名称里带 4.5V、4.6V、OCR 的视觉卡 | reasoning_effort 从 GLM-5.2 起才有文档。本次卡片没有 20 万这个数字，所以不写 |
| qwen3.8-max、qwen3.8-flash、qwen3.7-plus、qwen3.7-flash、qwen3.6-plus、qwen3.6-flash、qwen3.5-plus、qwen3.5-flash、qwen3-coder、qwen2.5-vl | 不写 | 无 | 1,000,000 | 视觉模型页列出的才开 | 阿里云 Model Studio 文本模型页，2026-10-11。思考开关是 enable_thinking，页面没有给出 effort 名单 |
| qwen3.7-max 本体、2026-05-20、2026-05-17、preview | 不写 | 无 | 1,000,000 | 否 | 文本接口。2026-06-08 快照改为可看图 |
| qwen3.5 开源规格、qwen3 小模型、qwen3.5-omni | 不写 | 无 | 256,000 | omni 与名称带 vl 的可以 | 同一文本模型页；omni 的窗口来自视觉模型页 |
| qwen-long | 不写 | 无 | 不写 | 否 | 官方是 10,000,000，超过客户端可保存的 1,000,000 |


## 5. 身份 overlay（2026-09-17 确认）

R Codex 接入 Responses 模型，是为了把它放进 Codex 工作区，不是让它扮演 GPT-6 来驾驶 Codex。

产品身份句按模型族套用，`{Family}` 为族名而不是路由 slug：

| 模型族 | 产品身份句 |
| --- | --- |
| Grok | 你是 Grok，在 Codex 工作区里用本地工具。 |
| DeepSeek | 你是 DeepSeek，在 Codex 工作区里用本地工具。 |
| GLM | 你是 GLM，在 Codex 工作区里用本地工具。 |
| Qwen | 你是 Qwen，在 Codex 工作区里用本地工具。 |
| Claude | 你是 Claude，在 Codex 工作区里用本地工具。 |
| 未知族 | 你是 {上游模型族名}，在 Codex 工作区里用本地工具。 |

`base_instructions` 与新版目录的 `model_messages.instructions_template` 开场都必须替换 bundled GPT 的「You are Codex, an agent based on GPT-6」。目录正文以英文为主时，在产品句后紧跟一行英文对应，避免和后续工具章节抢语言：

```text
你是 Grok，在 Codex 工作区里用本地工具。
You are Grok, working in a Codex workspace with local tools.
Use only the local workspace tools provided in this session.
```

要换掉的是 GPT-6 自称和官方专属能力广告；要留下的是 Codex 工作区合同：本地 `exec`、`apply_patch`、`tool_mode: "code_mode_only"`。不要为每个族手写完整 2 万字宪法，也不要把开场句追加在 GPT-6 身份后面。

必须从第三方条目去掉、不得当作用户可选能力的 GPT-6 专属面：

- 任何「based on GPT-6 / You are Codex」自称；
- GPT 才有的推理档 `max` / `ultra`；
- OpenAI Fast / `priority` 等 `service_tiers`、`additional_speed_tiers`（除非该族自己有同等合同）；
- GPT 模板强制的 `multi_agent_reasoning_effort`；第三方族验证前不继承模型专属子 agent 档位，Codex 本地的 `model_messages.multi_agent` 合同与版本字段继续保留；
- 官方 ChatGPT 对 Node REPL 的自动审核。目录 node_repl_disabled 对三方模型为 false，本机 Browser / Computer Use 才能用；node_repl_auto_review_required 为 false，不把官方审核模型当成前提；
- 把官方 MCP（如 `openaiDeveloperDocs`）暗示成这个模型拥有的能力。

本地工具不是 GPT-6 专属功能，Responses 透传下继续保留。

## 6. 目录应该怎么生成

继续 **一个生成器 + 官方骨架 + 族 profile overlay**。不要为 Grok、DeepSeek 各写一套完整 catalog 生成器。工具模式、可见性、`apply_patch` 类型等结构字段绑死 Codex 版本，应由 `codex debug models --bundled` 提供。`base_instructions` 的开场身份和第 5 节列出的 GPT-6 专属面必须 overlay，不得随骨架泄漏。

错在 overlay 几乎是空的，不是「不该克隆」。

### 能力来源优先级（建议）

| 优先级 | 来源 | 用途 |
| --- | --- | --- |
| 1 | 用户在接入 UI 勾选/修改的推理档、默认档，以及高级映射里的窗口数字 | 最终写入 RouteSet 的 `CapabilitySet` |
| 2 | 按 `upstream_model_id` 推断的内置 family profile | 未编辑时的默认 overlay（推理档 + 官方推荐窗） |
| 3 | 官方 bundled 精确 slug 命中 | 仅官方模型；三方 ID 对不上 |
| 4 | GPT/最接近 reasoning 模型骨架 | 只提供 Codex 未知必需字段，**不得**在 overlay 为空时留下 `max`/`ultra`、错误默认档，或 GPT 的 `context_window` / `max_context_window` / `auto_compact_token_limit` |

`GET /v1/models` 只提供 ID 列表。禁止从发现 JSON 猜测 effort 或窗口。窗口是客户端请求配置；上游/中转可能更小。

窗口解析（A2，2026-09-20）：

- `context_window`：UI 覆盖 → family 默认 → 仍无则从克隆条目 **删除** `context_window` / `max_context_window` / `auto_compact_token_limit`。有值时目录写入该数，并把 `max_context_window` 写成同一数字（避免 500k 窗配 872k max）。
- `auto_compact_token_limit`：UI 有则用 UI；否则 family 显式值（仅当 family compact ≤ 已解析窗，否则丢弃以免 `CapabilitySet.validate()` 拒绝更小的 UI 窗）；否则 `context_window × 95%`（对齐本次 258400 = 272000 × 95% 的 Codex 行为）。无窗口则不写阈值。
- 校验：`None` 或 `1..=1_000_000`；拒绝 `0` 和大于 1,000,000；两值都有时 compact ≤ window。2026-10-13 起正好 1,000,000 可以保存，因为当前 DeepSeek、GLM-5.2/5.3 和 Qwen 卡片写的就是这个数。
- Grok 族本轮默认 500000 / 475000（[xAI grok-4.6](https://docs.x.ai/developers/models/grok-4.6)）。上表里的 DeepSeek、GLM、Qwen 用各自卡片上的窗口，压缩阈值仍是窗口的 95%。Claude、对不上卡片的旧模型名，以及 qwen-long 这种超过客户端上限的窗口，保持 `None`，不把 GPT 骨架上的数字抄过来。
- 只写每模型目录字段，不写进程级 `model_context_window` / `model_auto_compact_token_limit`。兼容目录里的原生 GPT 行不走三方 overlay。
- UI 覆盖落在工作区选择行（`launch_preset_model_selections`），不是全局身份表。空 = family 默认。保存工作区并应用配置 / 重启默认 App 后才生效。

### 已实现的最小路径

1. 内置 family profile（数据，不是新生成器）：按 `upstream_model_id` 匹配 Grok / deepseek / Claude / 保守未知族。profile 含身份句、推理档与默认档、官方推荐窗、是否剥除 OpenAI 速度档。
2. 物化路由时按 profile 填 `CapabilitySet.reasoning_levels`（Grok 4.6 → `low/medium/high/xhigh`，默认 `high`），并按 UI/family 填 `context_window` / `auto_compact_token_limit`。
3. `model_entry_from_template` 同时替换 `base_instructions` 和 `model_messages.instructions_template` 的开场身份，覆盖推理档与窗口，删除非 GPT 族的 `max`/`ultra`、`default=low`、强制子 agent 推理档以及未声明的 Fast / priority 档。本机 node_repl 保持开放（node_repl_disabled=false），官方 JS 自动审核不要求。Codex 通用多 agent 合同继续保留。无 CapabilitySet 窗口时剥掉克隆来的窗口三键。
4. 每个 Responses 第三方条目把 `auto_review_model_override` 写成自身公共模型 ID。隔离 Profile 写入 `approval_policy = "on-request"`，不写 `approvals_reviewer`，合并时删掉旧的 `auto_review`。Codex 0.162 终端界面在策略缺省时会直接拒绝沙箱提权，所以显式要求询问；前端不再提供第二个审核模型选择。Chat 路由不写这个覆盖：审核回合常带 previous_response_id 和 Responses 专用 item，chat_bridge 会失败。函数工具已经接上，仍不够开 Chat 自审；要先抓一条真实自动审核请求再改。
5. 发现页继续只展示 id/name/ownedBy；用户不需要手工编辑推理档。
5. 不要把 `xhigh` 映射成 `high`。
6. Chat Completions 把 Codex 的 `reasoning.effort` 写成顶层 `reasoning_effort`。`none` 不写入这个字段，改为 `thinking.type = disabled`。其余档位原样传递，不在网关里折成 high 或 max。目录继续按模型族广告这些档。

硅基流动的 `enable_thinking` 先不做。CC Switch 在 `src-tauri/src/proxy/providers/codex.rs` 里按平台单独处理，记录于 2026-10-13，文件 blob `d23f41fe6274a0504aba8ed87535f1c4b5c9524d`。`infer_codex_chat_reasoning_config` 先调用 `infer_aggregator_platform_config`（约第 689 行），只看渠道名和 `base_url`，不看模型名。名字或地址里有 `siliconflow` 时（约第 826 行）写成布尔 `enable_thinking`，`supports_effort` 为 false，`effort_param` 为 `none`，思考文本从 `reasoning_content` 读回。同文件约第 2190 行的测试写明：挂在硅基流动上的 MiniMax、GLM 也走这个开关，不走厂商自己的 `reasoning_split` 或 `thinking`。R Codex 继续只做 Chat 标准字段，不按渠道名改参数。

未知 family：仍可用 GPT 骨架，但必须写入保守 overlay（第 5 节身份句 + 只声明文本、不声明 GPT 推理阶梯），或在 UI 强制用户选择档位后再生成目录。空 overlay 不是合法默认。

图像/搜索广告：目录按上游模型 ID 的系列名处理 `supports_image_detail_original`。`gpt-` 或含 `openai` 的系列保留该开关；Grok、DeepSeek、Claude 和未知族删除，不论 `supports_images`，也不看这条路由是官方账户还是 API 渠道。`supports_web_search=false` 时删除 `web_search_tool_type`。这不改推理档，也不要把 `xhigh` 映射成 `high`，Gateway 不改写出站正文。Grok 4.x Responses 看图仍是 family.supports_images=true，依据是标准 `input_image.image_url` 的 `detail=auto` / `high`。`original` 是 Codex 扩展档，不是三方模型族的档；某条渠道探针接受这个字符串，也不据此给非 GPT 系列广告该字段。用户把三方中转标成 GPT 系列后，如果中转拒绝 original，不在 R Codex 范围。不要把 grok-imagine 生图写进 grok-4.6 聊天能力。待办见[Grok 会话问题与下一步](designs/2026-09-19-grok-session-followups.md)。

## 7. 诊断时不要把三件事揉成一个 bug

| 观察 | 含义 | 不是 |
| --- | --- | --- |
| App 显示 Extra high，目录 `default_reasoning_level=low`，且含 `max`/`ultra` | GPT 骨架泄漏 | Gateway 改写了 effort |
| 第三方会话开场仍是「You are Codex, an agent based on GPT-6」 | 身份 overlay 未写 | 模型自己决定自称 |
| `config.toml` / session `turn_context` 为 `effort: "xhigh"` | Codex 按用户选择发出了 Extra high | 目录默认 low 被实际使用 |
| 中继控制台显示 grok+low | 中继自己的默认档、计费展示或字段名 | R Codex 一定丢掉了 xhigh |
| Codex `url: http://127.0.0.1:43123/...` 且空 body 502 | 系统代理吞了回环请求 | 上游 502（见 sidecar 是否有 `event=forward`） |
| 目录 input_modalities=[text] 且 view_image 被拒 | rcodex 未验证看图，CapabilitySet 写死纯文本 | Grok 4.6 产品本身一定非多模态 |
| 生图后台出现 gpt-5.6-luna | 走了 image2 Responses 宿主，不是当前 Grok 聊天模型 | Gateway 把聊天模型改成了 GPT |
| 改会话名/侧栏说明后台出现 gpt-5.6-luna | App ephemeral 写死 luna，兼容目录 merge + 官方回退白名单把它转走 | Gateway 把当前 Grok 聊天改成了 GPT；与生图 image2 宿主是同一件事 |
| 长会话 compact 报 incompatible_compaction_envelope | 外国 / 官方 **compaction** 信封无法在当前渠道展开。2026-09-20 起 reasoning 密文不再用此码 | 本路由 Grok reasoning 密文（应折叠后摘要）；普通回合 502 |
| 长会话 compact 报 incompatible_reasoning_envelope | Summarizer 折叠后仍残留非 compaction 密文 | 压缩信封；应引导新建会话或隔离 / 默认 App 本地压缩 |

空 body `Unknown error` 且 sidecar 无 `event=forward` 的定位见 README「本地网关如何工作」：优先查系统 HTTP 代理，而不是第三方档位。

## 8. 安全与产品边界

- 不记录 API Key、实例准入密钥、请求正文、响应正文。
- 目录与诊断只保存非敏感能力元数据。
- 精确路由、失败不回退：档位错误应表现为该模型的真实上游错误，不得改走另一模型或另一档而不提示。
- 能力如实展示优先于「让选择器看起来和官方 GPT 一样齐全」。

## 9. 验收清单

实现第 6 节最小路径后的验收包括：

- 生成的 Grok 4.6 / DeepSeek 等第三方条目 `base_instructions` 与 `model_messages.instructions_template` 开场为第 5 节身份句，不含「based on GPT-6」；
- 生成的 Grok 4.6 目录不含 `max`/`ultra`，含 `xhigh`，默认不是 GPT 的 `low`；窗口为 500000 / compact 475000，不得留下 GPT 的 272000/872000；
- 未知族生成目录在无 UI 覆盖时不得保留克隆来的 `context_window` / `max_context_window` / `auto_compact_token_limit`；
- 兼容目录里的原生 GPT 行不被三方 overlay 改窗；
- 未声明的 Fast / priority 速度档不出现在第三方条目；
- 非 GPT 系列不得留下 `supports_image_detail_original`，包括已打开看图的 Grok；`gpt-` / `openai` 系列保留该开关，即使它走的是 API 渠道；不支持搜索时不得留下 `web_search_tool_type`；
- 未验证的 GPT 子 agent 推理档不出现在第三方条目；本机 Node REPL 保持开放，官方 JS 自动审核不要求。Codex 通用多 agent 合同、`code_mode_only` 与 `apply_patch` 继续保留；
- 用户选 Extra high 时 session 仍为 `xhigh`；
- Chat 渠道转发 `reasoning_effort`；`none` 写成 `thinking.type = disabled`；
- `/v1/models` 探测行为不变（仍只点名）；
- 仍只有一个目录生成器，新增模型族只加 profile 数据。
- Responses 自动审核字段按当前目录条目指向自身；Chat-only 路由不写覆盖。缺口是审核回合的 previous_response_id 和专用 item，不是普通函数工具。
