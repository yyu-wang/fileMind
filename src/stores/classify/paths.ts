// 分类目标路径的拼接与校验：语义必须与 Rust 端对齐。

/**
 * 拼接目标路径：`...parts/fileName`（忽略空段、合并重复斜杠）。
 *
 * T6.12 手动分类用：分类的 `target_dir` 是相对扫描根的子目录，
 * 与 Rust `build_target_path` 的拼接语义一致——两边分叉会让前端预览的
 * 目标路径与后端实际落点不一致。
 */
export function joinPath(...parts: string[]): string {
  return parts
    .filter((p) => p && p.trim() !== '')
    .join('/')
    .replace(/\/{2,}/g, '/');
}

/** 校验分类目标目录：拒绝绝对路径 / 路径穿越（与 Rust `validate_relative_subpath` 语义对齐）。 */
export function isSafeTargetDir(targetDir: string): boolean {
  if (!targetDir || targetDir.trim() === '') return false;
  return !targetDir.includes('..') && !targetDir.startsWith('/') && !/^[A-Za-z]:/.test(targetDir);
}
