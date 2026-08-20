// 文件预览抽屉：按类型渲染 文本 / 图片 / PDF / 不支持 降级提示。
//
// props 受控：file 为 null 时不渲染；file 变化时重新拉取预览内容。
// PDF 走 react-pdf，worker 用同源 URL（CSP script-src 'self' 禁 blob:）。

import { useEffect, useState } from 'react';
import { Document, Page, pdfjs } from 'react-pdf';

import { formatFileSize } from '@/lib/format';
import { fileIpc } from '@/lib/ipc';
import type { FileInfo, FilePreview } from '@/types/ipc';

pdfjs.GlobalWorkerOptions.workerSrc = new URL(
  'pdfjs-dist/build/pdf.worker.min.mjs',
  import.meta.url,
).toString();

type PreviewState =
  | { phase: 'loading' }
  | { phase: 'error'; message: string }
  | { phase: 'ready'; preview: FilePreview };

interface FilePreviewDrawerProps {
  file: FileInfo | null;
  onClose: () => void;
  /** 初始页码（引用跳转定位用；txt/md 文本预览 best-effort 忽略） */
  initialPage?: number;
}

export function FilePreviewDrawer({ file, onClose, initialPage }: FilePreviewDrawerProps) {
  const [state, setState] = useState<PreviewState>({ phase: 'loading' });
  const [pageNumber, setPageNumber] = useState(() => initialPage ?? 1);
  const [numPages, setNumPages] = useState<number | null>(null);

  // 状态在 useState 初始化（loading / 第 1 页）；文件切换由父级 key 触发重挂载重置。
  useEffect(() => {
    if (!file) {
      return;
    }
    let cancelled = false;

    fileIpc.readFilePreview(file.path).then((result) => {
      if (cancelled) {
        return;
      }
      if (result.status === 'ok') {
        setState({ phase: 'ready', preview: result.data });
      } else {
        setState({ phase: 'error', message: result.error });
      }
    });
    return () => {
      cancelled = true;
    };
  }, [file]);

  if (!file) {
    return null;
  }

  return (
    <div className="files-preview" role="complementary" aria-label="文件预览">
      <div className="files-preview__head">
        <span className="files-preview__title" title={file.path}>
          {file.file_name}
        </span>
        <button
          type="button"
          className="files-preview__close"
          aria-label="关闭预览"
          onClick={onClose}
        >
          ×
        </button>
      </div>

      <div className="files-preview__body">
        {state.phase === 'loading' && <div className="files-preview__loading">加载中…</div>}
        {state.phase === 'error' && <div className="files-preview__error">{state.message}</div>}
        {state.phase === 'ready' && (
          <PreviewContent
            preview={state.preview}
            pageNumber={pageNumber}
            numPages={numPages}
            onDocumentLoad={setNumPages}
            onDocumentError={(message) => setState({ phase: 'error', message })}
            onPageChange={setPageNumber}
          />
        )}
      </div>

      <div className="files-preview__meta">
        <span className="files-preview__meta-item" title={file.path}>
          {file.path}
        </span>
        <span className="files-preview__meta-item">{formatFileSize(file.file_size)}</span>
        <span className="files-preview__meta-item">{file.category ?? '未分类'}</span>
      </div>
    </div>
  );
}

interface PreviewContentProps {
  preview: FilePreview;
  pageNumber: number;
  numPages: number | null;
  onDocumentLoad: (numPages: number) => void;
  onDocumentError: (message: string) => void;
  onPageChange: (page: number) => void;
}

function PreviewContent({
  preview,
  pageNumber,
  numPages,
  onDocumentLoad,
  onDocumentError,
  onPageChange,
}: PreviewContentProps) {
  switch (preview.kind) {
    case 'Text':
      return (
        <div className="files-preview__text">
          {preview.truncated && <div className="files-preview__hint">内容过长，仅预览前 512KB</div>}
          <pre>{preview.text ?? ''}</pre>
        </div>
      );
    case 'Image':
      return preview.data_url ? (
        <div className="files-preview__image">
          <img src={preview.data_url} alt={preview.file_name} />
        </div>
      ) : null;
    case 'Pdf':
      return preview.data_url ? (
        <div className="files-preview__pdf">
          <Document
            file={preview.data_url}
            onLoadSuccess={({ numPages: pages }) => onDocumentLoad(pages)}
            onLoadError={(error) => onDocumentError(`PDF 加载失败（${error.message}）`)}
          >
            <Page pageNumber={pageNumber} />
          </Document>
          <div className="files-preview__pager">
            <button
              type="button"
              className="files-preview__pagebtn"
              aria-label="上一页"
              disabled={pageNumber <= 1}
              onClick={() => onPageChange(pageNumber - 1)}
            >
              ‹
            </button>
            <span className="files-preview__pageinfo">
              {pageNumber} / {numPages ?? '…'}
            </span>
            <button
              type="button"
              className="files-preview__pagebtn"
              aria-label="下一页"
              disabled={numPages != null && pageNumber >= numPages}
              onClick={() => onPageChange(pageNumber + 1)}
            >
              ›
            </button>
          </div>
        </div>
      ) : null;
    case 'Unsupported':
      return <div className="files-preview__unsupported">暂不支持预览该文件类型</div>;
  }
}
