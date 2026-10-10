---
title: Gateway 与 502
description: 识别本机回环代理、Gateway 和上游错误。
---

## 先看有没有诊断记录

- **有 `event=forward` 或对应诊断**：Gateway 已收到请求，继续查看上游状态、协议响应和超时。
- **没有 Gateway 记录且正文为空**：请求可能被系统 HTTP/HTTPS/SOCKS 代理拦截在回环之外。

## 常见错误层级

| 层级 | 含义 | 下一步 |
| --- | --- | --- |
| 本机 Gateway | 端口、binding、准入密钥或 sidecar 不可用 | 检查 Gateway 健康状态并重试 |
| 网络 | 无法连接真实 Base URL 或解析失败 | 检查地址、代理和网络 |
| 上游 | Provider 返回 401、429、5xx 或不兼容响应 | 检查 Key、额度和协议能力 |
| 历史 | 旧会话携带目标 Provider 无法复用的 item | 开启请求形状诊断，必要时新建会话 |

## 为什么默认 App 要从 R Codex 启动

兼容配置会持续指向 `127.0.0.1`，但绕过系统代理的 `NO_PROXY/no_proxy` 只存在于 R Codex 派发的那次进程环境。Finder 或 Dock 直开无法获得这组变量，因此系统代理可能把回环请求送往代理端口，形成空 502。

## 诊断隐私

诊断只保存状态、耗时、错误码、请求大小、哈希和枚举类型，不保存 API Key、Authorization、Cookie、请求正文或响应正文。
