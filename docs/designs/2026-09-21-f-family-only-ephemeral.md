# 默认 App 旁路短模型：当前模型族 only（2026-09-21）

> 状态：**已实现（2026-09-21 F3）。** commander：没选 GPT 就不能容忍 luna 出现；只接入 DeepSeek / Grok 等三方时必须「当前模型族-only」。C 仍停放。
> 配套：[followups F](2026-09-19-grok-session-followups.md)、[错误待办 E11](2026-09-19-grok-session-error-todos.md)、[混合路由详细设计](../codex-hybrid-routing-design.md) 2026-09-05 兼容回退段。

## Goal

- 用户可见：工作区没选 GPT 族时，默认 App 改会话名、写侧栏说明、安全旁路等不得再打 `gpt-5.6-luna`，也不得把这些请求送去官方 OpenAI。标题/说明走当前工作区已选模型族（工作区默认那条 Responses 路由）。
- 范围：默认 App 兼容目录 merge、Gateway `compatibilityFallback` 白名单、App 写死的 ephemeral 短模型别名。只动默认 App 兼容投影；受管理 App 仍只发布选中路由。
- 非目标：不改用户点名的聊天模型；不因 401/502 换路、换 Provider、重试 POST；不处理 E2（grok-imagine 误走 image2）；不做 C 生图/技能管理；不改 Summarizer / 窗口 / 看图。

## 实现前现状（2026-09-21 只读；F3 已改）

ChatGPT.app 把 `thread_title` / `thread_description` / `thread_title_reconsideration` / `thread_summary` / `ambient_suggestion_safety` 写死为 `gpt-5.6-luna` + `effort=low`，`modelProvider=null`，`allowProviderModelFallback=true`，`ephemeral=true`。请求仍进本机 Gateway（`127.0.0.1:43123/v1/responses`），不是绕过 3R。

两处叠加让「3R 勾选里拿掉 GPT」不够：

1. **目录 merge**（`merge_compatibility_catalog`）：先保留用户接入前的原生 catalog（通常是 `codex debug models --bundled`），再让 3R 生成路由按 slug 覆盖。注释写明：只写路由目录会让原生选择器变空，所以**未覆盖的原生条目全部留下**。`gpt-5.6-luna` 因此继续出现在 `compat_default_*.models.json`。
2. **回退白名单**（`compatibility_fallback_from_config`）：用的是 **original_catalog**，不是 merge 后的目录。所有「原生 slug 且不在本次选中 RouteSet」都进 `compatibilityFallback.model_ids`。Gateway 找不到精确路由时，只要 slug 在这份名单里，就走 `compatibility_native`：转发到接入前的官方 Base URL，并转发请求里的官方认证。`GET /v1/models` 也会把这些 id 以 `owned_by=openai` 附上。

因此：工作区只勾 Grok 时，luna 仍在目录里，仍在回退名单里，App 一改名就打官方 luna。删 GPT 勾选不会改成 Grok。这是 2026-09-05 合同的字面结果（「未被选中的默认模型保留在兼容回退白名单」），不是 Gateway 把当前 Grok 聊天改写成 luna。

自动审核已经是另一条路：生成目录把 `auto_review_model_override` 写成条目自身公共 ID，Codex 请求的是当前对话模型，不是 luna。标题/说明没有同等配置键，App 写死 slug，必须在 RouteSet / 回退名单上处理。

## 产品锁

1. **工作区勾选是族白名单。** 没选 GPT 族 = 不得发布、不得回退、不得官方转发任何 GPT 族 slug（`looks_like_gpt_model_id`）。默认 App 里残留的 ChatGPT 登录不是「选了 GPT」。
2. **不能只 fail-closed。** 用户不能容忍「没 GPT 就改不了会话名」；要当前模型族-only。标题失败可以当没有 Responses 站桩时的最后手段，不是默认。
3. **GPT 族已选则保持现回退。** 工作区勾了官方或三方 GPT（例如 3rxai 的 `gpt-5.6-sol`）时，luna 仍可走原生兼容回退。那是用户接入了 GPT 族。
4. **别名只服务 App 写死的 ephemeral slug。** 用户点名的聊天模型仍精确匹配，禁止猜渠道。
5. **E2 分开。** 生图误走 image2 宿主不是本票。

「当前模型族」在 ephemeral 请求上没有父会话模型（`modelProvider=null`）。站桩取**工作区默认模型**（选择列表第一项）；若它不是 Responses，则取第一条已选 Responses 路由。Grok+DeepSeek 且默认是 Grok 时，标题走 Grok，不按「你正在改名的那条会话」猜。日后若 App 带上父模型再收紧。

## 机制

启动物化（`managed_launch`，默认 App 兼容投影）是唯一写入路径。Gateway 请求时不现编别名。

### 1. 是否选了 GPT 族

`gpt_family_selected` = 任一已选路由 `looks_like_gpt_model_id(public, upstream)`。已知 Grok / DeepSeek / Claude upstream 已被该函数排除。不要用「用户是否登录 ChatGPT」当开关。

### 2. 目录 merge

`gpt_family_selected == false` 时，`merge_compatibility_catalog` 不得再保留原生 GPT 族条目。3R 生成的 Grok/DeepSeek 行已经能填满选择器，不再需要靠 bundled GPT 防止空列表。非 GPT 的内部角色（`codex-auto-review`、`codex-subagent`）仍可按现特殊模型合同留下；它们不是 luna。

`gpt_family_selected == true` 时 merge 行为不变。

### 3. 回退白名单

`compatibility_fallback_from_config` 在未选 GPT 族时不得把 GPT 族 slug 写入 `model_ids`。Gateway 因此不能对 luna 走 `compatibility_native`。`GET /v1/models` 也不再附上这些 openai 行。

选了 GPT 族时白名单仍可含未勾选的原生 GPT slug（含 luna），与 2026-09-05 目标一致：官方模型认证不丢。

### 4. ephemeral 别名（不进 RouteSet 行）

`public_model_id` 全局唯一，不能给 luna 另插一条身份行。实现因此：

- 目录：从站桩条目克隆隐藏行（`visibility=hide` / `hidden=true` / `supported_in_api=false`），slug 仍是 App 写死的 `gpt-5.6-luna`。
- Gateway 快照：`ephemeralFamilyAliases` 映射 `gpt-5.6-luna → 站桩 public_model_id`。
- 请求时克隆站桩路由、标 `ephemeral_family_alias`，出站换成站桩 upstream，注入该 Provider Key。
- 不进普通选择器，不进公开 `/models`。账本记站桩渠道，不是官方 luna。

已知 slug：`gpt-5.6-luna`（title / description / reconsideration / summary / ambient_suggestion_safety 共用）。Codex 改写死 slug 后再追加 allowlist。

没有 Responses 站桩时 fail-closed：`model_not_in_route_set`，仍不得官方转发 luna。

### 5. 请求路径

未选 GPT、App POST `model=gpt-5.6-luna`：

1. RouteSet 没有 luna 行；快照别名命中站桩，不是 `compatibility_native`。
2. 出站 `model` 换成站桩 `upstream_model_id`，去掉官方认证。
3. 上游看见 Grok/DeepSeek，看不见 luna。

选了 GPT：luna 仍可走 compatibility_native，不写三方别名。

## 验收

- 单测：只选 Grok 时 merge 后无原生 luna/GPT 行；fallback.model_ids 不含 GPT 族；快照含 luna→站桩别名；目录有隐藏 luna 行。
- 单测：只选 DeepSeek 同上，站桩为 DeepSeek。
- 单测：选了 GPT（官方或三方 GPT slug）时不建三方别名；luna 可留在 fallback。
- 单测：Gateway mock：未选 GPT 时 luna 请求出站模型是站桩 upstream，适配器不是 `compatibility_native`；选了 GPT 时仍可 native。
- 单测：无 Responses 站桩 → luna 为 `model_not_in_route_set`。
- 现有 `compatibility_fallback_uses_original_provider_url_and_native_models_only` 改为：夹具含 GPT 族已选，或断言未选 GPT 时 native GPT 被滤掉。
- `cargo fmt --check`、`cargo test --lib`。
- 实机：默认 App 只挂 Grok 的工作区，改会话名后 3rcd/账本不得出现 luna；应出现站桩 Grok。须应用配置 / 重启默认 App。本设计阶段不改正在跑的 App。

## 回滚

还原 merge 过滤、fallback 过滤、别名路由与测试。已生成的 `compat_default_*.models.json` 不会自动变，用户再应用一次配置即可。

## 拒绝

- 请求时按 401/502 把 luna 改成「能过的模型」。
- 把别名写成用户可见的 luna 聊天模型。
- 未选 GPT 仍 merge 全量 bundled「以免选择器空」——三方行已经够。
- 用官方登录残留当 GPT 族已选。
- 和 D5 超时、B 看图、C 生图混票。
- 给 Gateway 加 `/v1/images/generations` 或改 grok-imagine 技能。

## Active Status

- 当前状态：F3 已实现并落档。`cargo test --lib` 311 passed。
- 阻塞：无。须应用配置 / 重启默认 App 后改名才走站桩。
- 用户手验（未做）：默认 App 只挂 Grok 的工作区改会话名，3rcd/账本不应再出现 luna。
- 不要做 C。
