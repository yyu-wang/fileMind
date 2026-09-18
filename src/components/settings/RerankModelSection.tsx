// 重排（Rerank）模型区块（设置页）：展示重排模型状态并提供下载。
//
// 背景：重排由 Sidecar 进程内的 sentence-transformers cross-encoder 完成，模型文件
// 不在 Embedding 注册表内（无 dim / 表名语义），需单独下载到本机
// （`{models_root}/bge-reranker-v2-m3`）。**未就绪不会中断问答**——检索退回 RRF
// 融合序直接生成（Sidecar 下发 `RERANK_DEGRADED` 提示），但相关性明显下降，
// 故此处给出明确的下载入口与状态。
//
// 复用既有下载链路：同一个 IPC（`start_model_download` / `model_download_status`）
// 与同一套镜像轮询 / 重试 / 进度，不新增命令（也就不需要重新生成 specta 类型）。

import { useEffect } from 'react';
import type { ModelDownloadStatus } from '@/types/ipc';
import { percentOf, progressText } from '../../lib/modelDownloadFormat';
import { useSettingsStore } from '../../stores/settingsStore';
import { DOWNLOAD_POLL_MS } from './EmbeddingModelSection';

/** 重排模型名（与 Sidecar `app/services/model_specs.RERANK_MODEL_NAME` 一致）。 */
export const RERANK_MODEL_NAME = 'bge-reranker-v2-m3';

interface RerankRowProps {
  isReady: boolean;
  isDownloading: boolean;
  hasFailed: boolean;
  onDownload: () => void;
}

/** 重排模型行：就绪/未下载徽标 + 下载按钮（未就绪才提供）。 */
function RerankRow({ isReady, isDownloading, hasFailed, onDownload }: RerankRowProps) {
  const buttonLabel = isDownloading ? '下载中...' : hasFailed ? '重新下载' : '下载';
  return (
    <div className={`model-item${isReady ? ' model-item--current' : ''}`}>
      <div className="model-info">
        <div className="name">{RERANK_MODEL_NAME}</div>
        <div className="meta">cross-encoder · {isReady ? '已就绪' : '未下载'}</div>
      </div>
      <span className={`tag ${isReady ? 'tag-green' : 'tag-gray'}`} data-testid="rerank-status">
        {isReady ? '已就绪' : '未下载'}
      </span>
      {!isReady && (
        <button
          type="button"
          className="btn btn--primary btn--sm"
          data-testid="rerank-download-btn"
          disabled={isDownloading}
          onClick={onDownload}
          title={isDownloading ? '正在下载中' : '下载重排模型文件'}
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
    <div style={{ marginTop: 12 }} data-testid="rerank-download-progress">
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
        正在下载重排模型：{progressText(download)}
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
      data-testid="rerank-download-failure"
    >
      下载失败：{error ?? '未知原因'}（已自动重试并切换镜像，请点击重试）
    </div>
  );
}

export function RerankModelSection() {
  // 下载状态与启动失败都按 model_name 分槽：归属由 key 天然保证，同页其它卡片不会互相覆盖
  const download = useSettingsStore((s) => s.modelDownloads[RERANK_MODEL_NAME] ?? null);
  const installError = useSettingsStore((s) => s.installErrors[RERANK_MODEL_NAME] ?? null);
  const startModelDownload = useSettingsStore((s) => s.startModelDownload);
  const refreshModelDownload = useSettingsStore((s) => s.refreshModelDownload);

  const isDownloading = download?.status === 'downloading';

  // 进页查一次状态：从设置页切走再回来时能接着显示进行中的进度
  useEffect(() => {
    void refreshModelDownload(RERANK_MODEL_NAME);
  }, [refreshModelDownload]);

  // 下载中按固定间隔轮询；结束（ready/failed）后自动停止
  useEffect(() => {
    if (!isDownloading) return;
    const timer = window.setInterval(() => {
      void refreshModelDownload(RERANK_MODEL_NAME);
    }, DOWNLOAD_POLL_MS);
    return () => window.clearInterval(timer);
  }, [isDownloading, refreshModelDownload]);

  return (
    <section className="settings-section" aria-labelledby="settings-rerank-title">
      <h3 id="settings-rerank-title" className="settings-section__title">
        🎯 重排模型（Rerank）
      </h3>
      <p className="section-desc">
        重排用于把检索到的片段按相关性重新排序，模型文件需下载到本机（约 2.3 GB）；
        未就绪时知识问答仍可用，但结果相关性会下降。
      </p>

      <RerankRow
        isReady={download?.status === 'ready'}
        isDownloading={isDownloading}
        hasFailed={download?.status === 'failed'}
        onDownload={() => void startModelDownload(RERANK_MODEL_NAME)}
      />

      {isDownloading && download && <DownloadProgress download={download} />}
      {download?.status === 'failed' && <DownloadFailure error={download.error} />}
      {installError && (
        <div
          style={{ marginTop: 12, fontSize: 12, color: 'var(--danger, #d33)' }}
          role="alert"
          data-testid="rerank-install-failure"
        >
          下载请求失败：{installError}
        </div>
      )}
    </section>
  );
}
