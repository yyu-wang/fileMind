// T9.5 E2E-003 知识问答（RAG）——真实后端 + 真实 sidecar，默认 skip，`RUN_E2E=1 --rag` 才跑。
//
// 前置（由 scripts/e2e-run.sh 的 003 门控分支负责准备与快速校验）：
//   1. 本机真实 Ollama + 生成模型（默认 `qwen3.8-27b`，见 V010 迁移）——本地生成仍走 Ollama；
//   2. 进程内 Embedding 的模型文件：脚本用 scripts/e2e-prepare-models.sh 幂等下到共享缓存
//      `e2e/.cache/models` 并注入 `FILEMIND_MODEL_DIR`（每个 spec 的数据目录都是全新的，
//      不注入就会 EmbeddingUnavailableError 卡在建立索引）；
//   3. Reranker 不依赖 Ollama，走 sentence-transformers 的 HF 缓存（~/.cache/huggingface）。
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

/**
 * 清空聊天历史并重载页面。
 *
 * chatStore 用 zustand persist 把 messages 写进 WebView 的 localStorage（key
 * `filemind-chat`），而 WebView 数据目录**不受**每个 spec 的 FILEMIND_DATA_HOME
 * 隔离 → 上一次运行（甚至开发者手动用过的一次真实会话）会被恢复成历史气泡，
 * 断言就打在旧回答上（实测读到过 2026-09-08 留下的旧回答，且那段旧文本恰好含
 * 断言关键字，只改断言会假阳性通过）。历史在 app 启动时即 hydrate 进内存，
 * 故必须「清 + 重载」，让 store 以空历史重新初始化。
 */
async function resetChatHistory(): Promise<void> {
  await browser.execute(() => {
    window.localStorage.removeItem('filemind-chat');
    // 延后一拍再重载：先让本次 executeScript 正常返回，避免 WebDriver 撞上导航中的页面
    window.setTimeout(() => window.location.reload(), 0);
  });
  // 重载后应用从头渲染：等文件页标题回来，后续用例按原流程继续
  await $(sel.filesTitle).waitForExist({ timeout: 120000 });
}

/**
 * 取最后一条助手气泡的文本。
 *
 * 多轮问答时 `$(sel.chatBubbleAssistant)` 命中的是**第一条**气泡（上一轮回答），
 * 故统一取最后一条——本次断言目标始终是最新回答。
 */
async function lastAssistantText(): Promise<string> {
  const bubbles = await $$(sel.chatBubbleAssistant);
  if (bubbles.length === 0) return '';
  return bubbles[bubbles.length - 1].getText();
}

describe('E2E-003 知识问答（RAG）', () => {
  ragTest('扫描 10 篇文档', async function () {
    this.timeout(120000);
    // 清掉跨运行残留的聊天历史（WebView localStorage 不受数据目录隔离），见函数注释
    await resetChatHistory();
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
    // 索引中… → 完成提示（进程内 ONNX 向量化 10 篇，首次含会话加载，可能较慢）
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

    const answer = await lastAssistantText();
    // 字符串断言必须用 Jest 匹配器：`expect(str).toHaveText()` 是**元素**匹配器，
    // 传字符串只会得到 Received: undefined（此前就是这么挂的）
    expect(answer).toContain('本地推理');
    expect(answer).toContain('云端推理');

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

    const answer = await lastAssistantText();
    expect(answer).toContain('隐私');
  });
});
