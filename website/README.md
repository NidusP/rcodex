# R Codex 官网

这是 R Codex 的 Astro + Starlight + Solid.js 静态官网。桌面应用继续位于仓库根目录，官网只负责产品解释和用户文档，不连接本机 Gateway。

## 本地运行

```bash
pnpm install
pnpm run dev
```

检查和构建：

```bash
ASTRO_TELEMETRY_DISABLED=1 pnpm run check
ASTRO_TELEMETRY_DISABLED=1 pnpm run build
```

## GitHub Pages

`.github/workflows/website.yml` 会在 `main` 分支的 `website/**` 变化时构建并发布 Pages。`astro.config.mjs` 会根据 `GITHUB_REPOSITORY` 自动推导项目站点的 `site` 和 `base`；如果使用组织/用户站点仓库（例如 `owner.github.io`），会自动使用根路径。

首次启用时，在仓库 Settings → Pages 中将 Source 设为 GitHub Actions。发布前请检查 `site` 推导出的 GitHub 仓库地址与实际仓库一致。

## 内容来源

- 产品定位和能力：根目录 `README.md`。
- 架构、认证、默认 App 和会话边界：`docs/3rcodex-product-principles.md`。
- 默认 App 回环代理和 502：`docs/compat-app-loopback-proxy.md`。
- 当前范围和隔离 App 实机验收：`docs/designs/2026-09-21-isolated-only-product-plan.md`；默认 App 快速发版文档仅作历史记录。
- 官网页面内容：`src/content/docs/`。
