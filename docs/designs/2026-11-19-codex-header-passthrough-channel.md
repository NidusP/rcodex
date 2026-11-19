# Codex 透传通道（cli_only 中转）

> 状态：historical（最后核对 2026-11-19）
>
> 整渠道 `pass_through` 草案；结论见 [2026-11-19-gpt-official-clients-conclusion.md](./2026-11-19-gpt-official-clients-conclusion.md)

## 背景与目标

Sub2API 等 OAuth 池开启 `codex_cli_only` 时，只接受官方 Codex 客户端指纹（`User-Agent`、`Originator`、`x-codex-*`、版本相关头）与真实 `Authorization`。官方 CLI 直连可用；经 Cockpit/自建网关常因剥头或合成假 UA 返回 403。

**产品目标**

| 通道 | 流量 | 行为 |
| --- | --- | --- |
| **透传（pass_through）** | 官方 Codex 出站 | 回环 Gateway 零篡改转发客户端身份头与 `Authorization`；请求体不做 Relay 改写；不注入渠道 API Key |
| **兼容（gateway_passthrough / chat）** | API Key 中继 | 现有 Responses 适配；**禁止**向上游伪造官方 Codex 身份（不把客户端 OAuth 头替换成渠道 Key 以外的假 CLI 指纹） |

非目标（本切片）：UI 渠道向导、官方订阅路由发布门、跨 Provider 压缩 Summarizer、LAN 暴露。

## MVP 验收

1. `127.0.0.1` sidecar 监听；实例准入密钥不变。
2. 渠道 `transport_mode = pass_through` 时：`POST /v1/responses` 把入站 `User-Agent`、`Originator`、`x-codex-*`、`Authorization` 原样带到上游；仅替换 `model` 为 `upstream_model_id`（公共 ID 与上游 ID 不同时）。
3. `GET /v1/models?client_version=…` 在绑定含透传上游时，查询串与身份头转发到上游 `/models`。
4. 多 `CODEX_HOME` 实例仍按 binding + RouteSet 隔离；每个实例独立准入密钥与快照。
5. 日志与诊断沿用脱敏规则；上游 HTTP 403 且 detail 含 `cli_only` / `codex_cli` 时 `gateway_code = upstream_cli_rejected`，与本地拒绝和通用 `upstream_error` 可区分。
6. 透传路径遇远程压缩 trigger → `unsupported_compaction`（fail-closed）。

## 技术设计

### 配置

- `TransportMode::CodexHeaderPassthrough`，SQLite/API 字符串 `pass_through`。
- `ProviderUpstreamRuntime.transport_mode` 在注册绑定时写入，数据面只读。

### 数据路径

```
Codex → 127.0.0.1 Gateway (admit) → [pass_through?] → 上游 Sub2API/自建
         │                              │
         └ compat: 现有 prepare_responses_relay + 渠道 Key
```

### 模块

- `managed_gateway/codex_passthrough.rs`：可转发头集合、Hop-by-hop 剥离、`/models` 代理、403 分类。
- `http.rs`：按 `transport_mode` 分支；透传时跳过 `prepare_responses_relay`，向 `forward_provider` 传入客户端 `HeaderMap`。
- `forward.rs`：`send_model_request` 在透传模式下附加客户端头，禁止 `bearer_auth(渠道 Key)`。

### 安全

- 透传模式要求入站 `Authorization`；不得把实例准入密钥当作上游凭据。
- 兼容模式继续不传客户端 `Authorization` 给 `forward_provider`（现有行为）。
- 完整请求档案仍对 `Authorization` 做 secret 替换；trace 不写头正文。

## 任务拆分

| # | 任务 | 状态 |
| --- | --- | --- |
| 1 | 设计文档与本表 | 完成 |
| 2 | `TransportMode` + schema v25 + `ProviderUpstreamRuntime` | 完成 |
| 3 | `codex_passthrough` 转发与 `/models` 代理 | 完成 |
| 4 | `http`/`forward`/`errors` 集成 + Gateway 单测 | 完成（3 项） |
| 5 | 前端 `TransportMode` 类型接受 `pass_through` | 完成 |
| 6 | UI 渠道选择、`check:all`、实机 Sub2API 验收 | 后续 |

## 风险与回滚

- 风险：透传跳过 agent_message 规范化，非 cli_only 上游行为与 compat 不一致 → 仅 `pass_through` 渠道启用。
- 回滚：移除 `pass_through` 枚举与分支，渠道改回 `gateway_passthrough`。