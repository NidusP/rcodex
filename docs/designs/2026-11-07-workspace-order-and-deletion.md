# 隔离空间固定排序与彻底删除

> 状态：active（最后核对 2026-11-08）

> 2026-11-07 commander 授权修复 Tab 选中后重排、增加现有隔离空间彻底删除，并允许启动 `pnpm tauri:dev` 观察运行日志。

## 当前合同

- Tab 按固定名称 A-E 排列；preferred 仅表示当前选择，不影响排序。切换、刷新、重启均保持相同顺序。
- 每个 Tab 名称后提供各自的删除入口（垃圾桶图标）：常态隐藏，仅当该空间可删除且悬停该 Tab 时显示（淡色，悬停图标加深）。该空间有活动实例（含 CLI、launching/orphaned/degraded）、正在删除或正在更新时垃圾桶完全不显示（2026-11-07 commander 调整，替换「常显+禁用态」）。点击直接弹出对应空间的 Mantine ControlDialog 确认，不切换选中，明确不可恢复：空间配置、该空间所有运行记录、隔离 CODEX_HOME/会话、Electron 桌面数据、遗留自有 SQLite 和该空间完整请求档案都会清除。
- 删除非选中的空间保持当前选中；删除选中的空间后选择剩余按槽位排序的第一个空间（后端 `finish_workspace_deletion` 语义，2026-11-07 commander 方案A确认）。
- 活动实例（含 CLI、launching/orphaned/degraded）须先停止并核验；删除不自动向其他实例发送信号。启动和导入共用互斥锁，空间操作租约阻止删除期间修改。
- 全部候选路径在第一次删除前校验归属；不跟随外部 symlink，不删除共享 Provider、凭据、透出偏好、模型身份或其它空间文件。
- 文件/档案失败必须返回明确错误并保留可重试的目标记录，不能显示成功；部分已删除数据不能承诺恢复。最终数据库图清理在单一事务内，失败可重试。
- 用户确认并通过预检后持久化删除意图；若中途失败，释放本轮租约但保留意图，空间不能重新启动或编辑，用户可重试完成删除。成功时清除此标记。
- 删除不重编号，新增复用最小空闲槽位。
- 允许删除最后一个空间。显式空状态持久化，重启不自动重建；用户可新增一个空空间。
- 不主动删除当前本机真实空间；使用临时数据和浏览器夹具验证功能，真实删除由用户在确认弹窗中发起。

## 分工与验证

| 负责人 | 范围 | 状态 |
| --- | --- | --- |
| 数据任务 | managed_launch 下独立删除服务、路径预检、事务图清理和测试；初始空状态标记 | 完成，定向 9 项与全库通过 |
| 档案任务 | conversation_archive 下按 scope 删除、阻止迟到写入、测试 | 完成，定向 10 项通过 |
| 界面任务 | src 下稳定排序、删除确认、空空间新建和开发夹具 | 完成，类型/构建及浏览器 18 项通过 |
| 主任务 | 后端快照排序、删除 command 装配、互斥/网关协作、运行时观察、集成验证与文档 | 完成，424 项库测试与开发启动通过 |

命令合同：`delete_managed_workspace({ input: { launchPresetId, expectedVersion } })` → `{ deletedPresetId, selectedPresetId: string|null }`。失败返回可读稳定错误。前端仅确认后调用，不直接读写文件。

验收：反复 A/B/C 切换与刷新排序不变；停止前不可删；A 删除后 B 配置/文件/档案不变；最后一个删除后空状态与重启；新建复用空槽位；失败/路径冲突/版本冲突不得误删。前端类型/构建、Rust 定向与必要库测试、真实浏览器夹具。开发服务允许留给 commander 继续观察。

## 状态与交接

- 已确认：`codex_control/snapshot.rs` 中 `ORDER BY is_preferred DESC` 会让选中的 preset 排到最前，前端直接沿用数组顺序。
- 补充：`codex_control/lifecycle.rs::apply_receipt` 每次重放 running 回执还会把 preferred 改回运行实例所属空间；观察运行状态不得覆盖用户选择，已移除这一额外写入路径。
- 本机开发库只读确认：旧 SQL 返回 B/A/C（B preferred），按固定槽位返回 A/B/C；未修改该库或执行真实空间删除。
- 当前：最终 `pnpm tauri:dev` 已重新构建 launcher/gateway 并进入 `target/debug/r-codex` 运行态；保留开发窗口和终端会话供 commander 操作。没有执行任何真实空间删除。
- 回滚：代码回退可隐藏删除入口；用户已确认执行的物理删除不可逆，不能通过恢复代码找回。不得删除/重建其它空间。

## 实施与验证（2026-11-07）

- 排序：`codex_control/snapshot.rs` 按固定槽位排序；`workspace-order.ts` 在前端提供同一排序，兼容旧快照。`codex_control/lifecycle.rs` 删除回执重放时对 preferred 的写入；切换失败恢复原选择。新增 2 个回归用例覆盖 preferred 改变但顺序不变、运行回执不覆盖手动选择。
- 删除：`managed_launch/deletion.rs` 负责全体实例/路径预检、租约与持久删除意图、私有目录删除和数据库关系图事务清理。`commands/workspaces.rs` 共用启动/导入锁，逐个撤销本空间 binding，清理空间及旧实例 scope 的档案，最后提交删除。`workspace.rs`、启动准备和旧控制面修改入口拒绝继续使用待删除空间。
- 档案：`conversation_archive/deletion.rs` 按精确 scope 清除正文与索引、校正容量；SQLite 持久 tombstone（仅删除标记）拦截迟到或旧进程 INSERT；失败保留隔离目录供重试。其它空间的 policy、正文和容量保持一致。
- 界面：每个 Tab 名称后新增各自垃圾桶（`workspace-tab-delete`），`components/workspace-delete/` 使用现有 Mantine ControlDialog；点击直接删除对应空间，不切换选中。可删除时淡色、悬停加深；该空间全体所属 App/CLI 活动态、正在删除或正在更新时禁用并悬停显示原因；取消不发命令；删除选中的空间成功后选择首个剩余槽位；失败保留错误与重试。「新增隔离空间」为纯 + 图标。最后空间删除后持久空状态，新建空空间恢复 A。
- 选中语义（2026-11-07 方案A）：`managed_launch/deletion.rs::finish_workspace_deletion` 只在删除当前选中空间时重选剩余第一个槽位，删除非选中空间保持 preferred 不变；回归用例 `deleting_an_unselected_workspace_keeps_the_current_selection`。开发夹具 `delete_managed_workspace` 同步该语义。
- PASS：`pnpm run check`、`pnpm run build`；前端仅保留既有 >500 KiB chunk 提示。
- PASS：`cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`、`cargo check --manifest-path src-tauri/Cargo.toml --all-targets`、`git diff --check`。Rust 测试模块仍有既有 112 个未使用导入等警告，当前普通开发构建无新增警告。
- PASS：`cargo test --manifest-path src-tauri/Cargo.toml --lib`：424 passed / 0 failed（包含删除服务 9 项、档案删除 10 项、排序/回执 2 项）；仅临时数据库、临时目录和本机 mock。未读凭据或真实会话正文。
- PASS：`node scripts/workspace-ui-check.mjs http://localhost:5173`，独立无头 Chrome 夹具 18 项断言通过：重复切换和刷新顺序、选中保存、取消零删除、确认仅删目标、保护另一空间配置、空槽位复用、删除最后一个与重载空态、新建空 A、orphaned CLI 阻止删除、切换失败回退、删除失败重试、无页面异常。
- 运行时：用户已授权启动开发服务。开发库只读核对仍有 A/B/C 三个原空间，4 个实例均 exited。确认无活动实例及网关请求后，用本轮专用 helper 通过自有 control/stop 关闭空闲开发 Gateway，并只停止本轮开发进程组，再运行 `pnpm tauri:dev`。日志确认最新二进制运行、前端端口 5173 就绪、开发网关 ready 文件出现；`target/debug/codex-gateway` 与 `binaries/codex-gateway-aarch64-apple-darwin` 哈希一致。
- 未执行：用户真实空间物理删除、真实 Codex 会话继续、Windows 桌面删除、发布/提交。此前并行编辑中触发过未完成接口的热重编译错误，最终全库编译和重启均成功，不能将中间错误视为当前状态。
- 复验：开发服务保持运行；commander 可切换 A/B/C 并刷新，停止目标空间后点击标签栏垃圾桶并确认。自动界面回归命令见上。实际删除不可逆；如发布前需撤回，仅隐藏入口并回退相关代码，不操作用户数据。

## 每 tab 垃圾桶与纯图标新增（2026-11-07 方案A）

- commander 需求：删除入口移到每个 tab 名称后（可删除时淡色、hover 加深、禁用态可区分），新增按钮改纯 + 图标。
- 变更：`workspace-surface.tsx` 的 `WorkspaceTabs` 在每个 `Tabs.Tab` rightSection 渲染垃圾桶（点击 `stopPropagation` 不切换 tab）与运行点；`deleteAction` 行尾按钮删除；`WorkspaceInstanceTab` 增加 `deleteDisabled/deleteReason`。`isolated-app-info.tsx` 按各空间计算禁用原因（活动实例/正在删除/正在更新）并传 `onDeleteInstance`。「新增」改纯图标，Tooltip 保留。
- 后端：`managed_launch/deletion.rs::finish_workspace_deletion` 改为删除非选中空间保持 preferred、删除选中空间才回落剩余第一个槽位；新增回归用例 `deleting_an_unselected_workspace_keeps_the_current_selection`。开发夹具 `dev-control-fixture.ts` 同步该语义。
- PASS：`pnpm run check`、`pnpm run build`（仅既有 >500 KiB chunk 提示）；`cargo fmt --all -- --check`；`cargo test --lib` 425 passed / 0 failed。
- PASS：`node scripts/workspace-ui-check.mjs http://127.0.0.1:5185`（脚本自述的仅前端 vite，跑完已停）18 项断言通过，垃圾桶选择器改为 `[aria-selected="true"] .workspace-tab-delete`，orphaned CLI 阻止删除断言改为垃圾桶 `is-disabled` + aria-label 含「先停止」。
- 2026-11-07 再调整（commander）：垃圾桶改为常态隐藏、可删除空间 hover tab 才显示（`visibility: hidden` → tab `:hover` 可见；不可删除时 `display: none`）；「新增隔离空间」创建空空间（无渠道/模型/登录渠道，`needs_model_selection`），不再复制当前空间，`createManagedInstance` 不传 `sourcePresetId` 并同步清理 `workspaceInput` 死分支；+ 按钮去边框。ui-check 新增「常隐」断言并把 orphaned CLI 断言扩展到 `display: none`，共 20 项。
- PASS（再调整）：`pnpm run check`、`pnpm run build`；`cargo fmt --all -- --check`；`cargo test --lib` 426 passed / 0 failed（新增空方案回归 `create_new_empty_preset_starts_as_needs_model_selection`）；`workspace-ui-check.mjs` 20 项、`model-exposure-ui-check.mjs` 13 项通过（仅前端 vite，跑完已停）。真实 hover 显示与空空间新增手感待 commander 开发窗口复核。
- 2026-11-07 修复（commander 反馈 1/3 交互无变化）：Mantine 9 类名前缀是 `rcodex-`，`mantine-Tabs-tab` 静态类不存在，hover 显示选择器悬空——改为 `.rcodex-Tabs-tab:hover .workspace-tab-delete`；旧规则 `.workspace-tabs > .mantine-Tabs-root` 同为死规则，改 `rcodex-Tabs-root`。+ 按钮的 `border: 0` 被 Mantine `data-variant` 规则（特异性更高）压过——提升为 `.workspace-tabs .workspace-tabs-create` 三级选择器。无头 Chrome 真实 `Input.dispatchMouseEvent` 悬停断言验证 trash hover 显示（workspace-ui-check 21 项）。教训：本仓库自定义了 Mantine classNamesPrefix `rcodex`，自定义覆盖样式必须用 `rcodex-` 前缀或足够特异性，新增规则建议用无头探针实测计算样式。
- 未执行：真实空间删除与真实实例下的手动验收（commander 开发窗口内复核）；Windows 桌面；发布/提交。
