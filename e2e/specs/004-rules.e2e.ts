// T9.5 E2E-004 规则编辑：新建 → 列表展示 → 启用/禁用 → 编辑取消 → 删除（真实后端）。
//
// 流程：直达文件页（SKIP_ONBOARDING）→ 侧栏进规则编辑 → 空态 →
// 新建「扩展名 pdf → 文档」规则 → 列表出现 → 禁用开关 → 编辑对话框取消 →
// 删除（覆盖 window.confirm）→ 回到空态。
//
// 说明：删除走原生 window.confirm，WebDriver 无法稳定点原生对话框，
// 用 browser.execute 覆写 confirm 恒返回 true，与后端删除链路保持一致。
/* global HTMLSelectElement */

import { expect } from '@wdio/globals';

import { sel } from '../utils/selectors';

const SIDEBAR_RULES = 'a.sidebar__item[title*="规则编辑"]';

describe('E2E-004 规则编辑', () => {
  it('空态 → 新建规则 → 列表展示', async () => {
    await $(sel.filesTitle).waitForExist({ timeout: 120000 });
    await $(SIDEBAR_RULES).click();
    await $(sel.rulesTitle).waitForExist({ timeout: 15000 });

    // 全新数据库 → 无规则 → 空态
    await $(sel.rulesEmpty).waitForExist({ timeout: 15000 });
    await expect($(sel.rulesEmpty)).toHaveText(/暂无自定义规则/);

    // 新建：扩展名 pdf → 目标分类 文档
    await $(sel.rulesNew).click();
    await $(sel.ruleForm).waitForExist({ timeout: 10000 });
    await $(sel.ruleNameInput).setValue('PDF 文档');
    await $(sel.rulePatternInput).setValue('pdf');
    // 目标分类：内置分类列表动态生成（listCategories IPC），取第一个非空 value。
    // 不用 selectByAttribute —— wdio 该实现无条件 value.trim()，正则/动态值必抛异常。
    await browser.execute(() => {
      const select = document.querySelector<HTMLSelectElement>('#rule-category');
      const option = select ? Array.from(select.options).find((o) => o.value) : undefined;
      if (select && option) {
        select.value = option.value;
        select.dispatchEvent(new Event('change', { bubbles: true }));
      }
    });
    await $(sel.ruleSave).click();

    // 保存后 store 重新拉取 → 列表出现该规则
    await $(sel.ruleItem).waitForExist({ timeout: 15000 });
    await expect($(sel.ruleItem)).toHaveText(/PDF 文档/);
    await expect($(sel.ruleItem)).toHaveText(/pdf/);
  });

  it('禁用开关 → 编辑对话框取消', async () => {
    await $(sel.ruleItem).waitForExist({ timeout: 15000 });
    const firstItem = $(sel.ruleItem);

    // 启用开关 → 关闭 → 「已禁用」标签出现
    // 注意：不能用 $(firstItem) 二次包装 —— chainable 元素会被 wdio 误当 selector，
    // 抛 "Spread syntax requires ...iterable"；元素方法直接挂在 firstItem 上。
    await firstItem.$(sel.ruleToggle).click();
    await expect(firstItem).toHaveText(/已禁用/);

    // 编辑 → 对话框预填 → 取消关闭，不改动数据
    await firstItem.$(sel.ruleEdit).click();
    await $(sel.ruleForm).waitForExist({ timeout: 10000 });
    await expect($(sel.ruleNameInput)).toHaveValue('PDF 文档');
    await $(sel.ruleCancel).click();
    await $(sel.ruleForm).waitForExist({ timeout: 10000, reverse: true });
  });

  it('删除规则（confirm 覆写为接受）', async () => {
    await browser.execute(() => {
      window.confirm = () => true;
    });
    await $(sel.ruleItem).waitForExist({ timeout: 15000 });
    await $(sel.ruleDelete).click();
    await browser.waitUntil(async () => (await $$(sel.ruleItem).length) === 0, {
      timeout: 15000,
      timeoutMsg: '删除后规则列表应为空',
    });
    await $(sel.rulesEmpty).waitForExist({ timeout: 15000 });
  });
});
