// 本地生成模型（GGUF）区块（设置页）：展示内置 llama.cpp 引擎所需权重的状态与下载入口。
//
// 背景：部署机器可能没装 Ollama。此时知识问答由内置的 llama.cpp 引擎完成，需要把 GGUF
// 权重（约 2 GB）下载到本机。**未就绪且本机无 Ollama 时，知识问答不可用**，故此处给出
// 明确的下载入口与状态；已装 Ollama 的用户可忽略本区块。
//
// 复用既有下载链路：同一个 IPC（`start_model_download` / `model_download_status`）
// 与同一套镜像轮询 / 重试 / 进度，不新增命令（也就不需要重新生成 specta 类型）。
//
// 状态来源：只读 store 的 modelDownloads 映射（Sidecar 的状态查询会读磁盘，故重启后
// 首次查询也能返回 ready），不走 Ollama 探测。

import { useEffect } from 'react';
import type { ModelDownloadStatus } from '@/types/ipc';
import { percentOf, progressText } from '../../lib/modelDownloadFormat';
import { useSettingsStore } from '../../stores/settingsStore';
import { DOWNLOAD_POLL_MS } from './EmbeddingModelSection';

/** GGUF 模型标识（与 Sidecar `app/services/model_specs.py` 一致；传给 IPC 的 model_name）。 */
export const LOCAL_LLM_MODEL_NAME = 'qwen2.5-3b-instruct';

/** 界面展示名（含量化档位，便于用户核对下载的是哪一份权重）。 */
const LOCAL_LLM_MODEL_LABEL = 'Qwen2.5-3B-Instruct · Q4_K_M';

interface LocalLlmRowProps {
  isReady: boolean;
  isDownloading: boolean;
  hasFailed: boolean;
  onDownload: () => void;
}

/** 本地生成模型行：就绪/未下载徽标 + 下载按钮（未就绪才提供）。 */
function LocalLlmRow({ isReady, isDownloading, hasFailed, onDownload }: LocalLlmRowProps) {
  const buttonLabel = isDownloading ? '下载中...' : hasFailed ? '重新下载' : '下载';
  return (
    <div className={`model-item${isReady ? ' model-item--current' : ''}`}>
      <div className="model-info">
        <div className="name">{LOCAL_LLM_MODEL_LABEL}</div>
        <div className="meta">GGUF · Q4_K_M · {isReady ? '已就绪' : '未下载'}</div>
      </div>
      <span className={`tag ${isReady ? 'tag-green' : 'tag-gray'}`} data-testid="local-llm-status">
        {isReady ? '已就绪' : '未下载'}
      </span>
      {!isReady && (
        <button
          type="button"
          className="btn btn--primary btn--sm"
          data-testid="local-llm-download-btn"
          disabled={isDownloading}
          onClick={onDownload}
          title={isDownloading ? '正在下载中' : '下载 GGUF 权重到本机'}
        >
          {buttonLabel}
        </button>
      )}
    </div>
  );
}

/** 下载中：进度条 + 文案（百分比 / 已下载量 / 镜像 / 尝试次数）。 */
function DownloadProgress({ download }: { download: ModelDownloadStatus }) {
  const percent = percentOf(download);
  return (
    <div style={{ marginTop: 12 }} data-testid="local-llm-download-progress">
      <div
        className="model-progress"
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent ?? undefined}
      >
        <div className="model-progress__bar" style={{ width: `${percent ?? 0}%` }} />
      </div>
      <div style={{ fontSize: 12, color: 'var(--muted)', marginTop: 'var(--sp-2)' }}>
        正在下载本地生成模型：{progressText(download)}
        {download.attempt > 1 ? ` · 第 ${download.attempt} 次尝试` : ''}
        {download.mirror ? ` · 镜像 ${download.mirror}` : ''}
      </div>
    </div>
  );
}

/** 自动重试用尽：展示原因，重试入口由行内按钮提供。 */
function DownloadFailure({ error }: { error: string | null }) {
  return (
    <div
      style={{ marginTop: 12, fontSize: 12, color: 'var(--danger, #d33)' }}
      role="alert"
      data-testid="local-llm-download-failure"
    >
      下载失败：{error ?? '未知原因'}（已自动重试并切换镜像，请点击重试）
    </div>
  );
}

export function LocalLlmModelSection() {
  // 下载状态与启动失败都按 model_name 分槽：归属由 key 天然保证，同页其它卡片不会互相覆盖
  const download = useSettingsStore((s) => s.modelDownloads[LOCAL_LLM_MODEL_NAME] ?? null);
  const installError = useSettingsStore((s) => s.installErrors[LOCAL_LLM_MODEL_NAME] ?? null);
  const startModelDownload = useSettingsStore((s) => s.startModelDownload);
  const refreshModelDownload = useSettingsStore((s) => s.refreshModelDownload);

  const isDownloading = download?.status === 'downloading';

  // 进页查一次状态：从设置页切走再回来时能接着显示进行中的进度
  useEffect(() => {
    void refreshModelDownload(LOCAL_LLM_MODEL_NAME);
  }, [refreshModelDownload]);

  // 下载中按固定间隔轮询；结束（ready/failed）后自动停止
  useEffect(() => {
    if (!isDownloading) return;
    const timer = window.setInterval(() => {
      void refreshModelDownload(LOCAL_LLM_MODEL_NAME);
    }, DOWNLOAD_POLL_MS);
    return () => window.clearInterval(timer);
  }, [isDownloading, refreshModelDownload]);

  return (
    <section className="settings-section" aria-labelledby="settings-local-llm-title">
      <h3 id="settings-local-llm-title" className="settings-section__title">
        🧠 本地生成模型（GGUF）
      </h3>
      <p className="section-desc">
        未安装 Ollama 时，知识问答由内置 llama.cpp 引擎完成，需要把 GGUF 权重下载到本机（约 2
        GB）；下载完成前该引擎不可用。
      </p>

      <LocalLlmRow
        isReady={download?.status === 'ready'}
        isDownloading={isDownloading}
        hasFailed={download?.status === 'failed'}
        onDownload={() => void startModelDownload(LOCAL_LLM_MODEL_NAME)}
      />

      {isDownloading && download && <DownloadProgress download={download} />}
      {download?.status === 'failed' && <DownloadFailure error={download.error} />}
      {installError && (
        <div
          style={{ marginTop: 12, fontSize: 12, color: 'var(--danger, #d33)' }}
          role="alert"
          data-testid="local-llm-install-failure"
        >
          下载请求失败：{installError}
        </div>
      )}
    </section>
  );
}
