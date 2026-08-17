# 性能预算与监控

> 企业级开发要求：每个功能有性能预算，超预算必须优化。

## 性能预算

### 前端性能指标

| 指标 | 预算 | 测量方式 | 检查时机 |
|------|------|----------|----------|
| 首屏加载 (FCP) | < 1.5s | Lighthouse | 每次 PR |
| 可交互时间 (TTI) | < 3s | Lighthouse | 每次 PR |
| JS bundle (gzip) | < 200KB | vite-bundle-analyzer | 每次 PR |
| CSS bundle (gzip) | < 30KB | vite-bundle-analyzer | 每次 PR |
| 内存占用 | < 200MB | DevTools Memory | 每周 |
| 组件渲染时间 | < 16ms (60fps) | React Profiler | Code Review |

### Rust 性能指标

| 指标 | 预算 | 测量方式 | 检查时机 |
|------|------|----------|----------|
| IPC 命令响应 | < 50ms（不含 IO） | tracing | Code Review |
| 文件扫描 10K 文件 | < 10s | bench 测试 | 每个 Epic |
| 文件扫描 100K 文件 | < 30s | bench 测试 | 发布前 |
| Sidecar 启动 | < 2s | 集成测试 | 每个 Epic |
| DB 查询（单表） | < 10ms | bench 测试 | Code Review |

### Python 性能指标

| 指标 | 预算 | 测量方式 | 检查时机 |
|------|------|----------|----------|
| API 响应（不含 LLM） | < 100ms | pytest-benchmark | Code Review |
| 分类单文件 | < 500ms | bench 测试 | 每个 Epic |
| Embedding 批量 100 文件 | < 30s | bench 测试 | 每个 Epic |
| RAG 首 token 延迟 | < 3s | 集成测试 | 发布前 |
| 内存占用 | < 500MB | psutil | 每周 |

## 前端性能规则

### 虚拟滚动（必须使用）

```tsx
// ✅ 正确：大列表使用虚拟滚动
import { useVirtualizer } from '@tanstack/react-virtual';

export function FileListTable({ files }: FileListTableProps) {
  const parentRef = useRef<HTMLDivElement>(null);

  const virtualizer = useVirtualizer({
    count: files.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 40,  // 行高
    overscan: 5,             // 预渲染 5 行
  });

  return (
    <div ref={parentRef} style={{ height: '100%', overflow: 'auto' }}>
      <div style={{ height: virtualizer.getTotalSize() }}>
        {virtualizer.getVirtualItems().map((item) => (
          <FileRow key={files[item.index].id} file={files[item.index]} />
        ))}
      </div>
    </div>
  );
}

// ❌ 错误：直接渲染大列表
export function FileListTable({ files }: FileListTableProps) {
  return (
    <div>
      {files.map((file) => <FileRow key={file.id} file={file} />)}  // 10K+ 行卡死
    </div>
  );
}
```

### 性能优化清单

| 场景 | 规则 | 检查方式 |
|------|------|----------|
| 大列表 (> 100 行) | 必须虚拟滚动 | Code Review |
| 昂贵计算 | useMemo / useCallback | Code Review |
| 图片加载 | loading="lazy" | Code Review |
| 组件卸载 | 清理事件监听/定时器 | Code Review |
| 重新渲染 | React.memo 纯展示组件 | React Profiler |
| 深层对象比较 | shallow compare | Code Review |

## Rust 性能规则

### 热路径优化

```rust
// ✅ 正确：批量 DB 操作
pub fn batch_insert_files(conn: &Connection, files: &[FileInfo]) -> AppResult<()> {
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare("INSERT INTO files (id, path, ...) VALUES (?1, ?2, ...)")?;
        for file in files {
            stmt.execute(rusqlite::params![file.id, file.path])?;
        }
    }
    tx.commit()?;
    Ok(())
}

// ❌ 错误：逐条插入（10K 文件 = 10K 次事务）
for file in files {
    conn.execute("INSERT INTO files ...", params![file.id])?;
}
```

### 内存规则

| 规则 | 说明 |
|------|------|
| 大文件哈希 | 流式读取（8KB buffer），不一次性读入内存 |
| 文件扫描结果 | 分批返回（每批 1000 条），不一次性加载 |
| Embedding 向量 | 用 f32 不用 f64，省一半内存 |

## Python 性能规则

### 异步优先

```python
# ✅ 正确：异步批量处理
async def batch_embed(texts: list[str]) -> list[list[float]]:
    """批量生成 embedding，利用并发。"""
    semaphore = asyncio.Semaphore(10)  # 限制并发 10

    async def embed_one(text: str) -> list[float]:
        async with semaphore:
            return await ollama.embeddings(model="bge-large-zh", prompt=text)

    return await asyncio.gather(*[embed_one(t) for t in texts])

# ❌ 错误：同步逐条处理
def batch_embed(texts: list[str]) -> list[list[float]]:
    results = []
    for text in texts:  # 逐条阻塞
        results.append(ollama.embeddings(model="bge-large-zh", prompt=text))
    return results
```

## CI 性能门控

```yaml
# .github/workflows/pr-check.yml 补充
  performance-check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with: { node-version: 20, cache: npm }
      - run: npm ci

      # Bundle size 检查
      - run: npm run build
      - name: Check bundle size
        run: |
          JS_SIZE=$(gzip -c dist/assets/*.js | wc -c)
          CSS_SIZE=$(gzip -c dist/assets/*.css | wc -c)
          echo "JS bundle (gzip): $JS_SIZE bytes"
          echo "CSS bundle (gzip): $CSS_SIZE bytes"
          if [ $JS_SIZE -gt 204800 ]; then
            echo "FAIL: JS bundle exceeds 200KB budget"
            exit 1
          fi
          if [ $CSS_SIZE -gt 30720 ]; then
            echo "FAIL: CSS bundle exceeds 30KB budget"
            exit 1
          fi

      # Rust bench
      - run: cargo bench --manifest-path src-tauri/Cargo.toml
