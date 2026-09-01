// 引用文件匹配：按文件名多策略查找 FileInfo（从 ChatPage 抽出的纯函数）。
//
// 匹配顺序：精确 → 大小写不敏感 → 去扩展名 → 前缀/子串 → Rust FTS 兜底。
// 前四级走内存列表（<1ms），兜底走慢速 IPC 并带 3s 超时保护。

import { fileIpc } from './ipc';
import type { FileInfo } from '@/types/ipc';

/** 按文件名查找 FileInfo：本地列表优先，兜底走 Rust 文件名搜索（多策略匹配）。 */
export async function findFileByName(
  fileName: string,
  fallbackFiles: FileInfo[],
): Promise<{ file: FileInfo; fastPath: boolean } | null> {
  const nameLower = fileName.toLowerCase();
  const nameNoExt = nameLower.replace(/\.[^.]+$/, '');

  // 1) 精确匹配（大小写敏感→不敏感）
  const exactHit = fallbackFiles.find((f) => f.file_name === fileName);
  if (exactHit) return { file: exactHit, fastPath: true };
  const ciHit = fallbackFiles.find((f) => f.file_name.toLowerCase() === nameLower);
  if (ciHit) return { file: ciHit, fastPath: true };

  // 2) 去扩展名匹配（数据库里是 .md/.txt 但 citation 丢了后缀）
  if (nameNoExt.length > 0) {
    const noExtHit = fallbackFiles.find((f) => {
      const base = f.file_name.toLowerCase().replace(/\.[^.]+$/, '');
      return base === nameNoExt || base === nameLower;
    });
    if (noExtHit) return { file: noExtHit, fastPath: true };
  }

  // 3) 共享前缀/子串包含匹配
  const looseHit = fallbackFiles.find((f) => {
    const db = f.file_name.toLowerCase();
    return db.includes(nameLower) || nameLower.includes(db);
  });
  if (looseHit) return { file: looseHit, fastPath: true };

  // 4) 兜底：Rust FTS/文件名 LIKE 模糊搜索（慢速 IPC 路径）。
  //    加 3s 超时：防止 Tauri invoke 卡死/无响应时永久 pending（用户感知"点了没反应"）。
  const timeout = new Promise<never>((_, reject) => {
    const id = setTimeout(() => {
      clearTimeout(id);
      reject(new Error('搜索引用文件超时（3s）'));
    }, 3000);
  });
  const result = await Promise.race([fileIpc.searchByFilename(fileName, 5), timeout]);
  if (result.status === 'ok' && result.data.length > 0) {
    const file = result.data.find((f) => f.file_name.toLowerCase() === nameLower) ?? result.data[0];
    return { file, fastPath: false };
  }
  return null;
}
