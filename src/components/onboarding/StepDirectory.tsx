// 步骤三：选择扫描目录（对齐交互原型 §Onboarding Step 3）。
//
// 调 @tauri-apps/plugin-dialog 的 open 选目录；
// 拖拽接收留 T6.4。选定后触发 completeOnboarding + scanFiles。

import { useEffect, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { getE2eTestDir } from '../../lib/e2e';
import type { InferenceMode } from '../../types/ipc';

interface StepDirectoryProps {
  onComplete: (path: string) => void;
  mode: InferenceMode;
}

export function StepDirectory({ onComplete, mode }: StepDirectoryProps) {
  const [directory, setDirectory] = useState<string | null>(null);

  // T9.5 E2E：存在测试目录时跳过原生对话框自动填充。
  useEffect(() => {
    let cancelled = false;
    void getE2eTestDir().then((dir) => {
      if (!cancelled && dir) {
        setDirectory(dir);
      }
    });
    return () => {
      cancelled = true;
    };
  }, []);

  /** 浏览选择目录 */
  const handleBrowse = async () => {
    const selected = await open({
      directory: true,
      multiple: false,
      title: '选择要管理的目录',
    });
    if (typeof selected === 'string') {
      setDirectory(selected);
    }
  };

  /** 开始使用 */
  const handleStart = () => {
    if (directory) {
      void onComplete(directory);
    }
  };

  const modeLabel = mode === 'Cloud' ? '云端' : '本地';

  return (
    <div>
      <h3>选择要管理的目录</h3>
      <p className="step-desc">
        选择你想要整理和建立知识库的目录。你当前选择的是{modeLabel}模式，扫描后的文件
        {mode === 'Cloud' ? '内容摘要将上传到云端处理' : '将完全在本地处理'}。
      </p>

      {/* 拖拽区 + 浏览按钮 */}
      {!directory && (
        <div
          className="dropzone"
          onClick={() => void handleBrowse()}
          role="button"
          tabIndex={0}
          onKeyDown={(e) => {
            if (e.key === 'Enter' || e.key === ' ') {
              e.preventDefault();
              void handleBrowse();
            }
          }}
        >
          <div className="icon" aria-hidden>
            📁
          </div>
          <p>拖拽文件夹到此处</p>
          <p style={{ fontSize: 12, color: 'var(--muted2)', marginTop: 4 }}>或点击浏览选择目录</p>
        </div>
      )}

      {/* 已选目录展示 */}
      {directory && (
        <div className="selected-dir">
          <span style={{ fontSize: 16 }} aria-hidden>
            📂
          </span>
          <span className="mono">{directory}</span>
          <span className="tag tag-green" style={{ marginLeft: 'auto' }}>
            已选择
          </span>
          <button
            type="button"
            className="btn btn--ghost btn--sm"
            onClick={() => void handleBrowse()}
            aria-label="重新选择目录"
          >
            重新选择
          </button>
        </div>
      )}

      <div className="step-actions step-actions--between">
        <button type="button" className="btn btn--ghost" onClick={() => void handleBrowse()}>
          ← 上一步
        </button>
        <button
          type="button"
          className="btn btn--primary"
          data-testid="onboarding-start"
          disabled={!directory}
          onClick={handleStart}
        >
          开始使用 →
        </button>
      </div>
    </div>
  );
}
