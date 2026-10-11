# 隔离实例的上下文压缩不由 R Codex 修正

日期：2026-10-08。本文是落档说明，可供以后官网引用。当前不发布到 `website/`。

## 可以放到官网的说明

隔离实例里的长对话，由 Codex 自己做上下文压缩。R Codex 的本机 Gateway 把这次请求转发到当前模型渠道。它不参与这次压缩，也不改哪一句用户要求算当前任务。

上下文变长时，Codex 会在本地做一次检查点：自己写一份交接摘要，再把更早的用户原话和这份摘要一起放进下一次请求。R Codex 收到的是合法的普通请求，里面没有远程压缩标记。Gateway 不会从这些文字里判断哪一句才是最新任务，也不会把偏到旧问题上的回答改回来。

转发时会做的，只是让请求能被当前渠道接受。所有路由都换成上游模型名。非 GPT 还会去掉中继不接受的字段、拿掉渠道不支持的工具，以及把 Codex 私有的协作消息改成公开格式。GPT 族按 Codex 发出的 Responses 原样转发请求体。这些都不改变用户说了什么。

所以，压缩后续跑如果拿起更早的一条要求，而放下刚刚正在做的方案，这是 Codex 本地压缩的组包方式。不是 R Codex 换了渠道、改了模型，或漏掉了最新问题。R Codex 不对这种任务偏移做兜底修正。


## 转发时能不能修正组包问题

可以改的是协议形状，不是任务选择。`managed_gateway.rs` 在普通转发前已经做这些机械修正：

- 把公共模型名换成该路由的 `upstream_model_id`。
- 非 GPT 才删掉 Responses 的 `metadata`。部分兼容中继会因此返回 400，而 Codex 不靠这个字段维持会话。GPT 族保留该字段。
- 非 GPT 且渠道不支持工具或网页搜索时，去掉对应的 `tools`。GPT 族保留 Codex 附带的工具。
- 非 GPT 的普通第三方回合把 Codex 私有的 `agent_message` 收成公开的 user 消息，并丢掉 `internal_chat_message_metadata_passthrough`。读不懂就拒绝，不猜测。GPT 族保留 `agent_message`。
- 非 NativeRemote 的普通回合把 R Codex 自己的 `3r-compaction-v1` 压缩项展开成助手文本。认不出的压缩信封直接拒绝。

隔离实例的 `local_client` 不走 GatewaySummarizer。请求里若出现 `compaction_trigger`，预检返回 `unsupported_compaction`，不把触发项改成另一条压缩路径。摘要路径才会折叠 reasoning 密文；普通转发不折叠。

这次偏移不属于上面任何一种。续跑请求是合法的 Responses JSON：每条用户消息都有角色和正文，没有损坏字段，也没有 `compaction_trigger`。问题是 Codex 把已经处理过的禁用批注、主题方案和「先暂停」的交接摘要放在同一次请求里。Gateway 没有字段可以标明哪一条过时。

把「最后一条用户消息」当成当前任务也不成立。检查点之后，交接摘要排在主题方案后面；它写的是方案已完成、先不要实现。按最后一条执行，仍然不会回到用户刚提出的主题方案。要修正就得阅读正文并删改用户原话，这是猜测，不是协议修复。

结论：转发时技术上读得到 `input`，也已经会改协议不兼容的形状。对「组包把旧任务和当前任务叠在一起」这种问题，不能可靠修正，因此不做。读不懂的私有项继续拒绝，不改写成一个猜测出的任务。

## 本次证据

会话 `01a10ad4-55df-7f61-985e-406c856a447c`，隔离实例，`compactMode` 合同为 `local_client`。

- `aea9ac45-c20c-449e-ba88-a937bec27c69`（2026-10-08 13:54:27）仍在写 Mantine 主题方案。
- `7d01e13d-c798-4e15-8f2f-5db6ded15023`（13:55:33）不是新的用户问题。请求里没有 `compaction_trigger`，也没有 `3r-compaction-v1` 信封。最后插入的是 Codex 自己的检查点指令：`You are performing a CONTEXT CHECKPOINT COMPACTION`。压缩摘要写的仍是主题方案。
- `2660b120-a668-4e41-8796-d479d83997be`（13:56:18）是压缩后的普通 `/v1/responses`。客户端重放了更早的用户原话，其中一条标题是「My request」，内容是已经处理过的禁用批注。主题方案还在，但交接说明把它标成先暂停。Gateway 没有改消息顺序。
- 下一条 `5cf5b86f-9058-4169-a258-3dda717b4c05` 的回答回到了禁用提示，没有继续主题方案。

## 和不做什么

隔离启动写入 `local_client`：Codex 本地摘要，正常路径不发 `compaction_trigger`。Gateway 按普通请求转发。若这类启动仍带上 `compaction_trigger`，只返回 `unsupported_compaction`，不改用 GatewaySummarizer。

GatewaySummarizer 只属于默认 App 的 `remote_v2`，不是本地压缩失败后的后备，也不负责比较新旧任务。不在 Gateway 里重排用户消息，不根据「My request」、最后一条或交接摘要猜测当前任务。协议形状修正保持上面列出的范围，不扩大到任务取舍。

官网页面尚未添加。以后若展示，使用上面「可以放到官网的说明」，不要把请求编号和内部合同名直接放进用户页面。
