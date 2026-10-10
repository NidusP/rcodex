# R Codex 独立 Codex 工作区：产品改造与验收

> 决策已确认；代码、275 项后端测试、前端类型/构建、浏览器夹具验证与 macOS 应用包已完成。真实桌面合同仍需实机验收。  
> 更新：2026-09-22  
> 本合同取代默认 App / 隔离 App 双入口、默认配置投影、默认历史修复及本地压缩开关的旧方案。

## 1. 产品目标

R Codex 只管理自己的一套 Codex 环境。用户配置多个模型渠道，选择可用模型及其中一个 API 登录渠道，然后点击启动图标。主按钮无文字，hover 为「启动」。界面只有「R Codex 工作区」，不再要求用户区分默认 App 与隔离 App。

用户直接打开 Codex 时继续使用自己的环境。R Codex 的保存、状态查询、启动、停止和网关恢复均不得读取或改写用户原 config.toml、auth.json、Cookie 或 Electron 数据。源会话仅在用户主动选择导入时允许只读访问。

不做默认目录清理向导，也不自动还原旧版本曾经写入的兼容配置。重启新版本只切换产品启动路径，不表示历史写入已撤销。

## 2. 唯一启动路径

- 内部目标仅 managed_app / cli。Codex App 使用独立 CODEX_HOME、Electron 用户数据根和启动配置；`CODEX_SQLITE_HOME` 必须等于本次启动的 `CODEX_HOME`。macOS 受管理 App 的短路径在 Step 2 仍是 symlink（现状）；Step 5 目标是短真实目录，见归档专项。不得再把活动对话库放到 `managed-app-sqlite/<preset>`。CLI 使用独立实例目录，两个环境变量本来就是同一路径。归档修复见 [隔离 App 会话归档](2026-10-02-isolated-app-thread-archive.md)。
- macOS 为避免 Unix socket 路径超限，启动用短 `CODEX_HOME`（`.../r/<short-id>`）。当前实现仍是指向实例长目录的符号链接；Electron 归档不跟随 symlink，与 sqlite 长 `rollout_path` 字面不一致，隔离会话在拆库收回后仍可能无法归档。下一步是让短路径成为真实目录，见 [归档修复 Step 5](2026-10-02-isolated-app-thread-archive.md)。Profile 仍写 `allow_symlinked_codex_home = true`，以免残留链接被沙箱拒绝。
- 隔离 App 首次引导若选了非编程角色，Codex 会把 `composer-permission-mode-visibility` 写成 false。权限菜单于是只剩 Ask for approval，Change permissions 被禁用。受管理 App 每次启动只在已有 `.codex-global-state.json` 里把这个开关恢复为 true，不新建该文件，也不改已选角色。
- 新版 Codex 是否显示欢迎页，不看 `electron:onboarding-welcome-pending`。没有 ChatGPT 账号的桌面引导完成时间时，它只认 `electron:onboarding-projectless-completed`，或侧边栏里至少有一个工作区根目录。工作区根目录是已经打开过的项目文件夹，不是隔离目录，也不等于 `[projects]` 里的信任级别。默认目录通常靠这些项目路径跳过欢迎页；隔离目录不复制这些路径，所以空的新环境会被当成第一次使用。首次继承，以及之后同步「引导与权限偏好」时，只要默认目录已经用过，且来源没有显式写下这条新记录，就把隔离环境的 `electron:onboarding-projectless-completed` 写成 true。已经用过指欢迎待处理明确为关闭、已有主界面模式，或已有项目信任。不复制工作区根目录，不写入 `something_else` 或非编程角色。来源若已经写了这条记录，按原值复制，包括显式的 false。
- App 的会话根与工作区绑定，退出、重启后复用；每次启动重建不可变路由快照和准入绑定。登录、认证文件和历史对话不继承。每个隔离目录首次启动时，从用户默认 `~/.codex` 复制一次应用偏好和引导状态：`[desktop]`、`[features]`、插件开关、项目信任级别，以及 `electron:onboarding-*`、引导完成标记和主界面模式；同时复制用户根目录 `AGENTS.md`、`rules/` 和个人 `skills/`，保留隔离环境自带的 `.system` skills。不复制 MCP、marketplace、shell 环境、模型路由、Cookie、会话和草稿。复制后 R Codex 路由字段仍覆盖来源配置。之后隔离环境里的修改不再被默认目录覆盖。每次启动再把 `[features].multi_agent_v2` 写成布尔值 `true`。不保留嵌套表，也不写 `non_code_mode_only = false`：Codex 0.160 上那种写法会拿掉 `collaboration` 命名空间，`spawn_agent` 就不再是直接工具。 版本边界：Codex CLI `0.159.0-alpha.12.1` 只保留 `goals = true` 时，`spawn_agent` 仍是直接工具。从 `0.160.0` 起，嵌套表 `non_code_mode_only = false` 会拿掉整个 `collaboration` 命名空间；隔离配置因此必须写成布尔值 `true`。更早小版本未逐个复测。
- 主界面提供「打开默认 Codex」。macOS 对与当前 Codex CLI 对应的应用包执行 `open -n`，不传 `--user-data-dir` 或工作区路径，并去掉隔离环境变量。隔离 App 仍在时，Finder 普通点击只会聚焦隔离窗口；这个入口另起默认实例。再次点击共享默认 Electron 数据目录，由应用自己的单实例锁回到已有默认窗口。
- 查看实例按钮旁提供「打开CODEX_HOME目录」。隔离 App 的 CODEX_HOME 等同于默认环境的 `~/.codex`，存放 config.toml 和引导状态，不是应用包，也不是 Electron 用户数据目录。目录在该工作区首次启动后才存在；应用正在运行时仍可打开。前端只传启动方案 ID，后端核验目录属于 R Codex 实例根之后才交给访达。

### 设置继承清单

隔离工作区创建时可从默认 Codex 生成一次设置快照；之后默认环境的变化不会覆盖隔离环境。官网和未来“同步设置”界面可按以下分组展示：

| 设置区域 | 默认继承 | 存储位置 | 说明 |
| --- | --- | --- | --- |
| Personal / General | 是 | `config.toml` 顶层与 `[desktop]` | 通知方式、网络访问、响应存储开关、人格、推理显示、语言、对话详情、跟进队列等稳定偏好；R Codex 路由字段由 R Codex 自己覆盖。 |
| Personal / Appearance | 是 | `.codex-global-state.json` 原子状态 | 主题/界面显示、侧边栏折叠、侧栏宽度、文件树开关、模型选择器视图、编辑器换行等不含会话 ID 的 UI 偏好。显示器窗口坐标和窗口大小不继承。 |
| Personal / Permissions | 部分 | `.codex-global-state.json` 与 `config.toml` | 权限选择、agent mode 可继承；R Codex 会保持 Change permissions 可见，并按 R Codex 启动合同设置审批审核器、沙箱和本地 Gateway。具体项目的可写目录、线程级权限和历史授权不继承。 |
| Personal / Onboarding | 是 | `.codex-global-state.json` | 首次引导完成标记、角色/工作模式、引导提示关闭状态。默认目录已用过但没有新版完成记录时，补写 `electron:onboarding-projectless-completed = true`；不补非编程角色，不复制工作区根目录。 |
| Integrations / Plugins | 是 | `config.toml` `[plugins]`、`skills/` | 插件启用/禁用状态、个人 skills、默认 `AGENTS.md`、`rules/`；隔离环境已有 `.system` skills 保留。插件安装目录只复制无链接的用户内容。 |
| Integrations / Browser | 部分 | `config.toml`、Electron 数据目录 | Browser/Chrome 插件开关和无账号的 UI 偏好可继承；浏览器 Cookie、登录、历史、书签、站点权限、缓存、浏览器 profile 和打开的标签页不继承。 |
| Integrations / Computer Use | 部分 | `config.toml`、隔离 `computer-use/` 目录、MCP/Electron 数据目录 | Computer Use 插件开关和能力显示偏好可继承。R Codex 为隔离目录生成指向其自身 `computer-use/Codex Computer Use.app` 的本地 MCP 配置；客户端授权、设备绑定、运行时 socket、进程状态和绝对路径不继承。 |
| Integrations / MCP | 否（默认） | `config.toml` `[mcp_servers]` | MCP 命令、环境变量、token、证书和本地路径可能执行本地程序或泄露凭据；需未来按服务器逐项导入，并在 UI 明确确认。 |
| Integrations / Marketplaces | 否（默认） | `config.toml` `[marketplaces]` 与缓存 | 来源可能包含本地路径、Git 凭据和可安装代码；只保留 R Codex 自带 marketplace。 |
| Models / Routing | 否 | R Codex 私有 Profile、RouteSet | 隔离实例必须使用 R Codex 的 Provider、模型目录、Gateway 和准入密钥；不复制默认模型路由或官方 Provider。 |
| Account / Login | 否 | `auth.json`、Keychain、Electron 数据 | 不复制 OAuth、API Key、Cookie、登录数据库或账户身份。API Key 仅按 R Codex 工作区登录渠道写入 R Codex 自有 `auth.json`。 |
| History / Sessions | 否 | SQLite、sessions、rollouts | 不复制历史对话、草稿、线程映射、浏览器会话或请求正文；项目导入是用户主动发起的独立单向流程。 |

当前实现已经继承插件启用状态、个人 skills、`AGENTS.md`、`rules/`，并补齐了稳定的 General、Appearance、Onboarding 原子偏好；Browser 和 Computer Use 的账号/运行时数据仍按上表隔离。
当前播种只挂在隔离 App 启动路径；隔离 CLI 实例只有不可变 profile、没有 `config.toml`。CLI 对齐同一白名单的合同见 [隔离 CLI 继承默认环境策略](2026-10-09-isolated-cli-preference-inheritance.md)。

### 2026-09-23：隔离 Computer Use 与按模型推理档实施计划

## Goal

- 用户可在隔离 App 中启用 Computer Use；首次使用由 macOS 在该隔离环境中授权，后续重启复用隔离目录的本地服务，不读取或复用默认 Codex 的服务、socket、Cookie、账户或配置。
- 工作区可为每一条「渠道 × 上游模型」选择经验证的推理档位和默认档；生成的 Codex 模型目录与 RouteSet 使用同一份保存值。

## Contract

- 本地 Browser/Computer Use 是一个例外 MCP：只允许生成固定的、同一隔离 `CODEX_HOME` 内的 `node_repl`/`cua_repl` 服务环境，并把 `SKY_CUA_SERVICE_PATH` 和 `CODEX_HOME` 指向隔离目录；`notify` 也必须指向隔离 `SkyComputerUseClient`。不导入默认 `[mcp_servers]`，不启用 `openaiDeveloperDocs` 或任何远程/用户自定义 MCP。若隔离 helper 缺失，启动前从已解析的桌面应用包（`ChatGPT.app` / `Codex.app`）把签名 `Codex Computer Use.app` 复制到隔离 `computer-use/`；复制源不得为 `~/.codex/computer-use`。已有隔离 helper 不覆盖。复制后仍找不到隔离 helper 则返回明确错误，不回退默认目录。
- `launch_preset_model_selections` 新增可空 `reasoning_levels_json` 与 `default_reasoning_level`。空值沿用模型族默认；非空值仅可用于 Responses 路由，必须去重、是已知 effort 标签，且默认值必须属于所选数组。Chat Completions 仍写空数组，因为适配器不转发 `reasoning`。
- 写入只发生在现有工作区保存事务；启动时把持久选择解析进不可变 RouteSet，再由目录生成器物化。Provider 发现 `/v1/models` 不获得写入权，也不推断档位。

## Acceptance And Verification

- 配置生成测试证明本地 Computer Use 使用隔离路径、未出现默认路径或其它官方 MCP；隔离 helper 可从桌面应用包安装，已有隔离 helper 不覆盖，默认目录不得作为复制源；缺失 helper 明确失败。
- 迁移测试证明旧行可读为空，保存/读取和 RouteSet 物化能保持每个 Provider 模型的不同档位；非法默认档、重复档和 Chat 路由被拒绝或清空。
- Rust 定向测试、`cargo fmt --check`、`pnpm run check` 与前端生产构建通过。真实桌面验收由 commander 在重启开发服务后完成：首次授权、打开 Safari、重启后再次调用，以及每个模型的档位下拉与请求行为。

## Risk And Rollback

- Computer Use 能控制本机 UI，首次 macOS 授权必须由 commander 完成；R Codex 不自动接受系统提示。回滚为删除 R Codex 生成的本地 MCP 节，插件开关可继续存在。
- 错误广告推理档会被上游忽略或返回 400。UI 只允许模型族给出的验证档位；未来 Provider 特定能力资料可扩展同一验证表。schema 采用新增可空列，回滚代码后旧列仍可安全忽略。

隔离工作区卡片提供「按清单同步设置」入口。用户可逐项选择 General、Appearance、引导与权限偏好、Plugins、个人指导文件和项目权限信任；操作只更新选择的安全白名单，不改变 R Codex 路由字段，也不复制登录、历史、MCP、marketplace、浏览器或 Computer Use 账号/运行时数据。同步是显式操作，可在首次继承之后再次执行。
- 所有新会话 Provider 身份固定为 rcodex。工作区可选择多个渠道模型，Gateway 按稳定公共模型 ID 精确匹配上游地址与密钥；无默认渠道回退、无认证头透传。
- 模型目录来自当前 Codex 的内置模板和工作区选择。读取版本/内置目录的探针也指定独立临时目录，清理继承的认证环境变量。每个第三方模型条目保留当前 Codex 要求的 `supported_reasoning_levels` 结构；没有已验证推理档位时写空数组且不写默认档位，不能通过删字段或复制 GPT 档位规避兼容性。生成目录按模型渠道名称、模型名称和公共模型 ID 稳定排序；`priority` 只反映目录顺序。默认模型另写入 R Codex 私有 Profile 的 `model` 字段，不能因为展示排序变化。R Codex 界面使用同一排序规则，但保存的首个模型仍是默认模型。
- 启动前会以隔离目录校验生成的模型目录和 Profile。校验失败会写入该次隔离实例的私有 `startup.log`，记录阶段、退出状态、字节数和内容哈希；标准错误仅保存限长、去 URL 且去凭据后的摘要，不记录 Key、认证头、完整 URL 或请求正文。
- 本代隔离启动延续 LocalClient 压缩合同，显示名为 R Codex Gateway；没有「开启本地压缩」开关。不把历史默认 App 的远程压缩验收当成本代桌面验收。
- 活动实例锁定依赖的渠道、模型和工作区。配置变化只在退出后、下次启动生效。状态不明时显式报错，不回退启动原 Codex。
- 官方账户登录和订阅模型经网关仍关闭；不读取或接管原 Codex 登录态。

## 3. API 登录

工作区保存 loginProviderId，只能指向已加入的渠道。单渠道可自动采用该渠道，多渠道必须明确选择；移除已选渠道时清除旧选择。变更使用版本校验和运行锁。

启动时从系统凭据库读取所选渠道 Key，只写 R Codex CODEX_HOME/auth.json 的 OPENAI_API_KEY。文件为私有权限，原子替换，拒绝目标链接；配置指定文件凭据存储，避免共用默认登录。写入失败撤销本次启动绑定并释放锁。

这份登录用于客户端初始化。真正模型请求仍由 Gateway 按模型选择对应渠道 Key，不能把登录渠道 Key 当成所有渠道的凭据。除这份 R Codex 自有登录文件及原有受管理直连边界外，Key 不进入普通数据库、前端、日志或请求档案。

## 4. 项目会话导入

首版只提供「我的 Codex → R Codex」，按项目整体复制，不双向同步，不导出回原环境。

1. 用户主动点击扫描；面板首次打开不读取原历史。源目录取当前 CODEX_HOME（未设时为用户 .codex），选择其中最新 state_N.sqlite。不会读取 config.toml 推断自定义数据库位置。
2. 把数据库及日志复制到私有临时目录，再查询项目、会话标题、时间、Provider 和路径。原数据库、WAL、SHM 保持字节不变，正文不进入预览。
3. 选择一个项目，预览该项目全部会话，确认原 Codex 和 R Codex App/CLI 已退出。后端再次检查进程；R Codex 不代关用户进程。进程无法确认或尚在运行时拒绝。
4. 目标必须已经由相同版本 Codex 初始化，threads 列结构一致；首版不自建原生 schema。先启动 R Codex 创建一次会话并退出，再导入。
5. 只复制 `sessions` 中的活动会话 JSONL 至目标 `sessions/imported`；`archived_sessions` 不形成项目列表，也不参与导入。副本首条 `session_meta` 的 Provider 改为 `rcodex`，正文其余字节保留。同 ID 且内容未变化的会话跳过；同 ID 但源 rollout 内容变化时生成新的隔离 thread ID 和 rollout 副本，保留原隔离副本，不覆盖历史。
6. 目标索引在事务中写入，Provider 和 rollout 路径指向目标副本；源 project_id / thread_section_id 不带入。失败回滚新增行并移除本次新文件，重试仍不会覆盖已有线程。
7. 拒绝源路径越界、链接、源/目标重叠、非法元数据及目标数据库日志链接。启动和导入在 R Codex 进程内互斥，耗时扫描/复制在后台线程执行。

导入结果显示复制数、未变化跳过数和冲突副本数。复制成功不保证跨渠道续聊兼容；推理密文、工具状态和附件仍受目标模型合同限制。项目文件及附件引用保留原路径，首版不搬运外部附件。自定义 CODEX_SQLITE_HOME 不自动发现。进程检测保守，其他 Codex 实例也需退出。

非正常终止可能留下未入库的目标副本；它们不会覆盖原会话，重试使用新的目标文件。首版不提供跨进程热同步或故障注入后的自动垃圾回收。

## 5. 删除与保留

删除默认配置快照/投影/恢复、旧会话 Provider 原地修复、默认 App UI、旧兼容启动和网关默认上游回退。旧 schema 表保留识别能力，不作为可用功能。

schema v18 只更新 R Codex 自有数据库：停用历史兼容绑定，释放对应运行锁，将旧兼容运行记录置为退出，并保存模型选择的推理档位；不读取旧回执、不检查其目录、不发送进程信号。网关恢复只接受独立 CLI/App 绑定。sidecar ready 带协议版本；已运行的旧网关通过控制接口撤销旧兼容绑定，无独立活动绑定时正常停止并在下次启动新版，有独立活动实例时保留其转发。该升级仅操作 R Codex 自有服务。旧配置仍在用户目录时不自行处理。

保留模型渠道、模型目录、不可变路由、启动方案、系统凭据库、普通诊断、用量和工作区级完整请求档案。删除结束的受管理 App 实例时，删除其生命周期目录、专属 `CODEX_HOME`（含已对齐的对话索引）和 R Codex 专属 Electron 数据；若仍存在旧的 `managed-app-sqlite/<preset>` 残留目录，作为 best-effort 一并删除。CLI 只删除其生命周期目录。工作区级模型渠道/API Key、启动方案和请求档案不随单个实例删除。完整档案在运行诊断中单独开启、读取和清空；正文按阶段分页，不进入控制面全局状态。

网关伴随进程与 UI 生命周期分离：有活动绑定时关闭 R Codex 窗口不意味着网关退出。停用或删除运行实例仍遵循既有生命周期锁。

## 6. 修改范围

- 前端：App.tsx、SessionImportPanel.tsx、ConversationArchivePanel.tsx、codex-control.ts、dev-control-fixture.ts、styles.css。
- 启动与认证：lib.rs、managed_launch 及 validation、managed_codex_config.rs、isolated_auth.rs、codex-launcher.rs。隔离 App 归档布局见 [2026-10-02 专项](2026-10-02-isolated-app-thread-archive.md)。
- 导入与生命周期：session_import.rs、database.rs、codex_control.rs。
- 网关：managed_gateway.rs、runtime_types.rs、sidecar.rs、codex-gateway.rs、routing.rs。
- 删除：history_provider_repair 模块、HistoryProviderRepairPanel、旧 ConversationHistoryPanel（档案功能收敛到独立面板）。
- 同步：AGENTS、README、产品原理、混合路由设计/执行计划和 UI 专项设计。旧默认 App/修复文档标记为历史。

## 7. 验收记录与发布边界

自动化覆盖：API 登录选择与移除、版本冲突及锁、隔离配置重启合并、配置链接拒绝、独立运行目录复用、旧兼容元数据迁移与绑定拒绝；项目导入源字节不变、重复跳过、源 WAL、链接/越界、schema 不匹配、元数据失败及写入失败回滚；多渠道网关认证、协议、取消、超时和诊断。

界面使用本地 ui-fixture 验证：单工作区、图标启动及 hover、登录选择、显式扫描、项目预览、未确认禁用、成功/失败提示、请求档案四阶段入口及正文预览；未访问真实源会话或真实 Key。

最终命令和结果记录在[混合路由执行计划](../codex-hybrid-routing-execution-plan.md)最新条目。真实 macOS Codex App 登录、多渠道连续对话、导入后列表与恢复、Windows 打包/ACL/启动仍需分别实机验收；不能用夹具通过替代这些合同。

## 8. 回滚与下一步

回滚仅涉及仓库代码、生成包及 R Codex 自有数据。升级前应由正常应用备份保留 R Codex 数据；schema v18 后旧二进制可能拒绝打开新库，不能把降级二进制等同于数据回滚。不还原用户原配置，不提供清理向导。

commander 重启新构建的 R Codex，配置模型与 API 登录渠道，启动 Codex 创建会话；退出双方 Codex 后按项目导入，再重启 R Codex 核验旧会话。全过程不需要交出原账户登录。
