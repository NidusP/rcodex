# website/ 官网子项目约定

> 适用范围：`website/`（Astro + Starlight + Solid.js 静态官网）。产品合同见仓库根 [AGENTS.md](../AGENTS.md)；内容映射、构建与发布约束见 `docs/website-plan.md`。

- 纯静态展示：页面与「请求如何流转」交互示意均为说明性内容，不连接本机 Gateway，不发送真实请求，不读取产品数据或凭据。
- 开发与构建在 `website/` 内执行：`pnpm --dir website dev`（仓库根别名 `pnpm run website:dev`）、`pnpm --dir website build`。
- 发布走 GitHub Pages：Pages 未开启时 website workflow 会在「Require GitHub Pages」守卫步快速失败——这是预期行为，不影响产品 Release。
- logo / favicon 等资产在 `website/src/assets/` 与 `website/public/`；变更资产后同步 `astro.config.mjs` 配置。
