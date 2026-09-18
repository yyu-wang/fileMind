// 选择要管理的目录并扫描（从 FilesPage 的 handleScan 抽出，页面只留一行派发）。
//
// T9.5 E2E：测试目录存在时跳过原生对话框（原生 open() 无法被 WebDriver 点击）。

import { open } from '@tauri-apps/plugin-dialog';

import { getE2eTestDir } from '@/lib/e2e';

/**
 * 打开目录选择框（或 E2E 测试目录）并交给 store 扫描。
 *
 * Args:
 *   scanFiles: 扫描目录并落库的 store action
 *   dataDirectory: 应用数据目录，作为对话框默认路径（null 时不设置）
 */
export async function scanDirectoryViaDialog(
  scanFiles: (path: string) => Promise<void>,
  dataDirectory: string | null,
): Promise<void> {
  const testDir = await getE2eTestDir();
  if (testDir) {
    await scanFiles(testDir);
    return;
  }
  const dialogOptions: Parameters<typeof open>[0] = {
    directory: true,
    multiple: false,
    title: '选择要管理的目录',
  };
  if (dataDirectory) {
    dialogOptions.defaultPath = dataDirectory;
  }
  const selected = await open(dialogOptions);
  if (typeof selected === 'string') {
    await scanFiles(selected);
  }
}
