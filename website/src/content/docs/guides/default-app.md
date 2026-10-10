---
title: 默认 App
description: 默认 App 兼容模式、启动方式和恢复边界。
---

## 默认环境是首版主入口

默认 App 兼容模式把模型服务地址指向本机 Gateway，同时保留默认 App 的认证、会话、历史和 Electron 数据。它不是一个新的隔离 App 实例。

## 两种使用方式

### 应用后自行重启

点击“应用默认 App 模型配置”，R Codex 写入带 marker 的兼容字段。默认 App 不会被强制关闭，用户自行重启后读取新模型目录。

### 由 R Codex 启动

点击“启动默认 App”，R Codex 在派发启动时补充 `NO_PROXY/no_proxy` 的回环地址。这样能降低系统 HTTP 代理误接管本机 Gateway 的概率。

Finder 或 Dock 直开拿不到这次进程环境；如果系统代理接管回环，可能出现空 502。遇到这种情况，请从 R Codex 启动默认 App。

## 恢复配置

恢复操作只删除仍与 R Codex marker 精确匹配的字段。用户后来手动改过的值会保留，避免整份 `config.toml` 被覆盖。

## 会话边界

默认 App 历史继续由默认 App 自己维护。切换 Provider 或模型发生在新请求边界；历史中的加密推理内容、工具调用关系和 `previous_response_id` 是否能被目标 Provider 复用，需要通过目标 Provider 合同验证。
