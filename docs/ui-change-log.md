# UI 变更留痕

> 状态：active（最后核对 2026-11-08）
> 状态说明：本页承接 AGENTS.md 原「UI 分步改造留痕」。每完成一项 UI 改造在此追加一条；规则与验收仍以 AGENTS.md「前端与交互规则」和对应 designs/ 文档为准。

按 demo.html 的布局和样式改真实控制面，不挂载静态演示页。详细验收在 docs/archive/designs/3rcodex-ui-quality-upgrade-plan.md。崩溃后续读本节即可接着做。

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
- 2026-10-08：隔离空间控制带去掉「更多」菜单。实例详情、请求记录、打开隔离目录、打开默认 Codex 改为纯图标按钮，Tooltip 说明动作。导入项目会话和按清单同步设置不再从首页挂载，后端命令保留。src/components/isolated-app/isolated-app-info.tsx、docs/archive/designs/2026-10-03-homepage-obsidian-restyle.md、docs/archive/designs/3rcodex-ui-quality-upgrade-plan.md、AGENTS.md。
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
- 2026-10-17：Windows 关窗口仍隐藏。托盘「退出」走 taskkill 停止隔离实例、launcher 和 Gateway。src-tauri/src/codex_control/lifecycle.rs、src-tauri/src/support/process.rs。
- 2026-10-17：隔离 config 路径经 join_config_path 拼装。src-tauri/src/managed_codex_config/io.rs、computer_use.rs、materialize.rs。
- 2026-10-11：程序坞「退出」和 Cmd+Q 与菜单栏「退出」走同一条清理。macOS 拦截 `applicationShouldTerminate:`，取消系统 terminate，改发 `exit(0)`，先停隔离实例再停 Gateway。红点关闭仍只隐藏。src-tauri/src/macos_quit.rs、src-tauri/src/lib.rs。
- 2026-10-10：恢复系统标题栏。去掉网页自绘标题栏、macOS 交通灯改位和 Windows 自绘窗口按钮。失联时的「重启」放在页面「R Codex」右侧。主题和更新也留在这一行。
- 2026-10-10：初版收口。实例详情和请求记录禁用。明暗和强调色入口隐藏。错误通知不弹出。见 docs/archive/designs/2026-10-10-initial-release-surface.md。
- 2026-10-10：初版固定浅色，不跟随系统，也不读取已保存明暗。index.html、src/app-model.ts、docs/archive/designs/2026-10-10-initial-release-surface.md。
- 2026-10-10：添加渠道不再确认。下拉选中后直接加入当前隔离空间，并排在第一个。
- 2026-10-10：系统标题栏不再显示「R Codex」。窗口标题仍保留给系统和菜单，页面里的「R Codex」和重启按钮不动。
- 2026-10-10：隔离 App 运行时，控制带 App 按钮直接显示停止图标，不再悬停才从播放换成停止。未运行时仍是播放图标。src/workspace-surface.css。
- 2026-11-07：每个隔离空间 tab 名称后放各自的垃圾桶：可删除时淡色、hover 加深；该空间有活动实例、正在删除或更新时禁用并悬停显示原因；点击直接弹对应空间确认框，不切换选中。行尾独立删除按钮移除；「新增隔离空间」改纯 + 图标。后端删除非选中空间保持当前选中（新回归用例），夹具同步。src/workspace-surface.tsx、src/workspace-surface.css、src/components/isolated-app/isolated-app-info.tsx、src/dev-control-fixture.ts、src-tauri/src/managed_launch/deletion.rs、src-tauri/src/managed_launch/tests_deletion.rs、scripts/workspace-ui-check.mjs、docs/designs/2026-11-07-workspace-order-and-deletion.md。
- 2026-11-07：「添加渠道」的待分配上游仍列出未透出模型的渠道，但状态列显示「不可用」（红点红字），悬停经 disabled-hint 包装层 + Mantine Tooltip 提示先透出模型；未配置/协议未确认等其他阻塞同样显示不可用并给出各自原因。src/workspace-surface.tsx、src/workspace-surface.css、scripts/workspace-ui-check.mjs、docs/designs/2026-11-07-model-exposure-image-probe.md。
- 2026-11-07：空间 tab 垃圾桶改为常态隐藏，仅可删除空间 hover tab 时显示（不可删除空间 display:none）；「新增隔离空间」改为创建空空间（无渠道/模型/登录渠道，needs_model_selection 选好前不可启动），不再复制当前空间，createManagedInstance 不传 sourcePresetId，新增 Rust 回归用例 create_new_empty_preset_starts_as_needs_model_selection；+ 按钮去边框。Mantine 9 类名前缀为 rcodex-：hover 选择器须用 rcodex-Tabs-tab，覆盖 Mantine 变体样式须提高特异性（workspace-tabs 三级选择器），已用无头 Chrome 真实鼠标悬停断言验证。src/use-app-controller.ts、src/App.tsx、src/components/isolated-app/isolated-app-info.tsx、src/workspace-surface.css、src-tauri/src/managed_launch/tests_workspace.rs、scripts/workspace-ui-check.mjs、docs/designs/2026-10-04-multiple-managed-instances.md、docs/designs/2026-11-07-workspace-order-and-deletion.md。

