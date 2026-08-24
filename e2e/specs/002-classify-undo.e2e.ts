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
import { existsSync } from 'node:fs';
import { join } from 'node:path';

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
    await expect($('.status-bar__text[title="文件数与索引状态"]')).toHaveText(/6 文件/);
  });

  it('智能分类：预览 3 个分组、零待确认', async () => {
    await $('a.sidebar__item[title*="智能分类"]').click();
    await $(sel.classifyStart).waitForClickable({ timeout: 15000 });
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
    const fileInDir = (dir: string, name: string) => existsSync(join(dataDir, dir, name));
    const allRestored = () => FIXTURE_FILES.every((f) => existsSync(join(dataDir, f)));

    // 确认执行全部 → 弹「选择分类方式」→ 移动分类
    await $(sel.classifyExecute).click();
    await $(sel.classifyModeMove).waitForClickable({ timeout: 10000 });
    await $(sel.classifyModeMove).click();

    // 执行完成 → DonePanel「分类完成」
    await $(sel.classifyDoneTitle).waitForExist({ timeout: 60000 });
    await expect($(sel.classifyDoneTitle)).toHaveText('分类完成');

    // 文件已移入分类子目录（移动模式：图片/文档/代码）
    await browser.waitUntil(
      () =>
        fileInDir('图片', 'photo-001.png') &&
        fileInDir('图片', 'photo-002.jpg') &&
        fileInDir('图片', 'photo-003.gif') &&
        fileInDir('文档', 'report-001.pdf') &&
        fileInDir('文档', 'report-002.pdf') &&
        fileInDir('代码', 'script-001.ts'),
      { timeout: 30000, timeoutMsg: '移动分类后文件应落入 图片/文档/代码 子目录' },
    );

    // 撤销本批 → 原位置全部恢复
    await $(sel.classifyUndo).click();
    await browser.waitUntil(allRestored, {
      timeout: 30000,
      timeoutMsg: '撤销后 6 个文件应全部回到扫描根目录',
    });
    for (const f of FIXTURE_FILES) {
      await expect(existsSync(join(dataDir, f))).toBe(true);
    }
  });
});
