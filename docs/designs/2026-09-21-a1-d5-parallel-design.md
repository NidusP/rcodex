# A1 目录泄漏与 D5 Summarizer 超时（2026-09-21 设计，待确认）

> 状态：**已实现**（2026-09-21）。commander 确认后落地 overlay 剥离与 Summarizer SSE/medium。实机 504 与看图探针不在本票。
> 配套：[问题与下一步](2026-09-19-grok-session-followups.md) A1 / D5；[错误待办](2026-09-19-grok-session-error-todos.md) E8-1 / E10；合同见 [混合路由详细设计 §8.2.1](../codex-hybrid-routing-design.md) 与 [第三方目录 overlay](../third-party-model-reasoning-levels.md) 第 6/7 节。
> 并行结论：两票可以同步实现。代码文件不相交；共享 Markdown 由主会话在合并时写一次。

## 并行边界

| | A1 | D5 |
| --- | --- | --- |
| 用户可见 | 生成的三方目录不再广告未验证的图像细节 / 图文搜索 | 默认 App Grok 远程压缩少被 30 秒首字节 504 误杀 |
| 层 | 启动物化 → 目录 JSON overlay | Gateway Summarizer 出站 |
| 写文件 | model_family_profile.rs（overlay + 单测）；可选 managed_codex_config.rs 目录夹具；overlay 文档 §6/7 | managed_gateway/compaction.rs；§8.2.1 第 2/5 条超时与 stream 句 |
| 不写 | Gateway、CapabilitySet 看图开关、窗口三键、生图 | overlay、Provider 超时表、family 500k 窗、换摘要模型 |
| 生效 | 保存工作区并应用配置 / 重启默认 App | 重启 Gateway sidecar（应用配置即可） |

不要和 B（看图探针）、C（生图产品化）、F（luna 旁路）混票。

---

## A1 删除 GPT 骨架的图像 / 搜索泄漏

### Goal

- 用户可见结果：纯文本三方条目（当前 Grok supports_images=false、supports_web_search=false）不再带 supports_image_detail_original=true 或 web_search_tool_type=text_and_image。
- 范围：overlay_third_party_catalog_entry 按 CapabilitySet 剥泄漏字段；回归测试；同步 overlay 文档第 6/7 节（原 A5 文档句并进本票）。
- 非目标：不把 supports_images 改 true；不改窗口（A2 已做）；不给 grok-4.6 广告生图；不做完整 Provider 能力编辑 UI；原生 GPT 行不进三方 overlay。

### 现状

model_entry_from_template 已把 supports_search_tool 写成 CapabilitySet.supports_web_search（三方默认 false），但克隆 GPT 骨架仍可能留下：

- supports_image_detail_original: true
- web_search_tool_type: "text_and_image"

与 input_modalities=[text]、supports_search_tool=false 矛盾。Gateway 预检读的是 CapabilitySet，目录广告却会骗过选择器。

### 合同

在 overlay_third_party_catalog_entry 末尾、窗口剥离之后：

1. capabilities.supports_images == false → 删除 supports_image_detail_original。
2. capabilities.supports_web_search == false → 删除 web_search_tool_type。
3. 能力为 true 时本票不改这两键（B3 打开看图后再说）。
4. RouteSource 非 ApiProvider、或内部审核模型提前 return：行为不变。
5. 删除字段，不改写成 text / false 之类发明值。

单一写入路径仍是目录生成器：先克隆 bundled 骨架，再 overlay。CapabilitySet 继续是能力真相；overlay 只负责不让骨架谎言留在 JSON。

### 验收

- 单测：Grok 骨架含上述两键 + 默认 CapabilitySet → overlay 后两键不存在；窗口 / 身份 / 推理档断言保持。
- 单测：supports_images=true 的夹具保留 supports_image_detail_original（证明按能力剥，不是无条件删）。
- 单测：原生 Official 行 overlay 为 no-op，两键保留。
- 未知族同样剥（只要是 ApiProvider 且能力为 false）。
- cargo fmt --all --manifest-path src-tauri/Cargo.toml -- --check
- cargo test --manifest-path src-tauri/Cargo.toml --lib 针对 overlay / catalog。
- 不跑付费矩阵；不改正在跑的默认 App。用户应用配置后抽查生成目录。

### 回滚

还原 overlay 那几行删除与测试。已生成的旧目录不会自动变；用户再应用一次配置即可。

### 拒绝

- 在 renderer 里改目录。
- 未探针就把 supports_images 打开（B）。
- 把 web_search_tool_type 改成与 false 搜索并存的新枚举。

---

## D5 Summarizer 首字节 504

### Goal

- 用户可见结果：默认 App Grok 远程压缩不再因为「等整段非流式 JSON」而在 30 秒首字节上误报 504。信封合同不变；失败仍 fail-closed，不换路、不重试。
- 范围：Summarizer 出站改走上游 SSE，收齐后再给 Codex 恰好一个 3r-compaction-v1 项；摘要请求把过高推理档降到 medium。
- 非目标：不改 3rxai 渠道在库里的 first_byte_seconds（界面已允许 1–600，且不得超过该渠道非流式总计）；不改 family 500k 窗；不换更便宜的摘要模型；普通聊天转发的 stream 合同不动。

### 现状

prepare_summarizer_request 强制 stream=false。collect_and_wrap 把 first_byte_seconds（3rxai 现为 30）打在 reqwest.send() 上。非流式中转常等整段摘要生成完才回头，30 秒实际在等开张，不是等一个 SSE 事件。2026-09-20 会话 01a0bcf6 连打 5 次约 30092ms 后第 6 次成功。收齐 SSE 的解析函数已经存在（summary_from_responses_bytes / extract_summary_from_responses_sse），只是出站没要 stream。

§8.2.1 第 5 条已要求「缓冲完整上游响应（SSE 先收齐再一次性写出）」——那是对 Codex 客户端的出口。本次把上游也改成 SSE，让首字节计时对准响应头 / 第一事件。

### 合同修订（确认后写入 §8.2.1）

保持第 1–4、6–8 条。补进 Summarizer 出站：

1. 发给当前路由上游的摘要副本：tools 为空、无 trigger、stream=true。禁止改打其它 Provider / 模型。
2. 首字节超时仍用该渠道 first_byte_seconds，打在 send()（等响应头）。之后用该渠道 non_stream_total_seconds 收齐 SSE 正文（现有 finish_summarizer_upstream 已经这样读 body）。空闲中断仍走现有 upstream_interrupted / upstream_response_timeout。
3. 上游 SSE 收齐后解析文本；给 Codex 的仍是一次性 compaction 项（客户端 stream 真则合成事件，假则 JSON）。不得把上游 token delta 转给 Codex 当 compact 流。
4. 摘要副本若 reasoning.effort 或同等字段为 high / xhigh / max / ultra，改为 medium。只改这一份出站副本，不改用户会话默认档。
5. 非 2xx、空摘要、截断：fail-closed，不回退成 stream=false，不重试，不换模型。

长摘要仍可能撞上该渠道非流式总计（3rxai 现 120 秒）。那是真超时，不是 30 秒误杀。若仍不够，由 commander 在 Provider 超时 UI 加长，不在本票写死新默认。

### 验收

- prepare_summarizer_request：stream=true；tools 仍空；外国信封仍拒绝；reasoning 密文仍折叠。
- 高推理入站 → 出站 medium；已是 low/medium 不升级。
- HTTP mock：上游 SSE 在首字节内返回 header + 后续 response.completed → Summarizer code=ok，客户端仍一个 compaction 项。
- 现有 mock：remote_v2_summarizer_folds_reasoning_ciphertext_before_upstream、remote_v2_foreign_envelope_never_reaches_upstream、summarizer_sse_is_synthetic_compaction_events_not_token_deltas 继续通过。
- cargo fmt + cargo test --lib 针对 compaction。
- 不跑付费矩阵；不在本设计阶段改正在跑的默认 App。
- 实现后用户可见验收：默认 App Grok 再触发 compact 时，不应再因约 30 秒首字节 504 让 Codex 重连。账本不应连续出现 upstream_first_byte_timeout。若仍有超时，应是非流式总计（现 120 秒）或上游真失败，并在诊断里能分开。

### 回滚

还原 prepare_summarizer_request 的 stream=false 与推理档处理。普通转发不受影响。

### 拒绝（E10 候选里不做）

- E10-1 自动改 3rxai 首字节到 60–120：这是渠道配置，不是 Summarizer 合同。用户可在界面改；本票不 UPDATE gateway_timeout_settings。
- E10-4 把 family 窗改小逼提早 compact：A2 已把 Grok 调到 500k（更晚 compact）。若要提早，用高级映射，另说。
- 504 后换路、换模型、计费重试。
- 普通聊天请求也强制 SSE 或降推理。

---

## Execution

确认后建议同一实现轮次、两个互不抢文件的切片（可并行，最后主会话写文档）：

- [ ] A1 overlay 剥两键 + 单测 + overlay §6/7 与 followups A1/A4/A5、E8-1/E8-3。
- [ ] D5 Summarizer stream=true + 推理降到 medium + §8.2.1 句 + E10 勾选与 followups D5。
- [ ] 主会话：执行计划文首交接、AGENTS 未完成项；cargo test --lib 一次。

---

## 对照原始目标（2026-09-21 复核）

本节核对 commander 回忆的原始现象，和落档票、现设计是不是同一件事。不改实现范围，除非确认后扩票。

### A1

原始现象：用 grok-imagine / grok-image 生图后，Codex 提示无法读取图片，代理不能 check 效果。当场错误是 view_image：you do not support image inputs。根因是 3rcodex 把 grok-4.6 写成 input_modalities=[text]、supports_images=false（CapabilitySet 启动写死），不是中转先拒图。

落档后的 A1 被收窄成：删除 GPT 骨架泄漏的 supports_image_detail_original 与 web_search_tool_type。看图总开关单列成 B（先 3rxai Responses 小图探针）和 A3（探针通过后才写入 CapabilitySet）。

现设计覆盖的是收窄后的 A1，不覆盖原始看图目标。做完本票后：

- 目录不再谎称图像细节 / 图文搜索（诚实展示）。
- Codex 仍然不会给 Grok 会话 view_image。生图技能可以继续出文件，代理照样看不见像素。

要回到原始目标，必须另做 B1 探针；成功才走 B3/A3 打开 supports_images。产品原理第 8 条禁止未探针改 true。A1 与 B 可以设计并行，但不能在同一实现里既剥泄漏又打开看图——打开看图要以探针证据改 CapabilitySet，那是 B 的写入路径。

### D5

原始现象：接入 Grok 触发压缩时 Codex 会重连（你记得两次）。分析是在等摘要结果。2026-09-20 会话 01a0bcf6 的账本是 5 次 upstream_first_byte_timeout（约 30 秒）后第 6 次成功；Gateway 按合同不重试，连打的是 Codex 客户端。打印的 url 是本机 127.0.0.1:43123，上游是 3rxai 非流式摘要开张过慢。

现设计覆盖这条因果：摘要改走 SSE，让 30 秒首字节对准响应头；摘要推理降到 medium，缩短开张。用户可见的「重连」应变少或消失。

仍不完备、需写明的残余：

- mock 之外已补用户可见验收：compact 时不应再因约 30 秒首字节 504 让 Codex 重连；若仍超时须能与 120 秒总计区分。实机留到实现后做，不是设计通过条件。
- 若 3rcd 对 SSE 仍缓冲到整段结束才发第一事件，首字节误杀会回来。
- 非流式总计仍是 120 秒；真的摘要太慢仍会超时，客户端仍可能再连。那不是 30 秒误杀。
- 次数「两次」与账本「五次」是同一机制，不另开票。

## Active Status

- 当前状态：代码已落地；cargo test --lib 302 passed。须应用配置 / 重启 Gateway。
- 阻塞：无。B 仍独立。
- 上次验证：只读 overlay、compaction.rs、§8.2.1、2026-09-20 实机 504 记录。
- 下一步：确认并行与两票合同后实现；不要和 B/C/F 混票。
