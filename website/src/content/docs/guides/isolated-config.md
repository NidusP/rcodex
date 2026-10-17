---
title: 隔离环境配置
description: R Codex 写进隔离 App 的路由、功能开关和只复制一次的偏好。
---

隔离 App 使用自己的 Codex 目录。R Codex 在这个目录里写 `config.toml` 和模型目录，不读取、也不修改你原来的 `~/.codex`。

下面是启动时会落到这份配置里的内容。真实桌面签名和发布验收仍未完成；这些字段描述的是当前程序已经写入的合同。

## 每次启动都会重写的路由

这些字段属于 R Codex，启动时按当前工作区重新写入。

| 配置 | 作用 |
| --- | --- |
| `model_provider = "rcodex"` | 对话走 R Codex 的模型渠道，不退回官方 `openai` 渠道。 |
| `model` | 这次工作区选中的默认公共模型。 |
| `model_catalog_json` | 这次启动生成的模型目录文件。 |
| `model_providers.rcodex.base_url` | 只指向本机 `127.0.0.1` 上的 Gateway，路径是 `/v1`。 |
| `wire_api = "responses"` | 与 Gateway 之间使用 Responses 协议。 |
| `cli_auth_credentials_store = "file"` | 登录文件只放在这个隔离目录。 |
| `check_for_update_on_startup = false` | 不让 Codex 自己检查更新。 |
| `allow_symlinked_codex_home = true` | macOS 上用短路径链接打开隔离目录时，仍然允许启动。 |
| `approvals_reviewer = "auto_review"` | 只有工作区打开自动审核时才写入。 |

Gateway 的准入信息不写进配置原文。配置里只记下环境变量的名字，真正的值由这次启动放进环境。

## 功能开关

`[features]` 里 R Codex 固定写入：

```toml
multi_agent_v2 = true
```

从 Codex `0.160.0` 起，隔离配置把这一项固定写成布尔值，用来保住 `collaboration` 命名空间里的直接工具，包括 `spawn_agent`。`0.159` 只保留 `goals` 时，这些工具仍然是直接调用。

不要写成下面这样：

```toml
[features.multi_agent_v2]
non_code_mode_only = false
```

嵌套表会把整个 `collaboration` 命名空间从请求里拿掉。从默认 Codex 复制偏好、或在 R Codex 里同步 general 之后，R Codex 仍会把这一项收回布尔值 `true`。复制来的其他开关，例如 `goals`，会留下。

## 只复制一次的偏好

隔离目录第一次启动时，可以从你的默认 Codex 复制这些非秘密偏好：

- `[desktop]`：语言、对话详情、跟进队列等界面偏好。
- `[features]`：功能开关。复制后仍按上一节收回 `multi_agent_v2`。
- `[plugins]`：只复制每个插件的启用或停用，不复制插件里的其他字段。启动后仍会打开 Browser、Chrome、Computer Use 和 unified Computer Use。
- `[projects]`：只复制项目的信任级别。
- 顶层的通知方式、网络访问、响应存储开关、人格和推理显示。
- 引导完成状态，以及不含会话编号的界面偏好。默认目录已经用过、但还没有新版无项目引导完成记录时，隔离目录会补上这条完成记录，避免再次进入欢迎页。已打开的项目文件夹不复制。
- 根目录的 `AGENTS.md`、`rules/` 和个人 `skills/`。隔离目录自带的 `.system` skills 保留。

不复制 MCP、marketplace、Cookie、会话、草稿和 API Key。复制完成后，默认目录后来的修改不会再盖住隔离目录。只有你在 R Codex 里明确选择同步某一类设置时，才会再刷新这一类。

## 本机工具

R Codex 不导入默认目录里的 MCP。它只在隔离目录里启用两个本机服务：

- `node_repl`
- `cua_repl`

它们指向这个隔离目录自己的 Computer Use，不指向 `~/.codex`。

即使从未打开过默认 Codex，隔离 App 也会打开 Browser / Computer Use 插件，并把权限选择器留在「Ask for approval」。不要把默认目录里的非编程引导或关掉的插件当成隔离实例的能力。

模型目录里的 `node_repl_disabled` 决定 Codex 会不会把本机 Node REPL 交给当前模型。0.1.4 对三方条目写 false。已安装的 0.1.2 仍写 true。这个开关不在系统设置里。装上 0.1.4 或 0.1.5 并重新启动隔离实例后，新对话回合才会放开内嵌浏览器。Windows 上 0.1.5 才会把 NODE_REPL_TRUSTED_CODE_PATHS 写成用分号拼接。

## 同一份启动配置里的模型目录

模型目录和上面的 `config.toml` 一起生成。对非 GPT 系列，目录里不写 Codex 自己的图片档 `supports_image_detail_original`。GPT 系列，也就是上游模型名以 `gpt-` 开头或包含 `openai` 的系列，保留这个字段。某条渠道偶尔接受 `original` 这个字符串，也不据此给非 GPT 系列加上它。

本地工具合同继续保留 `tool_mode = "code_mode_only"`。这和上面的 `multi_agent_v2 = true` 是两件事：代码模式负责 `exec`，协作工具仍然作为直接工具挂在 `collaboration` 命名空间里。
