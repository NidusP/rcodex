# 废弃 DirectResponses 直连启动

> 状态：已实施。受管理启动一律走本机 Gateway。
> 日期：2026-10-11

## 目标

删除单渠道直连真实 Base URL 的启动路径。Codex 只把 `rcodex` 指到回环 Gateway；渠道 Key 只在 Gateway 请求边界注入。

## 决策

| 对象 | 处理 |
| --- | --- |
| 启动路径 | 只保留 Gateway。`ManagedLaunchTransport::DirectResponses` 删除。 |
| `launch_presets.gateway_policy` | 列保留，避免重建整张表。不再用它选择直连或覆盖全工作区协议。新写入固定 `gateway_passthrough`。schema v23 把旧 `direct` 改成 `gateway_passthrough`。 |
| 渠道 `transport_mode` | 只保留 `gateway_passthrough` 与 `gateway_chat_completions`，对应 `wire_api`。旧 `direct_responses` 读取和迁移都当成透传。 |
| 路由协议 | 每条模型用渠道 revision 的 `wire_api`，不再被 preset 的 `gateway_chat_completions` 整表覆盖。 |

`gateway_policy` 不再是启动开关。Chat 还是 Responses 写在渠道 revision 上，启动时抄到 RouteSet。

## 非目标

- 不重建 `launch_presets` 去删列。
- 不改 Gateway 的 Responses / Chat 转换实现。
- 不开放官方订阅路由。

## 回滚

恢复 DirectResponses 分支和 v22 schema。已迁成 `gateway_passthrough` 的行保持 Gateway，行为与产品合同一致。
