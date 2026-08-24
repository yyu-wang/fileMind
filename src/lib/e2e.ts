// T9.5 E2E 测试钩子：包装 debug-only 的 `e2e_get_test_dir` 命令。
//
// 安全/健壮性：release 构建未注册该命令 → `invoke` 抛错 → 返回 null；
// dev 构建未设置 `FILEMIND_E2E_DATA_DIR` → 返回 null。两种情况都走正常交互路径，
// 不影响生产/开发使用。前端各调用方只在拿到非 null 测试目录时才改变行为。

import { fileIpc } from './ipc';

/**
 * 返回 E2E 测试文件目录；非 E2E 环境（release / 未设 env）返回 null。
 */
export async function getE2eTestDir(): Promise<string | null> {
  try {
    return await fileIpc.e2eGetTestDir();
  } catch {
    // 命令未注册（release）或 IPC 不可用（非 Tauri 环境）——按普通路径处理
    return null;
  }
}
