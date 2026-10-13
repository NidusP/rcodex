# Codex App 支持预研

> 状态：预研与隔离行为探针完成，受管理 App 条件可行；运行时按打包依赖和目标版本探针开放，真实签名桌面会话与上游凭据验收仍需发布前执行
> 日期：2026-08-28
> 范围：评估 R Codex 如何把 Provider、模型选择和本地 Gateway 带入官方 Codex 桌面 App；不把 Codex App 多开作为产品目标。

## 结论先行

### 2026-09-01 产品决策：历史边界与默认 App 兼容模式（已决策，基础实现完成）

受管理 App 保持为安全模式，但不同步、链接或修复默认 App 的历史会话。隔离 `CODEX_HOME` 的会话、附件、归档与索引只能在原隔离环境中继续使用；受管理 App 只从默认环境一次性应用白名单内的非敏感偏好。这样避免默认与隔离环境同时写入 Codex 历史、SQLite 或索引文件，也不承诺跨模式继续对话。

另行增加“默认 App 兼容模式”作为体验模式：它显式改写默认 `CODEX_HOME/config.toml` 的 R Codex 所有字段并记录字段级 marker，默认 App 因而保留自己的历史、登录和 Electron 数据。兼容模式的 Provider 路由是持久状态，而非一次性启动注入；它由独立兼容绑定保存，不创建或关联运行实例。默认 App 进程由系统拥有，R Codex 不读取、关闭或管理它；停用按 marker 逐字段清理并撤销绑定，绝不向 App 发送终止信号。兼容准入密钥会写入 Provider 的本地 `http_headers`，不使用 Provider API Key；当前 Provider 的有效 `base_url` 也会指向本机 Gateway，避免第三方模型绕过路由。原始目录中未选择的模型由 Gateway 回退到接入前的 Provider 地址，并仅转发该请求携带的认证 Header。R Codex 重启后可从活动绑定和系统凭据库恢复 Gateway。只有显式停用后才清理仍匹配的 R Codex 字段。当前已提供 `restore_default_codex_config` 一键恢复命令和前端入口；它不依赖默认 App 进程采样。`auth.json`、默认 Electron 数据和任何会话文件均不属于接管写入范围。

兼容 Gateway 明确由 R Codex 进程监督，而不是常驻系统服务。因此 UI 主动退出会先阻止退出并提示用户停止实例；实例停止后退出、异常退出或机器重启时，默认 App 的后续模型请求可能因本机 Gateway 不可用而失败；这是允许且必须可解释的产品行为，不自动恢复默认配置或转为直连。用户重新打开 R Codex 后，控制面从持久化兼容绑定和系统凭据库重新注册 Provider 路由，后续请求才恢复。兼容配置必须使用固定的当前用户回环端口，防止默认 `config.toml` 在重启后指向失效的随机端口；端口、凭据或版本探针失败时保持明确不可用。首期退出、重启和失败路径已覆盖自动化验证；真实签名桌面属于发布前环境验收。

兼容模式的配置不必全部物理存放于默认目录：模型目录可以由默认 `config.toml` 通过 `model_catalog_json` 指向 R Codex 管理的外部只读文件。但默认 App 未提供稳定的无参数 Profile 选择合同，当前 Provider、`model_provider`、Gateway 地址和准入字段仍需写入默认 `config.toml`。单文件软链在 Unix 上可能工作，但不是可靠产品合同：Codex 或其他编辑器的原子 rename 可能替换链接，Windows 还存在创建权限和解析差异。整个 `CODEX_HOME`、`auth.json`、会话或 SQLite 目录不得软链；否则会把默认历史和认证状态一并转移。默认实现采用真实 `config.toml` + marker/备份/原子恢复 + 外置模型目录。

由 R Codex 启动时指定 Profile 不会自动把对话记录分开。Profile 是配置覆盖层；相同 `CODEX_HOME` 仍共享认证、会话、项目和技能状态。当前公开的 app-server/Desktop 路径也没有稳定的 `--profile` 或环境变量选择合同，因此不能把它作为默认 App 兼容模式的现行实现；若未来合同稳定，才可用“启动时 Profile + 默认 home”替代部分文件物化。只有独立 `CODEX_HOME` 与 Electron 用户数据目录才是当前受管理 App 的完整隔离边界。

当前可以确认，R Codex 存在一条实现“运行实例 + 多 Provider + 受管理 Codex App”的技术路径，但它不是简单复用 CLI 启动参数。需要同时解决 Codex 共享配置、模型目录、App 的 Electron 用户数据目录、实例进程监督和 Gateway binding（绑定）生命周期。这里的 App 是运行实例的一个启动目标，与 CLI 并列；产品不计划提供多个 Codex App 窗口/账号的并行管理。

官方公开的 **Codex App Server** 是一个给自建富客户端使用的 JSON-RPC 协议。它可以承载认证、会话、审批和流式事件，适合开发自己的 Codex 客户端；它本身不是第三方 Provider 插件接口。App Server 的 WebSocket 传输仍明确标注为实验性、不建议用于生产，因此受管理 App 的首期数据面应优先使用 Codex App 读取的本地配置和 HTTP 回环 Gateway，而不是把 App Server WebSocket 当成转发通道。依据：[OpenAI Docs：Codex App Server](https://learn.chatgpt.com/docs/app-server)。

因此本阶段的产品决策是：

- 多 Provider、多模型和请求账本继续由运行实例统一承载；CLI 是默认启动目标，受管理 App 已具备运行时兼容性门控制的启动与请求闭环；
- 默认 Codex App 继续提供“打开 App + 工作目录”便利入口；受管理 App 则使用实例专属配置和 Gateway；
- 安全模式不修改默认 `~/.codex`、App 用户数据目录、App 安装包或官方登录态；兼容模式仅在用户显式启动时接管默认 `config.toml`，并以 marker/备份支持恢复，不触碰 `auth.json`、Electron 数据或历史文件；
- 受管理 App 可以进入“实验性、单平台、单实例”探针；只有目标版本和平台通过兼容性探针后才开放，探针失败必须禁用该入口，不能伪装成已接管。
- 官方账户 + API Provider 的混合路由已纳入数据模型与 UI 草稿，但控制面显式报告 `officialHybridAvailable=false`。在官方凭据代理和按模型分流合同验证前，启动保持关闭，避免出现“官方账户配置存在但 Gateway 实际无法完成认证”的假可用状态。

## 术语与边界

| 名称 | 在本项目中的含义 | 当前状态 |
| --- | --- | --- |
| Codex CLI | 在终端运行的 Codex 命令行客户端；可由 R Codex 生成隔离 `CODEX_HOME`、Profile、模型目录和实例密钥 | 已支持受管理启动 |
| Codex App | 官方桌面图形客户端；不是 R Codex 的窗口，也不是 CLI 的简单别名 | 默认 App 已支持；受管理 App 按运行时兼容性门开放 |
| Codex App Server | Codex 的客户端协议服务端；通过 JSON-RPC 提供线程、认证、审批和事件能力 | 已用于官方账户适配；不是官方 App 注入接口 |
| 受管理 App 启动目标 | 运行实例以 App 作为启动目标，使用实例专属状态、路由快照、进程监督和启动回执 | 启动骨架与 renderer/Gateway/退出探针已完成；按运行时兼容性门开放 |
| Gateway | R Codex 控制的本机回环路由层；多 Provider 实例必须经 Gateway | 只对受管理 CLI/明确绑定的实例生效 |

## 官方能力核对

### 1. App Server 的定位

OpenAI Docs 将 App Server 描述为 Codex 富客户端使用的接口，并以 VS Code 扩展作为客户端示例。协议采用双向 JSON-RPC，可通过 stdio、Unix socket 或 WebSocket 连接；客户端需要先 `initialize`，再创建或恢复 thread。它暴露的是“客户端与 Codex 运行时之间”的控制面，不是桌面 App 的第三方 Provider 插件系统。

官方文档还说明：WebSocket 监听是实验性能力；远程连接需要认证和 TLS，回环连接才适合本机使用。由此，R Codex 不应把一个临时 WebSocket 连接当成官方 App 的生产集成合同。

### 2. OpenCodex 已验证的 App 接入路径

OpenCodex 的公开实现说明了关键机制：

1. 在 Codex 共享 home 中写入 `openai_base_url` 或专用 `model_providers.opencodex`，让 App 的请求先到本机代理；
2. 生成 Codex 可解析的 `opencodex-catalog.json`，把 `xai/grok-4.5` 这类命名空间模型放进模型选择器；
3. 代理按 `provider/model` 解析路由，xAI Grok 使用 `openai-chat` 适配器，将 Responses 请求转换为上游 Chat Completions，再转回 Codex 格式；
4. 修改目录后处理 `models_cache.json`，必要时重启长驻 app-server，让 App 重新加载模型。

OpenCodex 明确表示它不修改 App 安装包，而是写入 CLI/TUI/App/SDK 共用的 Codex 配置和模型目录。[OpenCodex Codex Integration](https://opencodex.me/guides/codex-integration/)、[OpenCodex Codex App model picker](https://opencodex.me/guides/codex-app-models/)

这证明“App 读取共享配置 → 本机代理 → Grok”是现实可行的；但它的默认策略会修改用户默认 `~/.codex`，并由 OpenCodex 自己维护 Provider 认证，这与 R Codex 的默认环境保护和系统凭据库边界不同，不能直接照搬。

### 3. Provider 与模型配置的定位

Codex 配置参考公开了 `model_provider`、`model_providers` 和 `model_catalog_json` 等配置项，且明确区分用户级配置与项目级配置。结合 OpenCodex 的实测路径，可以确认 App 会读取这类共享配置；但官方没有承诺 R Codex 自定义的实例密钥、临时 Base URL 或 Electron 参数在所有版本都保持兼容，因此仍需目标版本探针。依据：[OpenAI Docs：Configuration Reference](https://learn.chatgpt.com/docs/config-file/config-reference)。

### 4. Cockpit 已验证的应用周期接管路径

Cockpit Tools 的公开代码和变更记录显示，桌面应用受管理启动至少需要两层隔离；这些机制可用于单个运行实例的生命周期接管，并不要求 R Codex 提供 App 多开：

- 每个实例独立的 `CODEX_HOME`，隔离配置、认证、会话和模型目录；
- 实例使用独立的 Electron 用户数据目录，并在 macOS/Windows 启动时同时传递 `CODEX_ELECTRON_USER_DATA_PATH` 与 `--user-data-dir`，避免受管理 App 与默认 App 的单实例锁、Web 登录状态和本地渲染状态互相冲突。

Cockpit 还按实例目录/真实进程识别 PID，并将 API 服务放在独立 sidecar 中。[Cockpit Codex multi-instance implementation](https://github.com/jlcodes99/cockpit-tools/blob/main/crates/cockpit-core/src/modules/codex_instance.rs)、[Cockpit Codex API Service handoff](https://github.com/jlcodes99/cockpit-tools/blob/main/docs/CODEX_API_SERVICE_HANDOFF.md)

我们只借鉴“隔离目录、启动参数、进程识别和 sidecar 生命周期”这条工程路径，不复制其凭据导入、OAuth relay 或全局接管行为。

### 3. 认证边界

App Server 支持 API Key 与 ChatGPT managed 等认证模式，并由 Codex 管理登录和令牌刷新。R Codex 当前只在隔离 `CODEX_HOME` 中调用官方认证适配器；不读取、复制或转发默认 `auth.json`，也不把官方 OAuth 令牌放进 Gateway 的第三方 Provider 路径。

## 当前项目实现核对

当前实现已经形成清晰的两条入口：

```text
受管理 CLI：R Codex -> codex-launcher -> 隔离 CODEX_HOME/Profile -> CLI -> Gateway/Provider

默认 App：R Codex -> codex-launcher --target app -> codex app <目录> -> App 自身配置/登录

受管理 App：R Codex -> codex-launcher --target managed_app -> 运行实例 CODEX_HOME + Electron data dir -> Codex App -> 127.0.0.1 Gateway -> 多 Provider
```

`open_codex_app(workspace_path?)` 不接受 Provider、账户、预设、RouteSet 或凭据引用；启动器还会拒绝任何受管理 CLI/App 参数进入默认 App 分支。受管理 App 使用独立的 `launchTarget=managed_app` 启动合同，不把参数偷偷塞进现有默认入口。当前实现这样可以保证：

- 打开 App 不会改变默认 Codex 环境；
- 退出或重启 R Codex 不会把 App 误认为受管理运行实例；
- Gateway 不会意外接管用户从 Dock、Finder 或系统快捷方式打开的 App；
- 运行实例请求账本不会宣称覆盖 App 请求，因为当前 App 路径没有经过 R Codex Gateway。

## 能力矩阵

| 能力 | 当前结论 | 产品处理 |
| --- | --- | --- |
| 打开已安装 Codex App | 可行 | 保留现有入口 |
| 传入一次性工作目录 | 可行（当前 macOS 路径） | 继续支持，失败时给出明确错误 |
| 把 API Key Provider 注入受管理 App | 技术上可行，需隔离配置和凭据边界 | 仅在实例专属 home 中实验 |
| 把运行实例的多个 Provider 汇总到 App 模型选择器 | 隔离配置与版本匹配目录已物化；App Server 已读到目录，真实 picker 点击仍需签名桌面会话验收 | 只在兼容性门通过时开放 |
| 让受管理 App 请求经过 R Codex Gateway | 实例级 binding、回环路由和 App Server 请求闭环已验证；真实桌面 renderer 触发仍需签名桌面会话验收 | 只允许实例专属 Gateway 路由 |
| 用 App Server 驱动自建客户端 | 条件可行 | 作为长期产品方向单独立项 |
| 在 App 路径记录 Provider/Token 账本 | 当前不可行 | 只统计经过 Gateway 的受管理实例 |
| 每个 App 实例独立登录/数据目录 | 已实现实例专属目录与进程监督，默认只允许一个活动受管理 App | 持续执行平台/版本发布验收 |

## 方案比较

### 方案 A：维持默认 App 入口（推荐）

继续使用当前 `codex app <目录>`，只做打开、工作目录和状态提示。多 Provider、模型选择、Gateway 及请求账本由受管理 CLI 提供。

优点是边界清晰、不会触碰用户默认登录、无额外进程监督成本；缺点是官方 App 暂时不能使用 R Codex 的多 Provider 模型选择。

### 方案 B：运行实例的 Codex App 启动目标（已实现，持续发布验收）

为运行实例创建隔离 `CODEX_HOME`、Electron 用户数据目录和临时路由配置，再启动并监督真实 App 进程及其派生 App Server。OpenCodex 的共享配置/目录机制和 Cockpit 的应用周期接管机制使该方案具备实现基础；当前实现已完成运行时探针，只有目标版本与平台通过探针时才开放。该方案不承诺多个 Codex App 并行运行。

最低验证项包括：真实 App PID 与启动身份、工作目录、数据目录、配置生效、模型目录生效、Gateway 请求到达、App 退出回执、崩溃/PID 复用和停止后的资源释放。任何一项无法证明，都应关闭“受管理 App”，而不是回退为默认 App。

### 方案 C：基于 App Server 自建 R Codex 客户端（长期方向）

R Codex 自己实现富客户端 UI，通过 App Server 管理线程、审批、认证和事件，并由 R Codex 自己控制 Provider/Gateway 路由。这不是“接管官方 Codex App”，而是另一种产品形态，开发量、兼容性和官方协议跟随成本都显著更高，不属于当前 P2 最小范围。

## 后续验证门

若未来要推进方案 B，应按以下顺序执行，且每一步都可单独停止：

1. **版本探针**：记录平台、Codex 版本、`codex app --help`、App Server schema 指纹和可用启动参数；不依赖一次本机探测永久有效。当前版本探针已完成，见下方 2026-08-28 记录。
2. **只读隔离探针**：使用临时目录验证 App 是否读取指定 `CODEX_HOME`/用户数据目录和模型目录；不得复制默认认证文件。当前 `CODEX_HOME`、Electron 用户数据目录、模型目录和默认 App 并存均已通过；真实上游请求仍受签名桌面会话与 Keychain 授权条件约束。
3. **单运行实例探针**：以一个运行实例启动 App，核验真实 App 进程、派生进程、工作目录、配置和退出事件；不测试 App 多开调度。
4. **路由探针**：仅使用无敏感 fixture 或测试 Provider，确认请求是否到达本机 Gateway、Provider 和正确模型；失败时不自动切换路由。
5. **发布门**：按平台、Codex 版本和 CLI/App 启动目标分别记录通过矩阵；未通过的平台只保留默认 App 入口。

## 推荐的 R Codex 组合架构

```text
R Codex UI
  -> AppLaunchService
      -> instance CODEX_HOME
      -> instance Electron user-data-dir
      -> instance config.toml + model catalog
      -> shared codex-gateway sidecar
           -> binding (受管理目标使用实例 ID；默认 App 使用兼容命名空间)
           -> Provider A / Provider B / Grok
      -> Codex App process + derived app-server
```

职责边界：

- `AppLaunchService` 只负责实例目录物化、启动参数、PID/启动身份和退出回执；
- `ManagedCodexConfig` 负责写入实例专属 `model_provider`、Gateway 地址和模型目录，不能写默认 `~/.codex`；
- `codex-gateway` 负责按 binding 校验准入、按公共模型 ID 路由 Provider，并注入渠道 API Key；默认 App 的兼容 binding 不代表一个由 R Codex 管理的进程实例；
- App 的官方登录仍由 Codex 自己持有；第三方 API Key 仍只由 Gateway 在请求边界读取；
- 一个 sidecar 可以服务多个 App/CLI binding，但每条请求必须能识别所属 binding；账本保持最小字段，不新增 App 专属复杂报表。

与 OpenCodex 的核心差异是：OpenCodex 偏向“修改共享默认 home、常驻代理”；R Codex 应采用“实例专属 home、实例专属配置快照、共享 Gateway + 实例 binding”。这样既得到 App 支持，又不牺牲默认环境保护。

## 还需要解决的关键拼图

### 1. App 模型 ID 与现有 RouteSet 身份

OpenCodex 使用 `provider/model`（例如 `xai/grok-4.5`）作为 Codex 看到的模型 ID；R Codex 当前 RouteSet 的 `public_model_id` 设计为永久稳定且禁止 `/`，这是为了避免 Provider 重命名、URL 或显示名变化影响 CLI 历史和路由。

首期不建议直接修改现有 RouteSet 身份规则。更稳妥的做法是：

- RouteSet 继续使用现有 slash-free 内部 ID；
- 为受管理 App 的模型目录生成稳定别名，例如 `provider-<stable-provider-id>/grok-4.5`；
- 在 App binding 中保存“App 模型别名 → RouteSet route identity”的映射；
- Gateway 收到 App 模型别名后先解析到不可变 RouteSet，再执行现有 Provider 路由。

这样 App 仍能展示“渠道/模型”层次，重复模型可区分，CLI 旧会话和现有请求账本不需要迁移。若后续验证 Codex App 对 slash-free ID 的展示已经足够，也可以直接使用 `provider-model` 形式，减少 Gateway 别名解析。

### 2. App 的启动参数不是 CLI 参数的简单复用

`CODEX_HOME` 负责 Codex 状态根目录，但桌面 App 还需要独立 Electron 用户数据目录来隔离单实例锁、Web 登录状态和本地渲染状态。Cockpit 的公开变更记录表明，macOS/Windows 需要同时传递 `CODEX_ELECTRON_USER_DATA_PATH` 和 `--user-data-dir`；这两个名称和行为必须按目标 Codex 版本实测，不能仅凭其他项目的参数名直接写入发布版本。

### 3. App 模型选择器可能有第二层过滤

OpenCodex 的文档记录了 Desktop renderer 的 `available_models` 白名单问题：模型已经出现在磁盘目录和 `model/list`，仍可能在 App picker 中被隐藏。受管理 App MVP 必须把“目录已写入”“app-server 已读取”“UI 可选中”“请求已到 Gateway”分成四个独立探针结果，不能只以文件存在判断成功。

### 4. 认证与账本边界不能照搬 OpenCodex

OpenCodex 可以自己维护 xAI OAuth/API Key，并在代理层选择 xAI 的不同上游；R Codex 当前的安全边界是 API Key 进入系统凭据库、由 Gateway 在请求边界读取，官方 Codex OAuth 不进入第三方 Provider。受管理 App 首期应先支持 API Key Provider；官方账户与第三方模型混合仍需单独的认证合同和真实账户发布门。

### 5. 共享 Gateway 还是每 App 一个 Gateway

技术上两者都能工作：

- **共享 Gateway + 每实例 binding（推荐）**：端口和资源少，复用当前 `codex-gateway` sidecar；需要严格校验实例密钥、RouteSet 和 App 模型别名。
- **每 App 一个 Gateway**：隔离最强，调试简单；但会增加进程、端口、启动恢复和账本管理复杂度，不符合当前“先做核心功能”的目标。

首期应沿用共享 Gateway，仅为每个 App 生成独立 binding，不为每个 App 常驻新的转发进程。

验收要求：探针失败可解释、不会污染默认 `~/.codex` 或 App 登录；运行实例关闭后隔离目录和临时路由可回收；App 请求应像 CLI 请求一样关联到运行实例并记录最小账本字段；任何认证错误原样归属于官方 App/运行时，不由 R Codex 伪造修复。

## 运行实例模型

运行实例是 Provider 组合和启动目标的统一载体：

```text
运行实例
  ├─ 一个或多个 Provider
  ├─ 一个不可变 RouteSet
  ├─ 一个 Gateway binding
  └─ 一个启动目标：Codex CLI 或 Codex App
```

CLI 与 App 的差异只在启动和进程接管层；Provider、模型目录、Gateway 路由、实例密钥和最小请求账本应复用同一套运行实例合同。App 目标首期不扩展为多开管理，也不新增独立的 App 账本。

## 直接点击 Codex App 图标时的产品语义

这里必须区分“系统默认 App 入口”和“运行实例 App 入口”：

| 用户动作 | 应有行为 | 是否受 R Codex 管理 |
| --- | --- | --- |
| 从 Dock、Finder、开始菜单或系统快捷方式点击 Codex App | 打开/聚焦默认 `CODEX_HOME`、默认模型目录和默认登录状态 | 否 |
| 在 R Codex 的运行实例上点击“打开/聚焦 Codex App” | 启动或聚焦该运行实例的 App，使用实例专属 `CODEX_HOME`、Electron data dir、RouteSet 和 Gateway binding | 是 |
| R Codex 已有受管理 App 运行时再次点击同一实例 | 聚焦已有窗口，不重复启动 | 是 |
| 用户直接点击系统 App 图标时试图“跳转”到受管理实例 | 不做隐式跳转，不修改默认入口语义 | 否 |

因此推荐用户体验是**方案 1 的受控版本**：默认 App 可以被直接打开，但它与受管理 App 的配置、模型目录、登录和 Gateway 路径必须隔离。方案 2 不推荐，因为它会让系统图标的实际行为依赖 R Codex 最近一次运行实例，用户无法预测自己打开的是哪个 `CODEX_HOME`，也会把默认 App 误认为受管理实例。

“不支持多开”在这里的含义是：R Codex 不提供多个受管理 Codex App 实例的并行编排、账号池和批量生命周期管理；它不等于系统层面绝对禁止默认 App 与一个受管理 App 同时存在。两者能否在目标 Codex 版本上真正并存，必须由探针验证 Electron 单实例锁和 App Server 行为：

- 若独立 Electron data dir 后两者可以并存，默认 App 请求使用默认配置，受管理 App 请求使用实例 Gateway，数据面互不串用；
- 若 Codex 仍以全局单实例方式把第二次启动转给已有进程，受管理 App 启动必须明确失败或提示，不能把默认 App 伪装成受管理 App；
- R Codex 不修改 App 安装包、不劫持系统图标、不把默认 App 的点击重定向到实例 App。

UI 文案应分别使用“打开默认 Codex App”和“打开运行实例 App”，避免两个入口都只写“打开 Codex App”。

## 明确不做的事情（安全模式与兼容模式共同边界）

- 安全模式不修改用户默认 `~/.codex/config.toml`、`auth.json` 或系统代理来“接管” App；兼容模式只允许显式、可恢复地修改 `config.toml`，不修改 `auth.json` 或系统代理，也不写 `$CODEX_HOME/.env` 冒充 `NO_PROXY`。系统代理拦截回环时，R Codex 支持的打通路径是安全启动默认 App；见[默认 App 回环 Gateway 与系统代理](compat-app-loopback-proxy.md)。
- 不向官方 App 注入 API Key、OAuth token 或 Cookie；兼容模式的本地实例准入密钥仅写入 Provider `http_headers`，并且不等同于任何上游凭据；
- 不通过修改 App 安装包、注入 Electron 启动脚本或 UI 自动化伪造 Provider 支持；
- 不把 App Server 的实验性 WebSocket 当成官方桌面 App 的稳定插件接口；
- 不为了 App 预研新增复杂的运行实例账本、价格系统或后台采集线程。

## 决策记录

- **Change**：完成 Codex App 支持可行性预研，确认“运行实例 + 共享配置/模型目录 + 本地 Gateway + Electron 数据目录隔离”可以构成受管理 App 启动目标的实现路径。
- **Boundary**：受管理运行实例统一承载 CLI/App、多 Provider、Gateway 和最小账本；默认 App 直接打开仍是原生入口，兼容模式是用户显式启用的 `config.toml` 接管并使用独立兼容绑定，不进入运行实例表；不规划 Codex App 多开。
- **Owner**：`LaunchService` 负责运行实例启动；CLI 与受管理 App 使用各自目标适配器；`open_codex_app` 只负责默认 App 派发。受管理 App 在后端拒绝第二个活动实例，避免依赖 Electron 全局单实例行为。
- **Consistency**：App 支持以版本/平台探针为发布门，不以“命令成功返回”推断配置已经生效。

## 2026-08-28 当前版本机械兼容性探针

本轮只做不启动桌面 App、不读取默认认证、不修改用户 `~/.codex` 的机械检查：

- 本机 Codex CLI（命令行客户端）版本为 `0.150.0-alpha.8`；桌面 App bundle（应用包）版本为 `26.820.60940`，bundle id 为 `com.openai.codex`。
- `codex app --help` 通过，当前安装支持 `codex app [OPTIONS] [PATH]`，可传入工作目录及显式配置覆盖。
- `codex app-server --help` 通过，当前安装支持 `daemon`、`proxy`、`generate-ts` 和 `generate-json-schema` 子命令。
- 在临时 `CODEX_HOME` 下执行 `app-server generate-json-schema` 成功并生成 v2 协议 schema；该动作未使用默认登录态。App Server 的 JSON-RPC、初始化顺序和 schema 生成能力与官方文档一致，但 WebSocket 仍是实验性、不建议生产使用的传输方式。[OpenAI Docs：Codex App Server](https://learn.chatgpt.com/docs/app-server)

这组结果最初只证明“当前版本具备命令与协议 schema 的机械入口”，不能单独证明 App 会读取隔离的 `CODEX_HOME`、Electron 用户数据目录或模型目录，也不能单独证明默认 App 与受管理 App 可以并存。后续原生探针已补齐这些证据；当前实现由本机运行时探针决定 `managedAppAvailable`（受管理 App 是否可用），并保留 `THREER_CODEX_DISABLE_MANAGED_APP=1` 作为紧急关闭开关。配置项 `model_provider`、`model_providers` 和 `model_catalog_json` 的官方定位可作为只读隔离探针的合同参考，但不能替代目标版本实测。[OpenAI Docs：Configuration Reference](https://learn.chatgpt.com/docs/config-file/config-reference)

下一步是临时目录下的只读隔离探针，然后才考虑单运行实例 App MVP；不会测试或编排多个受管理 Codex App。

## 2026-08-28 隔离启动探针结果

本轮使用临时目录启动一次官方 App，并在探针结束后将临时目录移入回收站：

- 默认 App 仍在运行时，受控 App 根进程和 GPU、网络、存储、Renderer 子进程均使用独立 `--user-data-dir`，未被系统单实例锁转发到默认 App。
- App 在受管理的 `CODEX_ELECTRON_USER_DATA_PATH` 下复用 Electron 偏好；同时在实例专属 `CODEX_HOME`（由短路径别名传入）下维护 Codex 配置与路由状态，证明两个根目录仍然分离。
- App 启动的 stdio App Server 版本报告为 `0.150.0-alpha.8`，initialize 握手成功；未登录时仅产生预期的“需要登录”诊断，没有复制或读取默认 `auth.json`。
- 临时目录中未发现 `auth.json`、`config.toml`、模型目录或 `models_cache.json`。因此本轮只证明状态根目录和 Electron 用户数据目录隔离，不证明模型目录/模型选择器生效，也不证明 Gateway 请求已到达。

补充验证：现有 `installed_codex_accepts_its_version_matched_managed_catalog_when_requested` 回归测试在本机 Codex `0.150.0-alpha.8` 上通过，证明 CLI 能读取版本匹配的受管理模型目录；随后 App Server 探针也已证明同一目录可被 App 数据面读取，但真实桌面 App renderer（界面渲染层）模型选择器仍需在签名桌面会话中点击验收。

运行态修复：受管理 App 的 Electron 用户数据根目录改为 R Codex 数据目录下稳定的 `managed-app-data`，不再随每个运行实例创建新的 `electron-data`。同一启动方案复用实例 `CODEX_HOME` 与同一路径上的 `CODEX_SQLITE_HOME`，因此会话、线程和本地索引随该方案绑定的实例 home 跨重启保留。旧的 `managed-app-sqlite/<launch-preset-id>` 只作一次性迁入源，不再作为活动对话库；拆开布局会让 Codex Desktop 在 `CODEX_HOME/state_5.sqlite` 找不到 threads 表，隔离会话无法归档。每次启动仍更新同一方案绑定实例的不可变路由快照和密钥，不复制旧数据库。首次创建时只允许迁移默认 App 的版本标记和经字段白名单确认的纯 UI 偏好；Local Storage 中的 onboarding 标记必须单独按目标版本探针迁移，不能整目录复制。受管理 App 不链接默认 HOME 的会话、归档会话、附件、`session_index.jsonl`、线程/目标/记忆索引或安装版本元数据，同时继续禁止链接 `auth.json`、Cookies、Login Data、认证数据库、模型缓存和路由配置。

本轮原生启动补充验证使用 `/Applications/ChatGPT.app` `26.820.60940`：将 `--user-data-dir` 以 `--user-data-dir=<绝对路径>` 形式传入后，App 主进程、Renderer、网络/存储子进程和 App Server 均使用临时 Electron 数据目录；临时 `CODEX_HOME` 也产生了独立状态。探针期间默认 App 仍保持运行，两个 App 根进程未发生单实例转发。该结果确认启动器必须使用等号形式传递 Electron 参数；它仍不证明 renderer 模型选择器、Gateway 数据请求或 R Codex 退出回执已经闭环。

随后使用当前 `codex-launcher` 对同一安装做了无凭据原生 smoke：启动器写入 `running` 回执，回执中的桌面 App PID 命令行包含实例专属 `--user-data-dir=<临时目录>`，向该 PID 发送 SIGTERM 后写入 `exited` 回执。该验证确认进程监督链路成立；退出信号本身没有退出码是预期的，不代表失败。真实 Gateway 请求和 renderer 模型选择仍未在无敏感 fixture 下完成。

进一步的临时 App Server 探针在同一版本、隔离 `CODEX_HOME` 和测试 `config.toml` 下请求 `model/list` 成功，返回受管理模型。随后使用同一隔离目录执行 `thread/start` 与 `turn/start`：请求到达本机 Gateway 的 `/v1/responses`，携带实例专属 `X-RCodex-Instance-Key`，上游返回完整 `response.completed`，且模型路由保持为选定的公共 ID。这证明 App Server 数据面已经能读取实例模型目录并经 Gateway 完成一次真实请求。对当前桌面 bundle 的静态 renderer 逻辑核对显示，自定义 Provider 的可见性由 `hidden` 字段决定；生成器现已强制写入 `hidden=false`，降低目录被 picker 过滤的风险，但仍需真实 UI 点击验证。

Gateway 数据面已有两条无敏感 fixture 回归通过：本机 Codex 请求可经回环 Gateway 到达模拟 Grok Responses/Chat-only upstream，并核验模型改写、Provider/API Key 隔离和响应转换。随后 App Server 的 `thread/start` 与 `turn/start` 探针已证明桌面数据面可携带实例准入 Header 并完成一次 Gateway 请求；真实官方桌面 renderer 的点击触发仍需在签名桌面会话中验收。

桌面控制通道补充探针：当前 Computer Use 服务使用带调用方签名授权的 Apple Event（事件类型 `SkCu/SndR`，请求版本 `CodexComputerUseNativeBridge-1`），而不是可由任意本地进程直接调用的普通 Unix socket。使用正确请求类型查询 App 状态时，外部探针收到 `Sender process is not authenticated`；因此当前无图形环境下不能把窗口/模型 picker 的结果伪装成已验证。后续必须在已授权的 Codex/Computer Use 会话中执行真实点击，失败则继续关闭受管理 App 入口。

在随后可用的授权桌面会话中，R Codex renderer 已显示“启动受管理 Codex App”入口，证明环境闸门和前端可见性生效；发布版运行时探针会在打包 launcher、Gateway 与桌面可执行文件齐备时将 `managedAppAvailable` 置为 `true`。点击后启动实例曾因旧 Gateway 进程无响应而失败；清理该测试 Gateway 后重试，sidecar 能监听控制端口，但当前未授权签名会话仍可能在 macOS Keychain（系统凭据库）读取 Provider API Key 或实例密钥时失败，未产生桌面 App PID 或启动器日志。现在这些同步 Keychain 读取均移到阻塞线程并设置 15 秒上限，前端映射为明确错误提示，避免控制面健康检查和 UI 永久挂起；正式请求链路需在能完成 Keychain 授权的签名桌面会话重跑。

改名迁移补充：旧版正常退出仍可能留下 SQLite 的 WAL/SHM 旁车文件；新版本在没有旧 Gateway 标记时先执行只读源库的 WAL checkpoint，再复制数据库并保留源目录。新增回归覆盖“残留空 WAL 仍能迁移且源库不被删除”，避免改名后的首次启动被旧旁车文件误判为活动进程。

结论：默认 App 与一个受控 App 在当前 macOS 安装上可以同时存在，满足“不劫持系统图标、不把默认 App 伪装成受管理实例”的基础门槛；App Server 读取实例模型目录、携带实例准入 Header 完成 Gateway 请求和响应闭环也已有独立证据。发布版运行时探针会在打包依赖齐备时开放受管理 App 入口；真实上游请求仍受桌面签名会话和 Keychain 授权条件约束，失败必须有界返回并保持默认 App 不受影响。

## 2026-08-28 受管理 App 启动骨架

- `LaunchCodexInput.launchTarget` 现在支持 `cli` 与 `managed_app`；启动目标会写入运行实例的配置快照，便于后续恢复和诊断。
- CLI 启动可指定本次工作目录；受管理 App 不预先绑定工作目录，启动后由 App 自己选择任意目录。后端会忽略 `managed_app` 请求中的目录参数，避免误把 CLI 目录选择带入 App。
- `managed_app` 使用实例专属 `CODEX_HOME`、稳定的 `managed-app-data` Electron 用户数据根、根级 `config.toml`、版本匹配模型目录和 Gateway 实例密钥；不会向 App 注入 Provider API Key 或官方 OAuth token。
- 受管理 App 启动时只会从默认 `CODEX_HOME` 引入经过清理的非敏感偏好配置；不再链接或复制会话目录、归档会话、附件、线程索引、目标索引或记忆数据库。受管理根配置随后覆盖模型 Provider、目录和 Gateway 字段，因此模型/API 与历史写入均由运行方案隔离控制。
- 启动器通过与 bundled Codex CLI 同包的桌面可执行文件启动 App，并传递 `CODEX_ELECTRON_USER_DATA_PATH` 与 `--user-data-dir`；进程 PID、退出状态和启动日志沿用现有实例监督合同。
- macOS 的受管理实例保留长生命周期目录用于持久化和回收，但启动器会为 `CODEX_HOME` 建立实例专属短路径别名；这是为了避开 Unix socket 路径长度限制，修复 renderer 的 `listen EINVAL .../ipc/ipc.sock`，不改变配置、模型目录或凭据隔离。
- 受管理 App 只允许 Gateway 传输，不允许单渠道直连；默认行为由本机运行时兼容性探针决定。探针要求打包的 `codex-launcher`、`codex-gateway` 和可发现的桌面 Codex 可执行文件均存在；未通过时后端返回 `managed_app_unavailable`，前端入口保持禁用。`THREER_CODEX_DISABLE_MANAGED_APP=1` 可用于紧急回滚。
- 受管理 App 启动在准备阶段和最终 SQLite 写事务中都检查活动实例；最终事务持有写锁，因此两个不同 Provider 的并发请求也不会绕过“单个受管理 App”限制，冲突返回 `managed_app_in_use`。
- 启动耗时主要来自 Gateway 注册时的系统凭据库（Keychain）读取，以及 Codex App 自身的 app-server/桌面初始化；受管理 App 不会因 `OPENAI_API_KEY` 环境变量做额外 API Key 校验（该变量被移除）。凭据库授权若被系统阻塞，会受 15 秒有界超时保护并返回明确错误。
- Responses 兼容层会在发送到 Provider 前移除 Codex 可选的 `metadata` 字段。实测部分 OpenAI 兼容中继（包括当前 Grok 中继）会因该标准字段直接返回 HTTP 400；该字段不参与模型路由、会话恢复或工具调用，R Codex 自己的脱敏请求诊断仍会保留必要元数据。
- Provider 探针会解析中继包装后的错误正文；即使上游认证失败被包装成 HTTP 502，模型渠道也会显示“认证失败”。`/models` 成功但文本请求失败、协议未确认时显示“不可用”，避免把凭据问题误报为协议问题，也不把未确认渠道显示成部分可用。
- Gateway sidecar 现在先发布本地 ready 地址，再异步恢复旧实例 binding；系统凭据库读取不会再阻塞新实例连接。恢复中的旧 binding 会在内存注册完成后生效，损坏或无法授权的 binding 继续被隔离跳过。
- 控制面读取 Gateway 状态时对 ready 地址请求设置 2 秒总超时；因此上次异常退出留下的失效 ready 文件只会短暂显示不可用，不会让启动页永久停留在“正在读取状态”。
- 受管理 Gateway 启动会复用准备阶段刚生成的实例密钥内存副本，不再在启动器派发前二次读取 Keychain；密钥仍写入 Keychain 供异常退出后的恢复使用，减少系统凭据库响应对 App 启动时间的影响。
- 凭据引用从旧版稳定的 `provider:<id>` 演进为版本化的 `provider:<id>:revision:<revision>`。受管理 Gateway 仅在当前活跃修订缺少新引用时回读同一 Provider 的旧引用，并将其尽力回填到该活跃修订；已退休修订绝不回退到旧引用或新的当前修订，避免运行实例的凭据漂移。
- 受管理 profile 显式设置 `check_for_update_on_startup = false`，避免 Codex CLI 在每次受管理 App 启动时提示 CLI 更新；桌面 App 本体仍由系统安装渠道负责更新。
- 启动前目录校验已合并为一次 `debug models` 调用，同时完成目录解析和模型存在性检查，避免重复启动 Codex CLI；Profile 探针仍保留用于验证最终配置。
- 默认 `open_codex_app` 仍走无管理参数的 `codex app` 分支，不会被新的启动目标替换或劫持。
- **Failure**：探针失败时关闭受管理 App 能力并保留默认 App；不自动回退、不修改全局配置。
- **最新 smoke**：单实例桌面启动已通过：最新构建可在约 4 秒内进入 `running`，renderer 使用稳定的 `managed-app-data`，Gateway `/v1/models` 仅返回所选的外部模型。3333 渠道的旧 `provider:<id>` 凭据已被兼容恢复；实现会尽力回填到当前修订，但即使系统凭据库拒绝该额外写入，当前启动仍可安全使用旧引用。该中继的 Responses 透传返回 HTTP 400，但使用“Gateway 转换 Chat（文本）”方案时，Gateway `/v1/responses` 已经由 Chat Completions 适配成功（HTTP 200，约 1.5 秒）。该模式只承诺普通文本对话，不应向 Codex 宣称工具调用或 MCP 能力；后端仍明确拒绝第二个受管理 App，不扩展为多 App 并行。
