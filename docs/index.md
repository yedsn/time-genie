---
layout: home
hero:
  name: "时序 TimeGenie"
  text: "本地优先的桌面工作台"
  tagline: 管理待办、记录与分配工时、生成日报/周报/月报，并与 Obsidian、SeaTable、Supabase 衔接。
  image:
    src: /logo.svg
    alt: "Project logo"
  actions:
    - theme: brand
      text: 数据结构设计
      link: /20260924_timegenie_数据结构设计
    - theme: alt
      text: 后端接口设计
      link: /20260924_timegenie_后端接口设计
features:
  - title: 工作闭环
    details: 从今日待办、计时、工时分配到日报、周报、月报都在同一个桌面应用中完成。
  - title: 本地优先
    details: 默认使用本机 SQLite，敏感 Token 和登录会话保存到系统凭据库，可选接入 Supabase 多设备同步。
  - title: 外部工具衔接
    details: 支持从 Obsidian 导入计划、把报告写回 Obsidian，也可以将待办同步到 SeaTable。
---

## 从这里开始

- 查看 [`数据结构设计`](/20260924_timegenie_数据结构设计)
- 查看 [`后端接口设计`](/20260924_timegenie_后端接口设计)
- 查看 [`GitHub Release 发布说明`](/github-release)
- 查看 [`GitHub Pages 部署`](/develop/github-pages)
- 本地预览文档：`npm run docs:dev`
- 构建文档站：`npm run docs:build`
