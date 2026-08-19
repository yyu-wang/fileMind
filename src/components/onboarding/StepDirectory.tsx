// 步骤三：选择扫描目录（03 设计稿 §3.3）。
//
// 调 @tauri-apps/plugin-dialog 的 open 选目录；
// 拖拽接收留 T6.4。选定后触发 completeOnboarding + scanFiles。

import { useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import type { InferenceMode } from '../../types/ipc';

interface StepDirectoryProps {
  onComplete: (path: string) => void;
  mode: InferenceMode;
}

export function StepDirectory({ onComplete, mode }: StepDirectoryProps) {
  const [directory, setDirectory] = useState<string | null>(null);

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
    <div className="onboarding__step">
      <h2 className="onboarding__title">选择要管理的目录</h2>
      <p className="onboarding__subtitle">
        选择你想要整理和建立知识库的目录。你当前选择的是{modeLabel}模式，扫描后的文件
        {mode === 'Cloud' ? '内容摘要将上传到云端处理' : '将完全在本地处理'}。
      </p>

      {/* 拖拽区 + 浏览按钮 */}
      <div
        className={`onboarding__drop-zone ${directory ? 'filled' : ''}`}
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
        {directory ? (
          <div className="onboarding__selected-dir">
            <span className="onboarding__dir-icon">📁</span>
            <span className="onboarding__dir-path">{directory}</span>
          </div>
        ) : (
          <>
            <div className="onboarding__drop-icon">📁</div>
            <p className="onboarding__drop-hint">点击选择目录</p>
            <p className="onboarding__drop-sub">或拖拽文件夹到此处（T6.4 支持）</p>
          </>
        )}
      </div>

      <div className="onboarding__actions onboarding__actions--between">
        <span className="onboarding__step-indicator">步骤 3/3</span>
        <button
          type="button"
          className="btn btn--primary"
          disabled={!directory}
          onClick={handleStart}
        >
          开始使用 →
        </button>
      </div>
    </div>
  );
}
