# GPT 全透传、cli_only 探针与目录淡出透出

> 状态：historical（最后核对 2026-11-19）
>
> 已由待审结论取代：[2026-11-19-gpt-official-clients-conclusion.md](./2026-11-19-gpt-official-clients-conclusion.md)

> Commander 确认的三项产品诉求；替代原「整渠道 pass_through」叙事。渠道级 `pass_through` 可保留为高级选项，默认由 **GPT 路由自动身份透传** 覆盖。

## 1. GPT 系列：Gateway 完全透传（身份 + 体）

**目标**：避免聚合上游在 **GPT 线路** 上返回 `official clients only` / `cli_only`。

| 层级 | GPT 族路由 | 非 GPT 路由 |
| --- | --- | --- |
| HTTP 身份 | 原样转发 `User-Agent`、`Originator`、`x-codex-*`、`Authorization` | compat：渠道 API Key，不伪造 Codex 身份 |
| 请求体 | 保持现有 GPT Responses 少改写（`needs_relay_compat = false`） | 现有 relay 适配 |
| 准入 | 仍 127.0.0.1 + 实例密钥 | 同左 |

**判定**：`looks_like_gpt_model_id(public, upstream)`（与启动 RouteSet 一致）。

**非目标**：不默认 App 官方订阅路由；不把非 GPT 改为身份透传。

## 2. 渠道探测：GPT + cli_only 失败仍可选用

当 `/models` 成功、文本探测对 GPT 模型因 **须 Codex 客户端** 失败时：

- 目录条目 **照常入库**（`models_check = ready`）。
- 连通性可记 `text_check = failed`，`error_code = gpt_codex_client_required`（参考探针，**不阻止** 保存渠道/透出）。
- **GPT 透出**：不跑图片探针；启动 RouteSet 对 GPT 族 **默认 `supports_images = true`**（证据表可保持 unknown）。
- **产品提示**：建议用户 **启动隔离 App** 自行验证 GPT 是否可用（UI 文案，非阻断）。

非 GPT 仍按原图片探针与证据合同。

## 3. 目录再发现：上游已下架的透出模型自动取消

刷新渠道目录并成功写入新 `catalog_revision` 后：

- 对已透出、但 **不在最新目录** 的 `upstream_model_id`：**自动 `exposed = 0`**。
- **同时** 从所有未删除空间的模型选择中移除该模型（系统淡出，**不要求** 用户先手动取消透出）。
- 用户手动取消透出时，**仍** 保留「空间仍引用则拒绝」以防误操作。

## 验收

1. 隔离 App 选 GPT 模型请求：上游收到客户端 OAuth/头，无渠道 Key 顶替。
2. 渠道探针：仅 GPT cli_only 文本失败时，目录可见、GPT 可勾选透出，无图片探针排队。
3. 上游掉模型后刷新目录：透出关闭 + 空间引用清除，无 `model_exposure_in_use`。

## 与未提交代码的关系

工作区已有 `pass_through` 传输模式与 `codex_passthrough` 模块；本合同以 **按路由 GPT 自动透传** 为主，`pass_through` 仅作整渠道强制开关（可选保留）。