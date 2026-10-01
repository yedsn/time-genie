import { defineConfig } from "vitepress";

const githubRepo = "https://github.com/yedsn/time-genie";
const repoName = process.env.GITHUB_REPOSITORY?.split("/")[1] ?? "time-genie";
const base = process.env.DOCS_BASE ?? (process.env.GITHUB_ACTIONS === "true" ? `/${repoName}/` : "/");
const siteDescription = "时序（TimeGenie）是一个 Windows 优先的本地优先桌面工作台，用于管理待办、记录与分配工时、生成日报/周报/月报，并与 Obsidian、SeaTable、Supabase 衔接。";

export default defineConfig({
  title: "时序 TimeGenie",
  description: siteDescription,
  lang: "zh-CN",
  base,
  cleanUrls: true,
  lastUpdated: true,
  head: [
    ["link", { rel: "icon", type: "image/svg+xml", href: `${base}logo.svg` }],
    ["meta", { name: "theme-color", content: "#1a2d24" }],
    ["meta", { property: "og:type", content: "website" }],
    ["meta", { property: "og:title", content: "时序 TimeGenie" }],
    ["meta", { property: "og:description", content: siteDescription }],
  ],
  themeConfig: {
    logo: "/logo.svg",
    nav: [
      { text: "首页", link: "/" },
      { text: "设计文档", link: "/20260924_timegenie_数据结构设计" },
      { text: "开发说明", link: "/develop/github-pages" },
      { text: "发布说明", link: "/github-release" },
      { text: "GitHub", link: githubRepo },
    ],
    sidebar: {
      "/": [
        {
          text: "项目文档",
          items: [
            { text: "首页", link: "/" },
            { text: "数据结构设计", link: "/20260924_timegenie_数据结构设计" },
            { text: "后端接口设计", link: "/20260924_timegenie_后端接口设计" },
            { text: "GitHub Release 发布", link: "/github-release" },
          ],
        },
        {
          text: "开发说明",
          items: [{ text: "GitHub Pages 部署", link: "/develop/github-pages" }],
        },
      ],
      "/develop/": [
        {
          text: "开发说明",
          items: [{ text: "GitHub Pages", link: "/develop/github-pages" }],
        },
      ],
    },
    socialLinks: [{ icon: "github", link: githubRepo }],
    search: { provider: "local" },
    editLink: {
      pattern: `${githubRepo}/edit/main/docs/:path`,
      text: "在 GitHub 上编辑此页",
    },
    outline: { label: "页面导航" },
    docFooter: { prev: "上一页", next: "下一页" },
    lastUpdated: { text: "最后更新于" },
    footer: {
      message: "时序 TimeGenie 文档站",
      copyright: "Copyright 2026 TimeGenie Contributors",
    },
  },
});
