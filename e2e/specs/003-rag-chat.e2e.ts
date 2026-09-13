// T9.5 E2E-003 知识问答（RAG）——真实后端 + 真实 sidecar，但需真实 Ollama 三模型
// （bge-large-zh-v1.5 / bge-reranker-v2-m3 / chat LLM），默认 skip，`RUN_E2E=1` 才跑。
//
// 流程：直达文件页 → 扫描 10 篇 rag-docs → 侧栏进知识问答 → 建立索引 →
// 等「索引完成：10 个文件」→ 提问「FileMind 支持哪些推理模式？」→ 流式光标 →
// 回答含「本地推理」「云端推理」→ 点引用 [1] → .files-preview 预览面板打开 →
// 追问「本地模式有什么优势？」→ 回答含「隐私」（上下文代入/代词消解）。
//
// 门控与 sidecar 层 E2E（RUN_E2E=1）一致；e2e-run.sh --rag 才进本 spec。

import { expect } from '@wdio/globals';

import { sel } from '../utils/selectors';

const RAG_ENABLED = process.env.RUN_E2E === '1';
// 门控：未启用时注册为 skip，保证 spec 文件在默认 `npm run test:e2e` 下不产生失败。
const ragTest = RAG_ENABLED ? it : it.skip;

describe('E2E-003 知识问答（RAG）', () => {
  ragTest('扫描 10 篇文档', async function () {
    this.timeout(120000);
    await $(sel.filesTitle).waitForExist({ timeout: 120000 });
    await $(sel.filesScan).click();
    await browser.waitUntil(async () => (await $$(sel.filesRow)).length === 10, {
      timeout: 30000,
      timeoutMsg: 'rag-docs fixture 应有 10 行',
    });
  });

  ragTest('建立索引 → 索引完成 10 个文件', async function () {
    this.timeout(300000);
    await $(`${sel.sidebarItem}[title*="知识问答"]`).click();
    await $(sel.buildIndex).waitForExist({ timeout: 15000 });
    await $(sel.buildIndex).click();
    // 索引中… → 完成提示（真实 Ollama 向量化 10 篇，可能较慢）
    await $(sel.chatIndexTip).waitForExist({ timeout: 240000 });
    await expect($(sel.chatIndexTip)).toHaveText(/索引完成：10 个文件/);
  });

  ragTest('提问推理模式 → 流式回答含 本地推理/云端推理 → 引用 [1] 可跳转', async function () {
    this.timeout(300000);
    await $(sel.chatInput).waitForExist({ timeout: 15000 });
    await $(sel.chatInput).setValue('FileMind 支持哪些推理模式？');
    await browser.keys('Enter');

    // 流式光标出现（发送成功，SSE 开始）→ 消失（回答完成）
    await $(sel.chatBubbleCursor).waitForExist({ timeout: 30000 });
    await $(sel.chatBubbleCursor).waitForExist({ reverse: true, timeout: 120000 });

    const answer = await $(sel.chatBubbleAssistant).getText();
    await expect(answer).toHaveText(/本地推理/);
    await expect(answer).toHaveText(/云端推理/);

    // 引用标签 [1] 存在且可点 → 文件预览面板打开
    const citations = $$('.chat-citation');
    await expect(citations).toBeElementsArrayOfSize({ gte: 1 });
    await $('.chat-citation').click();
    await $('.files-preview[aria-label="文件预览"]').waitForExist({ timeout: 15000 });
    await expect($('.files-preview__title')).toBeExisting();
  });

  ragTest('追问本地模式优势 → 回答含 隐私（上下文代入）', async function () {
    this.timeout(300000);
    // 预览面板遮挡输入区，先关闭
    const closeBtn = $('.files-preview__close');
    if (await closeBtn.isExisting()) {
      await closeBtn.click();
    }
    await $(sel.chatInput).waitForExist({ timeout: 15000 });
    await $(sel.chatInput).setValue('本地模式有什么优势？');
    await browser.keys('Enter');

    await $(sel.chatBubbleCursor).waitForExist({ timeout: 30000 });
    await $(sel.chatBubbleCursor).waitForExist({ reverse: true, timeout: 120000 });

    const answer = await $(sel.chatBubbleAssistant).getText();
    await expect(answer).toHaveText(/隐私/);
  });
});
