// 底部状态栏（静态占位，数据接入留 T6.2 接 Zustand store）。
//
// 显示项：推理模式标签 / 模型名 / 文件数 / 索引状态 / 版本号

export function StatusBar() {
  return (
    <footer className="status-bar" aria-label="状态栏">
      <span className="status-bar__tag status-bar__tag--local" title="当前推理模式">
        ● 本地模式
      </span>
      <span className="status-bar__sep" aria-hidden>
        |
      </span>
      <span className="status-bar__text" title="当前模型">
        Ollama · Qwen2.5:7B
      </span>
      <span className="status-bar__sep" aria-hidden>
        |
      </span>
      <span className="status-bar__text" title="文件数与索引状态">
        0 文件 · 索引就绪
      </span>
      <span className="status-bar__spacer" />
      <span className="status-bar__text status-bar__text--muted" title="版本号">
        v0.1.0
      </span>
    </footer>
  );
}
