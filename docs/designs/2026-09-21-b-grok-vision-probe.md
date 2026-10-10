# B Grok 看图探针与 CapabilitySet（2026-09-21）

> 状态：**B1 失败 / B2 已执行。** 2026-09-21 3rxai grok-4.6 带图 400/502，文本控制 200。supports_images 保持 false。A3 写入路径已接。D5 实机暂不验收。
> 配套：[followups B / A3](2026-09-19-grok-session-followups.md)、产品原理第 8 条、Gateway `unsupported_images`。

## 探针会不会在「添加任何 Provider」时自动跑？

**合同（可复用）：** 第三方 Responses 路由不得把 `supports_images=true` / `input_modalities` 含 `image` 写进 CapabilitySet 或目录，除非该族已经有一次真实 Responses 小图探针通过。现有接入探针（`connectivity.rs`）只打 `/v1/models` 和文本，**不含图**。

**本票（B）：** 只对当前 3rxai + `grok-4.6` 做一次离线探针。通过则 Grok 4.x family 默认打开看图，并由 A3 在启动物化时写入 CapabilitySet。Chat Completions 路由仍纯文本。搜索仍 false。

**本票不做：** 在「添加 Provider / 发现模型」向导里对每个新模型自动打图。那是把图像检查接进 `connectivity.rs` 的后续产品票，可复用本票的 fixture 与判定，不在 B 里做 UI。

## Goal

- 用户可见：3rxai Grok 4.x 默认 App 任务能 `view_image` 读本地 PNG（探针成功时）。失败则保持不能看图，文案说当前渠道未验证。
- 范围：B1 探针记录；成功则 B3+A3；失败则 B2。
- 非目标：C 生图、F luna、给 Gateway 加 `/v1/images/generations`、Chat 适配器看图、未探针改 true。

## 探针

- 上游：3rxai `https://ai.3rcd.com/v1/responses`（解开回环，不经 Gateway 预检）。
- 模型：`grok-4.6`。
- 图：仓库内固定小 PNG fixture（红方块），`input_image` + 一句要求只回答颜色和形状。
- 通过：HTTP 2xx，回复能识别红色或方块（英文 red/square 或中文红/方均可）。
- 失败：4xx/5xx、空回复、明确拒图、超时。保持 `supports_images=false`。
- 记录：状态码、是否说中图、usage 若有、渠道名。不记 Key、不把图和完整正文写入普通日志。

## 成功后的写入

- `ModelFamilyProfile` 增加 `supports_images`（Grok 4.x true，其余 false）。
- `managed_launch` A3：Responses 且 family.supports_images → CapabilitySet `input_modalities=[text,image]`、`supports_images=true`；否则纯文本。搜索不打开。
- overlay：`supports_images=true` 时保留/可写图像细节；false 时继续剥 A1 那两键。
- 目录 JSON 跟 CapabilitySet 的 `input_modalities`。
- Chat Completions 路由强制纯文本。

## 验收

- 探针记录写进 followups / 执行计划（平台、Codex 无关、渠道 3rxai、成功/失败原文摘要）。
- 单测：Grok Responses 物化后 supports_images=true；DeepSeek/未知族/Chat 仍 false；A1 剥泄漏在 false 时仍成立。
- `cargo fmt --check`、`cargo test --lib`。
- 默认 App 图形 `view_image` 需用户应用配置后手动看；本票不代替那次点击。
