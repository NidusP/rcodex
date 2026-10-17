# 三方 Responses 的目录、模型族与网关合同

> 状态：2026-10-16。三方目录开放本机 node_repl（node_repl_disabled=false），官方 JS 自动审核仍不要求。这份改动随 0.1.3 发版。Chat 自动审核不指向自己。模型族合同已按厂商卡片写入。换渠道续接和 namespace 摊平仍未做。
>
> 对外清单见文末。网站发布前以本文和当时代码为准，不要把「尚未单独剔除」写成已经关闭。

## 目标

三方 Responses 的行为由三处决定，变化时只改对应的一处：

1. 模型族合同声明这个上游家族有哪些数字和开关。
2. 目录生成把合同写成 Codex 能读的模型条目，从而决定客户端会不会发出某项能力。
3. 网关只读已经固化的路由能力，处理已经发出的请求。它不识别 Grok、DeepSeek 这类名字。

GPT 族请求体保持原样，只替换公共模型 ID、注入渠道 Key、去掉官方认证。非 GPT 才进入中继兼容。

## 现状

启动时 `managed_launch/prepare.rs` 按渠道协议和模型族写出 `CapabilitySet`，再交给目录生成。目录在 `managed_codex_config/catalog.rs` 克隆官方骨架，并调用 `overlay_third_party_catalog_entry`。网关在 `managed_gateway/http.rs` 用 `needs_relay_compat` 判断要不要改写，具体删除和改写散落在 `managed_gateway/protocol.rs`。

`needs_relay_compat` 今天等于「上游不像 GPT」。它适合当总开关，不适合在网关里增加 `if grok`。Grok 的推理档、50 万窗口和看图已经在 `model_family_profile.rs`，网关转发函数并不认识 Grok。

`protocol.rs` 同时放着中继改写、Chat 转换、用量和响应解析。Chat 转换还没作为正式能力打开，继续堆在这里会让 Responses 的规则难找。

## 改造

不改现有用户可见行为。先把所有权拆开，再允许以后按族加能力。

### 模型族

`model_family_profile` 只回答「这个上游 ID 属于哪一族，以及这一族声明了什么」：推理档、默认档、上下文窗口、压缩阈值、是否看图、是否去掉官方速度档。本机 `node_repl` 对三方模型保持开放；官方 ChatGPT 自动审核不要求。

没有官方数字或探针的项保持空或 false。禁止把 Grok 的窗口或看图抄给其他族。

### 目录

目录生成只做三件事：克隆当前 Codex 版本的骨架、写入身份和显示名、按 `CapabilitySet` 覆盖或删除字段。

覆盖必须显式。工具开关为开时，不得无声留下骨架里的 `experimental_supported_tools`、skills 或插件说明。要么合同声明保留，要么删掉。这一步今天还没做，见文末「尚未单独剔除」。

### 网关

非 GPT 的 Responses 改写收成有顺序的管道，入口仍在转发之前：

1. 按能力删除可选工具。当前只有网页搜索。
2. 删除中继不接受的请求字段。当前是 `metadata`。
3. 把 `agent_message` 收成公开 user 消息。读不出内容就拒绝。
4. 能力预检。不支持的工具、搜索或图片在本机失败，不发给上游。
5. 压缩触发按当前隔离实例的 `LocalClient` 合同拒绝，不转发官方远程压缩。

GPT 族不进入管道。`previous_response_id` 继续原样转发。换渠道后续不上时，应返回历史不兼容，不静默删除编号或历史。该项仍是既有待办，不属于这次整理。

网关新增行为时只增加能力位和对应步骤，不在 `http.rs` 写模型名。

### 文件

- 族合同留在 `model_family_profile.rs`。
- 目录覆盖留在目录生成。
- `protocol.rs` 里的 Responses 改写、Chat 转换、用量解析分开。Chat 文件可以先只搬家，不实现新的函数工具转换。
- 每一步改写用现有转发测试锁住输入和输出。

## 实施顺序

对应 2026-10-13 的优化讨论。前两步不改变发给上游的 JSON。后两步才改变失败方式，而且默认不改变 Grok 今天能用的工具。

### 第一步：把改写收成管道

2026-10-13 已落地。入口在替换模型 ID 之前只调用 prepare_responses_relay。顺序仍是：按能力删可选工具、删 metadata、能力预检、再把 Responses 的 agent_message 收成 user 消息。GPT 在删改之前返回。官方路由仍不改 agent_message，随后照旧返回 official_route_unavailable。

Chat 文本转换搬到 chat_bridge。转发只在渠道协议是 Chat Completions 时调用它。函数工具和 apply_patch 的转换以后加在这个文件里，不写进 Responses 管道。用量、结束状态和 SSE 仍留在 protocol.rs，两条协议共用。

测试固定当前行为：搜索工具被去掉，metadata 被去掉，agent_message 变成 user 消息，函数工具和 apply_patch 原样保留，previous_response_id 原样保留。

### 第二步：能力只从路由读

`needs_relay_compat` 仍表示「不是 GPT，才进入管道」。管道内部不再出现 Grok、DeepSeek、Claude 字符串。

搜索和图片继续读 `CapabilitySet`。namespace 增加一个能力位，默认是原样转发。没有任何模型族在这一步把它改成拒绝。模型族只负责写出能力，网关只负责执行。

目录继续关闭网页搜索、WebSocket 和 Responses Lite。本机 `node_repl` 对三方模型保持开放：目录 `node_repl_disabled` 为 false。Codex 会把这个开关写进回合元数据，本机 `node_repl` 读到 true 就会返回 `node_repl is unavailable for this model`，Browser、Computer Use 和其它依赖它的插件都会在开场失败。官方 ChatGPT 自动审核仍不要求，`node_repl_auto_review_required` 为 false。隔离实例继续写入本机 `node_repl` 与 `cua_repl`。工具开关为开时，骨架残留的实验工具、skills 和插件说明改为显式删除或显式保留，不再无声继承。

### 第三步：换渠道的续接编号在本机失败

网关记住自己转发成功的响应编号，以及它属于哪一个渠道版本。下一次请求里的 previous_response_id 如果就是这个渠道版本发出的，继续原样转发。

编号从未由这个渠道版本发出，包括官方 GPT 会话留下的编号，非 GPT 路由在本机返回 route_history_incompatible，不发给上游。不删除 input 里的历史。记录响应编号失败时维持今天的行为，原样转发，避免误伤正在使用的 Grok 会话。

同一渠道内的多轮对话不受影响。这一步解决的是 GPT 旧会话切到 Grok 后续不上、现在要等上游 502 才知道的情况。

### 第四步：namespace 仍默认不动

只有某个模型族被真实探针确认会拒绝 namespace 外壳，才为那一族打开摊平，并在响应里把名字还原回去。未确认的族，包括当前 Grok，保持原样转发。不把 xAI 的特例写成所有三方模型的默认手术。

## 不在这次


- 不实现 Chat 的函数工具和 `apply_patch` 包装。
- 不把 `namespace` 摊平。未知模型继续原样转发。
- 不打开网页搜索、WebSocket、Responses Lite 或官方远程压缩。
- 不自动清除 `previous_response_id`。

## 验收

- 同一条非 GPT Responses 请求，整理前后发给上游的 JSON 一致。
- GPT 族请求体除模型 ID 和认证外不变。
- 网关源码不再用 Grok、DeepSeek、Claude 决定删什么。
- 文末清单与代码一致。骨架里残留、但代码没有明确删除的项，仍标为尚未单独剔除。

## 对外清单

下面是 2026-10-13 三方 Responses 已经确定的行为，供以后放到网站。对用户只保留能力名称和结果。

### 不提供

- 网页搜索。目录不声明搜索，请求里的搜索工具会在转发前去掉。
- Fast 和 priority 速度档。
- 官方模型才有的 max、ultra 推理档。每个模型族用自己的档；没有档的族不凭空添加。
- 官方 ChatGPT 对 Node REPL 的自动审核。本机 JavaScript 仍走 Codex 自己的批准/elicitation，不把官方审核模型当成前提。
- Chat 路由的自动审核覆盖。Responses 条目指向当前对话模型自己；Chat 不写。审核回合常带 previous_response_id 和 Responses 专用 item，chat_bridge 会失败。
- 实验性上下文管理。
- WebSocket 优先，以及 Responses Lite。客户端按普通 HTTP 的 Responses 请求发送。
- 非 GPT 系列的图片 original 精度。Grok 4.x 可以看图，但只使用普通精度。
- 没有族合同、也没有单独填写时的上下文窗口数字。不会沿用 GPT 骨架上的窗口。
- 官方远程压缩。当前隔离实例收到压缩触发时在本机拒绝，不把压缩交给官方接口。

### 会改写

- 子任务之间的私有 agent message 会收成普通用户消息再转发。内容无法读取时，这一轮在本机失败。
- 请求上的 metadata 会去掉。它只是客户端标签，不是对话内容。

### 保留

- 本机 Node REPL。目录不把三方模型标成禁用；Browser、Computer Use 和其它走 `node_repl` 的本地插件可以调用。
- 本地函数工具，包括 shell 和子任务。
- apply_patch 这种自由文本补丁工具。语法不拆开，原文转发。
- 插件和 MCP 的 namespace 外壳，以及 tool_search。Codex 发出就会原样转发。
- previous_response_id。中继没有对应存档时可能报错，R Codex 不删这个编号，也不自动换协议。
- 并行工具调用。
- 模型族已经声明的能力。Grok 仍是自己的推理档和 50 万上下文，Grok 4.x Responses 可以看图。2026-10-13 起，DeepSeek、GLM、Qwen 的档位、窗口和看图也写在模型族合同里，目录按合同生成。网关仍然不按这些名字改请求。

### 尚未单独剔除

工具开关为开时，目录仍可能留下官方骨架上的实验工具列表、skills 说明和插件说明。代码没有逐项删除。网站在补上显式覆盖之前，不要写成这些也已关闭。
