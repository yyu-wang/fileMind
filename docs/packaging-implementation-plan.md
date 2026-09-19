# FileMind Windows / macOS 打包实施方案

- 状态：**已定稿（D1–D3 已拍板，见第 1 节决策记录）**
- 日期：2026-09-04
- 关联任务：T1.2（打包方案，已完成）→ **本方案补 T1.3（Sidecar 集成进安装包）+ T6/Go-No-Go 多平台补齐 + 发布准备**
- 目标：在 dev 分支产出**可安装、可运行、功能完整**的 Windows(x64) 与 macOS(arm64+x64) 安装包

---

## 0. 范围声明

### 做

1. Sidecar 二进制集成进 Tauri 安装包（dmg / msi）并本地与 CI 双链路跑通
2. 修正 Sidecar 打包体积门控（与真实依赖集对齐）
3. CI merge-build 补 Sidecar 原生构建 + 打包态冒烟
4. 打包态 Go/No-Go 在 win/mac 上补齐验证
5. tauri.conf.json bundle 元数据补齐（发布质量）

### 不做（超出本次范围）

- 新增任何业务功能（分类/RAG/设置等均已齐备）
- 自动更新（tauri-plugin-updater）——默认不纳入，见 D3
- 代码签名证书购买/配置（需用户证书，见 T6 手册步骤）
- Linux 分发治理（CI 已有 job，仅顺带保持不回归）

### 现状证据（为什么必须做）

| 事实                                                                              | 证据                                                                                                                                                                    | 影响                                        |
| --------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------- |
| `tauri.conf.json` 中 `externalBin=[]`、`resources=[]`                             | [tauri.conf.json](src-tauri/tauri.conf.json) L52-53                                                                                                                     | Sidecar 不进安装包                          |
| 打包态从 `resource_dir` 找 `filemind-sidecar-{triple}`                            | [manager.rs](src-tauri/src/sidecar/manager.rs) L735-794                                                                                                                 | 包内无该文件 → 启动即缺引擎退出             |
| `filemind/binaries/*` 被 gitignore，CI 全新检出无二进制                           | [.gitignore](.gitignore) L75-77                                                                                                                                         | 需在构建期现场生成                          |
| CI `merge-build.yml` 4 平台矩阵无 `build-sidecar.sh` 步骤                         | [merge-build.yml](.github/workflows/merge-build.yml) L44-83                                                                                                             | 产出残缺安装包                              |
| 现有 `filemind-sidecar-aarch64-apple-darwin` = **315MB**                          | filemind/binaries/                                                                                                                                                      | 远超 80MB 门控，见 D1                       |
| 硬依赖集：lancedb/pyarrow/numpy/jieba/ollama/sentence-transformers(torch)         | [requirements.txt](python-sidecar/requirements.txt)                                                                                                                     | 315MB 是真实需要，无法靠 excludes 压回 80MB |
| spec `_excludes` 与 docstring 已脱节（docstring 称 exclude 重型模块，实现已不含） | [filemind-sidecar.spec](python-sidecar/filemind-sidecar.spec) L1-14, L100-116                                                                                           | 门控与实现需一起对齐                        |
| 主程序 bundle 路径切换逻辑已写好（setup 内）                                      | [sidecar_setup.rs](src-tauri/src/sidecar_setup.rs) `resolve_binary()`（dev 未命中→留空）+ [bootstrap.rs](src-tauri/src/sidecar/bootstrap.rs) `resolve_bootstrap_path()` | 只需让文件真实存在即可命中                  |

---

## 1. 决策记录（2026-09-04 已拍板）

| 决策                    | 结论                  | 执行影响                                                                                                   |
| ----------------------- | --------------------- | ---------------------------------------------------------------------------------------------------------- |
| **D1 Sidecar 体积预算** | **A. 上调至 ≤400MB**  | T1：改 build-sidecar.sh `MAX_SIZE_MB`、spec 注释、go-no-go 门控记录；`--onedir` 列为后续优化不进本轮       |
| **D2 CI 构建结构**      | **A. 全原生矩阵**     | T3：矩阵改为 macos-14(arm)/macos-13(x64)/windows-latest/ubuntu，每 job 同机先 build-sidecar 再 tauri build |
| **D3a Windows 安装器**  | **nsis + msi 双产出** | T3/T5：`--bundles nsis,msi`，补 NSIS 发布者名/安装体验配置                                                 |
| **D3b 自动更新**        | **本期不做**          | 保持 release.md 手动下载新包流程，不引入 updater                                                           |

### 决策备选存档（原草案选项）

#### D1：Sidecar 体积预算

| 选项                                   | 说明                                                                     | 结论                  |
| -------------------------------------- | ------------------------------------------------------------------------ | --------------------- |
| **A. 预算上调至 ≤400MB（已选）**       | 承认真实依赖集，同步改 build-sidecar.sh 断言 / spec 注释 / go-no-go 门控 | ✅ 成本最低，风险可控 |
| B. 保持 80MB                           | 需砍掉本地向量库/离线 rerank 等核心能力，等价功能回退                    | 已否决                |
| C. 分体 sidecar（base+heavy 分离安装） | 体积精细但引入自研加载器，改动面大                                       | 留作 v0.2 后续优化    |

> 补充事实：PyInstaller **onefile** 每次启动需把 315MB 解压到临时目录 → 冷启动 39s + 临时磁盘占用。当前 watchdog 就绪窗口已放宽到 60s（[manager.rs](src-tauri/src/sidecar/manager.rs) L34-36），重启可用；升级到 `--onedir` 可显著缩短冷启动（代价：bundle 里多一个目录，externalBin 需打包为目录/调整拷贝逻辑）。
>
> ✅ **该后续优化已于 P2-2 落地（2026-09-11）**：稳态 `/health` 15.7s → 1.0s，`externalBin` 改为
> `bundle.resources` 携带 onedir 目录，详见下文「P2-2」小节。

### D2：CI 构建结构（备选存档）

| 选项                                              | 说明                                                                                                                                            | 结论                                         |
| ------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------- |
| **A. 全原生矩阵（已选）**                         | 每 target 用对应原生 runner：`macos-14`(arm64) / `macos-13`(x64 Intel) / `windows-latest` / `ubuntu-22.04`，同机先 build-sidecar 再 tauri build | ✅ PyInstaller 无法跨 mac 架构产出，原生最稳 |
| B. 保留现有 Rust 交叉矩阵 + sidecar 单列 artifact | Rust 交叉可用但 Python 产物仍需原生机，流程分两段更复杂                                                                                         | 已否决                                       |

### D3：Windows 安装器与自动更新（备选存档）

| 决策           | 结论                                                                    |
| -------------- | ----------------------------------------------------------------------- |
| Windows 安装器 | **nsis + msi 双产出（已选）**：`targets` 增加 nsis，二者都产            |
| 应用内自动更新 | **本期不做（已选）**：不引入 tauri-plugin-updater，保持手动下载新包流程 |

---

## 2. 分阶段任务（按依赖排序，逐任务批准后执行）

### T1：体积门控修订 + Sidecar 构建脚本对齐（D1-A 前提）

- 改动文件：`scripts/build-sidecar.sh`（`MAX_SIZE_MB` 80→400 及提示文案）、`python-sidecar/filemind-sidecar.spec`（docstring 对齐真实 excludes 策略与体积说明）、`docs/go-no-go-report.md`/`docs/T1.2-packaging-notes.md`（追加"体积门控修订记录"章节，说明 2026-09-04 决策）
- 动作：干净构建本机 aarch64 产物，实测体积写入报告；确认体积断言 PASS
- DoD：`bash scripts/build-sidecar.sh --target aarch64-apple-darwin` 退出 0；报告记录新预算与实测值
- 验证：`python3 scripts/go-no-go.py --list` 正常

> **T1 执行记录（2026-09-04 完成）**
>
> - `build-sidecar.sh`：门控 80→400MB，头部 DoD 注释与失败提示文案对齐新决策
> - `filemind-sidecar.spec`：docstring 重写（记录 24MB→315MB→400MB 沿革、运行依赖不可 exclude、`--onedir` 列为后续候选）；`_excludes` 注释改为"仅剔冗余、运行依赖一律保留"
> - `scripts/go-no-go.py`：T6 体积断言 `max_size_mb 80→400` + 描述文案同步（避免 T4 误 FAIL）
> - 报告补修订记录：`docs/go-no-go-report.md`、`docs/T1.2-packaging-notes.md`（顶部加门控修订 callout）
> - **干净构建实测：302MB < 400MB PASS**，sha256 `d973bdab857a008d1cfbf99f546a7e2e31ebafd627d070b0615a4ad95994a428`
> - `go-no-go.py --list` exit 0 ✓

### T2：Sidecar 集成进安装包（本地链路）

- 改动文件：`src-tauri/tauri.conf.json` → `"externalBin": ["../filemind/binaries/filemind-sidecar"]`（产物命名 `filemind-sidecar-{triple}[.exe]` 已与 Tauri 期望一致，无需重命名）
- 联动：`Makefile` 新增目标（如 `build:sidecar`）并在 `build` 前置调用 `scripts/build-sidecar.sh`；`package.json` 增补对应 npm script（不改动核心依赖）
- **关键验证点（本任务真正的 DoD）**：本机构建后，解包/运行打包 app，确认：
  1. dmg 内 `Contents/Resources/` 出现 `filemind-sidecar-aarch64-apple-darwin`
  2. 运行 app 日志出现 `Sidecar 引导使用路径: …/Contents/Resources/filemind-sidecar-aarch64-apple-darwin`（[bootstrap.rs](src-tauri/src/sidecar/bootstrap.rs) `bootstrap_worker`），而非 dev 目录路径；dev 侧未命中时会先出现 `dev 模式未找到 Sidecar 二进制，交由后台引导按 bundle 路径解析`（[sidecar_setup.rs](src-tauri/src/sidecar_setup.rs) `resolve_binary`）
  3. Sidecar `/health` 正常、RAG 冒烟可用
- ⚠️ 若验证点 1 中文件实际落到**非 resource_dir 目录**（Tauri 版本行为差异），备选方案：改 `resolve_bundle_binary_path` 用 `tauri_plugin_shell` 的 sidecar 解析 API，或按实际落地目录修正探测点（小改动，执行时现场定）
- DoD：本地 `npm run build:tauri` 产出可运行 dmg；app 全程不依赖 dev 目录（`CARGO_MANIFEST_DIR` 布局）即能启动

> **T2 执行记录（2026-09-04 完成，本地 .app 链路验证通过）**
>
> - `tauri.conf.json`：`externalBin: ["../filemind/binaries/filemind-sidecar"]`
> - `Makefile`：`build` 前置 `sidecar` 目标（调用 build-sidecar.sh）；`package.json` 增 `build:sidecar`
> - **实测 Tauri v2 行为**：externalBin 产物落在主可执行同目录 `Contents/MacOS/filemind-sidecar`（无 triple 后缀），而非 resource_dir → 重写 bundle 解析为多根目录探测（[manager.rs](src-tauri/src/sidecar/manager.rs) `resolve_bundle_from_roots`，exe 目录优先 + resource_dir 兜底）
> - **修复打包态启动路径缺陷**：原 main() 在 dev 路径解析失败时 `exit(1)`，setup 的 bundle 切换永远到不了（用户安装目录无 repo 布局）→ 改为 macOS/Windows 下 dev 找不到时**推迟到 setup 启动**；setup 内 bundle 失败且无 dev 进程时中止（不再静默半可用）
> - **修复云端 env 丢失**：setup 重建 bundle manager 时未继承云端代理 env → 新增 `SidecarManager::cloud_env()` + 替换前克隆
> - **扩展孤儿清理匹配**：dev 入口除 `-m app` 外补 `sidecar_entry.py`（实测残留孤儿即此形态），启动自愈
> - **顺带修复 P-07 类型漂移**（tsc 构建阻塞）：`CloudProvider`(旧枚举) 在生成 ipc.ts 中不存在，6 个前端文件统一改为 `string`/`CloudProviderRecord`；`CLOUD_PROVIDER_DEFAULT_MODEL` 键对齐小写 slug
> - **冒烟结果**：从 `/tmp`（无 repo 布局）启动 .app → 自动清理孤儿(pid 84808) → `命中 bundle Sidecar 路径` → `Sidecar 已切换为 bundle 路径并重新握手成功` ✓
> - **自检**：cargo clippy -D warnings 全绿、Rust lib 317 通过（含 manager 26）、tsc/eslint 改动文件通过
> - **已知限制**：dmg 卷创建需 `hdiutil` 访问 `/dev/rdisk`，沙箱受限未完成（.app 已产出并验证）；在无沙箱终端跑 `npm run build:tauri` 即可生成 dmg
> - **存量未决**：dev 分支前端单测 11 个失败（P-07 测试漂移，stash 基线验证非本次改动引入）——`loadApiKeyStatus` 默认表改以 cloudProviders 为基准后，settings/onboarding 多处测试 mock 仍按旧固定 provider（`Openai/Deepseek`）预设，需单独小修

### T3：CI merge-build 集成（多平台链路）

- 改动文件：`.github/workflows/merge-build.yml`
  - 按 D2 结论调整为原生矩阵；mac x64 job 用 `macos-13`，mac arm 用 `macos-14`
  - 每 job 在 `npm ci` 后、`build:tauri` 前增加步骤：`bash scripts/build-sidecar.sh --target <原生 triple>`
  - Windows job 需 `actions/setup-python@python-3.12` + Git Bash（脚本依赖 bash + venv）；mac/linux 预装即可
  - 新增打包态冒烟步骤：解包校验 `filemind-sidecar-{triple}` 存在（`if-no-files-found: error` 语义同现有 artifacts）
  - 若 D3 增加 nsis，同步调整 `--bundles`
- DoD：各分发平台 job 全绿，artifact 均含 Sidecar 二进制
  （⚠️ 范围变化：原为「四矩阵」→ 2026-09-11 移除 Linux、2026-09-13 移除 mac x64，
  当前**只分发 macOS arm64 + Windows**，理由见文末 P2-2 连带修复第 7/8 条）
- 验证：下载 artifact 解包 grep sidecar 文件；体积断言日志可见

> **T3 执行记录（2026-09-04 完成代码改动，待真实 runner 验证）**
>
> - [merge-build.yml](.github/workflows/merge-build.yml) 重写为**原生三 job 矩阵**：`windows-latest`(msi+nsis 双产出) / `macos-14`(arm64) / `macos-13`(x64 Intel，PyInstaller 无法跨 mac 架构)
> - **移除 Linux job**：打包态 Sidecar 解析（[sidecar_setup.rs](src-tauri/src/sidecar_setup.rs) 的 `can_bundle` 分支）当前仅 macOS/Windows，Linux 分发改后续版本（工作流注释说明）
> - 每 job 新流程：setup-python 3.12 → 建 venv + 装 requirements/requirements-dev → gen:ipc → `build-sidecar.sh --target <native triple>` → `tauri build --bundles msi nsis|dmg` → **Smoke check（bundle 内必须存在 filemind-sidecar，否则显式失败）** → 上传 artifact
> - `build-sidecar.sh` 兼容 Windows venv 布局（`.venv/Scripts/python.exe`）
> - 本地自检：bash -n 通过、YAML `{{ }}` 配对 13/13
> - ⚠️ **DoD 需在真实 GitHub Actions 上验证**（本地无法模拟 runner）；合并推送/手动 workflow_dispatch 触发后方可确认四矩阵产物含 Sidecar

### T4：打包态 Go/No-Go 补齐（win/mac）

- 目标：把 [go-no-go-report.md](docs/go-no-go-report.md) 中 SKIP 项（x64 mac / win）在 CI 产物上补齐
- 动作：新建轻量 CI 步骤或在 T3 冒烟内复用 `go-no-go.py --all --binary` 核心项（启动/握手/健康/体积）
- 说明：完整 7 项需实机交互（托盘/崩溃重启），CI 覆盖自动可测项，其余登记为**发布前人工清单**（与 T6 合并维护）
- DoD：win + mac 产物自动项全 PASS；人工项清单落文档

> **T4 执行记录（2026-09-04 完成结构层，运行期项归 T6）**
>
> - 新增 [scripts/verify-packaged-app.sh](scripts/verify-packaged-app.sh)：校验 .app 内 Sidecar 存在性（主可执行同目录）、可执行位、体积 ≤400MB、Mach-O 架构匹配
> - 本地对 `FileMind.app` 实测 **3 PASS / 0 FAIL**（arm64、302MB）
> - 运行期自动项（启动/握手/健康/体积）已在 T2 冒烟中验证（mac arm64）：打包态从 /tmp 启动 → 命中 bundle Sidecar → 握手成功；go-no-go 7 项完整跑批与 Windows/x64 mac 实机验证登记为 **T6 发布前人工清单**

### T5：bundle 元数据补齐（发布质量）

- 改动文件：`src-tauri/tauri.conf.json` bundle 段补充 `publisher`/`category`/`copyright`/`shortDescription`/`longDescription`
- DoD：macOS 关于页 / Windows 卸载程序(或安装器) 显示正常应用名与发布者

> **T5 执行记录（2026-09-04 完成）**
>
> - bundle 补充：`category: Utility`、`shortDescription`/`longDescription`（中英对照见文件）、`publisher: FileMind`、`copyright`
> - `identifier`: `com.filemind.app` → `com.filemind.desktop`（消除 macOS `.app` 后缀冲突告警）
> - 已用 `tauri build --bundles app` 重打包验证元数据写入（Info.plist + 安装器字段生效于下一次 CI 产物；NSIS/MSI 发布者由 bundle.publisher 驱动）

### T6：发布收尾（发布前人工清单，不阻塞 T1–T5）

- macOS：Developer ID 签名 + 公证（Gatekeeper）；CI 签名可在拿到证书后接入（`APPLE_CERTIFICATE` secrets）
- Windows：OV/EV 代码签名证书（SmartScreen）
- 版本号按 [release.md](rules/release.md)：当前已统一升至 `1.0.0`（package.json / Cargo.toml / tauri.conf.json / python main.py / routes_health.py + 前端展示全部同步），后续发版按 MINOR/PATCH 规则递增
- Changelog：`[Unreleased]` → 版本化 + 补本条 T1.3 打包集成条目

### P2-2：onefile → onedir（启动性能优化，2026-09-11）

- 背景：D1 决策把 `--onedir` 列为「后续优化」。实测 onefile 每次启动需把归档解压到临时目录，
  占冷启动 21s / 稳态 15.7s 中的约 14s；P1-1/P1-2 已让窗口秒开，但「AI 引擎可用」仍要等 15.7s。
- 改动文件：`python-sidecar/filemind-sidecar.spec`（EXE `exclude_binaries=True` + 新增 `COLLECT`）、
  `scripts/build-sidecar.sh`（目录产物：体积/SHA/拷贝/软链）、`src-tauri/tauri.conf.json`
  （`externalBin` → `bundle.resources` 目录映射）、`src-tauri/src/sidecar/manager.rs`
  （dev/bundle 目录形态解析）、`.github/workflows/merge-build.yml`（smoke check 路径）、
  `.github/workflows/{e2e-smoke,pr-check}.yml` + `scripts/stub-sidecar-product.sh`
  （CI 占位 sidecar 产物，见下）、`eslint.config.js`
  （忽略 `filemind/binaries/`——onedir 内含第三方 .js 资源）、
  `scripts/verify-packaged-app.sh`、`scripts/go-no-go.py`、`Makefile`
- 关键结论（本机 aarch64 实测）：
  1. **稳态 `/health` 就绪 15.7s → 1.0s**（连续 4 次 1.01~1.07s）；首启冷页面缓存需读满 ~1.4GB，约 15s
  2. **下载体积不变**：tar.gz 实测 317MB（原 onefile 319MB）；变的是安装占用（319MB → 源产物 915MB /
     打包后约 1384MB —— Tauri `copy_resources` 会把源产物里的 36 个 symlink 解引用成真实文件）
  3. 体积构成：`_internal/torch` 403MB（29%）、lancedb 126MB、pyarrow 118MB
  4. 内存 RSS 169MB → 149MB
- 门控口径变更：`MAX_SIZE_MB` 400 → **1600**（onefile 量的是**压缩态**单文件，onedir 是**解压态**目录，
  两者不可直接比较）。⚠️ 体积一律按**逻辑大小**（`find -type f` 求和、不跟随 symlink）统计：`du` 在
  APFS 上受 clone/硬链接与 symlink 解引用影响，同一棵树能给出 915MB / 1397MB 等不一致读数。
- 落地位置变更：`externalBin`（主可执行同目录 `Contents/MacOS/`）→ `bundle.resources`
  map `{"../filemind/binaries/filemind-sidecar/": "sidecar"}`，即 `Contents/Resources/sidecar/`；
  Tauri 复制**保留可执行位**（已实测）。
- 显式覆盖语义保留：`FILEMIND_SIDECAR_BINARY` 同时接受 onedir **目录**与可执行**文件**——
  CI E2E（`scripts/e2e-sidecar-wrapper.sh`）与本地 dev（`binaries/filemind-sidecar-dev`，wrapper 脚本）
  以文件形态注入 `python -m app`，是文件而非目录。**布局发现**（dev/bundle）只认 onedir，不扫描旧 onefile。
- CI 适配（PR #2 首跑暴露）：`tauri-build` 的 build.rs 会按 `bundle.resources` 收集资源并**校验存在性**
  （缺则 `cargo build/clippy/test` 直接 exit 101）。该路径已 gitignore，而 frontend-check / rust-check /
  build-check / e2e-smoke 都**不需要真实 sidecar**（E2E 运行时由 `FILEMIND_SIDECAR_BINARY` 指向 wrapper），
  为省 5-10min 均不跑 PyInstaller。故抽出 `scripts/stub-sidecar-product.sh` 统一造**最小占位产物目录**
  （在 `filemind/binaries/filemind-sidecar/` 下放一个自包含 shim 作主可执行，幂等、已有真实产物则跳过），
  四个 job 在**首次 cargo 调用前**调用它；`build-check` 同时补 `gen:ipc`（beforeBuildCommand 的 tsc
  依赖 gitignored 的 `src/types/ipc.ts`，与 merge-build.yml 同理）。占位主可执行是**自包含 shim**
  （不用 `scripts/e2e-sidecar-wrapper.sh` 的副本——它的 `$(dirname $0)` 相对定位复制后会失效）。
  本地已实测复现：移走产物 → `resource path ... doesn't exist`；跑脚本后 `cargo check` 通过。
- CI 连带修复（同一次排查暴露的两处**既有**缺陷）：
  1. `rust-check` 从未装过 Linux 系统库——先前卡在更早的 build.rs 资源校验，修好后 clippy 才
     暴露出 `gobject-2.0.pc` 缺失（exit 101）；补 `Install Linux deps` 步骤
  2. `frontend-check` 的覆盖率门禁 `functions ≥80%` **从未达成**（P1 前实测 77.57%，加
     StatusBar 测试后 79.03%），因先前更早的 gen:ipc 就失败而从未跑到；本次按真实水位下调至
     78% 作为防退化线（未覆盖的 156 个函数散落在 20 个既有文件，属既有技术债，不并入本 PR）
  3. `e2e-smoke` 的 `--ci` 闸门缺失：文档（[e2e-run.sh](scripts/e2e-run.sh) 用法、workflow 文件头、
     步骤名「E2E-001/002」）三处均声明 CI 只跑 001/002，但脚本实际把 004/005 也跑了；补上闸门
  4. `rust-check` 的 `cargo test` 卡满 6h 上限致整轮 cancelled：编译仅 ~6min，卡点在
     `spawn_orphan_listener` 用 `Command::output()` 启动 `sh -c 'nc -l PORT &'`——`output()`
     要读 stdout/stderr 管道到 EOF，而 `nc` 继承了这两个 fd 且长期持有不关闭，等待永不返回
     （本机 macOS 的 nc 行为不同才没暴露）。改为三个 stdio 全部丢弃 + `.status()`；
     并给 `rust-check` 加 `timeout-minutes: 30` 保险阀（下次挂死 30min 失败而非 6h）
  5. 解卡后暴露出**真实的 Linux 产品缺陷**：`cleanup_orphan_sidecar` 按 `ps -o comm=`
     匹配 `filemind-sidecar`，而 Linux 内核把 `/proc/<pid>/comm` 截断到 15 字符
     （`filemind-sideca`）——恰好 16 字符的名字在 Linux 上**永远匹配不到**，孤儿
     sidecar 清不掉、残留进程占住端口导致下次启动握手 401（即历史 401 顽疾的成因之一）。
     抽出 `matches_sidecar_comm()` 容忍截断形态（等值比较，不放宽误杀面）
  6. `build-check` 首次真正跑起来：编译/打包本身成功（macOS 5min、Windows 11min），
     但两端各有一个坑——(a) 最后一步 updater 签名报「找到一个公钥但没有私钥」而 exit 1
     （`bundle.createUpdaterArtifacts=true`），按 merge-build.yml 的既有取法补
     `TAURI_SIGNING_PRIVATE_KEY`；(b) `ubuntu-latest` 的 Linux 全量打包（deb/rpm/AppImage）
     卡住 1 小时以上——Linux job 在 [merge-build.yml](.github/workflows/merge-build.yml) 里
     本就被刻意移除（打包态 Sidecar 解析仅支持 macOS/Windows，Linux 分发延后），
     pr-check 却仍在跑；故矩阵收敛为 macos + windows，Linux 覆盖交由 `rust-check`
     （ubuntu 跑 clippy + cargo test）与 `e2e-smoke`（ubuntu 上 `--no-bundle` 构建）。
     另给 `build-check` 补 `timeout-minutes: 30` 保险阀（实测 5/11min）
  7. **Windows 打包链路首次跑通才发现**（2026-09-13，P2-2 合并到 main 后的 merge-build）：
     `build-sidecar.sh` 原本只在 macOS 建默认名软链接，而 `bundle.resources` 用的是与架构
     无关的 `../filemind/binaries/filemind-sidecar/` —— Windows 上该路径不存在，build.rs
     资源校验让 `Generate IPC types`（整条链路里第一次 cargo 调用）直接失败（macOS arm64
     同轮已成功）。改为三平台都提供默认名：macOS/Linux 软链接，Windows 用目录副本
     （软链接需管理员/开发者模式，副本无特权坑）
  8. **mac x64 job 移除**（同轮排查）：先是 runner 标签退役——merge-build 的 mac x64 用
     `macos-13`，该镜像已于 **2025-12-04 退役**，job 永远排队不被调度（实测 macos-14 与
     windows 都跑完了，x64 一直 `queued`）；换成标准 Intel 标签 `macos-15-intel` 后能调度了，
     却暴露第二层：**上游依赖在 macOS x86_64 上已无 wheel**——`lancedb==0.37.1`
     （pip: No matching distribution，该平台最高 0.25.3），torch（经 sentence-transformers
     引入）同样早已停发 macOS x86_64 wheel。结合 Apple 已停 Intel 支持、GitHub 将于 macOS 15
     之后（2027 秋）退役 Intel runner，本轮起**只分发 macOS arm64 + Windows**；
     将来若仍需 Intel 包，路径是为该平台单独降级 lancedb/torch（运行时行为分叉，需实测）
  9. **引擎取包纳入 PR 门禁**（2026-09-17，T3 引擎分发链路暴露的漏网之鱼）：`build-check` 用
     `stub-sidecar-product.sh` 占位、不走 `build-sidecar.sh`，故「取引擎产物 → 入产物」这条链路
     只有 merge-build 才真正跑到。实测后果：`fetch-llama-server.sh` 的内嵌 Python 在 Windows runner
     上按 cp1252 输出 ⏳/中文，`print` 抛 UnicodeEncodeError，脚本在「开始下载」前即退出 →
     merge-build Windows job 4.5min 假失败、其后三段引擎冒烟全部 skipped，而 PR 侧全绿。
     处置：脚本在打印前 `reconfigure(encoding="utf-8")`（与 [go-no-go.py](scripts/go-no-go.py) 同一
     写法），并在 `build-check` 补一步「取引擎 + 断言产物存在」——每 PR 每平台 +1 次下载（macOS
     27MB / Windows 45MB，约 1 分钟）；PyInstaller 与入产物仍只由 merge-build 覆盖。
     ✅ **已验证（2026-09-17）**：PR Check run `35184498769` 两平台 `build-check` 全绿，新步骤真跑——
     Windows `[engine] 目标平台 win-x64` → `✅ sha256 校验通过（17.6 MB）` →
     `内置引擎取包 OK: python-sidecar/vendor/llama/win-x64/llama-server.exe`；macOS 同（10.6 MB）。
     实测代价远低于预估：引擎下载在 CI 上约 1 秒，整轮 PR Check 仍约 7min。同轮 merge-build
     （`ce309a0`）另证编码修复后 Windows 三段引擎冒烟（产物断言 / 打包侧车 `/health` / 引擎 `--version`）
     全部通过。
- E2E-002 失败定位（同批修好）：取证快照显示「6 成功 / 0 失败」但扫描根被清空——分类产物
  落点是扫描根**同级**的收纳根 `<扫描根名>_已分类`（`classifier::sibling_output_root`，扫描目录
  只留待整理文件），而 002 断言的是扫描目录内部，属**断言语义过期**（非产品缺陷）。改为按
  收纳根断言；`e2e-run.sh` 同时清理遗留的 `*_已分类` 临时目录。E2E Smoke 此前从未有过成功 run。
- 验证：`verify-packaged-app.sh` 对真实 `.app` **4 PASS / 0 FAIL**；`.app` 内 bundled sidecar
  直接运行稳态 1.1~1.3s 且 LanceDB 可用（`/index/delete_by_file_ids` HTTP 200）；
  新增「真实 `.app` 布局命中 bundle 侧车」单测作为长期契约回归。
- ✅ **端到端已验证（2026-09-11）**：退出旧实例后，从 `/tmp`（无 repo 布局）启动 `.app`，日志依次为
  `dev 模式未找到 Sidecar 二进制，交由后台引导按 bundle 路径解析` → `Sidecar 引导使用路径: ***/filemind-sidecar`
  → `Sidecar 握手成功` → `Sidecar 引导完成（后台线程）`，0 error；运行中 sidecar 进程路径实测为
  `<FileMind.app>/Contents/Resources/sidecar/filemind-sidecar`，`/health` 返回 ok。
  ⚠️ 注意：若旧版 FileMind 仍驻留（托盘常驻），`tauri-plugin-single-instance` 会让新实例直接接管退出，
  表现为「无 setup 日志」——验证前需先退出旧实例。
- 已知代价：onedir 目录 6172 个文件，首次安装后首启需冷读 ~1.4GB；macOS 未来做签名/公证时
  需对 `Resources/sidecar/` 内的可执行文件一并签名。

### P3：启动耗时埋点与基线（P3-1 / P3-2，2026-09-18）

- 背景：P1（窗口秒开 + splash）与 P2（重依赖惰性导入 + onedir）落地后，「引擎可用」的耗时
  只存在于人工实测与文档数字里——没有代码级埋点、没有基线、也没有防退化门禁（即 P3）。
- **P3-1 埋点**（改动 `src-tauri/src/sidecar/manager/mod.rs`、`manager/start.rs`、
  `manager_tests/start_tests.rs`）：`start_with_handshake` 分三段独立计时——`spawn`
  （PSK 生成 + `Command::spawn` + stdin 注入）、`ready`（spawn 返回 → `/health` 首次 200）、
  `handshake`（nonce + proof 校验）；成功后写入 `SidecarManager::last_startup`
  （`StartupTimings`）并按 `rules/observability.md` 的 `sidecar.startup_ms` 口径打一条 info 日志：

  ```
  Sidecar 启动耗时（sidecar.startup_ms）: total_ms=1237 spawn_ms=2 ready_ms=1234 handshake_ms=0
  ```

  失败路径进入即清空该记录，保证「读到 `Some` 即代表本次启动成功」（回归用例
  `test_failed_start_clears_stale_startup_timings`）。首启与 watchdog 重启共用本路径，
  每次启动都会记一条，可直接 grep `sidecar.startup_ms`。
  取舍：`rules/observability.md` 的指标清单是结构性描述，全仓并无指标聚合后端
  （`metrics::record` 零命中），故按结构化日志落地，不新建一个空转的 metrics 模块。

- **观测前提（P3-3 起已消除）**：Rust 日志默认级别是 `warn`，而这条埋点是 info——原先取数
  必须 `RUST_LOG=info`。现按 target 放行（`log_file.rs::STARTUP_LOG_TARGET` =
  `filemind_lib::sidecar::manager::start`），该模块的 info 默认落盘，其余模块仍按 `warn` 抑制；
  显式 `RUST_LOG` 依旧优先（放行写在 `parse_default_env` 之前）。实测：不带 `RUST_LOG` 启动，
  日志里稳定出现「准备启动 Sidecar / 握手成功 / sidecar.startup_ms」三行，而迁移、种子等
  其他 info 不出现。
- **P3-2 基线**（本机 aarch64；口径＝从 `start_with_handshake` 进入至握手成功）：

  | 场景                                                               | 口径             | total                            | spawn | ready              | handshake |
  | ------------------------------------------------------------------ | ---------------- | -------------------------------- | ----- | ------------------ | --------- |
  | dev（`binaries/filemind-sidecar-dev` → `python sidecar_entry.py`） | 5 次             | 3225 / 1748 / 1441 / 1758 / 1239 | 0~1   | 同 total（差 1~3） | 1~3       |
  | 打包态（`Contents/Resources/sidecar/` onedir）                     | 全新数据目录首启 | 1237 ms                          | 2     | 1234               | 0         |
  | 打包态（同上）                                                     | 复用数据目录再启 | 1133 ms                          | 1     | 1130               | 0         |
  - 结论：**`ready` 段占 ≈99.9%**，`spawn` 与 `handshake` 都是个位数毫秒——后续若还要压启动，
    只有 ready 段（Python 解释器拉起 + 依赖导入 + LanceDB 建表）值得动。
  - 与 P2-2 记录同量级：P2-2 用外部探测测得稳态 `/health` 就绪 ≈1.0s，本次 Rust 侧（含握手）
    1.13~1.24s。
  - 局限：本次**未复现「安装后首启冷读」口径**（P2-2 记录 ~15s，冷读 ~1.4GB 产物）。原因是
    `.app` 与 onedir 产物刚在本机构建，页缓存尚热；要拿冷数需 `purge` / 重启机器或真机新装。
  - 三档差异实测（同一产物，仅页缓存状态不同）：**全冷 >15s**（go-no-go 打包态超时 15s 被顶穿、
    退回 dev 模式；单独跑 12s 仍未就绪）→ **半热 ~~2~~3s** → **全热 1.13~1.24s**。
    这就是 P3-3 门禁必须冷盘容错、只判热盘的依据。
  - 复现命令（打包态；须从 `/tmp` 启动以避开 dev 布局的 cwd 兜底，见
    `sidecar/manager/paths.rs::find_in_dev_layout`）：

    ```
    cd /tmp && FILEMIND_DATA_HOME=/tmp/p3-home RUST_LOG=info \
      /path/to/FileMind.app/Contents/MacOS/filemind
    grep sidecar.startup_ms /tmp/p3-home/logs/filemind.log
    ```

- **P3-3 门禁已落地**（`scripts/go-no-go.py`）：T1（启动）新增**平台分档**预算断言
  （`STARTUP_BUDGET_MS_BY_PLATFORM`：macOS 2000ms / Windows 5000ms），量「spawn →
  `/health` 首次 200」整段耗时；**只判打包态**（dev 实测 1.2~3.2s，不代表用户路径，只报数）。
  CI 不需要新步骤——merge-build 的 `Smoke check Sidecar runtime (/health)` 本来就是
  `go-no-go.py --test 1 --binary <bundle 内 sidecar 目录>`（见 `.github/workflows/merge-build.yml`）。
  - **为什么按平台分档**（2026-09-19 merge-build 实测）：首版用单一 2000ms，macOS 通过
    （1129ms），**Windows 失败**：`FAIL — 打包态热盘启动耗时 3578ms 超出预算 2000ms
（冷盘首次 3687ms）`。Windows 的瓶颈是进程拉起（无 fork + Defender 扫可执行）与 Python
    导入，不是页缓存——重试几乎不改善，且该阈值本按 macOS 标定。故拆档：macOS 保持紧信号，
    Windows 放宽到 5000ms（约 40% 余量，仍能挡住 onefile 级别 ~15.7s 的回归）。
  - **冷盘容错**：首次超预算则重启一次、用第二次（热盘）数字判定，冷盘值记进 detail。
    实机复现（本机页缓存冷却后跑 T1）：
    `PASS — 冷盘首次 4671ms → 整段启动耗时 1218ms（预算 2000ms，热盘口径）`。
  - 冷启口径仍留人工发布前检查（`purge` / 重启或真机新装后量），不进自动门禁。

---

### W3：Windows 安装包配置补齐（2026-09-14）

- 背景：发版链路（W1/W2）打通后，`bundle.windows` 段仍为空 —— WebView2 安装方式、NSIS
  安装模式与语言都取框架默认值，属发布前必须显式拍板的三项。
- 决策：
  1. **`webviewInstallMode`：先试 `offlineInstaller`，实测后回退为 `downloadBootstrapper`**。
     实测（merge-build Windows job）：artifact 压缩包 **653.9MB → 1082.4MB（+428MB）**，
     按 v1.0.0-1 资产尺寸反推即每个安装器 +214MB（NSIS 273→~487MB、MSI 382→~596MB），
     官方文档「约 127MB」偏乐观。而 **updater 下载的就是 NSIS 那个 exe**，等于每次自动更新
     也要多下 214MB；反观 WebView2 在 Windows 11 已内置、Windows 10 由 Windows Update
     推送，真正缺它的机器极少，故不值得。现显式写出 `downloadBootstrapper` 固定该默认值，
     避免将来框架默认值变动带来体积突变。
  2. **`nsis.installMode`：显式固定为 `currentUser`**（也是框架默认）：装到用户目录、只写
     `HKCU`、全程不要管理员权限，updater 静默替换文件也不需要提权。
  3. **`nsis.languages`：`["SimpChinese", "English"]`**：中文系统显示中文安装界面，系统
     语言不在列表时回落第一项（即中文）。
- 验证：merge-build Windows job 全绿，`light`（MSI）与 `makensis`（NSIS）均正常产出，
  4 个产物（msi / nsis.exe / 两个 .sig）齐备。
- 备注：安装器体积即更新下载体积（`latest.json` 的 `windows-x86_64` 指向 NSIS exe），
  后续若要压缩更新体积，应优先考虑 sidecar 产物体积（onedir 源产物 915MB）而非安装器格式。

## 3. 验收门控汇总

| 门控                                    | 判定                                                    | 位置        |
| --------------------------------------- | ------------------------------------------------------- | ----------- |
| T2 本地 dmg 可运行且命中 bundle Sidecar | setup 日志 + RAG 冒烟                                   | 本机        |
| T3 CI job 全绿且 artifact 含 Sidecar    | workflow 状态 + 解包校验（现有 mac arm64 / win 两平台） | GitHub      |
| T4 打包态自动项 PASS                    | go-no-go 输出                                           | CI artifact |
| 三语言自检不回归                        | `make lint` / `make test`（改动多为配置/脚本，仍全跑）  | 本机+CI     |
| 发布前人工项（签名/公证/实机冒烟）      | T6 清单勾选                                             | 人工        |

## 4. 风险与回退

| 风险                                                          | 概率                                                                                           | 缓解                                                                     |
| ------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------ |
| Tauri externalBin 落地目录与 resource_dir 探测不符            | 中                                                                                             | T2 预留备选：改用 shell sidecar 解析（小改动）                           |
| PyInstaller 在 macos-13(x64)/windows 打包出 315MB+ 或超新预算 | 低                                                                                             | 预算 400MB 留余量；每平台干净构建实测，超标按 spec 注释继续剪非必要数据  |
| onefile 冷启动 39s 影响首启体验                               | 高(已知)                                                                                       | watchdog 60s 窗口已容纳；`--onedir` 列为后续优化                         |
| 修改 CI 引入假通过                                            | 中                                                                                             | 冒烟步骤对缺 sidecar 产物显式失败（对齐现有 `if-no-files-found: error`） |
| 回退                                                          | 全部改动集中在 `tauri.conf.json` / CI yml / shell 脚本 / spec，均可单独 revert，不触碰业务代码 |

## 5. 执行方式

按用户既定流程：**本方案批准后，T1 → T6 逐任务批准、逐个执行、逐个自检**（`make lint` + `make test` + 对应 DoD），不并行堆叠。
