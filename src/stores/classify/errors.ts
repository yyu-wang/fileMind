/**
 * 把 invoke 层抛出的任意值归一化成可展示的消息（与 fileStore 同款兜底）。
 *
 * specta 的 typedError 会把命令失败包成 `{status:'error'}`，异常路径理论上不可达；
 * 但一旦抛出且调用方不复位状态，status 会永久停在 Previewing/Running——
 * 既有重入守卫会让分类页彻底不可用（按钮禁用、后续预览/执行全被拒绝）。
 *
 * 由 store 与 executor 共用，避免各自 `String(err)` 写出 `[object Object]`。
 */
export function ipcErrorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}
