// IPC 客户端：从 tauri-specta 生成的 commands 对象薄封装。
//
// 设计说明：
// - 所有方法签名、参数顺序、返回类型由 src/types/ipc.ts 自动生成（specta）
// - 返回值为 typedError 包装：`Promise<{ status: 'ok'; data: T } | { status: 'error'; error: string }>`
//   前端调用时需先判断 status，再取 data 或 error
// - 规范 06-§2：前端不得手动修改 src/types/ipc.ts，本文件仅做 re-export
//
// 用法示例：
//   const r = await fileIpc.scanDirectory(path);
//   if (r.status === 'ok') { const files = r.data; } else { toast.error(r.error); }

export { commands as fileIpc } from '../../types/ipc';
