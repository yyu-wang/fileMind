#!/usr/bin/env bash
# 复杂度门禁：把 rules/complexity.md 的硬性阈值变成可执行检查。
#
# 覆盖两个维度：
#   1. 文件行数（全部纳管语言）：阈值分组见 §文件行数限制，带基线白名单；
#   2. Python 函数行数（ast 扫描，源码 60 / 测试 120）：ruff 无对应规则（PLR0915 数的是
#      语句数），故这一维度由本脚本补齐；TS 与 Rust 的函数行数分别由 ESLint
#      `max-lines-per-function` 与 clippy `too_many_lines` 管控。
#
# 背景：这些阈值此前只写在规则文档里，CI/本地都没有对应检查，文件可以无限增长
# （2026-09-13 盘点：file_ops.rs 2572 行、manager.rs 1086 行、file_repo.rs 902 行…）。
# 本脚本把「强制拆分阈值」变成门禁，并引入**基线白名单**，让历史欠账可以逐个消账
# 而不是一次性大爆炸式重构：
#
#   - 超过强制阈值 且 未登记基线            → FAIL（禁止新增超限文件）
#   - 已登记基线 但行数 > 基线记录值         → FAIL（禁止继续增长）
#   - 已登记基线 且未增长                    → PASS 并计入「待消账」清单
#   - 超过警告阈值但未达强制阈值             → WARN（不阻断）
#
# 消账方式：把文件拆到阈值内后，从 scripts/file-size-baseline.txt 删除对应行。
#
# 用法：
#   bash scripts/check-file-size.sh
#
# 口径：
#   - 按**原始行数**统计（等价 `wc -l`），与 rules/complexity.md 示例脚本一致；
#     不剔除空行/注释（剔除后的口径会让「带大量注释的文件」绕过管控）。
#   - 生成物与依赖目录不在管控范围：src/types/ipc.ts 由 tauri-specta 生成（禁止手改），
#     node_modules / target / dist / .venv / __pycache__ / gen 同理。
#   - 阈值分组与 rules/complexity.md §文件行数限制 一一对应。

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

BASELINE_FILE="scripts/file-size-baseline.txt"

# 纳管根目录（其余路径一律不检查）
SCAN_ROOTS=(src src-tauri/src src-tauri/tests python-sidecar/app python-sidecar/tests tests)

# 生成物 / 依赖目录：不计入行数管控
EXCLUDE_PATTERNS=(
  'src/types/ipc.ts'
  '/node_modules/'
  '/target/'
  '/dist/'
  '/build/'
  '/.venv/'
  '/__pycache__/'
  '/gen/'
)

# 路径 → "警告阈值 强制阈值"（未纳管则输出空）
limits_for() {
  case "$1" in
    # 测试文件
    *.test.ts | *.test.tsx | *_tests.rs) echo "400 600" ;;
    tests/* | */tests/*) echo "400 600" ;;
    # React 页面 / 组件
    src/pages/*) echo "300 400" ;;
    src/components/*) echo "200 300" ;;
    *.tsx) echo "200 300" ;;
    # Rust 模块
    src-tauri/src/*) echo "300 500" ;;
    # Python 模块
    python-sidecar/app/*) echo "300 500" ;;
    # TypeScript 工具（src 下除页面/组件外的 .ts）
    src/*.ts) echo "150 250" ;;
    # 样式
    *.css) echo "200 400" ;;
    *) echo "" ;;
  esac
}

is_excluded() {
  local file="$1" pattern
  for pattern in "${EXCLUDE_PATTERNS[@]}"; do
    if [[ "$file" == *"$pattern"* ]]; then
      return 0
    fi
  done
  return 1
}

# 基线中记录的行数（未登记则无输出）
baseline_lines_for() {
  local file="$1"
  [[ -f "$BASELINE_FILE" ]] || return 0
  awk -v target="$file" '$1 == target { print $2; exit }' "$BASELINE_FILE"
}

list_managed_files() {
  find "${SCAN_ROOTS[@]}" -type f \
    \( -name '*.ts' -o -name '*.tsx' -o -name '*.rs' -o -name '*.py' -o -name '*.css' \) \
    2>/dev/null | LC_ALL=C sort
}

fails=0
warns=0
baselined=0

while IFS= read -r file; do
  is_excluded "$file" && continue
  limits=$(limits_for "$file")
  # 未纳管的扩展名/路径：limits 为空，跳过
  [[ -n "$limits" ]] || continue
  warn_limit="${limits%% *}"
  force_limit="${limits##* }"

  lines=$(wc -l <"$file" | tr -d ' ')
  base=$(baseline_lines_for "$file")

  if ((lines > force_limit)); then
    if [[ -z "$base" ]]; then
      # 注意：变量一律用 ${} 包裹——紧跟中文多字节字符时，bash 会把 `$var 中文`
      # 整体当作变量名（非 ASCII 字节被误判为标识符），报 unbound variable
      echo "FAIL: ${file} 有 ${lines} 行（强制阈值 ${force_limit}），且未登记基线——请先拆分"
      fails=$((fails + 1))
    elif ((lines > base)); then
      echo "FAIL: ${file} 由基线 ${base} 行增长到 ${lines} 行（超阈值文件只允许拆分，不允许增长）"
      fails=$((fails + 1))
    else
      baselined=$((baselined + 1))
    fi
  elif ((lines > warn_limit)); then
    echo "WARN: ${file} 有 ${lines} 行（警告阈值 ${warn_limit}，强制阈值 ${force_limit}）"
    warns=$((warns + 1))
  fi
done < <(list_managed_files)

echo "---- 文件行数门禁：FAIL $fails / WARN $warns / 基线内待消账 $baselined ----"

# ---- 函数行数门禁（Python）----
# 与 ESLint max-lines-per-function（TS）、clippy too_many_lines（Rust）同口径：
# 源码 60 行、测试 120 行（rules/complexity.md §函数复杂度限制）。
# ruff 没有「函数行数」规则（PLR0915 数的是语句数，不是行数），故用 ast 扫描补齐这一维度。
# 口径：函数定义行到结束行（含函数体内空行与注释），等价「原始行数」。
# 纳管根目录与文件行数门禁保持同口径（python-sidecar/app + python-sidecar/tests）；
# 绝不能整个 rglob('python-sidecar')——dist/ 下是 PyInstaller 产物内的第三方包。
# 解析器优先用项目虚拟环境（Python 3.12）——系统自带 python3 在 macOS 上是 3.9，
# 解析不了 3.12 语法（会误报「语法错误」）。
PY_BIN="python3"
for candidate in python-sidecar/.venv/bin/python .venv/bin/python python3.12; do
  if command -v "$candidate" >/dev/null 2>&1; then
    PY_BIN="$candidate"
    break
  fi
done
py_report=""
py_status=0
py_report=$("${PY_BIN}" - <<'PY'
import ast, pathlib, sys

# 输出强制 UTF-8：Windows 控制台/管道默认 cp1252，而本段消息含中文且**只在有超限时**
# 才产生，print 会抛 UnicodeEncodeError（CI 在 ubuntu 上不暴露，Windows 本地会踩）。
# 与 scripts/go-no-go.py、scripts/fetch-llama-server.sh 同一处置。
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8")

SOURCE_LIMIT, TEST_LIMIT = 60, 120
fails = []

for root in ('python-sidecar/app', 'python-sidecar/tests'):
    base = pathlib.Path(root)
    if not base.is_dir():
        continue
    for path in sorted(base.rglob('*.py')):
        rel = str(path)
        if any(part in rel for part in ('/__pycache__/', '/gen/', '/build/', '/dist/')):
            continue
        limit = TEST_LIMIT if ('/tests/' in rel or path.name.startswith('test_')) else SOURCE_LIMIT
        try:
            source = path.read_text(encoding='utf-8')
        except (UnicodeDecodeError, OSError) as exc:
            fails.append(f'FAIL: {rel} 无法按 UTF-8 读取（{exc}）——请检查文件编码')
            continue
        try:
            tree = ast.parse(source)
        except SyntaxError as exc:
            fails.append(f'FAIL: {rel} 语法错误，无法统计函数行数: {exc}')
            continue
        for node in ast.walk(tree):
            if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
                lines = (node.end_lineno or node.lineno) - node.lineno + 1
                if lines > limit:
                    fails.append(
                        f'FAIL: {rel}:{node.lineno} 函数 {node.name}() 有 {lines} 行'
                        f'（阈值 {limit}）——请拆分'
                    )

print('\n'.join(fails))
PY
) || py_status=$?

py_fails=0
if [[ ${py_status} -ne 0 ]]; then
  # 扫描没跑完：解释器缺失只告警（历史口径，环境问题不该卡住提交），解释器报错则必须
  # FAIL——否则报告丢失会伪装成 FAIL 0（原先 `|| true` 就是这个坑），这一维度白设。
  if command -v "${PY_BIN}" >/dev/null 2>&1; then
    echo "FAIL: Python 函数行数扫描未能完成（${PY_BIN} 退出码 ${py_status}，traceback 见上方日志）——门禁不得静默跳过"
    py_fails=1
  else
    echo "WARN: 未找到可用的 Python 解释器（需 3.12+），Python 函数行数检查被跳过——请保证该检查可执行"
  fi
elif [[ -n "${py_report}" ]]; then
  echo "${py_report}"
  py_fails=$(printf '%s\n' "${py_report}" | grep -c '^FAIL:' || true)
fi
echo "---- 函数行数门禁（Python）：FAIL ${py_fails} ----"

if ((fails > 0 || py_fails > 0)); then
  echo "提示：超限文件请按 rules/complexity.md §拆分策略 拆分；历史欠账登记在 $BASELINE_FILE"
  exit 1
fi
