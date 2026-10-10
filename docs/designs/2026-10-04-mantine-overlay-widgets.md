# 用 Mantine 承接下拉与失败提示

> 状态：已实现。2026-10-04。按钮、Tab、Menu、List、Checkbox、Select、Notification 已改用 Mantine，窗口系统按钮除外。pnpm run check 通过。未启动 Tauri。

## 选择

控制面引入 Mantine（@mantine/core）。Vite + React 的 Tauri 应用常用这一套。Select 用 Floating UI，默认 Portal，不会被底部 overflow 裁切。外观用主题变量和 classNames 覆盖，对齐现有青黑控制面。

## 范围

- 选择器改为 Mantine Select。底部默认模型、API 登录渠道，以及诊断抽屉的渠道筛选都走这一层。
- 最近失败条 incident-bubble 改为 Mantine Notification。点击仍关闭并打开诊断。
- 不把渠道加入菜单、更多菜单、窗口按钮改成 Mantine。

## 验收

- 底部两个选择器能打开并点中选项。
- 失败提示仍固定在标题栏下，点击进入诊断。
- pnpm run check 通过。不启动 Tauri。

## 后续

- 产品按钮走 HintedButton（Mantine Button / ActionIcon）。
- 实例标签和诊断页签走 Tabs。
- 添加渠道和更多走 Menu。
- 诊断列表、导入预览走 List。
- Windows 标题栏最小化/最大化/关闭仍是 ActionIcon，保留系统按钮样式。

## 模型渠道全屏抽屉

Mantine Drawer 自带 Overlay 遮罩。模型渠道使用 `size="100%"` 铺满标题栏以下区域，从右侧滑入。添加渠道等叠层抽屉 z-index 高于该全屏抽屉。实例和诊断仍用原来的侧栏抽屉。
