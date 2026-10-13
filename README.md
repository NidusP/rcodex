# R Codex

R Codex 是专为 Codex 做的本机路由转发工具。把多个模型服务商的模型收成一份目录，启动后按模型精确转发；不必每次改 API Key，也不必切换服务商。

R Codex 启动的 Codex 使用独立配置、登录、会话和桌面数据。你自己打开的 Codex 仍用原来的环境。R Codex 不读取、不修改原 `config.toml`、`auth.json`、Cookie 或桌面登录数据。

macOS 已经过本机测试，隔离 App 可用。Windows 安装包可以构建，功能待实机跑通。官方账号认证登录尚未完善。

## 使用流程

1. 添加 API Key 模型渠道，填写服务地址和密钥，检测并勾选模型。
2. 把多个渠道的模型加入工作区，设一个默认模型，并选一个渠道做 API 登录。单渠道会自动选中。
3. 点击启动。Codex 看到的是一份综合模型目录，对话里选哪个模型，请求就发到那个渠道。
4. 运行中仍可改渠道和模型，下次启动才用新的路由快照。

## 本地网关

Codex 不直连各家上游。R Codex 把 Codex 的 Provider 固定为 `rcodex`，请求先到本机 Gateway，再按启动时固化的路由转发。

```text
R Codex 启动的 Codex（独立环境）
    → 127.0.0.1 Gateway /v1/responses
    → 用公共模型 ID 精确匹配这次启动的 RouteSet
    → 换成上游模型名，带上该渠道自己的 API Key
    → 对应模型服务
```

- 网关是独立的 `codex-gateway` 伴随进程，只绑定 `127.0.0.1`，没有局域网或公网入口。
- 每个运行实例有自己的准入密钥。没有密钥或密钥不对，请求进不了网关。
- 路由在启动时拍成不可变快照。按公共模型 ID 精确匹配，不按模糊名字猜渠道；401 或 502 也不会自动换地址、换模型、换渠道或重试这次 POST。
- 单渠道和多渠道都走网关。渠道 Key 只在 Gateway 请求边界注入，不写进 Codex 进程环境。
- 上游非 2xx 算 `upstream`；本地路由、准入、网络或解析错误算 `gateway`。错误里带 `gateway_code`，必要时带脱敏后的上游信息。
- R Codex 拉起的进程会补上回环 `NO_PROXY` / `no_proxy`，避免本机系统代理把 Gateway 劫持成空 502。系统代理和用户默认环境不改。
- 关 R Codex 窗口只隐藏控制面，有活动实例时不拆网关。正在传输的流不保证跨网关重启续接。

## 三方模型如何兼容 Codex

Codex 客户端发出的是 Responses 请求。Gateway 先按公共模型 ID 精确匹配，换成上游模型名，并只注入这一条渠道自己的 API Key。官方 Authorization 不会发给第三方。

GPT 族模型按 Codex 发出的 Responses 原样转发，不改 `metadata`、可选工具或 `agent_message`。识别看公开名或上游名，例如 `gpt-*`、`o3`。上游是 Grok / DeepSeek / Claude 时仍走下面的兼容处理，即使公开名里带了 `gpt`。

非 GPT 模型再收成标准 Responses，让中继能读懂：

- 去掉中继常常拒收的 `metadata`。
- 渠道不支持的可选工具（例如网页搜索）从这次请求拿掉，而不是声称支持。
- Codex 私有的 `agent_message` 收成公开的 user 消息。读不懂的私有项明确失败，不假装兼容。
- 未知模型或能力对不上就失败，不改写原会话历史。

目前可用的是 Responses 上游透传。Chat Completions 协议转换尚未完善。官方账号认证登录也尚未完善。

## 密钥与数据如何存储

- 应用元数据、渠道、路由和工作区在 R Codex 自有 SQLite。数据库文件对当前用户私有。
- 渠道 API Key 存在同一库的 `provider_credentials` 表，按渠道修订精确读取。运行时不读系统钥匙串或凭据库。界面、日志、导出和 IPC 快照不带原文。
- 选中的 API 登录 Key 会额外写到这份隔离环境的 `auth.json`，权限 `0600`、原子替换。只给 R Codex 启动的 Codex 做客户端登录，不接管你原来的登录态。
- 实例准入密钥不是用户凭据。SQLite 只保存引用和哈希，明文只在内存和一次性管道里交接。
- 每个受管理实例有独立的 `CODEX_HOME`、会话库和 Electron 用户数据目录。App 的 `CODEX_SQLITE_HOME` 等于这份 `CODEX_HOME`。
- 普通诊断只记状态、耗时、Token 和脱敏错误，不记请求或响应正文。完整请求档案是可选的本机独立存储，不进入普通日志。
