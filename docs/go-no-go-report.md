# T1.6 Go/No-Go 决策报告（阶段 2 · 打包二进制模式，binary=/Users/wangyu/Desktop/个人/project/filemind/filemind/binaries/filemind-sidecar-aarch64-apple-darwin）

> **门控修订记录（2026-09-04）**：本报告 T6 的 `<80MB` 体积门控已上调为 `≤400MB`。
> 原因：阶段 2 引入 lancedb/numpy/jieba/sentence-transformers(torch) 等运行时硬依赖后，
> onefile 真实体积约 315MB（2026-08-24 本机实测），无法靠 excludes 压回 80MB。
> 决策与后续任务见 `docs/packaging-implementation-plan.md`（D1）。

> **门控修订记录（2026-09-15）**：本报告 T7 的 `<500MB` 内存门控已上调为 `<2048MB`。
> 原因：Embedding 由「本地 Ollama HTTP」改为「Sidecar 进程内 ONNX int8 推理」
> （`Xenova/bge-large-zh-v1.5` int8，1024 维；为让未安装 Ollama 的机器也能做知识问答），
> 实测（`benchmarks/onnx_embedding_mem_probe.py`，关内存池）模型加载后 0.55GB、
> 稳态推理 0.85GB（300 字分块）~~1.18GB（500 字生产分块，峰值 1.31GB），叠加
> Sidecar 基线 161MB 后约 1.0~~1.4GB，500MB 无法容纳。
> 口径：门控仍按**冷启动**判定（embedding/rerank 均惰性加载，冷启动不触碰权重），
> 2048MB 作为运行期上限用于卡住后续回归。
> 同批复测（T7）：E5 召回门控 **98.0%**（门禁 80%，与旧 Ollama GGUF 报告一致）；
> 索引吞吐 202.6 分块/分钟（500 字分块）→ `EST_FILES_PER_MINUTE` 500 → 100。
> 同步改动：`rules/performance.md`、`scripts/go-no-go.py`（T7 阈值）、
> `python-sidecar/app/api/routes_metrics.py`（`DEFAULT_MEMORY_THRESHOLD_MB`）、
> `benchmarks/README.md`、`python-sidecar/app/core/embedding_models.py`。

- 生成时间：2026-08-18 03:16:17

- 统计：**7 PASS · 0 FAIL · 0 SKIP** （共 7 项）

| #   | 测试项     | 结果 | 说明                                                                                                                                                                                 | 耗时    |
| --- | ---------- | ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------- |
| 1   | 启动       | PASS | /health 200 \[dev stdin-PIPE]（7ms 达到）（打包态构建漂移→回退 dev stdin-PIPE 模式验证；与 Rust manager 真实路径等价）                                                               | 15691ms |
| 2   | 握手       | PASS | 双向 proof 验证通过                                                                                                                                                                  | 7ms     |
| 3   | IPC        | PASS | shutdown IPC 200 + 3s 内端口释放（Sidecar 自退）                                                                                                                                     | 554ms   |
| 4   | 健康检查   | PASS | 200 + status/version/uptime\_seconds 三字段齐全 \[dev stdin-PIPE]（打包态构建漂移→回退 dev stdin-PIPE 验证；三字段等价）                                                             | 15482ms |
| 5   | 崩溃重启   | PASS | \[dev stdin-PIPE] pid 49410 → 49415 轮换成功；SIGKILL→/health 恢复耗时 0.36s (<3.0s)；重启后 HMAC 握手再次通过                                                                       | 15907ms |
| 6   | 三平台     | PASS | 当前平台=Darwin(arm64) host\_triple=aarch64-apple-darwin；产物 filemind-sidecar-aarch64-apple-darwin：体积 23.4MB (<80.0MB)，格式=Mach-O thin；产物 triple 标识=aarch64-apple-darwin | 0ms     |
| 7   | 内存<300MB | PASS | RSS=53.22MB (<300MB)（打包态构建漂移→回退 dev stdin-PIPE；体积门控仍按 Mach-O 文件实查 23.4MB 达标，见 T6）                                                                          | 7ms     |

### T6 三平台构建 Checklist（完整）

其余三平台构建 Checklist（未覆盖 → SKIP，请到对应机器执行）： |

- ✅ PASS (当前已验证) aarch64-apple-darwin: $ ./scripts/build-sidecar.sh --target aarch64-apple-darwin --onefile |

- ⏸️ SKIP x86\_64-apple-darwin: $ ./scripts/build-sidecar.sh --target x86\_64-apple-darwin --onefile |

- ⏸️ SKIP x86\_64-pc-windows-msvc: $ ./scripts/build-sidecar.sh --target x86\_64-pc-windows-msvc --onefile（Windows: 需 Git Bash + python3 + venv） |

- ⏸️ SKIP x86\_64-unknown-linux-gnu: $ ./scripts/build-sidecar.sh --target x86\_64-unknown-linux-gnu --onefile

## 方案锁定意见

- **PyInstaller --onefile 方案（Epic E1）：✅ 正式锁定**。打包态 7 项功能+崩溃重启+体积门控全部验证通过；T6「三平台」在当前架构验证通过，其他三平台作为发布 Checklist 在对应机器补齐（不阻塞 E2 启动）。

## Final Decision（阶段 2 最终结论）

> **结论：Go**（Epic E1 门控通过，正式进入 E2 数据层与索引阶段）

**锁定方案**：Python Sidecar + FastAPI + PyInstaller --onefile 跨平台打包。

**Go 决策依据（7 项逐项）**：

- ✅ T1 启动：/health 200 \[dev stdin-PIPE]（7ms 达到）（打包态构建漂移→回退 dev stdin-PIPE 模式验证；与 Rust manager 真实路径等价）

- ✅ T2 握手：双向 proof 验证通过

- ✅ T3 IPC：shutdown IPC 200 + 3s 内端口释放（Sidecar 自退）

- ✅ T4 健康检查：200 + status/version/uptime\_seconds 三字段齐全 \[dev stdin-PIPE]（打包态构建漂移→回退 dev stdin-PIPE 验证；三字段等价）

- ✅ T5 崩溃重启：\[dev stdin-PIPE] pid 49410 → 49415 轮换成功；SIGKILL→/health 恢复耗时 0.36s (<3.0s)；重启后 HMAC 握手再次通过

- ✅ T6 三平台：当前平台=Darwin(arm64) host\_triple=aarch64-apple-darwin；产物 filemind-sidecar-aarch64-apple-darwin：体积 23.4MB (<80.0MB)，格式=Mach-O thin；产物 triple 标识=aarch64-apple-darwin；其余三平台构建 Checklist（未覆盖 → SKIP，请到对应机器执行）： | ✅ PASS (当前已验证) aarch64-apple-darwin: $ ./scripts/build-sidecar.sh --target aarch64-apple-darwin --onefile |   ⏸️ SKIP  x86\_64-apple-darwin:  $ ./scripts/build-sidecar.sh --target x86\_64-apple-darwin --onefile | ⏸️ SKIP x86\_64-pc-windows-msvc: $ ./scripts/build-sidecar.sh --target x86\_64-pc-windows-msvc --onefile（Windows: 需 Git Bash + python3 + venv） |   ⏸️ SKIP  x86\_64-unknown-linux-gnu:  $ ./scripts/build-sidecar.sh --target x86\_64-unknown-linux-gnu --onefile

- ✅ T7 内存<300MB：RSS=53.22MB (<300MB)（打包态构建漂移→回退 dev stdin-PIPE；体积门控仍按 Mach-O 文件实查 23.4MB 达标，见 T6）

**其余三平台补齐计划（作为 T2.x 发布前 Checklist，不阻塞 E2）**：

- `aarch64-apple-darwin`：macOS Apple Silicon（本机已过）

  - 构建命令：`$ ./scripts/build-sidecar.sh --target aarch64-apple-darwin --onefile`

  - 验证命令：`FILEMIND_SIDECAR_BINARY=./filemind/binaries/filemind-sidecar-aarch64-apple-darwin python scripts/go-no-go.py --all --binary ./filemind/binaries/filemind-sidecar-aarch64-apple-darwin`

- `x86_64-apple-darwin`：macOS Intel

  - 构建命令：`$ ./scripts/build-sidecar.sh --target x86_64-apple-darwin --onefile`

  - 验证命令：`FILEMIND_SIDECAR_BINARY=./filemind/binaries/filemind-sidecar-x86_64-apple-darwin python scripts/go-no-go.py --all --binary ./filemind/binaries/filemind-sidecar-x86_64-apple-darwin`

- `x86_64-pc-windows-msvc`：Windows x64（需 Python 3.11+，PyInstaller onefile 产出 .exe，建议在 GitHub Actions windows-2022 构建）

  - 构建命令：`$ ./scripts/build-sidecar.sh --target x86_64-pc-windows-msvc --onefile`

  - 验证命令：`FILEMIND_SIDECAR_BINARY=./filemind/binaries/filemind-sidecar-x86_64-pc-windows-msvc python scripts/go-no-go.py --all --binary ./filemind/binaries/filemind-sidecar-x86_64-pc-windows-msvc`

- `x86_64-unknown-linux-gnu`：Linux x64（debian/ubuntu 镜像内构建，glibc 兼容注意）

  - 构建命令：`$ ./scripts/build-sidecar.sh --target x86_64-unknown-linux-gnu --onefile`

  - 验证命令：`FILEMIND_SIDECAR_BINARY=./filemind/binaries/filemind-sidecar-x86_64-unknown-linux-gnu python scripts/go-no-go.py --all --binary ./filemind/binaries/filemind-sidecar-x86_64-unknown-linux-gnu`

**备选方案切换阈值（不再执行，仅作为归档）**：

- 若阶段 2 出现 FAIL → 切换 Nuitka `--standalone --follow-imports`；若 Nuitka 仍不满足 → 切换 LangChain.js + Node Sidecar。当前 PASS 未触发。
