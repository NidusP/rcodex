# 3R Codex 官网计划

## 目标

- 使用 Astro + Starlight 建立面向用户的静态官网。
- 使用 Solid.js 承载一个轻量的“请求如何流转”交互示意。
- 将现有 README、产品原理、默认 App 回环代理、Provider 兼容性和发布计划整理为用户可理解的内容。
- 官网通过 GitHub Pages 发布，桌面应用仍保持 React + Tauri 实现，二者不共享运行时代码。

## 内容范围

- 首页：产品定位、核心能力、数据流、默认 App 主路径、隐私承诺和下载/源码入口。
- 快速开始：安装、添加 Provider、检测模型、应用默认 App 配置、启动和重启。
- 工作原理：控制面、Gateway 数据面、Provider 路由、模型公共 ID。
- 默认 App：兼容模式、系统代理/502、恢复规则和历史边界。
- 隔离环境配置：介绍隔离目录里由 3R 写入的路由、`multi_agent_v2` 布尔开关、只复制一次的偏好，以及非 GPT 不广告 `original`。不把签名发布写成已完成。
- Provider：API Key、模型目录、Responses/Chat 能力和检测失败恢复。
- 兼容说明（后补，不在本轮写页面）：说明 3R Codex 为让各类模型适配 Codex 所做的目录处理。例如非 GPT 系列不广告 Codex 自己的图片档 `original`。一次探针通过不写成该模型族支持这个档。
- Gateway 诊断：错误层级、常见 502、超时、SSE 和脱敏记录。
- 隐私边界：凭据、会话、日志和默认 App/隔离 App 的归属。
- 路线图：隔离 App 真实桌面验收、旧会话兼容诊断、Provider 预设、官方账户和 Windows 发布门。

## 非目标

- 官网不连接本机 Gateway，不读取本地配置，不提供在线模型请求。
- 官网不承诺隔离 App、官方订阅模型或三方旧会话兼容已经完成。
- 首版不复制桌面端业务状态管理，也不把现有 Tauri React 页面改造成网站。

## 技术方案

- Astro 静态构建；Starlight 负责文档导航、搜索和 Markdown 页面。
- `@astrojs/solid-js` 只用于客户端交互组件，默认页面保持静态输出。
- GitHub Actions 使用 Pages artifact 部署；`site` 和 `base` 通过环境变量或配置集中管理。
- 自定义 CSS 使用 3R Codex 的冷白、深墨和青绿色语义变量；不使用玻璃卡片作为主要信息承载。

## 验收标准

- 用户在首页 30 秒内理解 3R Codex 的用途、默认 App 路径和 Gateway 位置。
- 用户可以从导航进入快速开始、默认 App、Provider、502 诊断、隐私和路线图。
- 页面在窄屏下无横向溢出，键盘可以操作导航和 Solid.js 交互示意。
- 文案与当前代码、产品原理和默认 App 发版计划一致；暂缓能力明确标注。
- `pnpm --dir website build` 成功，GitHub Pages workflow 配置完成。

## 执行

- [x] 创建官网内容计划和文档映射。
- [x] 创建 Astro + Starlight + Solid.js 工程。
- [x] 完成首页和核心用户文档。
- [x] 配置 GitHub Pages workflow 和项目站点路径。
- [x] 执行类型检查、生产构建和生成链接检查；浏览器预览受当前环境 URL 安全策略限制。

## 实际交付

- 官网工程：`website/`。
- 首页交互：`website/src/components/FlowSwitcher.tsx`，使用 Solid.js 客户端 hydration。
- 视觉样式：`website/src/styles/site.css`，使用不透明内容卡和响应式布局。
- 发布 workflow：`.github/workflows/website.yml`。
- 验证：`ASTRO_TELEMETRY_DISABLED=1 pnpm run check` 通过；`ASTRO_TELEMETRY_DISABLED=1 pnpm run build` 通过，生成 9 个页面和 Pagefind 搜索索引。
- 限制：构建仍提示 Starlight 未配置专用 `docs/404` 条目，但已正常生成通用 `dist/404.html`；当前浏览器工具禁止访问本地文件或本地服务 URL，未完成交互截图验收。

## 尚未发布

- 隔离实例的上下文压缩说明。来源是 [隔离实例的上下文压缩](designs/2026-10-08-isolated-context-checkpoint.md) 里「可以放到官网的说明」。只说明 Codex 本地压缩、R Codex 不改写续跑；不放请求编号和内部合同名。当前不加入 `website/`。
