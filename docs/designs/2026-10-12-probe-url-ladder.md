# 探针地址阶梯

> 状态：已实现。2026-10-12 由 commander 锁定。本文取代 [添加模型渠道弹窗与探针结果](2026-10-02-channel-probe-dialog.md) 里关于「空路径先补 /v1」和「只有路径不存在才试 Chat」的合同。该文的弹窗、票据、隐私和「运行中不换协议」仍然有效。
>
> 日期：2026-10-12。

## 目标

用户输入的 Base URL 是前缀，不是已经带协议后缀的完整端点。探针按固定顺序尝试最多四个文本地址。第一个返回可解析成功响应的地址决定两件事：协议，以及以后转发用的前缀。四个都不通时，协议未确认。未确认的渠道不能加入隔离工作区，它的模型也不能进入工作区勾选或启动目录。

## 现状

这些行为和上面的目标不一致。

1. normalize_api_base_url（src-tauri/src/providers/operations.rs）在探针之前就把空路径写成 /v1。用户输入 https://api.deepseek.com 会先变成 https://api.deepseek.com/v1。已经写了路径则原样保留。
2. channel_probe::post_text 把路由常数 /v1/responses、/v1/chat/completions 去掉前缀 /v1，再拼到这个已经改过的 Base URL 上。Responses 实际只打 {base}/responses。以 DeepSeek 这条已保存渠道为例，只打过 https://api.deepseek.com/v1/responses。不会先打 https://api.deepseek.com/responses。
3. Chat 只在 Responses 明确表示路径不存在时再打一次 {base}/chat/completions。404、405，或 400 且正文属于固定的「未知路径」集合，才算路径不存在。401、402、429、5xx、超时和连接失败都停住，协议留空。
4. 新建渠道的修订把 transport_mode 写成 gateway_passthrough，因此 wire_api 固定是 responses（providers/store.rs 的 wire_api()）。协议没确认时，这个默认值仍然留在修订上。
5. 工作区加入渠道（addProviderToWorkspace）和勾选模型（toggleWorkspaceModelSelection）只排除图像生成模型和 Codex 系统角色。不看报告里的 wire_api。所以协议未确认时，模型仍然可以进隔离工作区。
6. 启动时 managed_launch/prepare.rs 用修订上的 wire_api 生成 RouteSet，再由 Gateway 拼 {base}/responses 或 {base}/chat/completions。未确认渠道会按默认 Responses 转发。
7. 一次探针最多 1 次 GET {base}/models 加 2 次文本 POST。模型列表用的也是已经补过 /v1 的前缀。

2026-10-12 的 DeepSeek 官方渠道记录符合第 2、3 条：GET /v1/models 成功，POST /v1/responses 返回 402，没有再打 Chat。报告 wire_api 为空，界面显示「协议未确认」，但修订仍是 Responses。

## 决定

1. 文本探针按下面的顺序进行。前一条没有确认成功，才打下一条。某一条返回 HTTP 2xx 且正文是可解析 JSON 时，立即停止。不再打后面的地址。
2. 用户前缀的路径在去掉末尾斜杠后已经以 /v1 结尾时，不再追加第二次 /v1。此时最多两条，而不是四条。
3. 四条（或已带 /v1 时的两条）都不确认：协议未确认。可以保存渠道和这次读到的模型名单，供渠道抽屉查看。不能把该渠道加入隔离工作区，也不能把它的模型写入工作区选择或启动目录。
4. 确认成功时，把赢的那条地址所对应的前缀写入渠道修订的 base_url，并把 wire_api 与 transport_mode 写成该协议。转发只使用这一对，不再在运行中走阶梯。
5. 401、402、429、5xx、超时、连接失败，以及路径不存在，都算这一条不通，继续下一条。不再把「只有 404/405 才换协议」当作探针规则。密钥错误时四条可能都是 401；这是本决定接受的代价，不在中途改判协议。
6. 运行中的 Gateway 转发仍然不因失败换协议、换前缀、换模型或重试 POST。阶梯只存在于用户点击的探针和「刷新模型目录」。

## 非目标

- 不按服务商名字、模型名字或「余额不足」等文案猜协议。
- 不在四个地址之外再试 /v1/v1、Anthropic /messages、Gemini generateContent，或带查询参数的 Azure 地址。
- 不把用户粘贴的完整端点（路径已经以 /responses 或 /chat/completions 结尾）自动削掉再拼。这种输入在校验时拒绝。
- 不改已经启动的 RouteSet。已经加入工作区的渠道，如果刷新后变成未确认，下次保存工作区或下次准备启动时剔除；正在运行的实例继续用旧快照。
- 不把响应正文写入普通诊断、日志或界面。诊断可以记录这次实际尝试的路径和 HTTP 状态。
- 不迁移已经确认过的旧渠道 Base URL。它们保持当初保存的前缀，直到用户再次探针。

## 地址

先做和现在相同的校验：只接受 HTTPS，回环地址可以是 HTTP；去掉查询、片段、用户名和密码；去掉末尾斜杠。空路径保持空，不再写成 /v1。

设用户前缀为 P。文本顺序：

| 顺序 | 地址 | 确认后的协议 | 写入修订的 base_url |
| --- | --- | --- | --- |
| 1 | P/responses | responses | P |
| 2 | P/v1/responses | responses | P/v1 |
| 3 | P/chat/completions | chat_completions | P |
| 4 | P/v1/chat/completions | chat_completions | P/v1 |

P 的路径已以 /v1 结尾时，跳过第 2 和第 4 条。例如 P 为 https://api.deepseek.com/v1 时，只打：

- https://api.deepseek.com/v1/responses
- https://api.deepseek.com/v1/chat/completions

P 为 https://api.deepseek.com 时，四条都打，直到某一条确认：

- https://api.deepseek.com/responses
- https://api.deepseek.com/v1/responses
- https://api.deepseek.com/chat/completions
- https://api.deepseek.com/v1/chat/completions

模型列表仍是文本探针的前提，因为它要提供候选模型 ID。顺序独立，而且更短：先 GET P/models；只有 P 不以 /v1 结尾且这一次没有得到非空模型列表时，再 GET P/v1/models。两次都没有可用模型，则不发文本请求，协议未确认。文本阶梯不因为模型列表出现在 /v1/models 就跳过第 1 条。

候选模型会先排除 Codex 内部角色（例如 `codex-auto-review`、`gpt-code-review`）和图像生成模型。它们不是普通文本对话模型，不能用来代表渠道的 Responses/Chat Completions 合同；完整模型目录仍然保存并由界面按现有规则隐藏。剩余候选按模型族的主流文本模型优先级排序：Grok 4.x、DeepSeek/GLM/Qwen/Claude 的常规对话 ID，以及 GPT 的 `terra`/`astra`/`sol`/`luna` 等常规后缀优先；无法识别的模型保持上游顺序。这个优先级只减少无效探针，不证明模型属于某个协议或一定可用。排序后最多取 8 个。某个候选返回模型级错误时继续用下一个候选，直到该地址有一个可解析的成功 JSON；路径明确不存在时不重复用其它模型撞同一路径。这样 `/models` 把内部角色或暂时不可用模型排在首位，都不会直接把整个渠道误判成另一种协议。

每条文本请求仍用现在的最小正文。Responses 是一句输入、max_output_tokens 为 8、stream 为 false。Chat 是一条 user 消息、max_tokens 为 8、stream 为 false。协议和确认前缀由第一个真正成功的候选决定，并把该候选模型写进探针报告。超时仍是每条 15 秒，不跟随重定向；单个地址最多尝试 8 个候选，完整阶梯最多 32 次文本 POST。

确认成功的定义不变：模型列表检查为 ready，文本检查为 ready，且 wire_api 是 responses 或 chat_completions。两条协议都可能成功时不会同时看到，因为 Responses 两条先打，成功即停。因此不需要再投票。

## 后备设计：混合协议 Provider 暂不实现

当前假设一个 Provider 的候选模型共享同一种上游协议。若未来出现同一 Base URL 下某些模型只支持 Responses、另一些模型只支持 Chat Completions，现有 Provider 级 `wire_api` 不能安全表达这种差异。

当前不为混合协议增加特殊入口或数据库字段。若探针或真实请求暴露出模型间协议冲突，应把渠道视为协议未确认并阻止加入工作区，避免用一个模型的成功结果覆盖另一个模型的协议要求。后续若要支持这种聚合接口，必须把协议能力写入每个模型路由，并让 RouteSet 按模型选择转发协议；不能继续只保存 Provider 级协议。

协议与传输的对应关系不变：

| 探针协议 | wire_api | transport_mode |
| --- | --- | --- |
| Responses | responses | gateway_passthrough |
| Chat Completions | chat_completions | gateway_chat_completions |

## 未确认与工作区

权威结论是连通报告上的 wire_api，不是修订里那个非空默认值。报告里的 wire_api 只有在本次探针确认成功时写成 responses 或 chat_completions；否则写成空。

以下都算没有确认：从未探针、模型列表失败、文本阶梯都不通。

- 渠道记录和凭据可以保存。未确认时修订的 base_url 只保存用户前缀 P，不补 /v1。修订上的 wire_api 因表约束不能为空，但启动和加入工作区不得读取它来拼上游地址。
- 加入隔离工作区的后端命令拒绝该渠道。界面不把它放进可加入列表；禁用说明写「协议未确认，不能加入隔离工作区」。
- 工作区模型勾选不列出该渠道的模型。渠道抽屉仍可展示这次探测到的名字，并标明协议未确认。
- 准备启动时，已选渠道的最新报告没有 wire_api，则这次准备失败，错误码 protocol_unconfirmed。不得用修订里的默认 Responses 生成 RouteSet。
- 刷新把曾经确认的渠道打成未确认时，不改已经在跑的 RouteSet。下次保存工作区时去掉该渠道及其模型；下次准备启动时按上一条失败。

确认成功且渠道没有被运行实例锁住修订时，同一次写入更新报告、模型目录，以及修订的 base_url、wire_api、transport_mode。只改前缀、协议没变，也要写修订，因为 P 和 P/v1 会打到不同主机路径。实例锁定期仍只更新报告和目录，不改修订；界面沿用现在的说明：正在运行的实例不变，停止后再刷新一次，下次启动才用新地址。

## 转发

Gateway 继续只拼一个地址：{修订 base_url}/responses 或 {修订 base_url}/chat/completions。不在 402、404 或 5xx 之后改去另一个前缀或另一种协议。

因此，探针第 1 条成功时，转发地址就是 P/responses。第 2 条成功时，转发地址是 P/v1/responses。Chat 两条同理。协议未确认的渠道到不了这一步。

## 实现位置

- normalize_api_base_url：删除「空路径补成 /v1」。保留 HTTPS、回环 HTTP，以及拒绝查询和片段。增加「路径以 /responses 或 /chat/completions 结尾则拒绝」。
- channel_probe：用同一张候选表生成模型 GET 和文本 POST。classify_text 改为按表推进，而不是「Responses 非 404 即停止」。
- 创建和刷新渠道：确认成功才把赢的前缀和协议写入修订。未确认只把报告 wire_api 置空。
- managed_launch/prepare.rs：选中渠道的报告没有协议时返回 protocol_unconfirmed。
- addProviderToWorkspace、toggleWorkspaceModelSelection，以及隔离工作区里的可加入渠道：没有确认协议时不写入。
- 诊断事件的 route 记下实际尝试的 /responses、/v1/responses、/chat/completions、/v1/chat/completions，不要再一律写成去掉前缀之前的常数。

不新增「已确认」列。现有 provider_connectivity_reports.wire_api 允许为空，空就是未确认。修订表的 wire_api 仍是非空约束，不能单独当成确认结果。

## 验收

- 前缀 https://probe.example：第 1 条 2xx JSON，则不再请求后三条；修订 base 为 https://probe.example，协议 Responses；转发 URL 为 https://probe.example/responses。
- 第 1 条 404 或 402，第 2 条 2xx JSON：修订 base 为 https://probe.example/v1，协议 Responses；不请求 Chat。
- 前两条都失败，第 3 条成功：协议 Chat Completions，修订 base 不含 /v1。
- 只有第 4 条成功：协议 Chat Completions，修订 base 以 /v1 结尾。
- 四条都失败：报告协议为空；保存渠道成功；加入隔离工作区和勾选模型被拒绝；准备启动返回 protocol_unconfirmed。
- 前缀已经是 https://probe.example/v1：只打两条，不出现 /v1/v1/responses。
- 401 不会把渠道标成另一种协议；阶梯会继续，直到某条成功或候选用尽。
- 转发测试：上游 402 或 404 时不会出现第二个 POST。
- 旧的「空路径补 /v1」单测改为「空路径保持原样」。

## 回滚

恢复 normalize_api_base_url 的空路径补 /v1，以及 classify_text 的「非路径缺失即停止」。工作区门禁和 protocol_unconfirmed 一并去掉后，行为回到 2026-10-02 的已实现合同。不需要数据库降级；未确认报告的空 wire_api 是现有列。

