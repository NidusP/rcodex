# 第三方 Responses：Codex `agent_message` 兼容（2026-09-23）

> 状态：**方案 A 已实现，隔离 App 实机通过（2026-09-23）。** Chat Completions 仍未处理。
> 证据：隔离实例 `45b5727c-d303-4d16-8588-d5f5bf27bb68` 子任务 502；9 月 21 日请求档案同形状；OpenCodex issue 3911；Cockpit cliproxy Responses 透传路径。
> 配套：[混合路由详细设计](../codex-hybrid-routing-design.md) Gateway 转发、[Grok 会话错误待办](2026-09-19-grok-session-error-todos.md)。

## 1. 问题

隔离 App 直发 Grok 正常。同一 RouteSet 上 `spawn_agent` 的子任务第一包、以及父会话吃到子任务回包之后的每一轮，Gateway 都能精确匹配 `a55ea5dd-0fb1-4599-a759-ce9b1c6e-grok-4.6` → `grok-4.6` 并转发到 3rcd，但上游约 1 秒返回 502 `Upstream request failed`。Codex 客户端把同一 POST 连打约 30 次，整条线程死亡。

根因是请求 `input` 里的 Codex 私有项 `type=agent_message`（内容部件常为 `type=encrypted_content`，槽里经常是明文任务）。标准 Responses / xAI Grok 的反序列化名单没有这项。OpenCodex 对 grok-4.6 的复现是 422 `unknown item type "agent_message"`；3rcd 把它 ste 成泛 502。

这不是 RouteSet 没命中，也不是 `codex-subagent` 模型，也不是少了一个 function 工具。父会话在 `spawn_agent` 的 `function_call` 阶段仍成功；一旦 `input` 里出现 `agent_message` 就失败。

## 2. 返回路径不必「再组合成 agent_message」

Codex 多代理的拼装发生在客户端：

1. 子会话向 Gateway 发 Responses 请求（带 `agent_message`）。
2. 模型应返回**普通** Responses 流（assistant 文本、工具调用）。
3. 子会话把最终回答交给父会话；父会话下一轮再把子结果作为 `agent_message` 放进自己的 `input`。

Gateway 的职责是：让第 2 步的上游能读懂第 1 步。不要把上游输出再包成 `agent_message`。只有在出站时改过 collaboration 工具名时，才需要在 SSE/JSON 入口把工具名还原成 Codex 认识的 `namespace=collaboration` / `spawn_agent`。

本切片只做 **Responses 透传**。Chat Completions 转换维持现状（文本能力、去工具），不在本票展开。

## 3. Cockpit 的标准 Responses 上游

Cockpit 不只在 Chat 转换里处理子任务。`sidecars/cockpit-cliproxy/provider_gateway.go` 在选择 Chat 还是 Responses 之前就会：

1. `OptimizeCodexMultiAgentV2Request`：把 collaboration 命名空间改名为 `collaboration-optimize`，去掉 `spawn_agent` / `send_message` / `followup_task` 参数上的 `message.encrypted`，必要时改 `spawn_agent` 描述。
2. `RewriteCodexMultiAgentV2Input`：`agent_message` 改 `type=message` 且 `role=user`；`encrypted_content` 部件改 `input_text`（槽内字符串原样当正文）。
3. 若 `wireAPI == responses`：仍 POST 上游 `/v1/responses`（标准 Responses 透传，不是 Chat）。
4. 回来时 `RestoreCodexMultiAgentV2Response` / SSE 行还原：把 `collaboration-optimize` 和 `collaboration__spawn_agent` 这类扁名还原成 Codex 的 namespace 工具。

Chat 分支是另一条路：Responses→Chat→再合成 Responses 事件。PR 2382 修的是「Chat 转换漏掉了上面第 2 步，任务正文丢失」。本票搁置 Chat，只借鉴 Responses 透传。Cockpit 许可证是 CC BY-NC-SA，3R **不复制其代码**，只对照行为。

Cockpit 就地改 `type`/`role`，可能留下 `author` / `recipient` / `id`。严格按字段反序列化的上游仍可能拒绝未知键。

## 4. OpenCodex 的 Responses 路由

非 `forward` 的 Responses 目标（含 xAI grok-4.6）：

1. `normalizeRoutedAgentMessages`：把 `agent_message` **换成新的** `{type:message, role:user, content:[...]}`，作者/收件人写成一行前缀；丢掉私有键。内容部件若不是 `input_text` / `input_image` / `input_file` 则**不改写**，交给 `unreadable_encrypted_agent_task` 失败关闭。
2. `request-strips`：删除 `internal_chat_message_metadata_passthrough`、不合法 item id、ChatGPT 私有工具字段（如 `defer_loading`）、若干顶层私有键。
3. `preparePlaintextV2AgentMessages`：主要给**官方 ChatGPT 线**用，把 `collaboration` 改成 `collaboration-optimize` 以免撞保留名；回来再还原。三方 Grok 不是这条的主路径。

OpenCodex issue 3911 证明：只把 `agent_message` 换成等价 `message`，同一 grok-4.6 请求从 422 变 200。我们档案里的任务正文在 `encrypted_content` 部件里，OpenCodex 这条函数会跳过；Cockpit 会展开。3R 需要把两者拼起来。

## 5. 方案

共同约束：

- 只作用于 `RouteSource::ApiProvider` 的 Responses 透传。官方路由、compaction 请求、Chat 适配器不走本切片。
- 不改 RouteSet，不引入 `codex-subagent`，不因 502 换渠道或重试 POST（Codex 客户端自己的重试不在 Gateway 合同里）。
- 出站改写必须可测：fixture 用合成 `agent_message`，禁止真实任务正文和 Key。
- 子任务首包和父会话回放 `agent_message` 的下一包都要覆盖。

### 方案 A（推荐）：干净消息 + 明文部件展开

出站（Cockpit 的 input 改写 + OpenCodex 的干净对象）：

1. 遇到 `agent_message`：建成新的 `message` / `role=user`，不要就地改 type 而留下 `author`、`recipient`、`internal_chat_message_metadata_passthrough`。
2. `author` / `recipient` 可写成一条 `input_text` 前缀，便于模型知道发言者。
3. 内容部件 `encrypted_content`：若值为普通字符串且**不是** `3r-compaction-v1:` / `ocx1:` / 明显 Fernet 密文，则改为 `input_text.text`。密文或空内容 fail-closed，Gateway 返回明确 `unreadable_agent_message`，不要再把密文交给 3rcd。
4. 其余 `input_text` 保留。删除每条 input 项上的 `internal_chat_message_metadata_passthrough`。

入站：原样透传 Responses SSE/JSON。不改 collaboration 工具名，因此不必还原。当前 Grok 父会话已经带着 `type=namespace name=collaboration` 成功，说明 3rcd 吃得下 namespace 工具。

验证：子任务第一包 200 且能跑工具；父会话在子任务 `FINAL_ANSWER` 回写后下一包仍 200。

风险：若某上游连 `author` 前缀都不想看见，可再收成纯正文。密文任务（官方 v2 加密派遣）本方案拒绝，与 OpenCodex 失败关闭一致。

### 方案 B：A + collaboration 改名与还原

在 A 之上做 Cockpit Optimize / OpenCodex plaintext-v2：

- 出站把 `collaboration` 改成请求作用域别名（例如 `collaboration-optimize`），并去掉工具 schema 里 `message.encrypted`。
- 入站把工具调用的 namespace/扁名还原，否则 Codex 不认 `spawn_agent`。

适用：上游把 `collaboration` 当保留名，或模型把工具打成 `collaboration__spawn_agent`。代价：SSE 还原、冲突检测、溢出限制。当前 3rcd 父会话已证明 namespace 可用，**本轮不必上 B**，除非 A 落地后子任务能聊但二次 `spawn_agent` 失败。

### 方案 C：A + 更完整的 ChatGPT 私有字段剥离

在 A 之上吸收 OpenCodex `request-strips` 里已观察的键：`defer_loading`、不合法 item id、Codex 0.155 顶层私有字段。这是卫生，不是子任务开关。可在 A 的同一转发函数里做小表，但不要做成未知字段黑名单。

建议：A 的测试里顺手删 `internal_chat_message_metadata_passthrough`（已在 A 第 4 步）；`defer_loading` 等放到 A 通过后再加，避免和子任务验收缠在一起。

### 方案 D：不改写，只把 502 说清楚

检测到 `agent_message` 时本地拒绝，文案说明三方 Responses 无此类型。不能让子任务工作，只避免连打 30 次泛 502。不作本票目标。

## 6. 建议

先做 **方案 A**。它覆盖实机失败形状，对齐 OpenCodex 3911 与 Cockpit Responses 透传的 input 改写，又不引入 collaboration 改名。Chat 转换、官方透传、压缩信封、密文 v2 派遣不在本切片。

落地位置：`managed_gateway` 在 `filter_relay_incompatible_fields` 之后、出站之前；只对 API Provider Responses。单测覆盖：纯 `agent_message`、带明文 `encrypted_content` 部件、带 compaction 前缀的密文 fail-closed、普通 `message` 不动、官方路由不动。

## 7. 验收

- 隔离 App 直发 Grok 仍成功（回归）。
- `fork_turns=none` 子任务第一包不再 502，能执行本地工具并给出最终回答。
- 父会话在收到子任务结果后继续回合，不再因回放 `agent_message` 死亡。
- 诊断：改写后的请求不应再出现 `agent_message`；失败时 `gateway_code` 可区分本地不支持与上游 502。
- 不启用完整档案也能用形状/item type 计数证明；若临时打开档案，禁止把任务正文写入普通日志。

## 8. 实际实现与验证（2026-09-23）

- 修改：`src-tauri/src/managed_gateway.rs` 新增 `normalize_routed_agent_messages`。只在 API Provider 的普通 Responses 转发前执行；检测到 `compaction_trigger` 时跳过，继续由既有压缩路径处理。
- 写入路径：每条 `agent_message` 重建为干净的 `message/user`，可读 author/recipient 作为文本前缀；普通 `input_text` 保留；明文 `encrypted_content` 展开为 `input_text`。所有普通 input 项上的 `internal_chat_message_metadata_passthrough` 都会移除。
- 失败行为：空、未知部件、已知 compaction 信封和 `gAAAA` Fernet 形状返回 HTTP 400 / `gateway_code=unreadable_agent_message`；错误以「子任务转发失败」开头，不请求上游。压缩请求仍返回既有 `unsupported_compaction`。
- 父任务恢复：Codex 将失败子任务写回父任务的 `Agent errored:` 消息仍会正常转成标准 `message`，并追加「子任务结果不可用；父任务可自行继续」的模型可读提示。Gateway 不伪造子任务成功、不自动重派、换模型或换渠道。
- 实际验证：`cargo fmt --all -- --check` 通过；`cargo test managed_gateway::tests::routed_agent_message -- --nocapture` 通过（4 项）；`cargo test managed_gateway::tests::compaction_request_keeps_its_existing_fail_closed_path -- --nocapture` 通过。集成用例确认第三方 mock 收到的是 `message/user`、不含 agent 私有字段、模型仍改写为绑定 upstream。
- 实机：commander 重启隔离 App 后，本会话并行派出 probe_docs 与 probe_code（fork_turns=none，只读）。两个子任务都返回最终回答，父会话接住回写。没有子任务首包 502 连打，也没有父会话因 agent_message 一起失败。
- 回滚：移除 API Provider 分支中的 `normalize_routed_agent_messages` 调用及对应辅助函数/测试，恢复原有普通 Responses 透传。
