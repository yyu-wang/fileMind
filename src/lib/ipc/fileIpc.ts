import { invoke } from '@tauri-apps/api/core';

export interface FileInfo {
  id: string;
  path: string;
  file_name: string;
  file_size: number;
  content_hash: string | null;
  category: string | null;
  created_at: string;
  updated_at: string;
}

export interface FileListResponse {
  files: FileInfo[];
  total: number;
  page: number;
  page_size: number;
}

export interface FileStats {
  total_files: number;
  categorized_files: number;
  uncategorized_files: number;
  duplicate_groups: number;
  total_size_bytes: number;
}

export interface SearchResult {
  file: FileInfo;
  score: number;
}

export interface FileOperation {
  source_path: string;
  target_path: string;
  operation_type: 'Move' | 'Rename' | 'Delete';
}

export interface OperationPreview {
  operation: FileOperation;
  source_exists: boolean;
  target_exists: boolean;
  conflict: boolean;
}

export interface BatchResult {
  results: Array<{
    operation: FileOperation;
    success: boolean;
    error: string | null;
  }>;
  success_count: number;
  failed_count: number;
}

export const fileIpc = {
  async scanDirectory(path: string): Promise<FileInfo[]> {
    return invoke<FileInfo[]>('scan_directory', { path });
  },

  async listFiles(params: {
    category?: string | null;
    page?: number;
    pageSize?: number;
  }): Promise<FileListResponse> {
    return invoke<FileListResponse>('list_files', {
      category: params.category ?? null,
      page: params.page ?? 0,
      pageSize: params.pageSize ?? 50,
    });
  },

  async searchFiles(query: string, limit?: number): Promise<SearchResult[]> {
    return invoke<SearchResult[]>('search_files', { query, limit: limit ?? 50 });
  },

  async searchByFilename(pattern: string, limit?: number): Promise<FileInfo[]> {
    return invoke<FileInfo[]>('search_by_filename', { pattern, limit: limit ?? 50 });
  },

  async getFileStats(): Promise<FileStats> {
    return invoke<FileStats>('get_file_stats');
  },

  async updateFileCategory(id: string, category: string): Promise<void> {
    return invoke<void>('update_file_category', { id, category });
  },

  async previewOperations(operations: FileOperation[]): Promise<OperationPreview[]> {
    return invoke<OperationPreview[]>('preview_operations', { operations });
  },

  async executeOperations(operations: FileOperation[]): Promise<BatchResult> {
    return invoke<BatchResult>('execute_operations', { operations });
  },

  async undoBatch(batchId: string): Promise<BatchResult> {
    return invoke<BatchResult>('undo_batch', { batchId });
  },
};
