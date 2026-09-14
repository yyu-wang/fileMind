# FileMind 自动更新（tauri-plugin-updater）发布运行手册

> 本应用端已接入 updater：设置页「检查更新」→ 发现新版 → 下载安装 → 重启。
> 本文档说明**发布侧**如何产出签名安装包并让更新清单生效。应用内代码已完成，
> 发布已由 `.github/workflows/release.yml` 自动化（推 tag 即发布，见第 5 节）。

## 1. 前置条件

- 更新清单与安装包托管在 GitHub Releases：
  `https://github.com/yyu-wang/fileMind/releases/latest/download/latest.json`
  （`latest.json` 作为 release 资产与安装包一起上传，"latest/download" 始终指最新版）

- **代码签名（必做，尤其 macOS）**：

  - macOS：Gatekeeper 要求 Developer ID 签名 + 公证，未签名安装包会被拦，更新必然失败

  - Windows：建议 OV 签名（SmartScreen），NSIS 更新同样建议签名

  - 证书不在手时，仅可内测（手动允许不明开发者），正式发布前补齐

## 2. 签名密钥（已生成，勿入库）

- 私钥：`~/.tauri/filemind/filemind-updater.key`（无密码，本机生成）

- 公钥：已写入 `src-tauri/tauri.conf.json → plugins.updater.pubkey`

- 后续发布/CI 需要的环境变量（tauri CLI 自动读取）：

  - `TAURI_SIGNING_PRIVATE_KEY_PATH=~/.tauri/filemind/filemind-updater.key`

  - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`（若换用带密码的密钥）

- ⚠️ 私钥丢失/泄露 = 无法再签更新或他人可伪造；正式发布建议换新密钥并将私钥+密码放入
  GitHub Actions Secrets（`TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`）

## 3. 构建带更新产物的安装包

`bundle.createUpdaterArtifacts: true` 已配置；构建时需产出 updater 目标：

```bash
# macOS：dmg 对外分发 + app 的 .tar.gz（updater 实际下载安装该包）+ 各自签名
npm run build:tauri -- --bundles app,dmg
# Windows：msi + nsis（nsis 的 .exe 供 updater 使用）
npm run build:tauri -- --target x86_64-pc-windows-msvc --bundles msi,nsis
```

构建会同时生成签名（`.sig`）文件：`bundle/{dmg,app,nsis,msi}/...` 旁（以 CLI 输出为准）。

## 4. 生成/维护更新清单 latest.json

模板见 [`docs/latest.json.template`](latest.json.template)：复制为 `latest.json`，把 `<...>` 占位符全部替换为真实值即可（产物文件名以 `tauri build` 实际输出为准）。

要点：

- `platforms` 只列**实际分发的平台**（当前为 `darwin-aarch64` + `windows-x86_64`；macOS Intel / Linux 不分发，不要列进去）

- `signature` 必须与对应安装包一一对应（.sig 文件内容是纯文本，可直接复制）

- URL 使用具体 `vX.Y.Z` tag（不要用 `latest/download` 指安装包自身，避免循环）

- `pub_date` 用 UTC RFC3339

- `version` / URL 里的版本号 / `pub_date` / 各 `signature` 必须与同一次构建的产物一致（构建两次签名会变）

## 5. 发布步骤

### 方式 A：CI 自动发布（推荐）

推 tag 触发 `.github/workflows/release.yml`：复用 merge-build 构建两平台 → 下载全部
artifact → `scripts/gen-latest-json.py` 生成 `latest.json` 与 Release 正文 → `gh release create`
上传安装包 / `.sig` / `latest.json`。

```bash
git tag -a v1.0.0 -m "Release 1.0.0"
git push origin v1.0.0
```

- 版本号必须与 tag 一致（`--check-version` 会在发布前卡住 package.json /
  tauri.conf.json / Cargo.toml 的不一致）
- tag 含 `-`（如 `v1.0.0-rc.1`）会发成 pre-release，不计入 `releases/latest`
- ⚠️ W4 接入 SignPath 后：签名步骤必须插在「生成 latest.json」之前，并用
  `tauri signer sign` 重签受影响安装包的 `.sig`（签名会改变字节，旧 `.sig` 立即失效）

### 方式 B：手工发布（CI 不可用时的兜底）

1. `release.md` 流程发版（版本号同步：package.json / Cargo.toml / tauri.conf.json / python main.py / routes_health.py + 前端 StatusBar / Sidebar / AboutSection）
2. 构建（第 3 节）→ 校验产物与 `.sig`
3. 按第 4 节生成 `latest.json`，签名内容取自 `.sig` 文件
4. GitHub Release：tag `v1.0.0`，上传全部安装包 + 各自 `.sig` + `latest.json`
5. 用户端点「检查更新」验证：桌面环境建议先装上一版再发新版实测一次

## 6. 常见问题

| 现象                   | 排查                                                                         |
| ---------------------- | ---------------------------------------------------------------------------- |
| 检查报 401/网络        | endpoint 用 `latest/download/latest.json` 需该 release 已含 latest.json 资产 |
| 下载安装后无法替换应用 | macOS 未签名/公证被 Gatekeeper 拦；确认 Developer ID + 公证通过              |
| 提示"签名不匹配"       | `latest.json` signature 与安装包不是同一份（构建两次会变）                   |
