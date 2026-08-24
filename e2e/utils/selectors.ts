// T9.5 E2E：选择器集中映射（testid / aria / 稳定 class），spec 不散落字符串。
//
// 命名规约：`filesScan` → `[data-testid="files-scan"]`；class 选择器用真实 DOM class。
// 选择器均已对照组件源码核实（见各文件行内注释），不要凭 CSS 规范猜 class。

export const sel = {
  // —— 首次引导（OnboardingWizard / StepModeSelect / StepConsent / StepDirectory）——
  onboardingWizard: '[role="dialog"][aria-label="首次启动引导"]',
  onboardingModeLocal: 'input[name="inference-mode"][value="Local"]',
  onboardingModeCloud: 'input[name="inference-mode"][value="Cloud"]',
  onboardingNext: '[data-testid="onboarding-next"]',
  onboardingConsentCheck: '[data-testid="onboarding-consent-check"]',
  onboardingConsentConfirm: '[data-testid="onboarding-consent-confirm"]',
  onboardingStart: '[data-testid="onboarding-start"]',
  onboardingSelectedDir: '.onboarding__selected-dir',

  // —— 布局 ——
  sidebar: 'nav.sidebar',

  // —— 文件页（FilesPage / FileListTable）——
  filesTitle: '.files-page__title',
  filesScan: '[data-testid="files-scan"]',
  filesRow: '.files-table__row',
  filesEmpty: '.files-page__empty',

  // —— 智能分类（ClassifyPage / ClassifyModeDialog / ClassifyDonePanel / ClassifyPreviewTree）——
  classifyStart: '[data-testid="classify-start"]',
  classifyExecute: '[data-testid="classify-execute"]',
  classifyModeMove: '[data-testid="classify-mode-move"]',
  classifyUndo: '[data-testid="classify-undo"]',
  classifyHeader: '.classify-tree-header .count', // 「N 个已分类 · M 个待确认」
  classifyTreePanel: '.classify-tree .tree-panel',
  classifyNodeName: '.tree-node.parent > span:nth-child(2)', // 组名（分类名/待确认）
  classifyNodeCount: '.tree-node.parent .node-count', // 「N 文件」
  classifyDoneTitle: '.classify-done__title', // 「分类完成」

  // —— 知识问答（ChatPage / ChatInput / ChatBubble）——
  buildIndex: '[data-testid="build-index"]',
  chatIndexTip: '.chat-page__index-tip',
  chatInput: '.chat-input__field',
  chatBubbleCursor: '.chat-bubble__cursor',
  chatBubbleAssistant: '.chat-bubble--assistant .chat-bubble__content',

  // —— 规则编辑（RulesPage / RuleForm / RuleList）——
  rulesTitle: '.rules-page__title',
  rulesNew: '[data-testid="rules-new"]',
  rulesEmpty: '.state-view--empty',
  ruleForm: '[data-testid="rule-form"]',
  ruleSave: '[data-testid="rule-save"]',
  ruleCancel: '[data-testid="rule-cancel"]',
  ruleItem: '[data-testid="rule-item"]',
  ruleToggle: '[data-testid="rule-toggle"]',
  ruleEdit: '[data-testid="rule-edit"]',
  ruleDelete: '[data-testid="rule-delete"]',
  ruleNameInput: '#rule-name',
  ruleTypeSelect: '#rule-type',
  rulePatternInput: '#rule-pattern',
  ruleCategorySelect: '#rule-category',
  rulePriorityInput: '#rule-priority',

  // —— 设置（SettingsPage / OllamaStatusCard / ThemeSection）——
  settingsTitle: '.settings-page__title',
  ollamaStatus: '.settings-status',
  ollamaRedetect: '[data-testid="ollama-redetect"]',
  themeSystem: '[data-testid="theme-system"]',
  themeLight: '[data-testid="theme-light"]',
  themeDark: '[data-testid="theme-dark"]',
  embeddingCurrent: '[data-testid="embedding-current"]',
} as const;
