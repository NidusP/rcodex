# 根组件拆分

> 状态：2026-10-04。App 只组装标题栏、隔离工作区、通知和功能目录导出的叠层。渠道/实例/诊断抽屉在 `src/components/model-utility`，启动确认在 `src/components/runtime-panels`。已删除 `utility-drawer.tsx`。

## 目标

[src/App.tsx](../../src/App.tsx) 继续持有本机快照、通知和全部写入。它的渲染不再把标题栏和首页写在一起，而是分成两个主组件，其余弹层再各成一块。新文件一律放在 `src/components`，不再使用 `shell`、`providers`、`instances` 这类目录。

不改用户能看到的行为，不改 Tauri 命令，不改工作区保存路径。

## App 里留下的两块

```tsx
<App>
  <TitleBar />
  <IsolatedAppInfo />
  <WorkspaceOverlays />
  <RuntimeOverlays />
</App>
```

`TitleBar` 是现在的窗口标题栏。品牌、Gateway 状态、CLI 与 App 版本、主题、刷新、Windows 窗口按钮留在这里。它不包含空间条，也不包含启动按钮。

`IsolatedAppInfo` 是隔离 App 这一整块，对应当前首页卡片里的内容：

- 基础信息：隔离实例标签、「新增实例」、模型渠道入口和已加入数量。
- 模型渠道：加入确认、拖拽排序、展开后勾选模型、移除和重新检测。
- 启动操作状态：默认调用模型、API 登录渠道、启动 CLI、启动 App、不可用原因，以及「更多」。
- 页脚状态条也放在这一块下面，显示 Gateway、监听地址和版本。它不是标题栏。

「更多」打开的实例记录、诊断、同步设置和项目导入仍由根组件提供回调。隔离 App 信息组件不自己写库。

渠道管理抽屉、实例抽屉、诊断抽屉、添加渠道弹窗、删除确认和启动对话框不是隔离 App 信息的内部段落。它们由 `App` 挂载功能目录导出的叠层，因为它们盖住首页，而不是首页里的一栏。

## 文件

| 文件 | 内容 | 从哪里来 |
| --- | --- | --- |
| `src/components/title-bar/` | 标题栏 | `App` 返回里的 `window-titlebar` |
| `src/components/isolated-app/` | 隔离 App 信息的组装和页脚状态条 | `WorkspacePanel` 加 `ChannelWorkspaceCard` |
| `src/components/model-utility/` | 模型渠道、实例、诊断抽屉和渠道确认层 | 原 `utility-drawer.tsx` 与渠道叠层 |
| `src/components/runtime-panels/` | 实例列表、启动确认和删除叠层 | `RuntimeInstanceList` 与运行时对话框 |
| `src/components/app-state/` | 控制面状态与 Mantine 主题 | `useAppController` 注入 |
| `src/components/hinted-button/` | 带禁用旁注的按钮 | 原禁用按钮封装 |
| `src/components/model-labels.ts` | 模型标签、渠道顺序、协议文案 | `App.tsx` 顶部的纯函数 |

[src/workspace-surface.tsx](../../src/workspace-surface.tsx) 在隔离 App 信息组装完成后删除，不保留一个平行入口。添加渠道弹窗、启动加载、会话导入和完整档案已经是单独文件，这次只把它们的 import 留在使用方，不改内部。

`src/workspace-model-selection.ts` 继续只管勾选和公共 ID。

未渲染的官方账户界面、旧诊断列表、更新卡片，以及打不开的方案详情抽屉，不放进 `src/components`。拆分时保持原样；要删另开一次审核。

## 不做什么

- 不把刷新、创建渠道、勾选模型、启动和停止实例拆成多个写入入口。隔离 App 信息和抽屉只接收回调。
- 不引入新的状态库，也不把 SQLite 或凭据读写放到 `src/components`。
- 不改首页从上到下的顺序：标题栏、空间条、渠道列表、控制带。
- 不把命令函数收成 hook。若搬完界面后根组件仍难读，再单独审核一份显式参数的普通函数文件。

## 顺序

`src/App.tsx` 每一波只在集成时改。同一波次的新文件不互相引用，除了都引用 `model-labels.ts`。

1. 新建 `src/components/model-labels.ts`，根组件改为引用它。
2. 同时搬：渠道抽屉列表、实例列表、诊断板、确认与启动对话框、状态条。这些都不改隔离 App 信息的 JSX。
3. 集成第 2 步，类型检查通过后再动首页。
4. 串行抽出标题栏，再抽出隔离 App 信息。控制带和渠道列表作为隔离 App 信息的内部文件，在这一步一起接线，避免首页出现两个组装入口。
5. 删除已经搬空的 `workspace-surface.tsx` 和 `channel-sort-list.tsx`。

## 验收

- `pnpm run check` 通过。
- `App` 的主路径能看出标题栏和隔离 App 信息两个组件。
- 新界面文件都在 `src/components`。
- 标题栏、空间条、渠道勾选、启动 CLI、启动 App、「更多」和三个抽屉的行为不变。
- 不运行 `pnpm run tauri:dev` 或打包。

## 风险

隔离 App 信息如果自己调用 `create_managed_launch_preset` 或 `test_api_key_provider`，就会和根组件形成第二条写入路径。渠道勾选、排序、启动和检测必须仍是根组件传入的函数。

第 2 步若各自复制 `model-labels.ts` 里的函数，后续改协议文案会漏改。第 1 步完成前不开始第 2 步。

## 第一版

- `src/components/title-bar.tsx`：窗口标题栏。
- `src/components/isolated-app-info.tsx`：隔离 App 信息，包含空间、渠道、启动操作，并带上页脚状态条。
- `src/components/status-bar.tsx`：页脚状态。
- `src/components/model-labels.ts`：模型标签、渠道顺序、协议文案。
- 根组件改为渲染 `TitleBar` 和 `IsolatedAppInfo`。写入命令仍留在 `App`。
- 渠道列表、实例列表、诊断板和确认框还没搬出 `App.tsx`。

## 控制器

`App` 现在只渲染 `AppView`，并把 `useAppController()` 的结果传进去。刷新、渠道写入、工作区保存、启动和实例操作都在 `useAppController` 里。`AppView` 只负责把标题栏、隔离 App 信息、抽屉和确认框接上。

## 控制器已移出

业务命令在 [src/use-app-controller.ts](../../src/use-app-controller.ts)。类型和纯函数在 [src/app-model.ts](../../src/app-model.ts)。`App` 只把控制器结果交给 `AppView`。`AppView` 负责标题栏、隔离 App 信息，以及 `WorkspaceOverlays` / `RuntimeOverlays`，文件里不再调用 `invokeControl`，也不再内联抽屉 JSX。
