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

| 事实                                                                              | 证据                                                                          | 影响                                        |
| --------------------------------------------------------------------------------- | ----------------------------------------------------------------------------- | ------------------------------------------- |
| `tauri.conf.json` 中 `externalBin=[]`、`resources=[]`                             | [tauri.conf.json](src-tauri/tauri.conf.json) L52-53                           | Sidecar 不进安装包                          |
| 打包态从 `resource_dir` 找 `filemind-sidecar-{triple}`                            | [manager.rs](src-tauri/src/sidecar/manager.rs) L735-794                       | 包内无该文件 → 启动即缺引擎退出             |
| `filemind/binaries/*` 被 gitignore，CI 全新检出无二进制                           | [.gitignore](.gitignore) L75-77                                               | 需在构建期现场生成                          |
| CI `merge-build.yml` 4 平台矩阵无 `build-sidecar.sh` 步骤                         | [merge-build.yml](.github/workflows/merge-build.yml) L44-83                   | 产出残缺安装包                              |
| 现有 `filemind-sidecar-aarch64-apple-darwin` = **315MB**                          | filemind/binaries/                                                            | 远超 80MB 门控，见 D1                       |
| 硬依赖集：lancedb/pyarrow/numpy/jieba/ollama/sentence-transformers(torch)         | [requirements.txt](python-sidecar/requirements.txt)                           | 315MB 是真实需要，无法靠 excludes 压回 80MB |
| spec `_excludes` 与 docstring 已脱节（docstring 称 exclude 重型模块，实现已不含） | [filemind-sidecar.spec](python-sidecar/filemind-sidecar.spec) L1-14, L100-116 | 门控与实现需一起对齐                        |
| 主程序 bundle 路径切换逻辑已写好（setup 内）                                      | [main.rs](src-tauri/src/main.rs) L488-555                                     | 只需让文件真实存在即可命中                  |

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
  2. 运行 app 日志出现 `命中 bundle Sidecar 路径`（[main.rs](src-tauri/src/main.rs) L504-506），而非 `未命中...回退 dev`
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
- DoD：PR 级手动触发 merge-build 四矩阵全绿，4 个 artifact 均含 Sidecar 二进制
- 验证：下载 artifact 解包 grep sidecar 文件；体积断言日志可见

> **T3 执行记录（2026-09-04 完成代码改动，待真实 runner 验证）**
>
> - [merge-build.yml](.github/workflows/merge-build.yml) 重写为**原生三 job 矩阵**：`windows-latest`(msi+nsis 双产出) / `macos-14`(arm64) / `macos-13`(x64 Intel，PyInstaller 无法跨 mac 架构)
> - **移除 Linux job**：打包态 Sidecar 解析（main.rs defer_to_bundle）当前仅 macOS/Windows，Linux 分发改后续版本（工作流注释说明）
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
  （把 wrapper stub 复制为 `filemind/binaries/filemind-sidecar/filemind-sidecar`，幂等、已有真实产物则跳过），
  四个 job 在**首次 cargo 调用前**调用它；`build-check` 同时补 `gen:ipc`（beforeBuildCommand 的 tsc
  依赖 gitignored 的 `src/types/ipc.ts`，与 merge-build.yml 同理）。
  本地已实测复现：移走产物 → `resource path ... doesn't exist`；跑脚本后 `cargo check` 通过。
- CI 连带修复（同一次排查暴露的两处**既有**缺陷）：
  1. `rust-check` 从未装过 Linux 系统库——先前卡在更早的 build.rs 资源校验，修好后 clippy 才
     暴露出 `gobject-2.0.pc` 缺失（exit 101）；补 `Install Linux deps` 步骤
  2. `frontend-check` 的覆盖率门禁 `functions ≥80%` **从未达成**（P1 前实测 77.57%，加
     StatusBar 测试后 79.03%），因先前更早的 gen:ipc 就失败而从未跑到；本次按真实水位下调至
     78% 作为防退化线（未覆盖的 156 个函数散落在 20 个既有文件，属既有技术债，不并入本 PR）
  3. `e2e-smoke` 的 `--ci` 闸门缺失：文档（[e2e-run.sh](scripts/e2e-run.sh) 用法、workflow 文件头、
     步骤名「E2E-001/002」）三处均声明 CI 只跑 001/002，但脚本实际把 004/005 也跑了；补上闸门
- ⚠️ 仍未解决：`002-classify-undo.e2e.ts` 第 3 个用例在 Linux WebKitGTK 上找不到
  `[data-testid="classify-execute"]`（前两个用例通过，说明应用与 stub sidecar 链路正常）。
  E2E Smoke 此前从未有过成功 run，本地仅在 macOS 验证过，属**既有且仅 Linux 暴露**的问题。
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

---

## 3. 验收门控汇总

| 门控                                    | 判定                                                   | 位置        |
| --------------------------------------- | ------------------------------------------------------ | ----------- |
| T2 本地 dmg 可运行且命中 bundle Sidecar | setup 日志 + RAG 冒烟                                  | 本机        |
| T3 CI 四矩阵全绿且 artifact 含 Sidecar  | workflow 状态 + 解包校验                               | GitHub      |
| T4 打包态自动项 PASS                    | go-no-go 输出                                          | CI artifact |
| 三语言自检不回归                        | `make lint` / `make test`（改动多为配置/脚本，仍全跑） | 本机+CI     |
| 发布前人工项（签名/公证/实机冒烟）      | T6 清单勾选                                            | 人工        |

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
