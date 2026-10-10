---
title: 路线图
description: 当前可用、正在补齐和暂缓开放的能力。
---

## 当前可用

- 默认 App 兼容路由。
- API Key Provider 与模型目录。
- 多 Provider 的公共模型路由。
- 本机 Gateway、Responses 透传、受限 Chat 转换。
- 脱敏诊断和本地用量观察。

## 正在补齐

- 三方模型旧会话字段级诊断和结构化 `route_history_incompatible` 错误。
- 真实 Provider 的新建、连续、恢复、回滚和模型切换矩阵。
- sidecar 重启时 SSE 流中断的恢复提示。

## 暂缓开放

- 隔离 App：等待签名、目录隔离、进程身份和真实桌面验收。
- 官方订阅模型：等待公开协议和账户合同验证。
- Windows 发布：等待 Tauri、launcher、Gateway sidecar 打包和签名链路。

路线图描述的是当前工程状态，不代表任何未验收能力已经对用户承诺。
