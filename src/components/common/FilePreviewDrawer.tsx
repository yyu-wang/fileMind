// 文件预览抽屉（公共组件）：外壳（遮罩 / 标题 / 目录树 / meta）+ 内容渲染槽位。
//
// 供 文件管理 / 智能分类 / 问答 等模块复用：只依赖 path + file_name 即可预览，
// file_size / category 为可选元信息（缺省时对应 meta 行不显示）。
//
// props 受控：file 为 null 时不渲染；file 变化时重新拉取预览内容。
// 内容渲染按类型分发（文本 / HTML / 图片 / PDF / 不支持）见 FilePreviewContent。

import { useEffect, useState, type MouseEvent } from 'react';

import { formatFileSize } from '@/lib/format';
import { fileIpc } from '@/lib/ipc';
import type { FilePreview } from '@/types/ipc';
import { fileExt, PreviewContent } from './FilePreviewContent';
import { FilePathTree } from './FilePathTree';

/** 预览目标的最小子集：path/file_name 必填，size/category 可选（缺省隐藏对应 meta 行）。 */
export interface FilePreviewTarget {
  path: string;
  file_name: string;
  /** 文件大小（字节）；缺省时 meta 不显示大小行 */
  file_size?: number;
  /** 分类名：undefined 隐藏分类行，null 显示「未分类」，字符串显示分类名 */
  category?: string | null;
}

type PreviewState =
  | { phase: 'loading' }
  | { phase: 'error'; message: string }
  | { phase: 'ready'; preview: FilePreview };

interface FilePreviewDrawerProps {
  file: FilePreviewTarget | null;
  onClose: () => void;
  /** 初始页码（引用跳转定位用；txt/md 文本预览 best-effort 忽略） */
  initialPage?: number;
}

export function FilePreviewDrawer({ file, onClose, initialPage }: FilePreviewDrawerProps) {
  const [state, setState] = useState<PreviewState>({ phase: 'loading' });
  const [pageNumber, setPageNumber] = useState(() => initialPage ?? 1);
  const [numPages, setNumPages] = useState<number | null>(null);

  // FE-M6：同文件不同页码的引用点击（ChatPage key={file.id} 不重挂载）需要
  // 在渲染期间同步 pageNumber——useState 初始化只读一次，旧页码会残留。
  // React 官方「渲染期间调整 state」模式：条件 setState 立即重渲染，
  // 比 useEffect 少一帧错页闪烁。
  const [prevInitialPage, setPrevInitialPage] = useState(initialPage);
  if (initialPage !== undefined && initialPage !== prevInitialPage) {
    setPrevInitialPage(initialPage);
    setPageNumber(initialPage);
  }

  // 原型 §Drawer：ESC 关闭
  useEffect(() => {
    if (!file) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [file, onClose]);

  // 状态在 useState 初始化（loading / 第 1 页）；文件切换由父级 key 触发重挂载重置。
  useEffect(() => {
    if (!file) {
      return;
    }
    let cancelled = false;

    // IPC 是系统边界：tauri-specta 生成的 commands 是 `typedError(invoke(...))`，
    // invoke(...) 在 typedError 包装前同步执行——若 __TAURI_INTERNALS 未就绪/
    // 参数异常会同步 throw，直接炸掉 useEffect → React 卸载整棵组件树
    // （用户看到"弹窗闪没/页面白屏"）。故用 try + Promise.resolve() 双保险。
    const fetchPreview = async () => {
      try {
        // Office 文档（docx/xlsx/pptx）经 Sidecar 抽取为纯文本预览；
        // 其余类型（文本/图片/PDF/不支持）仍走原生读文件命令
        const ext = fileExt(file.path);
        const result =
          ext === 'docx' || ext === 'xlsx' || ext === 'pptx'
            ? await fileIpc.readDocumentPreview(file.path)
            : await fileIpc.readFilePreview(file.path);
        if (cancelled) {
          return;
        }
        if (result.status === 'ok') {
          setState({ phase: 'ready', preview: result.data });
        } else {
          setState({ phase: 'error', message: result.error });
        }
      } catch (err) {
        if (cancelled) return;
        setState({
          phase: 'error',
          message: err instanceof Error ? err.message : '读取预览失败',
        });
      }
    };
    void fetchPreview();
    return () => {
      cancelled = true;
    };
  }, [file]);

  if (!file) {
    return null;
  }

  // 原型 §Drawer：半透明遮罩（点击遮罩关闭）+ 右侧滑入抽屉（460px 宽、100vh 高）
  // 不参与页面 flex 布局，从根本上避免"chat-page column 布局把抽屉挤出视口外"
  // 同时恢复用户期望的『弹出预览』形态，而不是常驻右侧 panel。
  const handleOverlayClick = (e: MouseEvent<HTMLDivElement>) => {
    if (e.target === e.currentTarget) onClose();
  };

  return (
    <div
      className="files-preview__overlay"
      role="dialog"
      aria-modal="true"
      aria-label="文件预览弹窗"
      onClick={handleOverlayClick}
    >
      <div className="files-preview" role="complementary" aria-label="文件预览">
        <div className="files-preview__head">
          <span className="files-preview__title" title={file.path}>
            {file.file_name}
          </span>
          <div className="files-preview__head-actions">
            <button
              type="button"
              className="files-preview__close"
              aria-label="关闭预览"
              onClick={onClose}
            >
              ×
            </button>
          </div>
        </div>

        <div className="files-preview__body">
          {/* 预览内容区：固定 2/3 高度；loading 阶段在这一整块内显示 loading 占位，
              避免 iframe/PDF 加载大文件时整行空白→用户以为无反应 */}
          <div className="files-preview__content-slot">
            {state.phase === 'loading' && (
              <div className="files-preview__loading" role="status" aria-live="polite">
                <span className="files-preview__spinner" aria-hidden />
                <span>正在加载文件内容…</span>
              </div>
            )}
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

          {/* 文件所在位置：目录树，固定 1/3 高度；总是显示（让用户先看到位置再等内容加载完） */}
          <div className="files-preview__tree-slot">
            <FilePathTree path={file.path} fileName={file.file_name} />
          </div>
        </div>

        <div className="files-preview__meta">
          <span className="files-preview__meta-item" title={file.path}>
            {file.path}
          </span>
          {file.file_size != null && (
            <span className="files-preview__meta-item">{formatFileSize(file.file_size)}</span>
          )}
          {file.category !== undefined && (
            <span className="files-preview__meta-item">{file.category ?? '未分类'}</span>
          )}
        </div>
      </div>
    </div>
  );
}
