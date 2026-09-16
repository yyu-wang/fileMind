// T9.5 E2E-004 规则编辑：内置默认规则 → 新建 → 启用/禁用 → 编辑预填 → 删除（真实后端）。
//
// 流程：应用外壳就绪（SKIP_ONBOARDING）→ 侧栏进规则编辑 → 断言 2 条内置默认规则 →
// 新建「扩展名 pdf → 内置分类」规则 → 列表出现且排第一 → 启用开关翻转 →
// 再次选中预填 → 取消关闭 → 删光全部规则回到空态。
//
// 前提（对齐后端真实行为，勿改回「全新库 = 空态」）：
// `RuleRepo::seed_default_rules` 在 rules 表为空时种入 2 条**默认禁用**规则
// （「PDF 文件归档」/「文本文件归档」，priority=40，指向 builtin-document），
// 故全新库进规则页看到的是 2 条列表而非空态；空态只在把规则全删光后出现
// （种子仅在启动时执行，不会自动补回）。列表排序 `ORDER BY priority DESC, name ASC`，
// 新建规则 priority 默认 100 → 排在 2 条内置规则之前。
//
// 重试：本 spec 是状态化链路（后一用例依赖前一用例的落库结果），重跑无法自愈，
// 故每个用例显式关掉 mochaOpts.retries。此前 case1 失败后触发重试，重试又从
// 「等文件页」开始（此时应用已停在规则页）→ 报告里只剩一条误导性的超时，
// 真正的失败点被覆盖，排障成本极高。
/* global HTMLSelectElement */

import { expect } from '@wdio/globals';

import { sel } from '../utils/selectors';

const SIDEBAR_RULES = `${sel.sidebarItem}[title*="规则编辑"]`;
// 内置默认规则条数（RuleRepo::DEFAULT_RULES）
const SEEDED_RULE_COUNT = 2;

/**
 * 浏览器上下文回调：把首个 value 非空的 option 设为选中并派发 change。
 *
 * 说明：不用 wdio 的 `selectByAttribute`——其实现无条件 `value.trim()`，
 * 遇到正则/动态值必抛异常。回调放在模块级而非 `it` 内联，既避免
 * `describe → it → execute → find` 四层嵌套（max-nested-callbacks），
 * 也让「页面上下文里做了什么」有名字可循。
 *
 * @param selector 目标 `<select>` 的 CSS 选择器
 */
function selectFirstNonEmptyOption(selector: string): void {
  const select = document.querySelector<HTMLSelectElement>(selector);
  const option = select ? Array.from(select.options).find((o) => o.value) : undefined;
  if (select && option) {
    select.value = option.value;
    select.dispatchEvent(new Event('change', { bubbles: true }));
  }
}

/**
 * 等待规则列表条数收敛到期望值。
 *
 * 列表由 store 的 `load()` 异步刷新，直接断言条数会读到旧值，
 * 故统一用 waitUntil 收敛（比固定 sleep 稳定）。
 *
 * @param expected 期望条数
 */
async function waitRuleCount(expected: number): Promise<void> {
  await browser.waitUntil(async () => (await $$(sel.ruleItem).length) === expected, {
    timeout: 15000,
    timeoutMsg: `规则列表应收敛到 ${expected} 条`,
  });
}

/** 删除列表第一条规则（点击选中 → 删除 → ConfirmDialog 二次确认）。 */
async function deleteFirstRule(): Promise<void> {
  const firstItem = $(sel.ruleItem);
  await firstItem.waitForExist({ timeout: 15000 });
  await firstItem.click();
  await $(sel.ruleDelete).waitForExist({ timeout: 10000 });
  await $(sel.ruleDelete).click();
  await $(sel.ruleDeleteConfirm).waitForExist({ timeout: 10000 });
  await $(sel.ruleDeleteConfirm).click();
}

describe('E2E-004 规则编辑', () => {
  it('内置默认规则就位 → 新建规则 → 列表展示', async function () {
    this.retries(0);

    // 等应用外壳（侧栏）而非文件页：用例失败重跑时应用可能已停在规则页
    await $(sel.sidebar).waitForExist({ timeout: 120000 });
    await $(SIDEBAR_RULES).click();
    await $(sel.rulesTitle).waitForExist({ timeout: 15000 });

    // 全新库 → 后端种入 2 条内置默认规则（禁用），列表按 priority DESC 排序
    await waitRuleCount(SEEDED_RULE_COUNT);
    await expect($(sel.ruleItem)).toHaveText(/PDF 文件归档/);

    // 新建：扩展名 pdf → 目标分类取首个非空项（内置分类列表动态生成，listCategories IPC）
    await $(sel.rulesNew).click();
    await $(sel.ruleForm).waitForExist({ timeout: 10000 });
    await $(sel.ruleNameInput).setValue('PDF 文档');
    await $(sel.rulePatternInput).setValue('pdf');
    await browser.execute(selectFirstNonEmptyOption, '#rule-category');
    await $(sel.ruleSave).click();

    // 保存后 store 重新拉取 → 列表出现该规则；priority=100 > 内置的 40 → 排第一
    await waitRuleCount(SEEDED_RULE_COUNT + 1);
    await expect($(sel.ruleItem)).toHaveText(/PDF 文档/);
    await expect($(sel.ruleItem)).toHaveText(/pdf/);
  });

  it('启用开关翻转 → 再次选中预填 → 取消关闭', async function () {
    this.retries(0);

    // 上一用例保存后表单停在编辑态（handleSave 置 selectedId=saved.id，
    // RuleForm 因 key 变化重挂载 → initial=已保存规则 → 名称/模式预填）
    await $(sel.ruleNameInput).waitForExist({ timeout: 10000 });
    await expect($(sel.ruleNameInput)).toHaveValue('PDF 文档');

    // 新建规则默认启用（RuleForm is_enabled 默认 true）→ 翻转后保存
    await expect($(sel.ruleToggle)).toHaveAttribute('aria-pressed', 'true');
    await $(sel.ruleToggle).click();
    await browser.waitUntil(
      async () => (await $(sel.ruleToggle).getAttribute('aria-pressed')) === 'false',
      { timeout: 5000, timeoutMsg: 'toggle 应翻转为 aria-pressed=false' },
    );
    await $(sel.ruleSave).click();
    // 列表项出现「已禁用」标签（saveRule → load() → RuleList 重渲染）。
    // 用 waitUntil 而非 toHaveText：后者默认 5s 超时在 IPC+load 链路偶发不够。
    await browser.waitUntil(async () => /已禁用/.test(await $(sel.ruleItem).getText()), {
      timeout: 15000,
      timeoutMsg: '列表项应显示"已禁用"',
    });

    // 取消关闭当前编辑态 → 再点列表项进入编辑 → 表单预填 → 取消关闭
    await $(sel.ruleCancel).click();
    await $(sel.ruleForm).waitForExist({ timeout: 10000, reverse: true });
    await $(sel.ruleItem).click();
    await $(sel.ruleForm).waitForExist({ timeout: 10000 });
    await expect($(sel.ruleNameInput)).toHaveValue('PDF 文档');
    await $(sel.ruleCancel).click();
    await $(sel.ruleForm).waitForExist({ timeout: 10000, reverse: true });
  });

  it('删除自定义规则 → 删光内置规则回到空态', async function () {
    this.retries(0);

    // 第一条是自定义规则（priority=100）→ 删掉后剩 2 条内置
    await deleteFirstRule();
    await waitRuleCount(SEEDED_RULE_COUNT);

    // 再删两条内置：rules 表清空 → 页面回到空态（种子只在启动时执行，不会补回）
    await deleteFirstRule();
    await waitRuleCount(SEEDED_RULE_COUNT - 1);
    await deleteFirstRule();
    await waitRuleCount(0);

    await $(sel.rulesEmpty).waitForExist({ timeout: 15000 });
    await expect($(sel.rulesEmpty)).toHaveText(/暂无自定义规则/);
    await $(sel.ruleItem).waitForExist({ timeout: 5000, reverse: true });
  });
});
