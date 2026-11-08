# R Codex 项目治理建议

> 状态：active（最后核对 2026-11-08）
> 状态说明：第一轮最小范围已于 2026-11-08 经 commander 批准实施（见文末实施记录）；后续轮次待 commander 启动。
> 记录日期：2026-11-06。

## 结论

当前 AI 开发变慢，首要原因是当前合同、历史设计、执行记录、未完成事项和本地未提交改动混在同一工作入口中。代码已经完成部分模块拆分，但仍有少数大型控制器和领域文件形成认知热点。建议后续启用轻量、当前状态优先的项目治理，不建议立即进行全仓库重构。

## 已观察到的原因

- `AGENTS.md` 同时承载产品合同、架构边界、开发规则、未完成项和 UI 历史留痕。
- `docs/` 有多份产品原理、详细设计、执行计划、专项设计和发布预案；主执行计划持续追加历史段落，当前状态与历史决策需要人工区分。
- 文档中仍有指向已拆除的 `managed_gateway.rs`、`managed_launch.rs` 等旧路径的链接或描述。
- `src/use-app-controller.ts`、`src-tauri/src/managed_launch/workspace.rs` 和部分 Gateway 文件较大，跨层改动需要加载较大上下文。
- 前端没有独立测试脚本；测试工作流没有统一执行前端检查/构建，并跳过 `managed_gateway` 测试。
- 本地存在大型构建缓存和运行产物。它们已被 Git 忽略，但会放大全目录扫描和粗略索引的噪声。

## 建议的最小治理范围

1. 建立一个唯一的当前状态入口，明确产品合同、架构入口、写入路径、验证命令和当前阻塞。
2. 给文档标记 `active`、`historical` 或 `archived`；历史执行记录与当前合同分离，避免继续向单一计划无限追加。
3. 增加 Markdown 本地链接和代码路径检查，发现模块拆分后的旧引用时阻止合并或明确标记。
4. 每个持续任务固定记录目标、范围、非目标、修改文件、验证结果、未完成项、回滚方式和下一步命令。
5. 保持提交前工作区可解释：未提交改动需要有任务归属，交接时说明基线和不能覆盖的文件。
6. 完善自动验证入口，至少让前端类型检查/构建和关键 Gateway 测试有清晰、可复用的命令。
7. 对大型控制器和领域文件先定义边界与责任，再按需求拆分；不把治理工作自动扩大为一次重构。

## 明确不在本轮执行

- 不移动或删除历史文档。
- 不批量重写代码目录。
- 不改变当前产品合同、数据库 schema、Provider 路由或发布版本。
- 不因为治理建议自动新增 CI 门禁或阻止现有开发。

## 启用条件与回滚

只有在 commander 明确启用治理后，才建立当前状态目录、文档状态标记、链接检查和任务模板。治理过程应先提交文档与检查入口，再逐步迁移历史内容；若影响正常开发，可保留原文档和旧命令，仅撤回新增索引与检查，不改业务数据和运行目录。

## 实施记录

### 第一轮（2026-11-08，commander 批准「按规范对齐版」）

- 依据 agents.md 规范与 Codex 文档对 AGENTS.md 的职责定义（约定与指针，不含状态与历史），新建 [CURRENT-STATE.md](CURRENT-STATE.md)（六节：合同要点、架构地图、写入路径、验证命令、阻塞与未完成、最近交接）承接状态，新建 [ui-change-log.md](ui-change-log.md) 承接 UI 留痕；AGENTS.md 247→160 行，约定全文保留。
- 全部 56 份 docs 加 `> 状态：active|historical|archived（最后核对日期）` 标记；既有描述性状态行改名为「状态说明」保留。
- 新增 `scripts/check-doc-links.mjs`（markdown 相对链接 + 反引号仓库路径；`<!-- doc-links-allow: ... -->` 豁免故意保留的已删除文件引用；默认 report-only，`--strict` 时 active 失效即失败）与 `scripts/run-ui-checks.mjs`（自动起停 vite 跑两份夹具）。
- active 文档 32 处旧路径引用清零：模块单文件引用改指现目录（managed_gateway/、managed_launch/、managed_codex_config/、codex_installation/），历史完成记录中已删除文件用 allow 指令豁免。
- package.json 增加 `check:ui`、`check:rust`、`test:rust`、`check:docs`、`ui-check`、`check:all`；AGENTS/README 引用同一入口。
- PASS：`pnpm run check:all`、`pnpm run build`、`pnpm run ui-check`（两份夹具 21+13 项）。
- 未做（按边界）：未移动/删除历史文档、未改业务代码、未启用 strict 门禁；第二轮候选——代码热点拆分（use-app-controller.ts 先立边界）、CI Test 长期失败修复、strict 模式入 CI、子目录 AGENTS.md。

### 第二轮：归档目录（2026-11-08，commander 批准）

- 38 份 historical/archived 文档与 stitch 设计资产目录 `git mv` 至 `docs/archive/`（镜像原结构：根文档在 `archive/`，designs 在 `archive/designs/`），git 历史保留；`docs/designs/` 只留 9 份 active 合同。
- 链接修复：迁移文档内部按「原位置解析→新位置重写」69 处；全库 docs 前缀字面路径替换 26 处；staying 文档相对链接重写 39 处；迁移文档之间的互链因镜像结构天然保持。
- `docs/archive/README.md` 声明归档只读与默认读取面；AGENTS.md「当前状态与历史记录」加入同一声明。
- PASS：`check-doc-links` active 0 失效（211 链接 / 330+ 仓库路径）；historical/archived 内 33 处旧代码路径按策略仅提示不改写。
- 回滚：`git mv` 逆操作，零数据风险。

### 第二轮补充：子项目 AGENTS.md（2026-11-08，commander 批准）

- 按 agents.md 规范嵌套机制拆分：`src/AGENTS.md`（交互状态/组件定义/Mantine 覆盖/UI 验证，含 `rcodex-` 前缀教训）、`src-tauri/AGENTS.md`（验证深度/构建与 sidecar/数据与迁移）、`website/AGENTS.md`（纯静态边界/构建/Pages 发布）；根 AGENTS.md 换成「子项目约定」节，只保留跨端合同，247→149 行。
- `scripts/check-doc-links.mjs` 扫描范围加入嵌套 AGENTS.md。
- 根文件恒加载内容更精；动对应子树时子项目约定就近生效。

### 第二轮补充：文档维护规则与机器看门（2026-11-08，commander 批准）

- 根 AGENTS.md 新增「文档维护规则」节：新文档必须有状态标记且只落在指定位置、执行记录追加不另开新档、状态页条目上限（3 主条目 + 1 更早概览）、生命周期与批量归档、提交前 `check:all` 必跑。
- `check-doc-links.mjs` 扩展：报告缺状态标记（索引类文件豁免，frontmatter 文档支持）、状态页交接条目超限；`--strict` 覆盖 active 失效 + 缺标记 + 状态页超限三类失败，可作 CI 门禁。
- PASS：`check:docs` 缺标记 0、状态页 0 项；`--strict` exit 0；`check:all` 全绿。状态页交接已按新规则压缩为 3 主条目 + 更早概览。
