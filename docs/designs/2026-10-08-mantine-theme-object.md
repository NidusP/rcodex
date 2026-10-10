# Mantine 主题对象改造

状态：已实现。旧的 data-theme 覆盖已删除。亮暗和五个强调色由 Mantine createTheme 提供。

目标：控制面只保留一套主题。亮色和暗色都由 [Mantine theme object](https://mantine.dev/theming/theme-object/) 决定，用户还可以在明暗之外选择自己喜欢的强调色。

## 现在的问题

主题被拆成三层，互相同步靠手工覆盖：

- `src/styles.css` 把亮色写成大量十六进制色，再用约 400 处 `html[data-theme="dark"]` 覆盖暗色。
- `src/surface.css` 还有少量暗色覆盖。三份样式合计超过 1800 个色值。
- `src/components/app-state/mantine-theme.tsx` 的 `createTheme` 只设置了字体、圆角、`primaryColor: "cyan"` 和几个组件默认属性。Mantine 组件跟手写界面不是同一套颜色。
- 明暗状态在 `use-app-controller.ts`：`light | dark | system` 写入 localStorage，再写到 `documentElement.dataset.theme`。`RcodexMantineProvider` 用 `forceColorScheme` 跟随解析后的亮暗。标题栏里已经看不到切换入口，但存储和解析还在。

因此改一个强调色，现在要同时改 CSS 字面量和 Mantine 的 `primaryColor`，而且暗色规则不会跟着变。

## 原则

- 明暗只有一个事实来源：Mantine 的 `data-mantine-color-scheme`。不再新增 `data-theme` 规则。
- 颜色只有一个事实来源：`createTheme` 的 `colors`、`primaryColor`、`primaryShade`。手写 CSS 读 Mantine 已经生成的变量，例如 `--mantine-color-text`、`--mantine-color-body`、`--mantine-primary-color-filled`。
- 不在这一轮重做布局、抽屉、锁定或图标。只换颜色从哪里来。
- 不把「喜欢的颜色」做成任意十六进制编辑器。先提供一组已经排好 10 级色阶的强调色。Mantine 的一个颜色必须是 10 级元组，单色值不够。
- 系统减少动态效果继续有效。Mantine 过渡交给 `respectReducedMotion`，手写动画仍走现有媒体查询。

## 第一步：清掉现有主题设置

直接删除现有主题，不保留一套还能切换的明暗。清的过程中页面可以暂时丢色，不需要过渡期，也不把 `data-theme` 暂时映射到 Mantine。

删除范围：

1. 去掉 `documentElement.dataset.theme` 以及所有 `html[data-theme]`、`html[data-theme="dark"]` 选择器。明暗存储 `light | dark | system` 可以留到第二步再接上，但这一步不要求它还能改变外观。
2. 删除 `styles.css` 和 `surface.css` 里成对的亮色字面量和暗色覆盖。还没接到新变量的规则可以先失去颜色，不另写一套临时色。
3. 标题栏、窗口按钮和启动播放按钮上的旧主题色一并清掉。它们不是 Mantine 组件，但也不在清除阶段单独保活。

这一步结束时，旧主题设置不再存在。明暗是否好看，留到第二步用 Mantine API 重做，不作为清除完成的条件。

## 第二步：用 Mantine API 重做亮暗

在 `mantine-theme.tsx` 用 `createTheme` 写完整主题，而不是只留 `primaryColor`：

- `colors`：保留 Mantine 默认色阶，并显式放入当前青色 10 级，作为默认强调色。
- `primaryColor` 先固定为这组青色。`primaryShade` 按明暗分开，亮色用较深的一级保证白字对比，暗色用较亮的一级。
- `white`、`black`、`autoContrast`、`luminanceThreshold` 用来决定强调按钮上的文字，不再手写 `#071016`。
- `fontFamily` 继续 `inherit`。等宽字体放进 `fontFamilyMonospace`，给渠道地址和模型 ID。
- `defaultRadius` 维持 8。`cursorType` 维持 pointer。`focusRing` 用 `auto`。
- `respectReducedMotion: true`。
- `components` 继续放 Select、输入框和 Notification 的默认属性，不在这里写颜色。
- 需要 Mantine 没有的名字时，放进 `theme.other`，并在一处 CSS 里变成 `--rcodex-*`。不要再为暗色复制一份。

明暗切换继续用现在的 `light | dark | system` 和 localStorage，避免和 Mantine 自己的存储各记一份。解析结果只传给 `forceColorScheme`。系统模式仍监听 `prefers-color-scheme`。

手写界面改读这些变量：

- 页面底：`--mantine-color-body`
- 正文：`--mantine-color-text`
- 次要文字：`--mantine-color-dimmed`
- 卡片和输入底：`--mantine-color-default`
- 边框：`--mantine-color-default-border`
- 强调：`--mantine-primary-color-filled` 和 `--mantine-primary-color-filled-hover`
- 错误和警告：Mantine 的 red / yellow 色阶，不再各写一套暗红和琥珀。

亮色和暗色因此是同一批规则。Mantine 在 `data-mantine-color-scheme` 变化时改写变量。

## 第三步：增加喜欢的颜色

明暗稳定之后再加强调色。存储键和主题模式分开，例如 `rcodex-accent`。

首批只提供已经备好 10 级色阶的选项：青、青绿、蓝、紫、橙。默认青，对应现在的控制带。用户选中的名字写入 `primaryColor`。`createTheme` 在渲染时按这个名字重建，Mantine 按钮和读了主色变量的手写样式一起变。

不在这一阶段接受任意十六进制。一个自定义色要先生成可用的 10 级色阶并检查亮暗两种底上的对比，否则播放按钮和链接会不可读。那是后续，不和这次混在一起。

入口放在现有主题控制旁：先选跟随系统、亮色或暗色，再选一个颜色。没有独立主题页。

## 实现记录

- 清除时没有保留可用的旧明暗。dataset.theme 和 html[data-theme] 选择器已删除。旧暗色主题文件已删除。
- createRcodexTheme 使用 Mantine 默认 10 级色阶。primaryShade 亮色为 7，暗色为 4。明暗仍存在 rcodex.theme，强调色存在 rcodex-accent。

- 标题栏用明暗图标（亮色、暗色、跟随系统）和色块。色块打开 ColorPicker，选中的颜色展开成 10 级后作为强调色。组件尺寸入口已删除。按钮、图标按钮、复选框、输入、数字输入、选择、菜单和标签固定为 Mantine 最小档 xs。旧的 localStorage 键 rcodex-component-size 在启动时删除。首页底部状态条已删除，工作区左右留出内边距。默认调用模型和 API 登录渠道保留自己的边框。
- 手写样式改读 Mantine 变量。未逐个替换的剩余色值不再按 data-theme 分叉。


- 不再用全局 button 规则、.icon-button/.text-button/.primary-action 或 .rcodex-Button-root 去改 Mantine 组件的颜色和尺寸。按钮使用 Mantine variant，主色跟随当前强调色，尺寸固定为 xs。窗口按钮和页面布局仍保留必要的位置样式。
## 验收

- 跟随系统、亮色、暗色都能切换，刷新后保持。系统外观变化时，跟随系统会跟着变。
- 源码里不再有 `data-theme` 选择器，也不再写 `dataset.theme`。清除阶段不要求明暗仍可用。
- 标题栏没有组件尺寸菜单。刷新后按钮、输入、选择、菜单和标签仍是 xs，不读取以前保存的尺寸。
- 五种强调色在亮色和暗色下，启动按钮、链接、焦点和 Mantine 主按钮是同一种颜色。
- 渠道行、抽屉、确认弹窗、通知和标题栏没有掉回旧的硬编码青色，文字在强调底上可读。
- 减少动态效果时，Mantine 过渡和现有 CSS 动画都不播放。

## 不做

- 不改窗口标题栏的系统按钮行为。
- 不把通知、抽屉层级或业务色（成功、失败、警告）交给用户自定义。
- 不在这一轮删除仍被运行实例占用时的配置锁。

## 布局样式

2026-10-08 起不再使用 src/styles.css 和 src/surface.css。样式跟组件放在一起：App.tsx 旁边是 App.css，title-bar.tsx 旁边是 title-bar.css，其余界面同样处理。组件文件自己 import 这份 CSS。main.tsx 只保留 Mantine 自己的样式。

已经没有对应界面的旧规则没有搬走，包括侧栏、旧顶栏、旧工作区卡片和失效抽屉。标题栏拖拽、macOS 交通灯留白、Windows 标题按钮、渠道行、控制带、启动图标悬停和 Gateway 状态脉冲都留在对应组件的 CSS 里。Mantine 的 AppShell、Stack、Group 不能表达这些外观，所以没有改成只靠布局组件。
