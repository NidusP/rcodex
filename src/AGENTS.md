# src/ 前端控制面约定

> 适用范围：`src/`（React + TypeScript + Mantine 控制面）。跨端合同（路由、Gateway、写入边界、安全隐私、发布流程）见仓库根 [AGENTS.md](../AGENTS.md)；Rust 侧见 `src-tauri/AGENTS.md`。

## 交互与状态

- 主界面只有 R Codex 工作区；主按钮只显示图标，hover「启动」。模型渠道、运行实例和诊断按需打开；项目导入在主界面显式扫描。
- UI 展示模型来源、能力、路由和错误层级时必须使用用户可理解的文案；内部 ID 只用于精确关联。
- loading、空态、失败态、冲突态、禁用态和成功反馈都要有明确状态，不以静默刷新或无内容页面代替。
- Provider、工作区、模型目录和启动动作遵循现有单一写入事务；不要在 renderer 中直接修改 SQLite、配置文件或凭据。
- UI 视觉和交互改造先更新 `docs/designs/` 中的方案与验收，再修改 `src/`。

## 组件定义

- 业务与可复用界面放在 `src/components/<feature>/`。`index.ts` 是唯一对外出口；内部文件不要被其他功能目录直接引用。跨功能纯函数可以留在 `src/components/*.ts`，不要为单个 helper 再套空目录。
- `src/main.tsx` 与 `src/App.tsx` 只做根组装，不要在根组件里写抽屉或弹层 JSX。仍留在 `src/` 根下的面板下次改动时迁入对应功能目录，不要继续扩大这些文件。
- React 组件写 `@component` 名称和 `@description` 用途。Hook 用 `@function`。纯函数、类型和常量用普通 JSDoc，不要标成组件。
- 按钮、菜单、列表、Tab、选择器、通知、抽屉、对话框优先用 Mantine，再用现有 className 覆盖外观，不要再写一套原生弹出层。Mantine-first 也适用于组件交互：优先使用 Mantine 提供的键盘导航、焦点管理、禁用态、ARIA 语义和响应式行为；只有 Mantine 没有对应能力时才新增自定义交互，并在相关设计记录中说明原因。控制面抽屉走 `ControlDrawer`：避开标题栏、带遮罩。确认和启动弹层走 `ControlDialog`。展示组件通过 `useAppState()` 或父级回调读写，不直接 `invoke` 业务 command，也不改 SQLite 或凭据文件。

## 样式与 Mantine 覆盖

- Mantine 类名前缀是 `rcodex-`（非 `mantine-`）：hover 选择器等必须用 `rcodex-` 前缀类名；覆盖 Mantine 变体样式须提高特异性。新增覆盖规则用无头探针实测计算样式，不能只看代码推断（教训记录见 `docs/ui-change-log.md` 2026-11-07）。
- 样式放组件旁 CSS（如 `src/workspace-surface.css`、`src/components/model-utility/drawer-layout.css`），不再新建全局 styles.css / surface.css。

## 验证

- UI 变更：`pnpm run check:ui`、`pnpm run build` 和真实浏览器的主要成功/失败路径。
- 夹具回归：`pnpm run ui-check`（自动起停 vite，跑 workspace-ui-check 与 model-exposure-ui-check 两份）。
