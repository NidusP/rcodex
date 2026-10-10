# Grok 会话问题、结论与下一步待办（2026-09-19）

> 状态：A1/A2/A3/B3/D5/F3 已落地。Grok 4.x Responses 看图已打开。C 停放。F 当前模型族-only 已实现。D5 实机暂不验收。
> 来源会话：确认当前模型是否为 Grok（thread 01a0b8da-0e36-7bf2-9344-81f272b14558，cwd /Users/populus/Documents/Codex/2026-09-19/n）。
> 接管原因：用户要把该会话归档；最后一轮「将上述问题及解决方案做成下一步待办并落档 md」在远程压缩阶段以 incompatible_compaction_envelope 失败（systemError）。本文件只记录问题、已确认结论和下一步；错误拆解见 [Grok 会话错误分析与待办](2026-09-19-grok-session-error-todos.md)。
> 设计依据：[产品原理](../3rcodex-product-principles.md) 第 8 / 15 节、[混合路由详细设计](../codex-hybrid-routing-design.md)、[第三方目录 overlay](../third-party-model-reasoning-levels.md)、[混合路由执行计划](../codex-hybrid-routing-execution-plan.md)。

## Goal

- 用户可见结果：后续实现可以按本清单继续，不必再翻已中断会话。Grok 聊天、看图、生图、目录能力和默认 App 长会话压缩被拆开，不再混成一个「Grok 能不能多模态」问题。
- 范围：把 2026-09-19 该会话里的问题、现场结论、已落地技能和 3R Codex 代码缺口写成可执行待办。
- 非目标：本文件不改 RouteSet、Gateway、目录生成器或个人技能；不把未探针的图像能力写成 true；不把 grok-imagine 生图能力写进 grok-4.6 聊天路由。

## 对话问题与已确认结论

| # | 用户问题 | 结论 | 状态 |
| --- | --- | --- | --- |
| Q1 | 当前模型应该是 Grok 吧？ | 是。会话 turn_context.model 为 a55ea5dd-0fb1-4599-a759-ce9b1c6e-grok-4.6，显示名 3rxai/grok-4.6。不是 Grok 5.6。 | 已确认 |
| Q2 | Codex 里能不能用 grok-image / grok-imagine 生图？ | 能，但必须走 Grok 族自己的 /v1/images/generations，凭证与当前 3rxai 聊天通道相同。不能借 GPT Responses 宿主，也不能打本机 Gateway 的 /v1/responses 冒充生图。 | 已确认；技能已落地在 ~/.codex/skills/grok-imagine |
| Q3 | 直接调用 grok-imagine-image 出一张小猫钓鱼 | 第一次误走 image2 + gpt-5.6-luna 宿主，图能出但链路错。纠正后用 3rxai key 打上游 /v1/images/generations 成功。 | 错误路径见错误待办 E2；正确路径已冒烟 |
| Q4 | 聊天模型不是 grok 5.6 吗？何来 gpt-5.6-luna？为何看不到图？ | Luna 只出现在错误的 image2 Responses 宿主请求里，不是这条聊天模型。当前 Grok 会话 view_image 被系统拒绝：you do not support image inputs。3rcodex 把 grok-4.6 标成 input_modalities: [text]、supports_images: false。 | 已确认 |
| Q5 | 期望参考 grok-build，用同一套 URL/Key 在 Grok 族内生图，并做成通用技能 | 可行。grok-build 与 grok-imagine-image 同在 3rxai。本机 127.0.0.1:43123 只有聊天 /v1/responses；技能解开回环后打上游图片接口。已写成 grok-imagine 技能，image2 在当前模型为 Grok 或用户点名 grok-imagine 时让路。 | 个人技能已落地；未进 3rcodex 仓库 |
| Q6 | 用 grok-imagine 技能生成美女撸猫 | 走 grok-imagine-image / 3rxai 成功，文件 outputs/grok-imagine/woman-petting-cat.jpg。代理仍看不到像素。 | 已确认 |
| Q7 | 有办法让当前会话看图吗？官方 GPT 能看吗？ | 当前这条 Grok 任务不能自己打开读图。官方 GPT Codex 有 localImage / imageView。要看像素需换 GPT Codex 任务，或等 3rcodex 打开并验证 Grok 图像输入。 | 待办 B |
| Q8 | 是不是 Grok 4.6 纯对话、非多模态，所以不能看图？ | 不完全是。xAI 产品侧 Grok 4 系通常能看图。卡住的是 Codex + 3rcodex 透传没把图像输入接上，不是「模型天生只会聊天」。 | 已确认 |
| Q9 | 「Codex + 3R 透传没把看图接上」是中转问题还是 3rcodex 应用问题？ | **3rcodex 应用**。启动时 CapabilitySet 写死纯文本；Gateway 见 input_image / image_url 且 supports_images=false 就返回 unsupported_images。3rxai Responses 是否真吃图，这条链路还没测到。把对话拖进 3rcodex 项目目录不是根因。 | 待办 A/B |
| Q10 | 为何 supportsImages 为 false，怎样正确设置，还有哪些能力要查？ | 这是**看图**开关，不是 grok-imagine **生图**。false 是产品原理第 8 条「能力如实展示」的故意保守，不是中转先掐图。正确做法：family profile 补 input_modalities / supports_images，写入 RouteSet，目录 overlay 跟着走；打开前必须用 3rxai Responses 小图探针。未验证保持 false。同批泄漏字段见下表。 | 待办 A |

## 能力字段对照（当前默认 App 目录实测）

来源：~/.codex/compat_default_842712d078a0531c.models.json 中 3rxai/grok-4.6。

| 字段 | 现状 | 问题 | 下一步 |
| --- | --- | --- | --- |
| input_modalities / supports_images | 原目录 [text]/false；B3 后 Grok 4.x Responses 为 [text, image]/true | 看图总开关 | 已写入 CapabilitySet。须应用配置后 view_image |
| reasoning_levels | family overlay 已覆盖，Grok 4.6 含 xhigh，默认 high | 本会话已修过 | 保持 |
| supports_tools | Responses 为 true | 合理 | 保持 |
| supports_search_tool | false | 合理的保守值 | 保持 false，直到有搜索合同 |
| web_search_tool_type | A1 已从纯文本三方条目删除 | GPT 骨架泄漏 | 已落地 |
| supports_image_detail_original | 非 GPT 系列删除，包括已打开看图的 Grok。上游 ID 以 gpt- 开头或含 openai 的系列保留 | 按模型系列名判断，不按路由来源。original 是 Codex 扩展档；探针通过也不给非 GPT 广告 | 2026-10-03 锁定。不改 Gateway。用户自备的三方 GPT 中转若拒绝 original，不在 3R 范围 |
| context_window / max_context_window | 启动后写 Grok 官方推荐 500000 / 500000；未覆盖时不再抄 GPT 272000/872000 | 旧目录仍可能是 GPT 窗，直到保存工作区并应用配置 / 重启默认 App | A2 已落地：UI 覆盖 → family 默认 → 仍无窗口则剥骨架。未探针不写 1000k |
| compaction_support | 默认 App 三方为 gateway_summarizer | 与图像无关；真实 Grok 长会话已踩坑 | 见错误待办 E1 |
| grok-imagine-* | 在 3rxai /models，不在 RouteSet | 生图模型，不是 grok-4.6 聊天能力 | 不要写进 grok-4.6 条目 |

启动物化：[src-tauri/src/managed_launch.rs](../../src-tauri/src/managed_launch.rs) A3 按 `family_image_support_for_route` 写 supports_images / input_modalities（Grok 4.x Responses 为 [text, image]）。A2 写窗口。A1 overlay 按上游模型系列名处理 `supports_image_detail_original`：gpt- / openai 保留，其余系列删除，看图打开也不保留。不支持搜索时删除 `web_search_tool_type`。不按官方账户或渠道来源判断，也不改 Gateway 出站正文。旧目录须保存工作区并应用配置 / 重启后才更新。已经写进旧会话的 original 不会被改写。

## 已落地（本会话，非 3rcodex 仓库）

- 个人技能 [grok-imagine/SKILL.md](/Users/populus/.codex/skills/grok-imagine/SKILL.md) 与脚本 scripts/generate_grok_image.py。凭证顺序：CLI/环境变量 → 当前 Grok 的 3R Provider → 非回环 config.toml → XAI_API_KEY + https://api.x.ai/v1。回环 Gateway 会被解开到上游 /v1/images/generations。失败不回退 GPT。
- image2 技能 description 已排除「当前模型是 Grok / 用户点名 grok-imagine」。
- 冒烟文件：/Users/populus/Documents/Codex/2026-09-19/n/outputs/grok-imagine/kitten-fishing-skill.jpg、woman-petting-cat.jpg。

这些技能不随 3R Codex 安装包分发。若产品要「所有 Codex+Grok 用户丝滑生图」，需要单独决定是文档推荐、技能安装器，还是应用内动作。

## 下一步待办

### A. 第三方目录 overlay：删泄漏、补可验证能力

- [x] **A1** 在 overlay_third_party_catalog_entry 删除或改写 GPT 骨架泄漏：`web_search_tool_type`、`supports_image_detail_original`。窗口三键已由 A2 覆盖或剥离，本票不再动窗口。
- [x] **A2** 每模型官方推荐窗 + 接入 UI 覆盖（2026-09-20）。只动窗口，不把 `input_modalities` / `supports_images` 放进 profile。优先级：工作区高级映射数字框 → family 默认 → 官方 bundled 精确 slug → 仍无窗口则从克隆条目删除 `context_window` / `max_context_window` / `auto_compact_token_limit`，不抄 GPT 272000/872000。Grok 族（含 4.6 及其它本轮未单列的 Grok 版本）默认 500000，自动压缩 475000（窗 × 95%）。DeepSeek / Claude / 未知族为 None（剥泄漏，不发明数字）。覆盖落在 `launch_preset_model_selections`（schema v16），不是身份表。校验 `None` 或 `1..=999_999`；拒绝 0 和 ≥1000000；两 UI 值都有时 compact ≤ window。启动物化：`context_window = UI or family`；`auto_compact = UI or (family if family ≤ 已解析窗) or window×95%`。不写进程级 `model_context_window`。已有工作区空列 = family 默认；必须保存工作区并应用配置 / 重启默认 App。未探针不写 1000k。写 500k 会比旧 GPT 272k 更晚 compact，超时仍归 D5。
- [x] **A3** 启动物化按 `family_image_support_for_route` 写 CapabilitySet.supports_images 与 input_modalities。当前 Grok 为 false/[text]。搜索仍不打开。
- [x] **A4** 回归：生成的 Grok 条目在 supports_images=false 时不得广告 supports_image_detail_original=true 或 web_search_tool_type=text_and_image；推理档、身份 overlay 与 A2 窗口测试保持。
- [x] **A5** 文档：图像/搜索泄漏修完后更新 [第三方目录 overlay](../third-party-model-reasoning-levels.md) 第 6/7 节。窗口合同（§6 优先级、产品原理第 15 节、schema v16）已随 A2 同步。未完成「Provider 能力编辑 UI」仍按原票，不在本切片做完整编辑器。

### B. Grok 看图（对话输入图片）

- [x] **B1** 2026-09-21 对 3rxai `https://ai.3rcd.com/v1/responses`、模型 grok-4.6、fixture `src-tauri/tests/fixtures/grok-vision-probe-red-square.png`（16×16 红方）。纯文本控制请求 HTTP 200（上游模型名 grok-4.6-build）。带 `input_image` / Chat `image_url`（data URL 或 https 红图）均为 400 Upstream rejected 或 502 Upstream request failed。脚本 `scripts/probe_grok_vision.py`。
  **官方合同不是 false：** [xAI Python SDK](https://github.com/xai-org/xai-sdk-python) README 的 Image Understanding 示例用 `client.chat.create(model="grok-4.6")` + `image(url)`，PNG/JPG、URL 或 base64。本机 `api.x.ai` / `docs.x.ai` 连不上（超时），无法对官方入口复验。3R `supports_images=false` 只表示 **3rxai OpenAI 兼容聊天入口未验证看图**，不是 Grok 4.6 产品为纯文本。
  复测传值：OpenAI 形 `image_url` 字符串/对象仍 400/502。Anthropic 形 `input_image.source={type:url|base64}` HTTP 200，但模型回复 `no image` / `Unknown`，input_tokens 与纯文本对照几乎相同（2190 vs 2190）。一次 `red circle` 不能当看图成功——未给「无图则说 no image」时的猜测。
  对照 OpenCodex：Chat 上游用 `{type:"image_url", image_url:{url, detail}}`，Responses 保留 Codex 的 `{type:"input_image", image_url: data URL}`。3rxai 上 Chat 形仍 400/502；Responses 在 `type=message` + `input_image.image_url=data:image/png;base64,...` + `detail=auto` 时出现 HTTP 200，左蓝右黄 fixture 回复 `yellow blue`（两色命中、左右颠倒）。同形此前多次 400，渠道不稳定。尚未改 supports_images。
- [x] **B2** 探针失败：family.supports_images 保持 false；A3 写入路径已接上，因此现在仍是纯文本。用户可见：当前 3rxai Grok 聊天渠道未验证看图。不要静默改 true。
- [x] **B3** Grok 4.x Responses：family.supports_images=true，启动写入 CapabilitySet `[text, image]`。依据 3rxai 标准 Responses `input_image.image_url` data URL（左蓝右黄回复 yellow blue）。Chat 与 grok-3 仍 false。默认 App `view_image` 须应用配置后手动看。
- [x] **B4** 非目标，保持：不把 grok-imagine 生图写进 grok-4.6 聊天能力；不伪造工具/搜索；Chat Completions 适配器仍只保证文本。

### C. grok-imagine 生图产品化（已停放）

2026-09-21 commander：原始需求是 Codex 里用 Grok 生图，与 3rcodex 无直接联系。可改为日后做类似 CC Switch / Cockpit 的技能管理，**优先级极低，暂不考虑**。个人技能 `~/.codex/skills/grok-imagine` 保持现状。不要给 Gateway 加 `/v1/images/generations`。不要和 F 混票。

- [ ] **C1–C4** 停放。不排进当前实现队列。

### D. 默认 App Grok 长会话压缩（阻塞归档）

本会话在用户要求落档时被远程压缩打断。合同与证据见错误待办 E1。与图像切片独立，但同属「Grok 在默认 App 上的真实矩阵」。

- [x] **D1** 区分 type=compaction 的外国信封与 type=reasoning 的 Grok encrypted_content。外国信封仍 `incompatible_compaction_envelope`；折叠后残留密文用 `incompatible_reasoning_envelope`。
- [x] **D2** 选项 1 已写入 §8.2.1 并改 Summarizer：摘要前折叠 reasoning 密文、可见文本写成 assistant 历史。不采用选项 2/3。
- [x] **D3** 2026-09-20 默认 App Grok 会话 `01a0bcf6-e8ee-7c60-8cb8-aa74474c914d` 触发 GatewaySummarizer：sidecar `support=gateway_summarizer trigger=1` 最终 `code=ok`；账本 input 约 228622 落到 30654。不是信封拒绝。中间 5 次 `upstream_first_byte_timeout`（约 30092ms）后第 6 次成功。超时优化见 D5。
- [x] **D4** 不得为了续接而把官方 compact blob 发给 3rxai，也不得静默删用户可见历史。合同与实现保持这条。
- [x] **D5** Summarizer 出站改为 `stream=true` 并收齐 SSE；摘要副本 high/xhigh/max/ultra 降为 medium。不改 3rxai 超时表、不改 family 窗。Gateway 一次压缩回合仍不换路、不重试。实机是否不再 30 秒首字节 504，留待下次默认 App Grok compact 观察。

### E. 验收与交接

- [x] **E1** A 切片：overlay/catalog 单测已随 A1/A2/A4 落地（`cargo test --lib`）。本机旧兼容目录须应用配置后抽查。
- [x] **E2** B 切片：3rxai Responses 小图探针已记入 B1/B3（标准 `input_image.image_url` data URL，左蓝右黄 → yellow blue）。Chat 形仍 400。mock 未代替。
- [x] **E3** D 切片：mock 已覆盖 reasoning blob；2026-09-20 实机 compact 一次已成功。剩余是 D5 超时实机观察，不是再验信封。
- [x] **E4** 证据已写回本文件、错误待办、执行计划、AGENTS、产品原理。原会话保持归档。F3 代码证据见 [F 设计](2026-09-21-f-family-only-ephemeral.md)。用户手验（改名/view_image/D5 504）仍待应用配置。

## 非目标

- 把未探针的 supports_images 改 true。
- 给 grok-4.6 聊天路由广告生图。
- 为生图把隔离 Provider 改名为 OpenAI，或把图片请求打进官方账户。
- 在本票实现完整 Provider 能力编辑 UI。
- 用本中断会话做字段级历史改写或自动恢复。

### F. Codex 旁路短模型（标题 / 侧栏说明）

**已实现（2026-09-21 F3）。** 设计见 [F 当前模型族-only](2026-09-21-f-family-only-ephemeral.md)。

默认 App 改会话名称、写侧栏说明，不是当前聊天模型在跑。ChatGPT.app 把 `thread_title` / `thread_description` / `thread_title_reconsideration` / `thread_summary` / `ambient_suggestion_safety` 写死为 `gpt-5.6-luna` + `effort=low`，`modelProvider=null`，`allowProviderModelFallback=true`，`ephemeral=true`。请求仍进本机 Gateway。2026-09-20 证据：Grok 会话起标题和压缩后 15:36 的 `thread_description`（194090 token）都打 luna、`compatibility_native`、官方转发；「分析其他用户cockpit」正文是 `gpt-5.6-sol`，开聊时同样用 luna 起名。

根因不是 3R 把 Grok 聊天改写成 luna，而是实现前两处合同叠在一起：兼容目录 `merge_compatibility_catalog` 保留全部原生 bundled 行；`compatibility_fallback_from_config` 把未选中的原生 slug（含 luna）放进官方回退白名单。只在 3R 勾选里拿掉 GPT **不够**。App 也不会自己改用当前 Grok 会话模型兜底。F3 已改：未选 GPT 族则剥 merge/回退，luna 走隐藏目录行 + Gateway 快照别名。

commander：只接入 DeepSeek / Grok、没选 GPT 时 **不能容忍** luna 出现，必须「当前模型族-only」。残留 ChatGPT 登录不算选了 GPT。

- [x] **F1** 产品决定已锁：未选 GPT 族不得发布/回退/官方转发 GPT 族 slug；App 写死的 luna 改为隐藏别名，出站走工作区默认 Responses 路由（Grok-only 就是 Grok）。选了 GPT 族则保持现 native 回退。删 GPT 不会自动改用 Grok——所以要实现本票，不是再观察。
- [x] **F2** 与 E2 分开：E2 是 grok-imagine 误走 image2 宿主；F 是 App ephemeral 标题/说明。不要当成 3R 把当前聊天改写成 luna。
- [x] **F3** 实现：[F 设计](2026-09-21-f-family-only-ephemeral.md)。未选 GPT 则 merge/回退去掉 GPT slug；luna 隐藏目录行 + Gateway 快照别名走站桩 Responses。不进 RouteSet 身份表。

## Active Status

- 当前状态：A1–A5、B1–B4、D1–D5、E1–E4、F1–F3 已勾完。C 停放（极低优先级，不做）。代码侧本清单已闭环。
- 阻塞：无代码阻塞。正在跑的默认 App 仍是旧投影，须保存并应用配置 / 重启后 view_image 与改名别名才生效。
- 最近验证：`cargo test --lib` 311 passed（含 merge 剥 GPT、fallback 滤 GPT、luna 快照别名转发到站桩、无别名 404）。
- 用户手验（未做，不挡代码完成）：应用配置后 Grok 任务看一张本地 PNG；改会话名 3rcd 不再出现 luna；下次 compact 观察 D5 是否还有约 30 秒首字节 504。
- 不要做 C。原 thread `01a0b8da-...` 保持归档。
- 2026-10-03 更正：菜单栏会话和「回复问候」续聊的 400，不是 `detail: original`。`src-tauri/icons/tray-icon.png` 是 22×22（484 像素）。同一天对 `code.3rcd.com` 的 `grok-4.7` 探针里，总像素数小于 512 的图返回 400，512 像素及以上（包括 64×64 的 `original` 和 512×512 的 `high`）返回 200。目录不再广告 original 的改动保留，但不能让已经看过这张小图的旧线程恢复。
- 2026-10-03 收口：Codex + Grok 的大图可用。3rxai `grok-4.7` 上 64×64 的 `high`/`original` 与 512×512 的 `original` 均为 HTTP 200。小图暂不修。22×22 菜单栏图标在 3rxai 与 uuAI 的 Grok 4.6/4.7 上都是 400，正文只有 `Upstream rejected the request`。512 像素只是 3rxai 当天的对照分界，不是厂商合同，uuAI 未做阶梯。官方 api.x.ai 未测，公开 SDK 与社区检索没有对上的最小像素说明。非 GPT 继续删除 `supports_image_detail_original`。旧线程里的小图不改写。
