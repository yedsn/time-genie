# GitHub Pages 部署

Timegenie 使用 VitePress 构建文档站，并通过 GitHub Actions 发布到 GitHub Pages。

站点地址通常为：

<https://yedsn.github.io/time-genie/>

## 本地预览

```bash
npm run docs:dev
npm run docs:build
npm run docs:preview
```

## 自动发布

将提交推送到 `main` 分支后，`.github/workflows/pages.yml` 会在文档或依赖配置变化时运行：

1. 安装 Node.js 依赖。
2. 使用仓库名设置 VitePress 的 `base` 路径。
3. 构建 `docs/.vitepress/dist`。
4. 上传 GitHub Pages artifact 并部署。

首次启用时，在 GitHub 仓库的 **Settings -> Pages** 中将发布来源设置为 **GitHub Actions**。

## 路径规则

本地开发默认使用 `/`。GitHub Pages 项目站点使用 `/time-genie/`，由工作流中的 `DOCS_BASE` 传入，不需要在本地配置中手动切换。
