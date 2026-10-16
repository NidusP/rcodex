# Chat Completions 函数工具

> 状态：2026-10-13 出站、同步回流和流式 tool_calls 已接上。同日改为协议级工具映射：namespace 摊平后还原，tool_search 和 additional_tools 进入 Chat function，参数统一收成 JSON 字符串。不按模型解析正文里的伪工具标记。previous_response_id 仍拒绝。
>
> 日期：2026-10-12。

## 结论

Chat Completions 渠道可以支持 Codex 的函数工具，包括 spawn_agent 和本地 shell。不能借此宣布自定义 grammar、网页搜索、图片或 Responses 的加密推理也能用。

现在的纯文本合同是转换器的缺口，不是这类模型只能聊天。缺口有两处：

1. Chat 路由把 supports_tools 设成 false。请求离开本机前，tools 和 tool_choice 被删掉。预检因此看不到工具，请求仍会发出。
2. response_request_to_chat 只复制文本消息。历史里的 function_call 会报 unsupported_chat_history_item。回流时，finish_reason 为 tool_calls，或 delta 里出现 tool_calls，会报 unsupported_chat_output。流式正文只读取 delta.content。

硅基流动上的 DeepSeek-V4-Flash 因此收不到工具定义，却把调用写成正文里的 DSML。转换层把这段正文当成普通 assistant 文本，finish_reason 仍是 stop，Codex 就正常结束。这不是模型把一次标准 tool_calls 写坏了。

上面两处是当时的缺口。现在出站会带上 Chat tools，回流把 tool_calls 还原成 Codex 的 function_call、custom_tool_call 或 tool_search_call。

## 要做的转换

只转换 Responses 的 function 工具和 Chat Completions 的 tools / tool_calls。

出站：

- type 为 function 的工具保留，写成 Chat 的 {type: function, function: {name, description, parameters}}。
- 历史中的 function_call 写成 assistant.tool_calls。紧随的 function_call_output 写成 role 为 tool 的消息，并用同一个 call id。
- 转换只看 Responses 和 Chat Completions 的字段形状，不看模型名。CC Switch 的 `transform_codex_chat` 是这个方向的参照：Codex 发 Responses，上游只有 Chat。Cockpit 的 `build_responses_body_from_chat_completions` 是反方向，给 Chat 客户端接 Responses 上游，R Codex 的 Codex 入口用不到。
- function 工具写成 `{type: function, function: {name, description, parameters}}`。`parameters` 不是 object 时补成 object。`arguments` 无论原来是字符串还是对象，出站都收成 JSON 字符串；回流仍把这个字符串放回 function_call.arguments。
- custom / apply_patch 包成只有字符串参数 input 的 function。description 带上原工具 JSON，grammar 不拆字段。回流若 arguments 是 `{"input": ...}` 就取出 input，否则整段字符串当作 input，还原成 custom_tool_call。
- namespace 外壳不发给 Chat。子 function 的名字改成 `namespace__name`，超过 64 字符时截断并加上稳定哈希。回流按这张对照表还原 name 和 namespace。
- tool_search 写成同名 function，参数是 query 和 limit。历史里的 tool_search_call 写成 tool_calls，tool_search_output 写成 role=tool。回流还原成 tool_search_call，arguments 是对象。
- input 里的 additional_tools 和 tool_search_output 只用来登记工具，additional_tools 本身不是一条消息。
- web_search、图像生成和 code interpreter 没有 Chat 对应字段，从工具列表去掉。tool_choice 点名它们时，请求失败，错误码仍是 unsupported_tools。
- previous_response_id 和压缩触发器继续拒绝。图片仍按现有规则写成 image_url。不补 reasoning_content，那是部分思考模型的私有字段，不是 Chat Completions 协议。

回程：

- 流式 delta.tool_calls 按 index 拼装。arguments 保持上游给的 JSON 字符串，不在网关里执行。
- finish_reason 为 tool_calls 时，向 Codex 返回一个或多个 function_call 输出项，状态为 completed。Codex 在本地执行后再把结果送回来。
- 只有文本时，行为与现在相同。
- 正文里出现 DSML 或其它伪工具标记、但没有 tool_calls 字段时，不解析、不执行。这一轮改为失败，错误码 chat_tool_call_not_structured，避免再显示成一次成功的空操作。

启动目录里，Chat 路由的 supports_tools 改为 true。function 与 apply_patch 这类 custom 工具都进入转换器。custom 以上游 function 的形式出现，回到 Codex 时仍是 custom_tool_call。

## 图片与搜索

图片可以转换。用户消息里的 input_image.image_url 写成 Chat 消息里的 image_url。工具结果里的 input_text 收成 role=tool 的字符串，同一结果里的 input_image 不放进这段字符串。很多 Chat 中继拒绝工具消息带图片，所以图片改到紧随其后的 user 消息，类型仍是 image_url。detail 只保留 auto、low、high。是否对 Codex 广告看图，沿用现有模型族合同：Grok 4.x 为 true，DeepSeek 和未知族为 false。没有族合同或真实看图探针的模型仍然不广告看图。detail=original 仍只留给 GPT 系列。

目录不因某家渠道拒绝看图而改写。Responses 和 Chat 都先把图片照常发给上游。只有 HTTP 400 且错误原文明确拒绝图片时，例如 not a VLM、text-only、does not support image，才把该请求里的图片块换成「这里有一张图，当前模型看不到」，向同一渠道再发一次。不换渠道，不重试第二次，余额不足和其他 400 不走这条。

网页搜索不能按函数工具那样通用转发。Chat Completions 没有 Codex 的 web_search 工具，也没有对应的服务端搜索结果项。个别厂商有自己的搜索参数或插件，名称和结果形状都不统一。在没有该厂商的单独合同之前，Chat 渠道继续不广告 web_search。

## 不做

- 不把 apply_patch 的 grammar 拆成多个 JSON 字段。只包一个字符串参数 input。
- 不实现网页搜索和音频。
- 不把 DSML 当成第二种工具协议。
- 不在转发失败后改打 Responses 或重试。
- 不改已经启动的 RouteSet。下次启动才带上新的 supports_tools。

## 验收

- 只有 function 工具的请求，上游能看到 tools，Codex 能收到 function_call 并在本地执行。
- 带 function_call 和 function_call_output 的下一轮，上游能看到 tool_calls 和 tool 消息。
- 上游只回文本时，Codex 仍收到普通消息。
- 上游把工具写成正文标记、没有 tool_calls 时，Codex 收到失败，而不是一段 DSML 文本。
- 点名 web_search、图像生成或 code interpreter 时，请求在本机失败，不发给上游。点名 custom 或 tool_search 会转成对应的 Chat function。

## Codex 原生能力对照

官方 GPT 走 Responses，目录和请求都保留下面这些能力。三方模型只映射有稳定字段、而且该模型族声明支持的部分。没有对应字段的远端能力从目录拿掉，避免 Codex 发出上游必然拒绝的工具。

| 能力 | 官方 GPT | 三方 Responses | 三方 Chat | 处理 |
| --- | --- | --- | --- | --- |
| 本地 shell、spawn_agent 等 function 工具 | 保留 | 原样转发 | 转成 Chat function，结果再转回 function_call | 映射 |
| apply_patch 等 custom / grammar | 保留 freeform | 原样转发 | 包成 function，唯一字符串参数 input 放补丁原文；返回后还原成 custom_tool_call。grammar 不拆成 JSON 字段 | 借鉴 CC Switch 的包装 |
| 插件、skills、MCP namespace | 保留 | 原样转发 | 子 function 摊平为 namespace__name，回流还原 namespace | 协议映射 |
| tool_search | 保留 | 原样转发 | 写成 function，回流还原 tool_search_call | 协议映射 |
| 自动审核跟随当前模型 | 官方审核 | 指向该 Responses 模型自己 | 审核回合常带 previous_response_id 和 Responses 专用 item | Chat 不设置，待抓真实审核请求后再开 |
| 看图 input_image | 保留，含 detail=original | Grok 4.x 开，DeepSeek 和未知族关；去掉 original | 同样按模型族；input_image 写成 image_url | 按族映射 |
| 图像生成工具、code interpreter | 保留 | 无服务端实现 | 无服务端实现 | 剔除 |
| web_search | 保留 | 目录不广告 | 目录不广告，请求里也不转发 | 剔除 |
| 推理档 | max / ultra 等官方档 | 该族自己的档，去掉 max / ultra | 同 Responses | 按族映射 |
| Fast / priority 速度档 | 保留 | 删除 service_tiers 和 additional_speed_tiers | 同 Responses | 剔除 |
| 上下文窗口和压缩阈值 | 官方数字 | 仅族合同里有数字才写，否则删掉骨架数字 | 同 Responses | 按族映射 |
| 远程压缩 /responses/compact | 官方远程压缩 | 不走官方远程压缩，用 Gateway 摘要 | Chat 没有这条路径 | 三方改本地摘要 |
| supports_experimental_context | 保留 | 删除 | 删除 | 剔除 |
| previous_response_id 云端续接 | 保留 | 中继不一定接受，失败不换协议 | 必须把历史放进 messages | Chat 不使用 |
| Node REPL、Browser、Computer Use 运行时 | 保留 | 目录 node_repl_disabled=false，本机 MCP 可用；官方 JS 自动审核不要求 | 同 Responses。审核回合仍不指向 Chat 自己 | 本机运行时，不经 Chat 上游 |
| WebSocket 优先、Responses lite | 按官方 | 目录写 false | 目录写 false | 剔除 |
| 并行工具调用广告 | 保留 | 跟 supports_tools，Responses 为开 | 函数工具接上之前为关 | Responses 映射，Chat 先关 |

函数工具、apply_patch 的包装和按族看图，是 Chat 要对齐三方 Responses 的部分。web_search、远端生图、code interpreter 和官方远程压缩，两条三方路径都不承诺。本机 node_repl 由目录开关和隔离 MCP 提供，不经上游 Chat 或 Responses 实现。
