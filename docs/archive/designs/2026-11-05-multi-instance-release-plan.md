# 多隔离实例发布预案（待 commander 审阅）

> 状态：historical（最后核对 2026-11-08）
> 状态说明：实施记录，按 commander 2026-11-06 决策完成首版代码落地。
> 目标版本：0.1.7 同时包含工作区恢复、缺凭据恢复入口和多隔离空间首版。空间名称固定为「隔离空间 A」到「隔离空间 E」，最多 5 个。

## 2026-11-06 模型透出与按模型图片探针方案（初稿，已由 2026-11-07 合同替代）

> commander 于 2026-11-07 确认：偏好与证据分表、两种协议按实际链路探测、旧选择自动保留、取消前先解除空间引用。当前数据、交互、并发和验收以[实施方案](../../designs/2026-11-07-model-exposure-image-probe.md)为准。下文单表与取消后冲突描述仅保留初稿背景。本功能增加 schema v24，不能沿用下文多实例自身“不升级 schema 即可回退旧二进制”的判断；回退需要升级前数据库备份或向前修复。

### 产品决策

Provider 的 `/models` 只负责发现模型。用户在「模型渠道」抽屉中选择哪些模型透出到 R Codex；只有用户勾选透出的模型才执行图片能力探针。隔离空间只展示已透出的普通对话模型，隔离空间的模型勾选不再触发探针。

Provider 级合同继续负责 API Key、Base URL、连通性、文本探针和协议判定。图片能力属于模型级证据，不把一个 Provider 的所有模型统一标成支持或不支持。

### 用户流程

1. 渠道探测通过 `/models` 返回完整模型目录。
2. 模型渠道抽屉展示模型列表，每行有「透出」复选框。
3. 用户第一次勾选透出时，异步执行该模型的图片能力探针，并保留探针状态。
4. 用户取消透出不会删除探针结果；再次透出同一 Provider revision + 模型时复用缓存。
5. 模型行提供显式「刷新探针」按钮，只有用户主动点击才重复图片探针。
6. 隔离空间只列出已透出的普通对话模型。没有任何透出模型的 Provider 在「添加渠道」列表中置灰，提示「请先在模型渠道中选择要透出的模型」。

### 状态与能力边界

- `exposed`：模型是否进入 R Codex 工作区目录。
- `image_probe_state`：`unknown`、`supported`、`unsupported`、`failed`。
- 图片探针失败或明确不支持时，模型仍可作为文本模型透出；图片能力单独显示为未支持或探测失败。
- Codex 系统角色继续不可透出；图像生成模型继续走独立生图合同，不进入对话模型目录。
- Provider revision 变化后，旧图片探针不能直接继续作为有效证据。透出偏好可以保留，图片状态回到 `unknown`，等待用户刷新探针。
- 隔离空间已经保存但后来取消透出的模型不能静默删除；应显示为「已取消透出」，阻止新启动或要求用户重新选择。

### 数据合同建议

新增独立的模型透出/图片能力记录，不把状态直接写入会被刷新替换的 `provider_model_catalog_entries`。建议表形状：

```text
provider_model_exposures
- provider_id
- provider_revision_id
- upstream_model_id
- exposed
- image_probe_state
- image_probe_checked_at
- image_probe_error_code
- image_probe_contract_version
```

缓存键至少包含 `provider_revision_id`、`upstream_model_id`、`wire_api` 和探针合同版本。已有 `provider_model_catalog_revisions`、`provider_model_catalog_entries`、`model_route_identities` 和 `launch_preset_model_selections` 继续作为目录、稳定身份和空间选择的事实来源。

### 后续实现切片

- 数据库迁移和模型透出 Store：单一写入路径、Provider revision 失效、事务和回滚。
- Provider 抽屉：复选框、探针状态、最后探测时间、刷新按钮和失败提示。
- 控制面快照：返回每个模型的透出和图片探针状态。
- 隔离空间：过滤未透出模型；为已取消透出的历史选择提供明确冲突态。
- 添加渠道：按透出普通模型数量决定可用性和 hover 提示。
- RouteSet/catalog：读取缓存的模型级图片证据，生成 `supports_images` 与 `input_modalities`。
- 验证：首次透出只探测一次、取消后重新透出不重复探测、显式刷新会重新探测、Provider revision 变化使图片状态失效、不同空间的模型选择互不影响。

### 成本控制

普通 `/models` 刷新和文本探针不增加图片请求。图片探针只针对用户明确透出的模型，使用固定小图片和极小输出上限；不在每次刷新目录时批量探测，不自动重试，不因网络超时把模型判定为不支持。

## 一、上次任务的现状

### 已确认的根因

schema v22 的凭据迁移不会读取旧 Keychain Key，而是把已有 Provider 标为 `configured=0`、revision 标为 `credential_missing`。旧控制面快照又只保留 active revision，于是仍存在的工作区被整个过滤，页面看起来像隔离实例消失。

这不是在线更新删除实例目录：更新器只替换应用包，R Codex 数据仍在本机 `app_data_dir`。当前源码工作树里的修复已经让“缺凭据”变成可见的未配置状态，而不是过滤掉工作区。

### 本机只读核对（2026-11-05）

- SQLite `user_version=23`，`PRAGMA integrity_check` 和 `PRAGMA foreign_key_check` 均为 `ok`。
- `launch_presets` 共 8 条：1 条未删除，7 条软删除；未删除条仍保留原 `launch_preset_id`。
- `codex_instances` 共 9 条：8 条已退出，1 条运行中；9 条都仍关联同一条未删除的启动方案。
- `launch_preset_runtime_bindings` 有 1 条活动绑定。
- `codex-instances` 目录约 532 MB，旧隔离 `CODEX_HOME`/Electron 数据仍在。
- 当前工作树有 11 个文件未提交，包含控制面可见性修复、空工作区已有渠道选择和执行计划留痕；版本文件仍为 0.1.6。

### 多实例代码的真实状态

已经存在的能力：

- schema v21 删除 `launch_presets_entry_kind` 的唯一索引，允许多条 `managed_app` 启动方案。
- `create_managed_launch_preset` 的 `createNew=true` 会插入新方案，不覆盖源方案。
- 每个方案首次启动时分配独立 lifecycle 目录、`CODEX_HOME` 和 Electron 数据目录；每个方案最多一个活动 App 运行绑定。
- 方案删除是软删除；运行实例、请求记录和历史配置没有因切换方案而自动删除。

本轮已经覆盖的原三条缺口：

- `workspace-surface.tsx` 已渲染固定 Tab 和新增入口；UI 与后端都执行 5 个空间上限。
- 创建输入曾携带 `sourcePresetId`/`loginProviderId` 并复制源渠道；2026-11-07 起改为创建空方案，不再传源（见[多实例文档](../../designs/2026-10-04-multiple-managed-instances.md)）。
- 已增加旧空间 A 规范化、五空间上限、目录/实例不重建、默认 App 未使用时的偏好兜底测试；真实双空间实机验收仍待发布候选。

### 本版产品合同

- 未删除 `managed_app` 方案最多 5 条，对应固定名称「隔离空间 A」到「隔离空间 E」。名称不开放编辑，也不按 UI 数组下标动态生成。
- 现有旧方案保留原 ID 和数据；升级时只在事务内把显示名规范化到稳定槽位。软删除空间释放槽位，新建时使用最小空闲槽位。
- Provider 定义、API Key、探针结果和模型目录是全局共享资源；空间拥有自己的 Provider 绑定、顺序、模型选择、默认调用模型、API 登录渠道和自动审核选择。
- 创建空间创建空方案（无渠道、无模型、无 `loginProviderId`，标记 `needs_model_selection`），由用户自行添加；不再复制当前选中空间（2026-11-07 commander 调整，见[多实例文档](../../designs/2026-10-04-multiple-managed-instances.md)）。创建动作必须是单事务；达到 5 个时返回 `managed_app_limit_reached`，源空间不变化。
- 管理 App 的改名命令在 0.1.7 返回 `managed_app_name_fixed`；前端不显示改名入口。Provider 改名和凭据更新仍是全局渠道库操作，会反映到所有引用该 Provider 的空间。
- 首版沿用 schema v21/v23，不新增要求旧版拒绝的 schema 版本。槽位由服务层从现有 `name` 值和未删除方案集合分配。

## 二、不可破坏的旧实例合同

发布多实例功能时，以下约束必须是硬门槛：

1. **不重建旧方案。** 现有未删除 `managed_app` 行的 ID、版本、`is_preferred`、composition 和历史引用保持不变。显示名可以在规范化事务中改成对应的固定槽位名称，但不能把旧方案复制成“隔离空间 A”后删除原行。
2. **不重绑旧实例。** 既有 `codex_instances.launch_preset_id`、`launch_configuration_revision_id`、runtime binding、`managed_codex_home` 和 `lifecycle_dir` 不改写、不迁移到新方案。
3. **新增方案只插入。** 创建第二个实例必须生成新的 preset ID；源方案的版本、渠道、模型、登录渠道和运行状态不变。写入应在一个 SQLite 事务内完成，失败时源方案完全不变。
4. **新增运行目录只分配新的目录。** 新方案首次启动时才创建新的 lifecycle、`CODEX_HOME`、SQLite 会话根和 Electron 数据目录；不得复用旧方案目录，也不得复制用户默认 `~/.codex` 的认证或会话数据库。
5. **迁移不清理用户数据。** 多实例发布不能删除 `codex-instances`、`managed-app-sqlite` 遗留目录、旧会话库、旧运行记录或软删除方案。清理只能由用户确认的实例删除动作触发。
6. **保留缺凭据可见性。** Provider 缺凭据时，旧方案仍显示为未配置并可进入补 Key 流程；不能再次因 revision 状态把整个方案过滤掉。
7. **旧版本可安全回退。** 不新增必须由旧客户端拒绝的 schema 版本；多实例 UI 应建立在现有 schema v21/v23 合同上。若需要回退，只隐藏入口或恢复旧二进制，不能把 schema 降回 v20。

## 三、0.1.7 实施顺序

0.1.7 采用同版交付，但按以下顺序合入和验证，任何一层失败都不能进入发布候选。

预计修改面：后端集中在 `src-tauri/src/managed_launch/models.rs`、`workspace.rs`、`tests_workspace.rs`、`src-tauri/src/commands/launch.rs` 和 `src-tauri/src/codex_control/presets.rs`；控制面类型与状态在 `src/codex-control.ts`、`src/use-app-controller.ts`、`src/App.tsx`；Tab 和空间卡片在 `src/workspace-surface.tsx`、`src/workspace-surface.css`、`src/components/isolated-app/isolated-app-info.tsx` 及其样式；开发夹具在 `src/dev-control-fixture.ts`；恢复入口继续涉及 `src/components/model-utility/add-channel-dialog.tsx` 与 Provider Store。执行计划和 UI 设计记录同步更新，版本文件最后统一改为 0.1.7。

### 1. 数据与服务层

- 在 `ManagedLaunchStore::create_preset` 内增加未删除 `managed_app` 计数和槽位分配：A-E 中选择最小空闲槽位；超过 5 条返回 `managed_app_limit_reached`。
- 增加一次性、幂等的槽位规范化：优先保留当前 preferred 方案为 A，再按 `created_at`/ID 稳定排序填充其它空闲槽位；只更新 `name`，不碰 ID、版本、composition、runtime binding 或目录。若发现既有未删除空间超过 5 条，保留全部数据并进入升级阻塞态，不自动删除或合并。
- 将创建输入扩展为明确的复制来源和登录渠道字段（建议 `source_preset_id`、`login_provider_id`），后端在同一事务内读取源 composition、Provider 顺序、模型选择、自动审核角色和 profile options。
- 新方案写入固定名称；旧方案只在专门的规范化步骤中改名，保留所有 ID、版本历史和运行引用。
- 创建成功后新方案成为 preferred；切换 preferred 只更新选择状态，不增加版本、不改 composition。
- 明确“共享渠道定义、空间独立绑定”：ProviderStore 只维护全局 Provider；`launch_preset_api_providers`、`launch_preset_model_selections`、profile options 只维护单个空间。
- 添加后端回归测试：五个空间成功、第六个失败且事务回滚；复制多渠道空间保留 login provider；A/B 绑定互不影响；删除后复用空闲槽位；旧 preset/instance/home 引用不变。

### 2. 控制器与类型

- `LaunchPresetSummary` 增加稳定槽位/显示名来源的前端字段，或直接把后端固定名称作为唯一展示值。
- `workspaceInput` 仅服务编辑路径（更新请求携带 `launchPresetId`）；新建走独立的空方案输入，不携带 `sourcePresetId`/`loginProviderId`（2026-11-07 调整）。
- `useAppController` 统一维护当前 preset ID；刷新后优先恢复 preferred，创建成功后选中新 ID，选择失败时回滚本地选择。
- 将达到上限、空间不存在、空间运行中和凭据缺失转换为用户可理解的状态提示。
- 删除 managed_app 的改名调用和双击编辑状态；收到旧调用时显示固定名称提示，不修改数据库。

### 3. 首页 Tab UI

- 在 `workspace-surface.tsx` 真正渲染空间 Tab：A-E 固定标签、当前选中态、运行态标记和“新增隔离空间”按钮。
- Tab 选择只调用 `select_managed_launch_preset`；所有编辑、启动、实例详情、请求记录和打开目录都继续使用当前 preset ID。
- 新增按钮在 5 个空间时禁用并显示“最多 5 个隔离空间”；创建期间防重复点击，成功后自动切到新 Tab。
- 删除入口沿用现有软删除/实例删除合同；删除不重排已有字母，释放的槽位只供下一次新建复用。
- 空工作区仍能从全局渠道库选择已有 Provider；加入后只写入当前空间的绑定。

### 4. 运行与存储验证

- 旧方案重启继续复用原 runtime binding 和原 `CODEX_HOME`；升级或切 Tab 不重新分配实例 ID。
- 新方案首次启动才创建新的 runtime binding、lifecycle、`CODEX_HOME` 和 Electron data；两个方案同时启动时目录和 Gateway binding 必须不同。
- legacy SQLite 到目标 home 的迁移仍按现有一次性合同执行；目标已有状态库时返回冲突且不覆盖源文件。
- 关闭控制面、Gateway 重启、实例重连、停止和删除都按 `instance_id` 处理，不能用 preferred 方案代替真实绑定。

## 四、发布前验收证据

### 数据与迁移

- 在复制的旧数据库上运行升级：记录升级前后 `launch_presets`、`codex_instances`、runtime bindings、每个实例目录和关键文件哈希。
- 断言升级不改变旧 preset ID、旧 instance ID、`launch_preset_id`、home 路径、Electron 路径和会话库文件。
- 覆盖 1 条未删除旧方案、多个软删除方案、多个历史实例、1 个运行中实例、缺凭据 Provider、空工作区和五空间上限。
- 注入迁移/插入失败，确认事务回滚且源方案和旧目录不变。

### 后端与并发

- 创建 B 后，A 的 version/composition/login provider 完全不变；B 的 ID、composition、固定名称和 preferred 状态正确。
- A-E 五个方案可以创建；第六个方案被 UI 和后端拒绝，已有五个方案完全不变。
- 两个方案同时启动：两个 `instance_id`、lifecycle、`CODEX_HOME`、Electron data、RouteSet 和 Gateway binding 均不同。
- A 修改 Provider 绑定、模型、默认模型或登录渠道后，B 的对应 composition 完全不变。
- 同一方案重复启动仍返回 `managed_app_in_use`；不同方案可以并行启动。
- 运行中编辑只影响下次启动；停止、重启、调和和删除按方案/实例精确作用。

### UI 与实机

- macOS：升级后旧实例显示为 A 且会话仍可打开；创建 B-E 并同时运行两个空间；关闭一个空间不影响另一个。
- Windows：安装更新后旧实例目录和 App 数据仍在；两个隔离 App 使用不同 Electron 用户数据目录；卸载不删除 `%APPDATA%\io.rcodex`。
- 前端检查、生产构建、Rust 格式/编译/库测试、数据库迁移测试全部通过；再做真实 macOS 和 Windows 桌面验收。

## 五、回滚与停止发布

- 发现旧方案缺失、旧目录被重命名、实例绑定错配、登录渠道丢失或两个实例共享会话目录时，立即停止发布，不做现场手工重建。
- 回滚优先恢复上一版应用；不要删除或改写 SQLite、`codex-instances`、隔离 `CODEX_HOME` 或备份文件。
- 若只需撤回多实例 UI，可关闭入口并保留后端 `createNew` 数据；已创建的新方案不自动删除。
- 生产修复前先复制数据库并确认 App/Gateway 已退出；保留可回滚备份路径和校验值。

## 六、当前未纳入首版的事项

- 软删除空间的 UI 恢复入口继续不做；恢复仍可作为后续管理功能单独设计。
- 空间数量上限固定为 5；首版不做无限扩展、拖拽排序或自定义名称。

