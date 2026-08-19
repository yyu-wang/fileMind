// IPC 入口：re-export 生成的 commands 对象与所有类型。
//
// 类型来源：src/types/ipc.ts（tauri-specta 自动生成，禁止手改）
// 命令客户端：./fileIpc（薄 re-export 层）

export { fileIpc } from './fileIpc';
export type * from '../../types/ipc';
