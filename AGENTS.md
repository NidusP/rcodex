# R Codex AI 编程总览

本文件是 R Codex 项目级 AI 编程入口。它把产品原理、架构设计、执行计划和专项文档中的稳定约束集中起来。修改代码前先读取本文件，再按任务类型读取对应的详细文档；若详细文档与本文件冲突，以较新的产品决策、执行记录和用户明确要求为准，并同步修正文档。

## 项目定位

R Codex 是专为 Codex 做的本机路由转发工具，把多个模型服务商聚合成一份目录，经本机 Gateway 按模型精确转发。目标平台是 macOS 与 Windows。技术 slug 是 `rcodex`；`3r-gpt` 和 `3rcodex` 只作为升级迁移兼容来源保留。显示名与技术 namespace 的执行计划见 [R Codex 品牌与技术命名空间改造](docs/designs/2026-10-04-r-codex-display-rename.md)。

当前产品基线：

- 纯 API Key 多 Provider、多模型的受管理 CLI/App 路由已实现。
- 受管理运行都经过本机回环 Gateway。DirectResponses 直连路径已删除。
- Gateway 使用独立 `codex-gateway` sidecar（伴随进程），只绑定 `127.0.0.1`，不提供 LAN 或公网入口。
- 每个受管理实例使用独立 `CODEX_HOME`、模型目录、Profile、RouteSet 和实例准入密钥。
- 产品只保留 R Codex 工作区，启动 App/CLI 均使用独立配置、认证和会话目录。不得读取或修改用户原 config.toml、auth.json、Cookie、Electron 数据；仅用户主动项目导入允许只读源会话。当前合同见 [独立工作区改造](docs/designs/2026-09-21-isolated-only-product-plan.md)。
- 官方订阅模型经自定义 Gateway 的真实协议和账户合同仍受发布门限制，不能默认打开。
- 产品目标包含 macOS 与 Windows。发版矩阵产出 macOS Apple Silicon、macOS Intel 和 Windows x64 安装包。Windows 包能在运行器上构建，安装和打开仍需 Windows 桌面验收。

产品与架构的完整背景见：

- [README](README.md)：当前实现、运行路径、开发和打包入口。
- [产品原理](docs/3rcodex-product-principles.md)：用户承诺、所有权、安全边界和会话规则。
- [混合路由详细设计](docs/codex-hybrid-routing-design.md)：领域对象、SQLite schema、IPC、Gateway 和启动合同。
- [混合路由执行计划](docs/codex-hybrid-routing-execution-plan.md)：分片进度、验收证据、阻塞项和后续任务。
- [独立工作区改造计划](docs/designs/2026-09-21-isolated-only-product-plan.md)：当前产品合同、单向导入、API 登录与发布验收。默认 App 快速发版计划已作历史记录。
- [官网计划](docs/website-plan.md)：Astro + Starlight + Solid.js 官网内容映射、构建和 Pages 发布约束。

## 文档读取顺序

按任务选择最少的一组文档：

| 任务 | 首先阅读 | 再阅读 |
| --- | --- | --- |
| 独立启动、API 登录、项目导入 | `docs/designs/2026-09-21-isolated-only-product-plan.md` | 产品原理、混合路由设计 |
| 按会话记录完整请求、转发链路优化 | `docs/designs/conversation-request-archive-design.md` | 产品原理、Gateway 诊断路线、混合路由执行计划 |
| 产品、模型切换、会话恢复 | `docs/3rcodex-product-principles.md` | `docs/codex-hybrid-routing-design.md`、`docs/codex-hybrid-routing-execution-plan.md` |
| Provider、模型目录、协议转换 | `docs/codex-api-protocol-compatibility-plan.md` | `docs/third-party-model-reasoning-levels.md`、`docs/codex-hybrid-routing-design.md` |
| Gateway、502、超时、诊断 | `docs/codex-roadmap-and-gateway-diagnostics.md` | `docs/compat-app-loopback-proxy.md`、README 的本地网关章节 |
| 默认 App、受管理 App、启动隔离 | `docs/codex-app-support-research.md` | `docs/codex-auth-launch-execution-plan.md`、混合路由详细设计 |
| 认证、Keychain、实例锁 | `docs/codex-official-auth-authorization-design.md` | `docs/internal-edition-execution-plan.md` |
| UI、布局、交互状态 | `docs/designs/3rcodex-ui-quality-upgrade-plan.md` | `docs/designs/3rcodex-ui-redesign.md`、README |
| 品牌、路径和迁移 | `docs/product-and-app-renaming-plan.md` | README、产品原理 |
| Grok 看图/生图、目录能力泄漏、默认 App compact 信封、旁路 luna | `docs/designs/2026-09-19-grok-session-followups.md` | `docs/designs/2026-09-19-grok-session-error-todos.md`、[F 当前模型族-only](docs/designs/2026-09-21-f-family-only-ephemeral.md)、第三方目录 overlay、混合路由执行计划 |
| Grok 默认 App 长会话压缩（E1 选项 1 已锁定） | [混合路由详细设计 §8.2.1](docs/codex-hybrid-routing-design.md) GatewaySummarizer 第 3 条 | 执行计划文首「2026-09-20」交接；真实 compact 验收见 followups D。不要和 A/B/C 混票 |

执行计划是状态记录，不是新的架构合同；它的“已完成、待开始、阻塞和下一步”必须与实际代码和测试结果同步。

## Docs-first 工作流

所有会改变持久行为、API、配置、数据流、架构、用户流程或长期任务的工作都必须遵循：

1. 先定位产品原理、详细设计和执行计划中的相关条款。
2. 若需求尚未定义，先在现有 Markdown 中补目标、范围、非目标、约束、验收、风险和回滚点。
3. 再实现代码，保持单一写入路径和现有所有权边界。
4. 按风险验证成功、失败、恢复和安全路径。
5. 把实际结果、偏差、未完成项和下一步写回执行计划或专项文档。

微小的拼写、只读调查和一次性命令不需要创建计划；只要行为或配置有持久影响，就不能只改代码不改文档。修复 bug 时，优先补充可重复的回归测试或明确的复现检查。

## 架构与写入边界

### 路由

- `Provider` 是模型渠道；一个 `LaunchPresetComposition` 可以引用多个 API Key Provider 和多个模型。
- `ModelRouteIdentity` 保存稳定的公共模型身份；`RouteSetRevision` 是启动时固化的不可变路由快照。
- Gateway 先按公共模型 ID 精确匹配 RouteSet，再替换为 `upstream_model_id`，读取绑定的 Provider revision 和凭据。
- 第三方请求必须删除官方认证，只在当前请求边界从 R Codex 自有 SQLite `provider_credentials` 表读取对应 Provider revision 的 API Key；运行时不读取系统凭据库。
- 不按模糊模型名猜渠道，不因 401/502 自动换地址、换模型、换 Provider 或重试 POST。

### Gateway

- 数据路径是 `Codex → 127.0.0.1 Gateway → Provider`；UI 不承载模型 token 流量。
- Gateway 只接受回环请求和当前 binding 的准入密钥；sidecar 与 UI 生命周期分离。
- GPT 族 Responses 原样转发请求体（仍替换公共模型 ID、注入该渠道 Key、去掉官方认证）。非 GPT 才做中继兼容：删 `metadata`、去掉渠道不支持的可选工具、把 `agent_message` 收成公开 user 消息。Responses→Chat 文本转换仍按 `CapabilitySet` 宣布能力；不伪造工具、图片、MCP、推理或多轮续接兼容性。
- 上游非 2xx 属于 `upstream` 层；本地路由、准入、网络或解析错误属于 `gateway` 层。错误响应保留 `gateway_code`、可选 `upstream_status` 和脱敏 `upstream_detail`。
- 普通诊断和用量记录是 best-effort，不得阻断模型请求；不得记录 API Key、实例密钥、Authorization、请求正文、响应正文或完整敏感 URL。用户显式启用的完整请求档案是独立存储例外，当前已接入首版，按专项设计维护，不能把正文加入普通日志或用量表。

### 配置与实例

- 受管理 CLI/App 使用实例专属 `CODEX_HOME`、`CODEX_SQLITE_HOME`、Profile 和模型目录；受管理 App 的 `CODEX_SQLITE_HOME` 必须等于启动 `CODEX_HOME`，活动对话库不得再放到 `managed-app-sqlite/<preset>`。启动时生成不可变快照。运行中的实例继续使用那份快照。工作区可以修改渠道、模型、排序和 API 登录渠道，下次启动才生效；保存后提示「当前隔离实例正在运行，重启可应用修改」。
- 默认 App 兼容投影/恢复已移除，不做清理向导。schema v18 仅更新 R Codex 自有库，停用旧兼容绑定和运行锁并保存模型推理档位，不操作用户文件或进程。
- 默认 App 不创建 `codex_instances`，不取得运行实例锁，不向默认 App PID 发送停止信号。
- 不读取原登录或复制 OAuth token。工作区选定的 API 登录渠道 Key 可写入 R Codex 自有 auth.json（0600、原子替换、文件凭据存储）；请求认证仍按精确路由取渠道 Key。不得注入用户直接打开的 Codex。

### 会话与模型切换

- 所有受管理任务保持 `model_provider_id = rcodex`；不要在恢复、回滚、重放时恢复成 `openai`。
- 跨 Provider 切换发生在新请求边界。历史中的 `previous_response_id`、`reasoning.encrypted_content`、图片、工具调用关系、item `id/status/phase` 和 Provider 私有状态都必须通过目标 Provider 合同才能复用。
- 不在运行中的 SQLite、rollout 或会话历史上静默删字段、改 Provider 或降级原模型。
- 当前待办是“三方模型切换后的旧会话兼容性”：按 `docs/designs/conversation-request-archive-design.md` 先修转发正确性、建立原生会话关联及四阶段请求档案，再以档案副本做受控对照；根因未确认，不自动改写原历史。

## 独立 App 与回环代理

- 所有产品启动均为独立环境；不允许以旧兼容模式或用户默认配置作失败回退。
- R Codex 启动进程补充回环 NO_PROXY/no_proxy，不改变系统代理或 Shell 设置。
- 默认配置修复、原生上游 fallback 和官方认证头转发已删除；旧兼容回环代理文档只保留历史排障依据。

## 前端与交互规则

- 主界面只有 R Codex 工作区；主按钮只显示图标，hover「启动」。模型渠道、运行实例和诊断按需打开；项目导入在主界面显式扫描。
- UI 展示模型来源、能力、路由和错误层级时必须使用用户可理解的文案；内部 ID 只用于精确关联。
- loading、空态、失败态、冲突态、禁用态和成功反馈都要有明确状态，不以静默刷新或无内容页面代替。
- Provider、工作区、模型目录和启动动作遵循现有单一写入事务；不要在 renderer 中直接修改 SQLite、配置文件或凭据。
- UI 视觉和交互改造先更新 `docs/designs/` 中的方案与验收，再修改 `src/`。

组件定义：

- 业务与可复用界面放在 `src/components/<feature>/`。`index.ts` 是唯一对外出口；内部文件不要被其他功能目录直接引用。跨功能纯函数可以留在 `src/components/*.ts`，不要为单个 helper 再套空目录。
- `src/main.tsx` 与 `src/App.tsx` 只做根组装，不要在根组件里写抽屉或弹层 JSX。仍留在 `src/` 根下的面板下次改动时迁入对应功能目录，不要继续扩大这些文件。
- React 组件写 `@component` 名称和 `@description` 用途。Hook 用 `@function`。纯函数、类型和常量用普通 JSDoc，不要标成组件。
- 按钮、菜单、列表、Tab、选择器、通知、抽屉、对话框优先用 Mantine，再用现有 className 覆盖外观，不要再写一套原生弹出层。控制面抽屉走 `ControlDrawer`：避开标题栏、带遮罩。确认和启动弹层走 `ControlDialog`。展示组件通过 `useAppState()` 或父级回调读写，不直接 `invoke` 业务 command，也不改 SQLite 或凭据文件。

## 安全与隐私

- 渠道 API Key 以 R Codex 自有 SQLite `provider_credentials` 表为事实来源，数据库文件保持当前用户私有权限；运行时不读取系统凭据库。所选 API 登录 Key 仅允许额外投影至 R Codex 自有 auth.json。实例准入密钥不是用户凭据，不进钥匙串：SQLite 只保存引用和哈希，明文只经内存与一次性 FIFO 传递。日志、导出、IPC 快照和 UI 状态不保存原文。
- 官方 Authorization 不得发送给第三方 Provider；第三方 Key 仅用于其对应渠道请求与明确选择的 R Codex 独立 API 登录，不得发送给无关上游或用户原 Codex。
- 不读取原配置或登录。显式项目导入是源会话只读的限定例外：预览元数据、确认停机，复制到 R Codex 后只改副本 Provider 和索引路径；源正文/数据库不写入，凭据不复制。原地会话修复已移除。
- “完整请求记录”允许将经 R Codex 转发的正文存入本机独立档案并展示，原生历史只读元数据用于关联；不回填旧正文、不保存认证凭据、不改写原历史。首版已接入四阶段采集、容量、故障降级和清理合同，细节见 `docs/designs/conversation-request-archive-design.md`。
- 普通诊断增强仍采用形状、计数、哈希和枚举值；完整档案不得进入普通日志、遥测、控制面全局快照或修复备份。
- 对外网络请求必须有边界、超时、错误分类和最小响应解析；真实 Provider 探针可能产生费用，默认使用 mock 或无敏感 fixture。

## 开发、测试与打包

- 日常开发中，AI 不运行 `pnpm run tauri:build`、任何 Tauri 打包命令或 `codesign`；只有 commander 明确要求生成或校验发布包时才可执行。
- 应用在线更新见 `docs/designs/2026-10-09-app-updater.md`。不要提交 `work/updater.key`。发版仓库名在 `src/app-release.ts` 与 `tauri.conf.json` updater endpoints，必须一致。
- commander 自行运行 `pnpm run tauri:dev`。AI 不启动、停止或重启 Tauri 开发服务，除非 commander 明确要求。

在仓库根目录：

```bash
pnpm run check         # TypeScript 类型检查
pnpm run build         # 前端生产构建
pnpm run tauri:dev       # 构建 sidecar 并启动 Tauri 开发窗口
pnpm run tauri:build --bundles app
```

在 `src-tauri/`：

```bash
cargo fmt --all -- --check
cargo check --all-targets
cargo test --all-targets
```

验证深度按风险选择：

- 领域或纯函数变更：针对性 Rust 单测和格式/编译检查。
- Gateway、协议、异步流、诊断、迁移或生命周期变更：覆盖成功、上游失败、本地失败、取消、超时、恢复和隐私边界。
- UI 变更：TypeScript 检查、生产构建和真实浏览器的主要成功/失败路径。
- 启动、默认 App、凭据文件或真实 Provider：记录平台、Codex 版本、配置来源和手动验收结果；不能用 mock 结果代替真实合同验收。

Tauri 构建脚本会先构建 `codex-launcher` 和 `codex-gateway`，再嵌入 `dist`。遇到原生包仍显示旧页面时，按 README 说明清理 Cargo 构建缓存后重试。生成的 macOS 包当前可能是 ad-hoc 签名，不能当作 Developer ID 公证发布包。

## 当前未完成项

以 [独立工作区改造](docs/designs/2026-09-21-isolated-only-product-plan.md) 和路由执行计划的最新验收为准：

- 真实 Codex App 的独立 API 登录、多渠道连续会话、重启与导入后恢复验收；不得读取默认认证验证。
- 隔离 Browser/Computer Use 的本地 `node_repl`/`cua_repl` 依赖已补齐代码，但签名桌面授权、工具实际出现和官方 entitlement 门控仍需实机记录；不能把 Settings 的组织/地区提示直接归因于默认配置继承。
- Windows 打包、启动目录和凭据文件权限，签名/公证桌面发布与凭据文件权限验收。
- 官方账户与订阅模型路由仍关闭；不接管原 Codex 登录态。
- 跨渠道旧历史的字段兼容性和旁路模型须按真实合同验证；不能通过改源历史、自动换渠道或回退默认上游修复。
- 压缩无用户开关；独立启动沿用 LocalClient，Provider 为 rcodex，显示名为 R Codex Gateway。旧默认 App remote_v2 / Summarizer 记录只作历史，不恢复兼容入口。 客户端上下文检查点的续跑由 Codex 自己组包；R Codex 不修正其中的旧任务偏移，见 [隔离实例的上下文压缩](docs/designs/2026-10-08-isolated-context-checkpoint.md)。
- Gateway 重启时的流中途续接；项目导入外部附件搬运、自定义源 SQLite 位置和异常中止遗留副本清理不属于首版。


## 变更交接

完成一个任务时，在对应执行计划中记录：变更目的、修改文件、实际验证命令和结果、未完成项、阻塞原因、回滚方式及下一步命令。不要把“命令未启动”“网络被阻断”写成测试通过；不要在没有证据时把推测写成根因。后续 AI 应能只读本文件、相关计划和最后验证记录后继续工作。

## UI 分步改造留痕

按 demo.html 的布局和样式改真实控制面，不挂载静态演示页。详细验收在 docs/designs/3rcodex-ui-quality-upgrade-plan.md。崩溃后续读本节即可接着做。

- 第 1 步完成：暗色色板、等宽标签、网格和径向光。亮色未改。
- 第 2 步完成：顶栏含品牌、真实 Gateway 状态、四个页面、真实 CLI 与 App 版本。无窗口圆点、PING、TLS。src/App.tsx、src/workspace-surface.tsx、src/surface.css。
- 第 3 步完成：标题行左侧是「只管理独立环境」，右侧只在有真实最近失败时显示，点击打开诊断。没有失败时右侧留空。工作区卡片内不再重复失败条。提示条改为跟随内容，避免挡住失败条。src/App.tsx、src/surface.css、src/dev-control-fixture.ts。
- 第 4 步完成：工作区在左侧卡片，标题为独立隔离空间。确认后才写入、未观测、图标启动、锁定后查看实例都还在。更多菜单仍是打开隔离目录、按清单同步设置、打开默认 Codex、导入项目会话。右侧暂放已加入渠道、默认模型、API 登录、活动实例。src/App.tsx、src/workspace-surface.tsx、src/surface.css。
- 第 5 步完成：右侧四张入口卡。工作区配置打开现有启动入口抽屉；模型渠道、实例、诊断打开现有页面。计数与顶栏一致。抽屉不显示 API Key 原文，没有假终端。src/App.tsx、src/workspace-surface.tsx、src/surface.css。
- 第 6 步完成：右侧最近诊断只列真实事件和请求，最多 6 条；没有记录时为空状态。页脚是真实 Gateway 状态、监听地址、CLI 与 App 版本。没有内存、磁盘、PID 或倒计时。src/App.tsx、src/workspace-surface.tsx、src/surface.css。
- 第 7 步完成：Gateway 状态点只在运行中脉冲。停止、不可用，以及系统开启减少动态效果时都不播放。src/surface.css。
- 2026-09-29：移除顶栏 surface-nav。模型渠道、实例、诊断改为右侧抽屉，工作区保持可见。src/App.tsx、src/workspace-surface.tsx、src/surface.css。
- 渠道行按设计分色：青、琥珀、品红循环。悬停时边框、左侧色条和名称跟着变。src/workspace-surface.tsx、src/surface.css。
- 渠道行可拖拽排序。顺序写入渠道 position，模型目录按同一渠道顺序生成，渠道内模型相对顺序不变。锁定时不能拖。src/App.tsx、src/workspace-surface.tsx、src/surface.css。
- 默认调用模型和 API 登录渠道改为自定义菜单，不再用系统下拉框。src/workspace-surface.tsx、src/surface.css。
- 已加入渠道标题与添加渠道按设计稿排成一行：等宽小节名、浅青标题、真实可用/待命计数。添加渠道是自定义菜单，选中后仍要确认才写入。启动入口的加入渠道，和诊断的渠道筛选，用同一套菜单。不编造延迟。src/workspace-surface.tsx、src/App.tsx、src/surface.css。
- 启动时全屏 R Codex。背景是毛玻璃，粒子只在 R Codex 字形里流动。R Codex 下方一行小字「正在获取本机状态...」。读完本地状态后关闭，至少停留约 0.7 秒。减少动态效果时粒子静止。src/boot-loading.tsx、src/App.tsx、src/surface.css。
- 独立隔离空间卡片按设计稿恢复背景和边框：近黑玻璃底、青色细边、顶边青到品红的亮线。标题区和底栏分开，中间渠道区不再套一层边框。
- 渠道拖拽改用 dnd-kit。拖动时行会让开并跟随指针，松手后才保存顺序。src/channel-sort-list.tsx、src/workspace-surface.tsx。
- 2026-09-30：现有顶栏收进窗口标题栏，下面单独放设计稿 header。标题栏保留品牌、真实 Gateway、CLI/App 版本、主题和刷新。macOS 用覆盖式标题栏和系统三色按钮；Windows 关掉系统标题栏装饰，由页面绘制最小化、最大化和关闭，关闭仍只隐藏窗口。设计稿 header 只有「模型渠道」和「全域诊断」。失败提示改为不占高度的固定气泡。删除右侧入口坞和最近诊断。空间标签只显示当前隔离空间，「新建空间」禁用。src/App.tsx、src/workspace-surface.tsx、src/surface.css、src-tauri/tauri.conf.json、src-tauri/src/lib.rs。
- 2026-10-01：macOS 红黄绿改为在 TaoView 与 WryWebViewParent 的 drawRect: 之后居中，并对本窗口标题栏容器 / NSTitlebarView 做 layout 挂钩。挂钩 NSTitlebarContainerView.drawRect: 无效。src-tauri/src/macos_titlebar.rs、src-tauri/src/lib.rs。
- 2026-10-03：根界面按渠道、运行实例和抽屉拆开。App 只保留标题、提示和隔离空间，抽屉与弹窗通过 useAppState 读取同一份控制器状态。禁用按钮收成 HintedButton。未挂载的官方账户面板、旧诊断列表和更新卡片已删除。src/App.tsx、src/components/。

- 2026-10-03：首页按 Obsidian 设计稿改成空间条、渠道面板、控制带。第二标题栏、启动入口抽屉、调整模型和铅笔按钮删除。模型勾选在渠道行展开后完成，仍写入现有工作区。实例记录和诊断进入控制带「更多」。src/App.tsx、src/workspace-surface.tsx、src/channel-sort-list.tsx、src/workspace-model-selection.ts、src/surface.css、src/styles.css。
- 2026-10-03：根组件第一版拆分。标题栏在 src/components/title-bar.tsx，隔离 App 信息在 src/components/isolated-app-info.tsx。写入命令仍在 src/App.tsx。
- 2026-10-04：模型渠道抽屉和新增渠道抽屉迁到 src/components/model-utility，index.ts 对外导出。
- 2026-10-04：模型渠道改用 Mantine 全屏 Drawer（size 100% + Overlay），避开标题栏。src/components/utility-drawer.tsx。
- 2026-10-04：按钮、Tab、Menu、List 也改用 Mantine，现有 className 覆盖外观。Windows 标题栏系统按钮除外。src/components/hinted-button.tsx、src/workspace-surface.tsx、src/components/isolated-app-info.tsx、src/components/utility-drawer.tsx。
- 2026-10-04：选择器与失败提示改用 Mantine。Select 走 Portal + flip，incident-bubble 走 Notification，样式覆盖对齐现有控制面。src/components/mantine-theme.tsx、src/workspace-surface.tsx、src/App.tsx、src/surface.css。
- 2026-10-04：底部默认调用模型和 API 登录渠道的下拉改为向上展开，避免被窗口裁切后点不到选项。src/surface.css。
- 2026-10-04：首页「新建空间」改为「新增实例」并可点击。新增会复制当前实例的渠道和模型，写入一条新的 managed_app 启动方案；标签切换当前实例。同时仍只能运行一个受管理 App。src/workspace-surface.tsx、src/components/isolated-app-info.tsx、src/use-app-controller.ts、src-tauri/src/managed_launch.rs、src-tauri/src/database.rs。
- 2026-10-04：标题栏网关状态避开系统按钮。macOS 放在最右，不显示「R Codex」。Windows 放在最左，并在其右侧显示「R Codex」；最小化、最大化和关闭仍由页面绘制。src/components/title-bar.tsx、src/surface.css。
- 2026-10-04：删除 utility-drawer.tsx。模型渠道、实例、诊断抽屉和渠道确认层并入 src/components/model-utility，App 只挂 WorkspaceOverlays。其余控制面组件改成功能目录加 index.ts。组件定义规则写入 AGENTS.md 前端与交互规则。
- 2026-10-05：实例标签去掉「活动」和「主控」。双击标签可改名称，只更新启动方案名称；运行中的实例锁定名称。src/workspace-surface.tsx、src/components/isolated-app/isolated-app-info.tsx、src/use-app-controller.ts、src/App.tsx、src-tauri/src/codex_control.rs、src-tauri/src/lib.rs。
- 2026-10-05：操作结果提示改为 Mantine Notifications。成功约 4.5 秒、失败约 7 秒后关闭，新提示替换上一条。配置占用时仍可从提示打开运行实例。诊断失败气泡仍用原来的 Notification。src/main.tsx、src/components/app-state/mantine-theme.tsx、src/use-app-controller.ts、src/App.tsx、src/surface.css。
- 2026-10-07：添加模型渠道改为全屏抽屉，层级高于模型渠道抽屉。表单改用 Mantine 表单组件。src/components/model-utility/add-channel-dialog.tsx、src/components/model-utility/drawer-layout.tsx、src/components/app-state/mantine-theme.tsx。
- 2026-10-07：更多菜单的实例详情和请求记录只展示当前标签对应的实例。请求记录可以进入该实例的完整请求档案。src/components/isolated-app/isolated-app-info.tsx、src/components/model-utility/instance-drawer.tsx、src/components/model-utility/diagnostics-drawer.tsx、src/ConversationArchivePanel.tsx、src/use-app-controller.ts、src/App.tsx。
- 2026-10-07：确认和启动弹层改为 Mantine Dialog。遮罩避开标题栏，层级高于全屏抽屉；忙碌时不能通过遮罩或 Escape 关闭。卡片仍用原来的确认和启动样式。src/components/control-dialog/control-dialog.tsx、src/components/runtime-panels/runtime-panels.tsx、src/components/model-utility/channel-workspace.tsx、src/styles.css。
- 2026-10-07：操作结果提示的层级高于控制面抽屉和确认弹层。src/components/app-state/mantine-theme.tsx。
- 2026-10-07：隔离实例各自使用 Electron 桌面数据，多个受管理 App 可以同时运行。首页「新增实例」暂时隐藏。src/workspace-surface.tsx、src-tauri/src/managed_launch.rs、src-tauri/src/lib.rs。
- 2026-10-07：模型渠道去掉「运行中，配置已锁定」。编辑按钮可在实例使用渠道时修改名称，运行中的实例下次启动才使用新名称。src/components/model-utility/channel-workspace.tsx、src/use-app-controller.ts、src-tauri/src/providers.rs、src-tauri/src/lib.rs。
- 2026-10-07：模型渠道列表去掉「本地首选」和「API Key 渠道」。修改名称改为弹窗，可以取消或保存。src/components/model-utility/channel-workspace.tsx。
- 2026-10-07：HintedButton 的提示改为 Mantine Tooltip，不再使用原生 title。禁用按钮仍可悬停或聚焦查看原因。src/components/hinted-button/hinted-button.tsx、src/styles.css。
- 2026-10-08：模型目录抽屉改为 Mantine ControlDrawer，宽 480，叠在模型渠道抽屉之上，关闭沿用抽屉自带按钮和遮罩。src/components/model-utility/channel-workspace.tsx。
- 2026-10-08：渠道行直接用横向可换行的复选框选择对话模型，不再展开「选择模型」。模型目录抽屉说明每条模型的用途；图像生成模型（如 grok-imagine-image-2.0）和 Codex 系统角色不会出现在工作区勾选中。src/channel-sort-list.tsx、src/components/model-labels.ts、src/components/model-utility/channel-workspace.tsx、src/components/isolated-app/isolated-app-info.tsx、src/use-app-controller.ts。
- 2026-10-08：Mantine 抽屉和确认弹窗改为淡入淡出，遮罩盖住标题栏以下。减少动态效果时不播放动画。src/components/model-utility/drawer-layout.tsx、src/components/control-dialog/control-dialog.tsx、src/surface.css、src/styles.css。
- 2026-10-08：隔离空间控制带去掉「更多」菜单。实例详情、请求记录、打开隔离目录、打开默认 Codex 改为纯图标按钮，Tooltip 说明动作。导入项目会话和按清单同步设置不再从首页挂载，后端命令保留。src/components/isolated-app/isolated-app-info.tsx、docs/designs/2026-10-03-homepage-obsidian-restyle.md、docs/designs/3rcodex-ui-quality-upgrade-plan.md、AGENTS.md。
- 2026-10-08：启动 CLI 和启动 App 也改为纯图标，和控制带其余动作排成 36px 自适应网格。默认模型、登录渠道、诊断筛选和档案对照直接使用 Mantine Select 的标题与说明。src/workspace-surface.tsx、src/surface.css、src/ConversationArchivePanel.tsx、src/components/model-utility/diagnostics-board.tsx、src/SessionImportPanel.tsx。
- 2026-10-08：启动 CLI 改为终端图标并隐藏入口。启动 App 使用播放图标；未运行时提示「启动当前app实例」，运行中悬停显示停止图标并提示「停止当前app实例」。打开默认 Codex 的图标不变。src/workspace-surface.tsx、src/components/isolated-app/isolated-app-info.tsx、src/App.tsx、src/surface.css。
- 2026-10-08：运行中的隔离实例仍然禁用渠道、模型、排序和 API 登录渠道，不放开编辑。禁用提示改为「这个隔离实例正在运行，渠道、模型、排序和 API 登录渠道已禁用，停止后才能修改。」移除渠道改为可取消的气泡确认。拖拽按钮相对整行垂直居中。src/surface.css、src/codex-control.ts、src/workspace-surface.tsx、src/use-app-controller.ts、src/channel-sort-list.tsx。
- 2026-10-08：首页渠道标题改为「接入渠道」，去掉 `// ROUTING_CHANNELS` 和可用/待命计数。未挂载的 src/demo-console.tsx、src/demo-console.css 删除。src/workspace-surface.tsx、src/surface.css。
- 2026-10-08：首页去掉实例标签行。左侧是 R Codex，右侧仍是模型渠道。原「接入渠道」改为当前实例名称。控制带六个图标排成上三下三，窄屏独占一行时三列等分。src/workspace-surface.tsx、src/workspace-surface.css。
- 2026-10-08：运行中的隔离实例不再用按钮提示说明禁用。文案改为「当前隔离实例正在运行，渠道、模型等均已禁止修改」，显示在接入渠道标题后。控制带和渠道列表的图标按钮改为 xl。启动 CLI 按钮恢复，提示为「以当前配置模型运行codex cli」；每次确认都会新开一个 CLI。src/workspace-surface.tsx、src/components/isolated-app/isolated-app-info.tsx、src/components/model-utility/channel-workspace.tsx、src/codex-control.ts。
- 2026-10-08：启动时的全屏粒子加载改为 Mantine LoadingOverlay，loader 为粉色 bars。boot-loading.tsx 与 boot-loading.css 删除。控制带正常时默认调用模型、API 登录渠道和操作按钮三等分；宽度不超过 860px 时前两项同一行二等分，操作按钮独占下一行。src/App.tsx、src/workspace-surface.css。
- 2026-10-08：标题栏去掉组件尺寸菜单。按钮、图标按钮、复选框、输入、选择、菜单和标签固定为 Mantine 最小档 xs，启动时删除旧的 rcodex-component-size。src/styles.css 与 src/surface.css 已删除。仍在使用的外观改到对应组件旁边的 CSS，例如 title-bar.css、workspace-surface.css、channel-sort-list.css。main.tsx 不再引入这两份全局样式。
- 2026-10-09：控制带操作区 `.control-actions` 使用与默认调用模型、API 登录渠道相同的边框、圆角和底色，高度拉齐。src/workspace-surface.css。
- 2026-10-09：首页「当前隔离实例正在运行」按该工作区活动实例判断，不再沿用过期的 lockedByInstances 快照。停止后轮询到 exited/failed 会刷新控制面快照。src/codex-control.ts、src/use-app-controller.ts、src/components/isolated-app/isolated-app-info.tsx、src-tauri/src/codex_control.rs。
- 2026-10-09：运行中的隔离实例不再锁工作区。渠道、模型、排序和 API 登录可以改，下次启动才用新配置；保存后提示「当前隔离实例正在运行，重启可应用修改」。删除模型渠道仍要先停掉占用它的实例。
- 2026-10-09：删除全部旧选择器自定义类名和样式，包括选择器宽度规则。src/workspace-surface.tsx、src/SessionImportPanel.tsx、src/ConversationArchivePanel.tsx、src/components/model-utility/diagnostics-board.tsx 及对应 CSS。
- 2026-10-10：标题栏不再显示 Gateway 空闲或转发。只有失联时右侧显示「重启」。应用启动即拉起 Gateway，真正退出先停隔离实例再停 Gateway。关窗口仍只隐藏。
- 2026-10-11：程序坞「退出」和 Cmd+Q 与菜单栏「退出」走同一条清理。macOS 拦截 `applicationShouldTerminate:`，取消系统 terminate，改发 `exit(0)`，先停隔离实例再停 Gateway。红点关闭仍只隐藏。src-tauri/src/macos_quit.rs、src-tauri/src/lib.rs。
- 2026-10-10：恢复系统标题栏。去掉网页自绘标题栏、macOS 交通灯改位和 Windows 自绘窗口按钮。失联时的「重启」放在页面「R Codex」右侧。主题和更新也留在这一行。
- 2026-10-10：初版收口。实例详情和请求记录禁用。明暗和强调色入口隐藏。错误通知不弹出。见 docs/designs/2026-10-10-initial-release-surface.md。
- 2026-10-10：初版固定浅色，不跟随系统，也不读取已保存明暗。index.html、src/app-model.ts、docs/designs/2026-10-10-initial-release-surface.md。
- 2026-10-10：添加渠道不再确认。下拉选中后直接加入当前隔离空间，并排在第一个。
- 2026-10-10：系统标题栏不再显示「R Codex」。窗口标题仍保留给系统和菜单，页面里的「R Codex」和重启按钮不动。
- 2026-10-10：隔离 App 运行时，控制带 App 按钮直接显示停止图标，不再悬停才从播放换成停止。未运行时仍是播放图标。src/workspace-surface.css。
