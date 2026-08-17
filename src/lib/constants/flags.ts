export const FEATURE_FLAGS = {
  RAG_ENABLED: false,
  CLOUD_INFERENCE: false,
  RULE_EDITOR: true,
} as const;

export const SIDECAR_PORT = 8765;
export const MAX_FILE_SIZE_MB = 100;
export const SUPPORTED_FILE_TYPES = [
  '.txt', '.md', '.pdf', '.docx', '.xlsx', '.pptx',
  '.jpg', '.png', '.gif', '.svg',
  '.csv', '.json', '.xml', '.html',
  '.js', '.ts', '.py', '.rs', '.go', '.java',
] as const;
