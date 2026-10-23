---
layout: home
hero:
  name: "TimeGenie（时序）"
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
    details: 默认使用本机 SQLite；也可选择 Supabase 在线云端空间。两个空间独立，云端直接读写、断网只读，不产生本地待同步队列。
  - title: 外部工具衔接
    details: 支持从 Obsidian 导入计划、把报告写回 Obsidian，也可以将待办同步到 SeaTable。
---

## 从这里开始

- 查看 [`数据结构设计`](/20260924_timegenie_数据结构设计)
- 查看 [`后端接口设计`](/20260924_timegenie_后端接口设计)
- 查看 [`多设备计时同步`](/timer-multi-device-sync)
- 查看 [`数据备份与恢复`](/data-protection)
- 查看 [`云端操作记录`](/cloud-operation-history)
- 查看 [`GitHub Release 发布说明`](/github-release)
- 查看 [`GitHub Pages 部署`](/develop/github-pages)
- 本地预览文档：`npm run docs:dev`
- 构建文档站：`npm run docs:build`
