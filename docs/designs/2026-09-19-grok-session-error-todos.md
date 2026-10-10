# Grok 会话错误分析与待办（2026-09-19）

> 状态：E1-2 已锁；E1-3 mock 已覆盖；A2 窗口已落地；E8-1/E8-3 与 E10-2/E10-3 已实现；E11/F3 已实现（当前模型族-only）。E10-1 超时表、E10-4 改小窗、E2 防回归未做。C 停放。
> 来源会话：确认当前模型是否为 Grok（thread 01a0b8da-0e36-7bf2-9344-81f272b14558）。
> 配套：[问题与下一步待办](2026-09-19-grok-session-followups.md)。
> 结论等级：E1/E2/E7/E8 有会话与代码双证据；E3/E4/E5/E6 有当场 HTTP/行为证据，但未保存完整脱敏档案正文。
> 2026-09-20：合同锁定选项 1。不要从零再调研 `encrypted_content`。

## Goal

- 用户可见结果：把该会话里每类失败拆成可复现的错误、根因边界、错误码问题和修复待办，避免下次把「生图失败 / 看不见图 / 会话突然死掉」当成同一个 bug。
- 范围：现场错误、错误路径、Gateway/目录合同偏差、以及会再次打断默认 App Grok 长会话的压缩行为。
- 非目标：不在本文件改代码；不把「命令未发出」写成已修复；不把 xAI 官网能力写成 3rxai 中转合同。

## 错误总览

| ID | 现象 | 层 | 根因边界 | 用户影响 | 优先级 |
| --- | --- | --- | --- | --- | --- |
| E1 | 归档前 Error running remote compact task: incompatible_compaction_envelope | Gateway Summarizer | 2026-09-19 实现拒绝任何剩余 encrypted_content；Grok reasoning 密文被当成外国压缩信封。2026-09-20 已按选项 1 折叠 reasoning | 修复前默认 App Grok 长会话无法 compact | P0（代码已修，01a0bcf6 实机成功） |
| E10 | compact 触发后 504 Gateway Timeout：等待上游首字节超时 | Gateway Summarizer → 3rxai | 3rxai first_byte=30s；Summarizer 强制 stream=false，send() 等响应头；大包+未降 reasoning，中转常等整段摘要。Codex 客户端连打 5 次后第 6 次成功 | 压缩已触发但看起来像断线；会话后来能续 | P1（待确认优化） |
| E11 | Grok / sol 会话后台出现 gpt-5.6-luna | Codex App ephemeral + 兼容回退 | App 写死 luna。F3：未选 GPT 则剥 merge/回退，快照别名走站桩 | 应用配置后改名应打站桩族，不再官方 luna | P1（已实现，待手验） |
| E2 | 点名 grok-imagine-image 却打出 gpt-5.6-luna | 技能/代理选择 | 当时没有 grok-imagine 技能，走了 image2 Responses 宿主 | 后台计费/日志像 GPT；图可能来自错误宿主 | P1（技能已纠，需防回归） |
| E3 | POST .../v1/images/generations 返回 502 Upstream access forbidden | 中转 | 部分 3rcd 入口禁图片路由或当前 key 无图权限 | 直连图片接口失败，容易退回错误宿主 | P2 |
| E4 | 直连 api.x.ai 连接被重置 | 网络 | 本机到官方 xAI 不可达 | 不能把官方 xAI 当默认 | P2 |
| E5 | 本机 127.0.0.1:43123 无图片路由 | Gateway 产品边界 | sidecar 只提供聊天 /v1/responses | 把 grok-4.6 塞进 Responses image_generation 工具不会出图 | 按设计；文档化 |
| E6 | grok-4.6 + image_generation 工具只写字 | 能力/协议 | 聊天模型不会当 Imagine 用 | 出图必须换模型或换 /v1/images/generations | 已确认 |
| E7 | view_image：you do not support image inputs | 目录/CapabilitySet | 3rcodex 写死纯文本，Codex 不发读图工具 | 代理看不见自己刚生成的图 | P1 |
| E8 | 目录泄漏 GPT 图像/窗口字段 | overlay 不完整 | 克隆 GPT 骨架后未覆盖 | 选择器/压缩阈值可能按 GPT 窗工作；能力展示不实 | P1 |
| E9 | 错误文案把 reasoning 密文说成「压缩信封」 | 错误码 | 2026-09-20 已拆 `incompatible_reasoning_envelope` | 修复前用户以为是 compact blob | P1（随 E1，文案已拆） |

## E1 远程压缩失败（会话中断）

### 现象

用户 2026-09-19 11:38 发送「将上述问题及解决方案做成下一步待办并落档 md，我要将此会话归档」。turn 01a0b975-ad75-7793-9a47-3ac399ffcf92 在 224ms 内失败，没有 agent 输出。线程状态 systemError。

可见错误：

    Error running remote compact task:
    gateway_code=incompatible_compaction_envelope
    layer=gateway
    message=3R Codex Gateway 会话含有无法在当前渠道解开的压缩信封，请新建会话。
    Codex 打印的 url 是本机 Gateway（127.0.0.1:43123），不是上游。

### 证据

- 会话是默认 App 兼容路径：session_meta.model_provider = OpenAI，模型仍是 Grok 4.6。按合同 compactMode=remote_v2，Grok 路由 compaction_support=gateway_summarizer。
- 上一成功回合 input 约 225525 tokens，窗口 258400，触发远程 compact 符合预期。
- rollout 中 response_item：reasoning 122、custom_tool_call 109、message 51。**122 条 reasoning 全部带 encrypted_content**，前缀不是 3r-compaction-v1:，而是普通 base64（例：N6+UUp4P...，长度 142–3410）。另有 1 条 custom_tool_call_output 含 encrypted_content。
- 普通回合能继续：expand_compaction_history 只展开 type=compaction，reasoning 密文留给历史兼容切片。
- Summarizer 在 [compaction.rs](../../src-tauri/src/managed_gateway/compaction.rs) prepare_summarizer_request 中：先展开 3R 信封，再 reject_foreign_encrypted_content。该函数对 input 里任意 encrypted_content 失败。单测 prepare_summarizer_strips_trigger_expands_envelope_and_rejects_foreign_blobs 明确要求带 type=reasoning + encrypted_content 的请求返回 incompatible_compaction_envelope。

### 结论等级

**已确定**：这次 compact 失败发生在 Gateway 本地预检，请求按当前实现不得发给 3rxai。失败物是 Grok reasoning 密文，不是已证明的官方 openai-compaction-v1 项。代码注释也写了 reasoning 密文留待历史兼容切片。普通回合已经把同类 reasoning 项转给同一 3rxai 路由；Summarizer 比普通转发更严，不是「当前渠道绝对不能看见自己签发的密文」。

**不能断言**：上游 3rxai 在 **摘要请求** 里是否接受这些 reasoning 项；若 Summarizer 丢弃 blob 后摘要质量如何；custom_tool_call_output 里那一条密文的具体形状（未展开）。

### 为何普通对话还能走

Gateway 普通转发不跑 reject_foreign_encrypted_content。因此短/中等 Grok 会话看起来正常，直到默认 App 因上下文窗口发起 remote compact。这是「新建成功、用久了突然死」的形状，和执行计划里旧会话 502 不同，但同属历史 item 合同。

### 待办

- [x] **E1-1** 拆错误码：type=compaction 且非 3r-compaction-v1: → 仍用 incompatible_compaction_envelope；折叠后仍残留的非 compaction 密文 → incompatible_reasoning_envelope。用户文案说「推理密文无法在当前压缩路径展开」，不说成压缩信封。
- [x] **E1-2** 选项 1 已写入 §8.2.1 第 3 条并改 `prepare_summarizer_request`：折叠 reasoning 密文、可见文本写成 assistant 历史；外国 compaction 信封继续拒绝。选项 2/3 不采用。
- [x] **E1-3** mock 已覆盖「仅 reasoning 密文 + trigger」「仅外国 compaction 信封 + trigger」「3r-compaction-v1 + reasoning 密文」。2026-09-20 会话 `01a0bcf6` 实机 compact 成功（sidecar `code=ok`，input 228622→30654）。不得把官方 compact blob 发给三方。
- [ ] **E1-3b** 实机 compact 伴随 5 次 `upstream_first_byte_timeout` / HTTP 504，见 E10；不把这次 504 写成信封回归。
- [ ] **E1-4** 与[旧会话兼容待办](../codex-hybrid-routing-execution-plan.md)合并评估，不要开第三条历史改写路径。完整请求档案打开时采集 compact 请求的 item type 集合（无正文）。

### E1 调研（2026-09-19）：`encrypted_content` 是什么、能否发给当前三方、同行怎么做

本小节只落档调研，不改 `compaction.rs`。核对：3R [`compaction.rs`](../../src-tauri/src/managed_gateway/compaction.rs)、OpenAI Responses 文档、OpenCodex `compaction.ts` / `parser.ts` / `reasoning-envelope.ts` / `sanitizeReasoningInputContent`、CC Switch PR #5536 与历史指南、Cockpit 发行说明与 #2012。GitHub HTML 抓取曾被拦，源码用 raw/API。

#### 字段是槽位，不是一种密文

Responses / Codex 的 `encrypted_content` 是客户端原样重放的不透明槽。常见三类：

| 出现位置 | 实际内容 | 谁能解开 | 人能不能读 |
| --- | --- | --- | --- |
| `type=reasoning` | 隐藏思维 / CoT 的续接状态 | **签发它的后端 + 账号/Key** | 不能。可见部分在 `summary` / `content` |
| `type=compaction`（官方远程 v2） | OpenAI 服务端压好的会话检查点 | 官方 compact 合同 | 不能 |
| `type=compaction`（网关合成） | `3r-compaction-v1:` / `ocx1:` / `cc-switch-compaction-v1:` + base64(utf8 摘要) | 签发网关自己 decode | 解开后是纯文本摘要 |

官方语义：`include: ["reasoning.encrypted_content"]`（或 `store: false` / ZDR）时 reasoning 项带加密思维，下一轮塞回 `input`，后端内存解开再丢。[Reasoning](https://developers.openai.com/api/docs/guides/reasoning/)、[Create response `include`](https://developers.openai.com/api/reference/resources/responses/methods/create.md)、[Cookbook](https://cookbook.openai.com/examples/responses_api/reasoning_items)。

约束：绑定签发方（换 Key / Azure 实例 / Provider 常见 `was not issued to this caller` / `could not be verified`）；客户端当字节重放、不解密；Chat Completions 无此槽。名字像加密，**不等于对话正文被加密**。

本会话 122 条属于第一类：普通 base64（例 `N6+UUp4P...`，长度 142–3410），不是 `3r-compaction-v1:`，也未证明是 `openai-compaction-v1`。另有 1 条 `custom_tool_call_output` 带同名字段，形状未展开。

#### 不是「当前三方绝对不能看见自己签发的密文」

| 东西 | 发给当前 3rxai 做**摘要**？ | 说明 |
| --- | --- | --- |
| `3r-compaction-v1:` | 先 decode 成文本再摘要 | 与 `ocx1:` / `cc-switch-compaction-v1:` 同类 |
| 官方 `openai-compaction-v1:` / 非 3R compaction 信封 | **不发**，fail-closed | Codex/官方私有项；三方解不开。§8.2.1 第 3 条字面仍对 |
| 本路由 Grok `type=reasoning` 密文 | 普通回合**已经在发**；Summarizer **不必**发密文，留可见文本即可 | 不是跨签发方泄密问题，是效用和协议 |
| 换签发方后的任意密文 | **不发** | 全员一致 |

Summarizer 仍不该原样塞满 reasoning 密文的原因是效用/协议，不是「发给签发方会泄密」：

1. 摘要模型读文本；密文对 Grok 摘要没有信息量。
2. 122 条密文会把摘要请求撑得比原对话更肥。
3. 形状会 400。OpenCodex 记录：`content: null` 配 `encrypted_content` 时 xAI 回 `Could not decode the compaction blob`——报的是隔壁字段，拒的是 schema。
4. Chat 适配没有这个槽；Summarizer 走 Chat 时密文只能丢或失败。
5. 换 Key / 换入口 / 混进官方 blob 才变成跨签发方硬禁止。

**实现比合同更严。** §8.2.1 第 3 条按字面是「无法识别的官方密文或其它代理前缀不得发给第三方」。[`reject_foreign_encrypted_content`](../../src-tauri/src/managed_gateway/compaction.rs) 在展开 3R 信封后拒绝 **任何** 剩余 `encrypted_content`。执行计划切片 2 第 3 步写的就是这个更严实现。单测 `prepare_summarizer_strips_trigger_expands_envelope_and_rejects_foreign_blobs` 要求 `type=reasoning` + 密文必须失败。

#### 同行 GatewaySummarizer

共同结论：合成信封给 Codex 看形状；**发给三方的是文本**；官方密文保持不透明。没有一家靠「把外国/隐藏密文原样喂给摘要模型」续接。

| 工具 | Summarizer 怎么做 | reasoning / 外国信封 | 对 3R |
| --- | --- | --- | --- |
| [OpenCodex](https://github.com/lidge-jun/opencodex/blob/main/src/responses/compaction.ts) | Design B 挡在名为 `OpenAI` 的 Provider 前；路由模型无工具摘要 → `ocx1:` + base64(utf8) | 非 `ocx1:` compact 不解码，换成短说明。reasoning 用 `ocxr1:` 走私 Anthropic/Kiro 签名；非 `ocxr1:` 加密-only 当不透明，不编造明文。[sanitizeReasoningInputContent](https://github.com/lidge-jun/opencodex/blob/main/src/adapters/openai-responses/reasoning.ts) 发往 ChatGPT 前剥 `ocxr1:`。xAI 也会签发「原生」blob（[PR #2249](https://github.com/lidge-jun/opencodex/pull/2249)）；[PR #2228](https://github.com/lidge-jun/opencodex/pull/2228) 要求只回发给签发后端。API Key `openai-responses` 若原样转发 trigger → 0 个 compaction item（[#422](https://github.com/lidge-jun/opencodex/issues/422)）。回原生 Codex 前要 `ocx recover-history`（[codex#36704](https://github.com/openai/codex/issues/36704)） | 3R 信封保持 `3r-compaction-v1:`，不要抄 `ocx1:`。Summarizer 对齐「剥 reasoning 密文、官方 compact 不喂三方」 |
| [CC Switch PR #5536](https://github.com/farion1231/cc-switch/pull/5536)（落档时仍 open） | Chat 渠道 remote compact → 无工具摘要；JSON/SSE 恰好一个 compaction；`cc-switch-compaction-v1:` | [跨后端解不开 `encrypted_content`](https://github.com/farion1231/cc-switch/blob/main/docs/guides/codex-unified-session-history-guide-en.md)，回原 Provider 或新开。xAI 清洗把 **content part** `type=encrypted_content` 展成 `input_text`（说明文字），不是把官方 Fernet 当正文 | 完整 Summarizer 形状与 3R 切片 2 同类；打开 Remote Compaction 才会改名为 `OpenAI` |
| [Cockpit](https://github.com/jlcodes99/cockpit-tools/releases) | 更常逼成本地压缩（1.3.53 改名踩坑，1.3.55 撤回），不是三方上完整 Summarizer | 混历史时丢掉非官方 `encrypted_content`，改写成合法 summary；过期密文在 HTTP/SSE/Compact/WS 出口剥掉再重试一次（[v1.3.16](https://github.com/jlcodes99/cockpit-tools/releases/tag/v1.3.16)）。[#2012](https://github.com/jlcodes99/cockpit-tools/issues/2012) 是换模型后重放旧 reasoning 的 schema 问题 | 不要把托管 Provider 改名为 `OpenAI`。剥 reasoning 密文是为了续接，不是整次 compact 失败 |
| [Codex++](https://github.com/BigPizzaV3/CodexPlusPlus/issues/1414) | **没有** GatewaySummarizer；只写窗口 / auto-compact 阈值 | 换官方账号可见 `invalid_encrypted_content` | 窗口代替不了协议桥 |
| [CPA compact-bridge](https://github.com/patrick-fu/cpa-codex-compact-bridge) | 官方合同透传，否则桥成明文摘要；桥接项的 `encrypted_content` 就是明文 | 官方密文和合成信封混用 → `invalid_compaction_state` | 与 3R NativeRemote XOR Summarizer 同构 |

#### 推荐（2026-09-20 已锁定选项 1）

默认 App 上 Grok 远程压缩按 **E1-2 选项 1** 落地，不要 2 或 3 作为主路径。

- 3 只把现状说清楚，和这次测试一样。
- 2 普通回合能活，摘要又贵又容易 400；OpenCodex 对非自有信封也不喂给路由模型。
- 1 对齐「官方 compact 不上三方、不静默删用户可见历史」：只去掉不可读的 reasoning 密文（有可见 summary/文本就留下），再走现有 Summarizer。`custom_tool_call_output` 上的密文按同样规则折叠，不当 compaction 信封。

临时绕开（现有能力，不是 E1 修复）：隔离入口，或默认 App 打开「开启本地压缩」后应用配置并重启。未重启则 Codex 仍可能发 `compaction_trigger`，Gateway 按 `local_client` fail-closed。

#### 阅读与执行顺序（明天新会话）

完整清单以[执行计划 2026-09-19 下班交接](../codex-hybrid-routing-execution-plan.md)为准。这里是同一顺序的压缩入口。

**读**

1. [AGENTS.md](../../AGENTS.md) 未完成项 Grok 压缩条。
2. 执行计划文首「2026-09-19（下班）」交接（含阅读顺序全文）。
3. 本文件 E1 现象/证据/结论 + 上文「E1 调研」。E2–E8 跳过。
4. [followups D + Active Status](2026-09-19-grok-session-followups.md)。
5. [§8.2.1 第 3 条原文 + 实现偏差注](../codex-hybrid-routing-design.md)。偏差注不是合同。
6. 动手时才打开 [`compaction.rs`](../../src-tauri/src/managed_gateway/compaction.rs) 的 `prepare_summarizer_request` / `reject_foreign_encrypted_content`。

**做**

1. 把选项 1 写入 §8.2.1 第 3 条（锁定合同）。用户改选 2/3 才写别的。
2. 改 Summarizer：折叠 reasoning 密文、保留可见文本；外国 compaction 信封继续拒绝。
3. E1-1 / E9 拆错误码和文案。
4. E1-3 mock 三案（仅 reasoning / 仅外国信封 / 3R 信封 + reasoning）。
5. 不要付费矩阵、不要 A/B/C、不要静默改历史、不要续写原 thread。

## E2 误走 image2 / gpt-5.6-luna

### 现象

用户点名 grok-imagine-image 出小猫钓鱼。代理先查 xAI，随后改走 image2 的 Responses 流：宿主 gpt-5.6-luna，工具字段写 grok-imagine-image。用户在中转后台看到 luna 请求。图落到 outputs/kitten-fishing-grok.png（1312×1199），代理看不到像素。

### 根因

当时 Codex 技能表没有 grok-imagine。image2 是默认生图技能，用 GPT Responses + image_generation 工具。本机 Gateway 和直连 xAI 都走不通后，代理把 luna 当能过鉴权的宿主。这违反用户「整条链路留在 Grok 族」的预期。

### 已纠正

grok-imagine 技能 + image2 排除规则。后续用 grok-imagine 技能生成美女撸猫时未再出现 luna。

### 待办

- [ ] **E2-1** 保持互斥：Grok 模型或用户点名 grok-imagine 时，禁止再选 image2 / 内置 image_gen。
- [ ] **E2-2** 若 3R 要产品化生图，日志/用量应显示图片模型 grok-imagine-image 和渠道 3rxai，而不是 Responses 宿主 GPT 名。
- [ ] **E2-3** 不要把「图出来了」当成 grok-imagine 合同验收。第一次出图的宿主是 GPT。

## E3 中转 /v1/images/generations 502

当场对部分 3rcd Base URL 打图片接口得到 502 Upstream access forbidden。稍后用 3rxai Provider 的同一类上游 https://ai.3rcd.com/v1 + 该渠道 key 成功。

### 结论等级

高度指向「入口或 key 的图片权限不一致」，不是 Gateway 预检。未保存完整响应体（符合普通日志不记正文）。

### 待办

- [ ] **E3-1** 技能/文档写明：必须用**当前 Grok 聊天渠道**的 Base URL 与 key，不要用 image2 的 gpt.3rcd.com 钥匙去打 Imagine。
- [ ] **E3-2** 失败时原样报 HTTP 状态，不自动换 GPT 宿主（已写入 grok-imagine 技能）。

## E4 直连 api.x.ai 被重置

本机直连官方 xAI 失败。技能回退顺序把官方 xAI 放在最后是对的。

- [ ] **E4-1** 不要把 XAI_API_KEY + api.x.ai 当 3R 默认用户路径。
- [ ] **E4-2** 官方 xAI 用户若需要，单独文档说明；与 3rxai 中转验收分开。

## E5 / E6 Gateway 无图片路由；聊天模型不会生图

本机 sidecar 只绑定 127.0.0.1 的聊天 Responses。把 grok-4.6 当作 Responses image_generation 工具宿主时，模型只写字。正确路径是上游 /v1/images/generations + grok-imagine-image。

- [ ] **E5-1** 文档写清：Gateway 不承载 grok-imagine 流量；技能解开回环。
- [ ] **E5-2** 除非另开票，不要给 sidecar 加图片 API。UI 进程也不要承载模型 token / 图片字节。

## E7 看不见图

系统拒绝 view_image。目录 input_modalities=[text]。这是 3rcodex 故意保守，见待办 B。不是文件损坏，也不是「Grok 4.6 天生非多模态」的完整解释。

- [ ] **E7-1** 在未打开看图前，生图成功文案应告诉用户：当前 Grok 任务不能检视像素，请打开文件或换 GPT 任务看图。
- [x] **E7-2** 打开看图已先做 3rxai Responses 探针，见 followups B1–B3。

## E8 GPT 骨架泄漏

当前兼容目录 grok-4.6 **在未保存并应用 A2 之前**仍可能有 GPT 窗；应用后窗口应为 500000 / max 500000 / compact 475000。图像/搜索泄漏仍在：

- supports_image_detail_original: true
- web_search_tool_type: text_and_image（同时 supports_search_tool: false）

与产品原理第 8 条「能力如实展示」冲突。本会话窗口曾显示 258400 = 旧目录 `context_window` 272000 × 95%。A2 起 Grok 写官方推荐 500000 / compact 475000，未知族剥骨架窗；这是客户端请求配置，不是 3rxai 探针窗。图像/搜索泄漏已由 E8-1/E8-3 overlay 剥离（2026-09-21）。

- [x] **E8-1** overlay 删除无对应 CapabilitySet 的图像/搜索泄漏字段（`supports_image_detail_original`、`web_search_tool_type`）。
- [x] **E8-2** 为 Grok 写入官方推荐窗 500000 与 compact 475000（窗 × 95%）；未覆盖的 DeepSeek/Claude/未知族剥 GPT 骨架窗，不发明数字。未探针不写 1000k。不是 3rxai 实测窗。
- [x] **E8-3** catalog 测试锁定：纯文本 Grok 条目不得广告图像细节或图文搜索工具类型。

## E9 错误码语义

incompatible_compaction_envelope 的用户文案只适合「compaction item 里有解不开的信封」。E1 实际撞上的是 reasoning 密文。同类工具（Cockpit #2012、OpenCodex 信封）也把 reasoning/encrypted_content 当独立问题。

- [x] **E9-1** 与 E1-1 一起改码和文案。
- [ ] **E9-2** 诊断日志区分 item_type=compaction|reasoning|other，不记录密文正文。

## 建议修复顺序

1. **E1/E9**：默认 App Grok 长会话现在会在 compact 时死掉；先改错误码/产品选择，否则看图/生图做好了也会在归档时中断。
2. **E8/A**：不依赖上游，先停 GPT 骨架泄漏。
3. **E7/B**：看图探针与 CapabilitySet。
4. **E2/C**：生图技能产品化与防回归。
5. **E3/E4/E5**：文档化边界，不改 Gateway 范围。

## 证据路径（只读）

- 会话 rollout：/Users/populus/.codex/sessions/2026/09/19/rollout-2026-09-19T16-48-13-01a0b8da-0e36-7bf2-9344-81f272b14558.jsonl 与同日前缀 T17-27-18 的续文件
- 生成目录：/Users/populus/.codex/compat_default_842712d078a0531c.models.json
- 技能：/Users/populus/.codex/skills/grok-imagine/
- 出图：/Users/populus/Documents/Codex/2026-09-19/n/outputs/
- 代码：src-tauri/src/managed_gateway/compaction.rs、managed_launch.rs、model_family_profile.rs、managed_codex_config.rs

不要把 rollout 正文、图片或 key 复制进 3R 数据库或普通日志。

## E10 Summarizer 首字节 504（2026-09-20）

### 现象

默认 App Grok 长会话 compact 时，Codex 报 504：`3R Codex Gateway 无法转发到 https://ai.3rcd.com/v1/responses：等待上游首字节超时`。打印 url 是 `127.0.0.1:43123`。同一笔约 918KB 的请求连打 5 次（账本 2519–2523，各约 30092ms），第 6 次 sidecar `compact_summarize code=ok`，随后普通回合 token 掉到约 3 万，会话可续。

### 根因边界

- 信封已过：不是 `incompatible_compaction_envelope`。
- 3rxai `first_byte_seconds=30`；Summarizer 清空 tools、强制 `stream=false`，超时打在 `reqwest.send()`（等 HTTP 头）。非流式中转常等整段生成完才回头。
- 摘要请求未降 `reasoning`。Gateway 按合同不换路、不重试；重试来自 Codex 客户端。

### 待办（待 commander 确认后再改代码）

- [ ] **E10-1** 加长 3rxai 首字节（不超过该渠道非流式总计，当前 120）。
- [x] **E10-2** Summarizer 按 §8.2.1 改走 SSE 再收齐，让首字节变成真正的第一个事件。
- [x] **E10-3** 摘要请求降到 `low` / `medium`。
- [ ] **E10-4** 窗口提早 compact。A2 已把 Grok 从 272k 调到官方 500k（更晚 compact）；若仍要提早，用高级映射把窗/阈值改小，不要再动 family 默认。

## E11 Codex 旁路短模型打 luna（2026-09-20 / 合同 2026-09-21）

### 现象

Grok 会话改名、压缩后写侧栏说明，以及「分析其他用户cockpit」（正文 `gpt-5.6-sol`）开聊起标题，3rcd 后台出现 `gpt-5.6-luna`。

### 根因边界

ChatGPT.app 将 `thread_title` / `thread_description` / `thread_title_reconsideration` / `thread_summary` / `ambient_suggestion_safety` 写死 `gpt-5.6-luna` + `effort=low`，`modelProvider=null`，`allowProviderModelFallback=true`，`ephemeral=true`。请求仍进本机 Gateway。精确路由找不到 luna 时，`compatibility_fallback_from_config` 把它留在官方回退白名单，走 `compatibility_native`。兼容目录 `merge_compatibility_catalog` 同时保留 bundled 原生行，所以只在 3R 勾选里拿掉 GPT 不够。不是 3R 把当前 Grok 聊天改写成 luna，App 也不会自己改用 Grok 兜底。

与 E2 不同：E2 是 grok-imagine 误选 image2 宿主。

### 待办

- [x] **E11-1** 产品决定：不能容忍未选 GPT 时出现 luna。不是 fail-closed 改不了名，而是当前模型族-only。
- [x] **E11-2** 合同已写入 [F 设计](2026-09-21-f-family-only-ephemeral.md)：未选 GPT 族则 merge/回退都去掉 GPT slug；luna 做隐藏别名，出站走工作区默认 Responses。选了 GPT 族则保持 native 回退。残留 ChatGPT 登录不算选了 GPT。
- [x] **E11-3** 已实现：merge/回退滤 GPT 族；luna 隐藏目录行 + Gateway `ephemeralFamilyAliases` 映射到站桩 Responses。不进 RouteSet 身份表。
