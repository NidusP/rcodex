# R Codex AI 编程总览

本文件是 R Codex 项目级 AI 编程入口。它把产品原理、架构设计、执行计划和专项文档中的稳定约束集中起来，只保留跨端合同；子项目约定在 `src/`、`src-tauri/`、`website/` 的嵌套 AGENTS.md（见「子项目约定」）。修改代码前先读取本文件与对应子项目文件，再按任务类型读取详细文档；若详细文档与本文件冲突，以较新的产品决策、执行记录和用户明确要求为准，并同步修正文档。

## 项目定位

R Codex 是专为 Codex 做的本机路由转发工具，把多个模型服务商聚合成一份目录，经本机 Gateway 按模型精确转发。目标平台是 macOS 与 Windows。技术 slug 是 `rcodex`；`3r-gpt` 和 `3rcodex` 只作为升级迁移兼容来源保留。

当前产品基线要点（完整合同与背景见下列文档）：

- 纯 API Key 多 Provider、多模型的受管理 CLI/App 路由；受管理运行都经过本机回环 Gateway（独立 `codex-gateway` sidecar，只绑定 `127.0.0.1`，不提供 LAN 或公网入口），DirectResponses 直连路径已删除。
- 每个受管理实例使用独立 `CODEX_HOME`、模型目录、Profile、RouteSet 和实例准入密钥。产品只保留 R Codex 工作区，不得读取或修改用户原 config.toml、auth.json、Cookie、Electron 数据；仅用户主动项目导入允许只读源会话。
- 官方订阅模型经自定义 Gateway 的真实协议和账户合同仍受发布门限制，不能默认打开。发版矩阵产出 macOS Apple Silicon、macOS Intel 和 Windows x64 安装包；Windows 安装和打开仍需桌面验收。

产品与架构的完整背景见：

- [当前状态页](docs/CURRENT-STATE.md)：当前阻塞、未完成项与最近任务交接（每次任务交接更新）。
- [README](README.md)：当前实现、运行路径、开发和打包入口。
- [产品原理](docs/3rcodex-product-principles.md)：用户承诺、所有权、安全边界和会话规则。
- [混合路由详细设计](docs/codex-hybrid-routing-design.md)：领域对象、SQLite schema、IPC、Gateway 和启动合同。
- [混合路由执行计划](docs/codex-hybrid-routing-execution-plan.md)：分片进度、验收证据、阻塞项和后续任务。
- [独立工作区改造计划](docs/designs/2026-09-21-isolated-only-product-plan.md)：当前产品合同、单向导入、API 登录与发布验收。默认 App 快速发版计划已作历史记录。
- [官网计划](docs/website-plan.md)：Astro + Starlight + Solid.js 官网内容映射、构建和 Pages 发布约束。
- [项目治理建议](docs/project-governance-proposal.md)：治理范围与启用状态；第一轮已按其最小范围实施（见状态页交接）。

## 文档读取顺序

先读 [docs/CURRENT-STATE.md](docs/CURRENT-STATE.md) 了解当前阻塞与交接，再按任务选择最少的一组文档：

| 任务 | 首先阅读 | 再阅读 |
| --- | --- | --- |
| 独立启动、API 登录、项目导入 | `docs/designs/2026-09-21-isolated-only-product-plan.md` | 产品原理、混合路由设计 |
| 按会话记录完整请求、转发链路优化 | `docs/designs/conversation-request-archive-design.md` | 产品原理、Gateway 诊断路线、混合路由执行计划 |
| 产品、模型切换、会话恢复 | `docs/3rcodex-product-principles.md` | `docs/codex-hybrid-routing-design.md`、`docs/codex-hybrid-routing-execution-plan.md` |
| Provider、模型目录、协议转换 | `docs/codex-api-protocol-compatibility-plan.md` | `docs/third-party-model-reasoning-levels.md`、`docs/codex-hybrid-routing-design.md` |
| 模型透出、按模型图片探针 | `docs/designs/2026-11-07-model-exposure-image-probe.md` | 协议兼容方案、产品原理 |
| Gateway、502、超时、诊断 | `docs/codex-roadmap-and-gateway-diagnostics.md` | `docs/archive/compat-app-loopback-proxy.md`、README 的本地网关章节 |
| 默认 App、受管理 App、启动隔离 | `docs/designs/2026-09-21-isolated-only-product-plan.md` | `docs/designs/2026-10-17-windows-install-root-launch.md`、混合路由详细设计 |
| Windows 隔离 App / CLI | `docs/designs/2026-10-17-windows-install-root-launch.md` | `docs/designs/2026-10-11-windows-isolated-app-launch.md` |
| 认证、Keychain、实例锁 | `docs/codex-official-auth-authorization-design.md` | `docs/archive/internal-edition-execution-plan.md` |
| UI、布局、交互状态 | 本文件「前端与交互规则」 | `docs/ui-change-log.md`、`docs/archive/designs/3rcodex-ui-quality-upgrade-plan.md`、README |
| 品牌、路径和迁移 | `docs/archive/product-and-app-renaming-plan.md` | README、产品原理 |
| Grok 看图/生图、目录能力泄漏、默认 App compact 信封、旁路 luna | `docs/archive/designs/2026-09-19-grok-session-followups.md` | `docs/archive/designs/2026-09-19-grok-session-error-todos.md`、[F 当前模型族-only](docs/archive/designs/2026-09-21-f-family-only-ephemeral.md)、第三方目录 overlay、混合路由执行计划 |
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

## 文档维护规则

- 新文档必须有 `> 状态：active|historical|archived（最后核对日期）` 标记。任务合同与设计进 `docs/designs/`（日期前缀命名）；执行记录追加到既有 active 计划，不另开新档。
- 任务完成必须更新 [docs/CURRENT-STATE.md](docs/CURRENT-STATE.md) 的「当前阻塞与未完成」「最近任务交接」两节；最近交接只保留 3 条主条目 + 1 条更早概览，旧条目压缩进 [治理提案实施记录](docs/project-governance-proposal.md)。
- 设计/计划落地完成后标记改 historical，整体过时改 archived；归档搬运随治理轮批量 `git mv` 至 `docs/archive/`，不逐个手搬。
- 状态页与根文件只写当前生效的合同；历史叙事进执行计划或 `docs/archive/`。提交前跑 `pnpm run check:all`：`check:docs` 的 active 失效、缺标记、状态页超限必须清零（`node scripts/check-doc-links.mjs --strict` 可作门禁）。

## 架构与写入边界

### 路由

- `Provider` 是模型渠道；一个 `LaunchPresetComposition` 可以引用多个 API Key Provider 和多个模型。
- `ModelRouteIdentity` 保存稳定的公共模型身份；`RouteSetRevision` 是启动时固化的不可变路由快照。
- Gateway 先按公共模型 ID 精确匹配 RouteSet，再替换为 `upstream_model_id`，读取绑定的 Provider revision 和凭据。
- 第三方请求必须删除官方认证，只在当前请求边界从 R Codex 自有 SQLite `provider_credentials` 表读取对应 Provider revision 的 API Key；运行时不读取系统凭据库。
- 不按模糊模型名猜渠道，不因 401/502 自动换地址、换模型、换 Provider 或重试 POST。
- 渠道发现完整模型目录，用户在模型目录勾选透出后空间才可选。透出偏好与图片证据分别保存；新启动的图片能力只取当前渠道修订、模型、协议和探针版本的成功证据，模型族默认不授予图片能力。升级回填旧空间已选模型；取消透出前必须先移除所有未删除空间引用。

### Gateway

- 数据路径是 `Codex → 127.0.0.1 Gateway → Provider`；UI 不承载模型 token 流量。
- Gateway 只接受回环请求和当前 binding 的准入密钥；sidecar 与 UI 生命周期分离。
- GPT 族 Responses 原样转发请求体（仍替换公共模型 ID、注入该渠道 Key、去掉官方认证）。非 GPT 才做中继兼容：删 `metadata`、去掉渠道不支持的可选工具、把 `agent_message` 收成公开 user 消息。Responses→Chat 文本转换仍按 `CapabilitySet` 宣布能力；不伪造工具、图片、MCP、推理或多轮续接兼容性。
- 上游非 2xx 属于 `upstream` 层；本地路由、准入、网络或解析错误属于 `gateway` 层。错误响应保留 `gateway_code`、可选 `upstream_status` 和脱敏 `upstream_detail`。
- 普通诊断和用量记录是 best-effort，不得阻断模型请求；不得记录 API Key、实例密钥、Authorization、请求正文、响应正文或完整敏感 URL。用户显式启用的完整请求档案是独立存储例外，当前已接入首版，按专项设计维护，不能把正文加入普通日志或用量表。

### 配置与实例

- 受管理 CLI/App 使用实例专属 `CODEX_HOME`、`CODEX_SQLITE_HOME`、Profile 和模型目录；受管理 App 的 `CODEX_SQLITE_HOME` 必须等于启动 `CODEX_HOME`，活动对话库不得再放到 `managed-app-sqlite/<preset>`。启动时生成不可变快照。运行中的实例继续使用那份快照。工作区可以修改渠道、模型、排序和 API 登录渠道，下次启动才生效；保存后提示「当前隔离实例正在运行，重启可应用修改」。
- 空间 Tab 按固定 A-E 槽位排序，preferred 只表示用户选择；运行回执不得写回选择。彻底删除空间须用户确认且所有所属实例已停止，按空间精确清理配置/隔离数据/档案，保留共享渠道。失败保留删除意图并允许重试；最后一个删除后不自动重建。见 `docs/designs/2026-11-07-workspace-order-and-deletion.md`。
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

## 子项目约定

本仓库按子项目拆分嵌套 AGENTS.md（agents.md 规范：就近生效，离被编辑文件最近的优先）：

- `src/`（前端控制面）：[src/AGENTS.md](src/AGENTS.md) —— 交互与状态、组件定义、Mantine 覆盖、UI 验证。
- `src-tauri/`（Rust 后端与 sidecar）：[src-tauri/AGENTS.md](src-tauri/AGENTS.md) —— 验证深度、构建与 sidecar、数据与迁移。
- `website/`（静态官网）：[website/AGENTS.md](website/AGENTS.md) —— 纯静态边界、构建与 Pages 发布。

本文件只保留跨端合同：路由、Gateway、配置与实例、会话切换、安全隐私、发布流程。

## 安全与隐私

- 渠道 API Key 以 R Codex 自有 SQLite `provider_credentials` 表为事实来源，数据库文件保持当前用户私有权限；运行时不读取系统凭据库。所选 API 登录 Key 仅允许额外投影至 R Codex 自有 auth.json。实例准入密钥不是用户凭据，不进钥匙串：SQLite 只保存引用和哈希，明文只经内存与一次性 FIFO 传递。日志、导出、IPC 快照和 UI 状态不保存原文。
- 官方 Authorization 不得发送给第三方 Provider；第三方 Key 仅用于其对应渠道请求与明确选择的 R Codex 独立 API 登录，不得发送给无关上游或用户原 Codex。
- 不读取原配置或登录。显式项目导入是源会话只读的限定例外：预览元数据、确认停机，复制到 R Codex 后只改副本 Provider 和索引路径；源正文/数据库不写入，凭据不复制。原地会话修复已移除。
- “完整请求记录”允许将经 R Codex 转发的正文存入本机独立档案并展示，原生历史只读元数据用于关联；不回填旧正文、不保存认证凭据、不改写原历史。首版已接入四阶段采集、容量、故障降级和清理合同，细节见 `docs/designs/conversation-request-archive-design.md`。
- 普通诊断增强仍采用形状、计数、哈希和枚举值；完整档案不得进入普通日志、遥测、控制面全局快照或修复备份。
- 对外网络请求必须有边界、超时、错误分类和最小响应解析；真实 Provider 探针可能产生费用，默认使用 mock 或无敏感 fixture。

## 开发、测试与打包

- 日常开发中，AI 不运行 `pnpm run tauri:build`、任何 Tauri 打包命令或 `codesign`；只有 commander 明确要求生成或校验发布包时才可执行。
- 应用在线更新见 `docs/designs/2026-10-09-app-updater.md`。不要提交 `work/updater.key`。发版仓库名在 `src/app-release.ts` 与 `tauri.conf.json` updater endpoints，必须一致。四个版本文件改成同一新号并推 `main` 后，CI 自动打包并发布；对应 tag 已存在则跳过。
- commander 自行运行 `pnpm run tauri:dev`。AI 不启动、停止或重启 Tauri 开发服务，除非 commander 明确要求。

在仓库根目录（聚合命令定义见 `package.json`）：

```bash
pnpm run check:ui      # TypeScript 类型检查（即 pnpm run check）
pnpm run build         # 前端生产构建
pnpm run check:rust    # cargo fmt --check + cargo check --all-targets
pnpm run test:rust     # cargo test --lib --bins -- --skip managed_gateway（与 CI test.yml 一致）
pnpm run check:docs    # 文档链接与代码路径检查
pnpm run ui-check      # 无头浏览器夹具回归（自动起停 vite，跑 workspace/model-exposure 两份）
pnpm run check:all     # check:ui + check:rust + check:docs
pnpm run tauri:dev       # 构建 sidecar 并启动 Tauri 开发窗口（commander 自行运行）
pnpm run tauri:build --bundles app
```

各子项目的验证深度、构建细节与数据约束见对应嵌套 AGENTS.md（见「子项目约定」）。

## 变更交接

完成一个任务时，在对应执行计划中记录：变更目的、修改文件、实际验证命令和结果、未完成项、阻塞原因、回滚方式及下一步命令。不要把“命令未启动”“网络被阻断”写成测试通过；不要在没有证据时把推测写成根因。后续 AI 应能只读本文件、相关计划和最后验证记录后继续工作。

## 当前状态与历史记录

- agent 默认读取面 = 本文件 → [docs/CURRENT-STATE.md](docs/CURRENT-STATE.md) → 按任务表选 active 文档；`docs/archive/` 只在考古历史决策时检索。
- 当前阻塞、未完成项与最近任务交接：[docs/CURRENT-STATE.md](docs/CURRENT-STATE.md)。
- UI 变更历史留痕：[docs/ui-change-log.md](docs/ui-change-log.md)；完成一项 UI 改造时在该文件追加条目。
