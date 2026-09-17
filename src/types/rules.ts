// 规则类型前端模型：规则类型枚举与表单元数据。
//
// 从 models.ts 拆出（该文件是前端模型统一入口，仍再导出本模块的符号）。

/**
 * 规则类型（对应 Rust `Rule.rule_type` 字符串字段）。
 *
 * 后端 `classifier::match_rule` 当前仅支持 extension / path_keyword / regex；
 * magic_number / size 属 E4 补充范围，表单中以禁用占位呈现（后端 `_ => false` 不匹配）。
 */
export enum RuleType {
  Extension = 'extension',
  PathKeyword = 'path_keyword',
  Regex = 'regex',
  MagicNumber = 'magic_number',
  Size = 'size',
}

/** 规则类型元数据：展示名 + 匹配模式输入提示 + 是否可用。 */
export const RULE_TYPE_META: Record<RuleType, { label: string; hint: string; disabled: boolean }> =
  {
    [RuleType.Extension]: {
      label: '扩展名',
      hint: '逗号分隔扩展名，如 pdf,doc,docx',
      disabled: false,
    },
    [RuleType.PathKeyword]: {
      label: '路径关键词',
      hint: '文件名包含的关键词，如 项目、发票',
      disabled: false,
    },
    [RuleType.Regex]: {
      label: '正则表达式',
      hint: '匹配文件名的正则，如 ^20\\d{2}',
      disabled: false,
    },
    [RuleType.MagicNumber]: {
      label: '魔数 / 文件签名',
      hint: '按文件头部字节识别（即将推出）',
      disabled: true,
    },
    [RuleType.Size]: {
      label: '文件大小',
      hint: '按文件大小阈值匹配（即将推出）',
      disabled: true,
    },
  };
