---
title: 隐私与安全
description: 凭据、会话和诊断数据如何归属。
---

## 凭据

API Key 只进入 macOS Keychain 或 Windows Credential Manager。SQLite、IPC 快照、日志和官网都不保存密钥原文。

## 默认 App

默认 App 的 `auth.json`、Electron 数据、会话和历史不由 R Codex 复制或接管。兼容模式只写入带 marker 的模型路由字段和独立 Gateway binding。

## 隔离 App

隔离 App 使用独立 `CODEX_HOME`、会话 SQLite、Electron 数据目录和进程身份。R Codex 写进这个目录的路由和功能开关见[隔离环境配置](/rcodex/guides/isolated-config/)。真实签名和桌面发布验收尚未完成。

## 诊断

诊断和用量记录是 best-effort（尽力记录），不能阻断模型请求。它们不保存请求正文、响应正文、认证 Token 或完整敏感 URL。

## 会话连续性

不同 Provider 之间不能默认复用加密推理内容、工具调用关系、图片和 `previous_response_id`。R Codex 会先验证目标能力；无法无损转换时应提示历史不兼容，而不是静默删除历史。
