/**
 * 把 invoke 层抛出的任意值归一化成可展示的消息。
 *
 * specta 的 typedError 会把命令失败包成 `{status:'error'}`，所以异常路径理论上不可达；
 * 但一旦真的抛出（IPC 通道断开、序列化失败、`__TAURI_INTERNALS` 未就绪等），若不兜住：
 * - fileStore：`isScanning` 永久为 true → 按钮全禁用，用户只能重启应用
 * - classifyStore：status 卡在 Previewing/Running → 重入守卫永久拒绝后续预览与执行
 *
 * 由多个 store 共用（原先 fileStore 与 classify 各存一份逐字相同的实现）。
 */
export function ipcErrorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}
