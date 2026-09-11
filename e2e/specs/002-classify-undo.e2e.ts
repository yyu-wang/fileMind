// T9.5 E2E-002 扫描→分类→执行→撤销（真实后端 + 真实 sidecar，零待确认 fixture）。
//
// 流程：直达文件页（SKIP_ONBOARDING）→ 扫描 → 6 行 + 状态栏 6 文件 →
// 侧栏进智能分类 → 预览 3 组零待确认 → 确认执行（移动）→ 分类完成 →
// 撤销本批 → fs.existsSync 断言原文件全部回到原位。
//
// fixture（e2e/fixtures/scan-tree，纯扩展名启发式命中 → 零待确认不触发 LLM 兜底）：
//   图片(3)：photo-001.png / photo-002.jpg / photo-003.gif
//   文档(2)：report-001.pdf / report-002.pdf
//   代码(1)：script-001.ts

import { expect } from '@wdio/globals';
import { existsSync, readdirSync, type Dirent } from 'node:fs';
import { dirname, join } from 'node:path';

import { sel } from '../utils/selectors';
import { e2eDataDir } from '../utils/reset';

const FIXTURE_FILES = [
  'photo-001.png',
  'photo-002.jpg',
  'photo-003.gif',
  'report-001.pdf',
  'report-002.pdf',
  'script-001.ts',
] as const;

/** 递归列出目录树（最多 2 层），用于失败取证。 */
function snapshotTree(root: string): string {
  const lines: string[] = [];
  const walk = (dir: string, depth: number): void => {
    if (depth > 2) return;
    let entries: Dirent[];
    try {
      entries = readdirSync(dir, { withFileTypes: true });
    } catch (err: unknown) {
      lines.push(`${'  '.repeat(depth)}<读取失败: ${String(err)}>`);
      return;
    }
    for (const entry of entries) {
      lines.push(`${'  '.repeat(depth)}${entry.name}${entry.isDirectory() ? '/' : ''}`);
      if (entry.isDirectory()) walk(join(dir, entry.name), depth + 1);
    }
  };
  walk(root, 0);
  return lines.join('\n');
}

describe('E2E-002 扫描→分类→执行→撤销', () => {
  it('扫描后列表显示 6 个文件 + 状态栏 6 文件', async () => {
    // 直达文件页（FILEMIND_E2E_SKIP_ONBOARDING=1 预置完成引导）
    await $(sel.filesTitle).waitForExist({ timeout: 120000 });

    await $(sel.filesScan).click();
    await browser.waitUntil(async () => (await $$(sel.filesRow)).length === FIXTURE_FILES.length, {
      timeout: 30000,
      timeoutMsg: `扫描后文件表应有 ${FIXTURE_FILES.length} 行`,
    });
    await expect($(sel.filesScan)).toHaveText('扫描目录'); // 扫描态已结束
    // toHaveText 传正则做局部匹配（expect-webdriverio v6 无 toHaveTextContaining）
    await expect($(sel.statusFiles)).toHaveText(/6 文件/);
  });

  it('智能分类：预览 3 个分组、零待确认', async () => {
    await $(`${sel.sidebarItem}[title*="智能分类"]`).click();
    await $(sel.classifyStart).waitForExist({ timeout: 15000 });
    await $(sel.classifyStart).click();

    // 预览树（classify_preview 对 6 个 fixture 全部启发式命中 → 无需 LLM 兜底）
    await $('.classify-tree').waitForExist({ timeout: 30000 });
    await expect($(sel.classifyHeader)).toHaveText(/6 个已分类/);
    await expect($(sel.classifyHeader)).toHaveText(/0 个待确认/);
    // 图片 / 文档 / 代码 三个分组
    const panels = await $$(sel.classifyTreePanel);
    await expect(panels).toBeElementsArrayOfSize(3);
  });

  it('确认执行（移动）→ 分类完成 → 撤销本批还原原位', async () => {
    const dataDir = e2eDataDir();
    // 分类输出统一落到与扫描根**同级**的收纳根 `<扫描根名>_已分类`（Rust
    // classifier::sibling_output_root），不在扫描目录内部——扫描目录只留待整理文件。
    const outputDir = `${dataDir}_已分类`;
    const fileInOutput = (dir: string, name: string) => existsSync(join(outputDir, dir, name));
    const allRestored = () => FIXTURE_FILES.every((f) => existsSync(join(dataDir, f)));

    // 确认执行全部 → 弹「选择分类方式」→ 移动分类
    await $(sel.classifyExecute).click();
    await $(sel.classifyModeMove).waitForExist({ timeout: 10000 });
    await $(sel.classifyModeMove).click();

    // 执行完成 → DonePanel「分类完成」
    await $(sel.classifyDoneTitle).waitForExist({ timeout: 60000 });
    await expect($(sel.classifyDoneTitle)).toHaveText('分类完成');

    // 文件已移入分类子目录（移动模式：图片/文档/代码），随后撤销回原位。
    // 包 try/catch 取证：WDIO 对失败的 it 会整块重跑一次，而重跑时预览已被消费
    // （classify-execute 消失），最终报出的错误指向重跑，掩盖首轮真实失败点。
    try {
      await browser.waitUntil(
        () =>
          fileInOutput('图片', 'photo-001.png') &&
          fileInOutput('图片', 'photo-002.jpg') &&
          fileInOutput('图片', 'photo-003.gif') &&
          fileInOutput('文档', 'report-001.pdf') &&
          fileInOutput('文档', 'report-002.pdf') &&
          fileInOutput('代码', 'script-001.ts'),
        { timeout: 30000, timeoutMsg: `移动分类后文件应落入 ${outputDir}/图片|文档|代码` },
      );

      // 撤销本批 → 原位置全部恢复
      await $(sel.classifyUndo).click();
      await browser.waitUntil(allRestored, {
        timeout: 30000,
        timeoutMsg: '撤销后 6 个文件应全部回到扫描根目录',
      });
    } catch (err: unknown) {
      const pageText = String(await browser.execute(() => document.body.innerText));
      // 扫描根的**父目录**：一眼看清源目录与同级收纳根各自的内容
      const parentDir = dirname(dataDir);
      console.error(`[E2E-002 取证] ${parentDir} 实际树:\n${snapshotTree(parentDir)}`);
      console.error(`[E2E-002 取证] 页面文本:\n${pageText}`);
      throw err;
    }

    for (const f of FIXTURE_FILES) {
      await expect(existsSync(join(dataDir, f))).toBe(true);
    }
  });
});
