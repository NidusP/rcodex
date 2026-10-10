---
title: 工作原理
description: 了解 Codex、Gateway 和 Provider 之间的边界。
---

## 一条本机请求路径

```text
Codex → 127.0.0.1 Gateway → Provider 的真实 Base URL
```

Codex 只看到工作区中的公共模型 ID。Gateway（本机网关）负责校验当前 binding，读取已固化的 RouteSet，再把模型 ID 映射到具体 Provider 和上游模型。

## 控制面和数据面

| 部分 | 负责什么 | 不负责什么 |
| --- | --- | --- |
| 控制面 | 管理 Provider、模型、启动方案、修订和诊断 | 不承载每个模型 token |
| Gateway 数据面 | 接收回环请求、精确路由、协议转换和流式返回 | 不猜测 Provider，不自动换模型 |
| 默认 App | 保留自己的认证、会话和历史 | 不成为 R Codex 管理的运行实例 |

## 为什么需要 Gateway

Codex 一次只使用一个模型服务入口，而工作区可以同时接入多个 Provider。Gateway 将这些模型收敛到一个本机入口，再按公共模型 ID 选择真正的上游。

单个 Provider 且支持 Responses 直连时，受管理 CLI 可以直连真实 Base URL；默认 App 兼容模式仍使用独立的 Gateway binding，以保持配置和诊断边界。

## 路由在什么时候固定

启动或应用配置时会生成不可变的路由快照。运行中的请求不会重新读取后来修改的 Provider、模型目录或 API Key 修订；这样每次请求都能解释“用了哪个渠道”。
