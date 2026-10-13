import { defineConfig } from "astro/config";
import solid from "@astrojs/solid-js";
import starlight from "@astrojs/starlight";

const repository = process.env.GITHUB_REPOSITORY?.split("/")[1] ?? "rcodex";
const owner = process.env.GITHUB_REPOSITORY_OWNER ?? "NidusP";
const base = process.env.PUBLIC_BASE ?? (repository.endsWith(".github.io") ? "" : `/${repository}`);

export default defineConfig({
  site: process.env.PUBLIC_SITE_URL ?? `https://${owner}.github.io${base}`,
  base,
  integrations: [
    solid(),
    starlight({
      title: "R Codex",
      description: "专为 Codex 做的本机路由转发工具：多渠道模型目录，按模型精确转发。",
      defaultLocale: "root",
      locales: {
        root: { label: "简体中文", lang: "zh-CN" },
      },
      logo: {
        src: "./src/assets/app-icon.png",
        alt: "",
      },
      favicon: "/favicon.png",
      customCss: ["./src/styles/site.css"],
      sidebar: [
        {
          label: "开始使用",
          items: [
            { label: "产品首页", slug: "index" },
            { label: "快速开始", slug: "getting-started/quick-start" },
            { label: "工作原理", slug: "getting-started/how-it-works" },
          ],
        },
        {
          label: "使用指南",
          items: [
            { label: "默认 App", slug: "guides/default-app" },
            { label: "隔离环境配置", slug: "guides/isolated-config" },
            { label: "模型渠道与模型", slug: "guides/providers" },
            { label: "一次桌面文件测试", slug: "guides/field-note" },
            { label: "Gateway 与 502", slug: "guides/gateway-502" },
          ],
        },
        {
          label: "了解边界",
          items: [
            { label: "隐私与安全", slug: "reference/privacy" },
            { label: "路线图", slug: "roadmap" },
          ],
        },
      ],
    }),
  ],
});
