// T9.5 E2E-004 规则编辑：新建 → 列表展示 → 启用/禁用 → 编辑取消 → 删除（真实后端）。
//
// 流程：直达文件页（SKIP_ONBOARDING）→ 侧栏进规则编辑 → 空态 →
// 新建「扩展名 pdf → 文档」规则 → 列表出现 → 禁用开关 → 点击列表项编辑后取消 →
// 删除（ConfirmDialog 二次确认）→ 回到空态。
//
// 说明：删除确认已从原生 window.confirm 改为应用内 ConfirmDialog，
// WebDriver 直接点 danger 确认按钮即可。
/* global HTMLSelectElement */

import { expect } from '@wdio/globals';

import { sel } from '../utils/selectors';

const SIDEBAR_RULES = `${sel.sidebarItem}[title*="规则编辑"]`;

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

  it('禁用开关 → 编辑预填 → 取消关闭', async () => {
    await $(sel.ruleItem).waitForExist({ timeout: 15000 });
    const firstItem = $(sel.ruleItem);

    // 第一个 it 保存后表单停在编辑态，但 RuleForm 的 useState 仍从 initial=null
    // （新建态挂载）初始化，name/pattern 为空。关闭表单后重新点击列表项，
    // 让 RuleForm 以 initial=savedRule 重新挂载，name/pattern 正确预填，
    // handleSubmit 校验才能通过。
    await $(sel.ruleCancel).click();
    await $(sel.ruleForm).waitForExist({ timeout: 10000, reverse: true });
    await firstItem.click();
    await $(sel.ruleForm).waitForExist({ timeout: 10000 });
    await expect($(sel.ruleNameInput)).toHaveValue('PDF 文档');

    // 幂等翻转：仅在当前 aria-pressed=true 时点击。WDIO 对失败的 it 会整块重试，
    // 若首轮 toggle 已落库 is_enabled=false，重试时 toggle 已是 false，再点会翻回 true
    // 导致 waitUntil(aria-pressed=false) 必然超时。
    await $(sel.ruleToggle).waitForExist({ timeout: 10000 });
    const currentPressed = await $(sel.ruleToggle).getAttribute('aria-pressed');
    if (currentPressed === 'true') {
      await $(sel.ruleToggle).click();
      await browser.waitUntil(
        async () => (await $(sel.ruleToggle).getAttribute('aria-pressed')) === 'false',
        { timeout: 5000, timeoutMsg: 'toggle 应翻转为 aria-pressed=false' },
      );
    }
    await $(sel.ruleSave).click();
    // 等待列表项出现"已禁用"标签（saveRule → load() → RuleList re-render）。
    // 用 waitUntil 而非 toHaveText：后者默认 5s 超时在 IPC+load 链路偶发不够。
    await browser.waitUntil(async () => /已禁用/.test(await firstItem.getText()), {
      timeout: 15000,
      timeoutMsg: '列表项应显示"已禁用"',
    });

    // 取消关闭当前编辑态 → 再点列表项进入编辑 → 表单预填 → 取消关闭
    await $(sel.ruleCancel).click();
    await $(sel.ruleForm).waitForExist({ timeout: 10000, reverse: true });
    await firstItem.click();
    await $(sel.ruleForm).waitForExist({ timeout: 10000 });
    await expect($(sel.ruleNameInput)).toHaveValue('PDF 文档');
    await $(sel.ruleCancel).click();
    await $(sel.ruleForm).waitForExist({ timeout: 10000, reverse: true });
  });

  it('删除规则（ConfirmDialog 二次确认）', async () => {
    // 点击列表项打开编辑表单 → 删除按钮在表单底部
    await $(sel.ruleItem).waitForExist({ timeout: 15000 });
    await $(sel.ruleItem).click();
    await $(sel.ruleDelete).waitForExist({ timeout: 10000 });
    await $(sel.ruleDelete).click();

    // ConfirmDialog 弹出 → 点「删除」（danger 按钮）确认
    await $(sel.ruleDeleteConfirm).waitForExist({ timeout: 10000 });
    await $(sel.ruleDeleteConfirm).click();

    await browser.waitUntil(async () => (await $$(sel.ruleItem).length) === 0, {
      timeout: 15000,
      timeoutMsg: '删除后规则列表应为空',
    });
    await $(sel.rulesEmpty).waitForExist({ timeout: 15000 });
  });
});
