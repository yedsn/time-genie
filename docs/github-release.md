# GitHub Release 发布说明

## 发布流程

1. 确认工作区只包含本次发布要提交的改动。
2. 生成 Tauri updater 签名密钥：`npx tauri signer generate`。
3. 将公钥填入 `src-tauri/tauri.conf.json` 的 `plugins.updater.pubkey`。
4. 在 GitHub 仓库 Secrets 中配置私钥和密码。
5. 运行 `scripts/release/release.ps1 -Push`，或在 VS Code 中启动 `release`。
6. GitHub Actions 会根据 `v*` 标签构建 Windows NSIS、macOS Apple Silicon DMG、macOS Intel DMG，并上传到 GitHub Release。
7. 构建完成后，工作流会运行 `scripts/release/publish_updater_json.py` 生成并上传 Tauri 更新器需要的 `latest.json`。
8. 如果开启 Gitee 同步，会在 `latest.json` 上传成功后再同步到 Gitee，确保客户端优先从 Gitee 拉取更新清单。

发布脚本会把当前工作区所有改动纳入 `release: vX.Y.Z` 提交。运行前请先处理无关改动。

## 必需的 GitHub 设置

- `Settings -> Actions -> General`：启用 Actions，并允许工作流写入 Release。
- `Settings -> Secrets and variables -> Actions -> Secrets`：配置 `TAURI_SIGNING_PRIVATE_KEY` 和 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。
- 配置 `GITEE_ACCESS_TOKEN`，发布完成后会自动同步到 Gitee latest Release。
- `Settings -> Secrets and variables -> Actions -> Variables`：配置 `ENABLE_GITEE_SYNC=true` 开启 Gitee 同步；如自建 runner 下载 GitHub Release 资产需要代理，配置 `GITEE_SYNC_PROXY`。
- `Settings -> Actions -> Runners` 中确认自建 runner 在线，标签为 `self-hosted, linux, x64, gitee-sync`。
- 更新端点顺序必须保持 Gitee 在前、GitHub 在后：客户端会优先读取 Gitee `latest.json`，Gitee 不可用时再回退到 GitHub。

## 更新器验证

发布完成后检查：

- GitHub Release 中存在安装包、签名文件和 `latest.json`。
- `https://github.com/yedsn/time-genie/releases/latest/download/latest.json` 可下载。
- `https://gitee.com/hongxiaojian/time-genie/releases/download/latest/latest.json` 可下载，且其中安装包 URL 指向 Gitee。
- Gitee latest Release 中除 `latest.json` 外，安装包、更新归档和签名文件统一使用 `TimeGenie` 英文前缀，例如 `TimeGenie_0.1.9_x64-setup.exe`。
- 断开或临时阻断 Gitee 端点时，旧客户端可以回退到 GitHub `latest.json` 检查更新。
- 用旧版本客户端检查更新、下载安装、重启后，应用版本变为新版本。

## 常见问题

### 检查更新失败：Could not fetch a valid release JSON from the remote

这个错误通常不是客户端代码问题，而是更新端点没有返回有效的 `latest.json`。

按顺序检查：

- Gitee：`https://gitee.com/hongxiaojian/time-genie/releases/download/latest/latest.json`
- GitHub：`https://github.com/yedsn/time-genie/releases/latest/download/latest.json`
- GitHub Release 资产列表里是否存在 `latest.json`。
- Gitee latest Release 资产列表里是否存在 `latest.json`。

如果 GitHub Release 缺少 `latest.json`，重新运行发布工作流，或在有 `GITHUB_TOKEN`/`GH_TOKEN` 的环境中执行：

```powershell
python scripts/release/publish_updater_json.py --tag vX.Y.Z --github-owner yedsn --github-repo time-genie
```

如果 GitHub 有 `latest.json` 但 Gitee 没有，确认 `ENABLE_GITEE_SYNC=true` 已配置在 GitHub Actions Variables 里，并重新运行 Gitee 同步。

## macOS 分发说明

应用包含透明悬浮窗，macOS 构建启用了 Tauri 的 `macos-private-api`。这适合普通 GitHub/Gitee DMG 分发，不适合作为 Mac App Store 构建配置。
