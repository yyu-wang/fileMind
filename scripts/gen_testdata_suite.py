#!/usr/bin/env python3
"""FileMind 第二批测试数据生成脚本。

在 ``~/Desktop/filemind-test-batch2/`` 下生成多场景测试文件，
用于全面测试 FileMind 的文件扫描、分类、RAG 问答、图片预览等功能。

特性：
- 固定随机种子（默认 42），可复现
- ``--clean`` 删除测试目录
- ``--dry-run`` 仅输出计划

用法：
    python3 gen_batch2_testdata.py
    python3 gen_batch2_testdata.py --clean
    python3 gen_batch2_testdata.py --dry-run
"""

from __future__ import annotations

import argparse
import hashlib
import logging
import random
import shutil
import sys
import uuid
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

logger = logging.getLogger("gen_batch2")

TESTDATA_ROOT = Path.home() / "Desktop" / "filemind-test-suite"
SEED = 42

# ============================================================================
# RAG 文本内容库 —— 有真实可读的中文/英文内容，用于 FTS 检索和 RAG 问答测试
# ============================================================================

RAG_DOCS: list[tuple[str, str, str]] = [
    # (子目录, 文件名, 内容)
    (
        "技术文档",
        "RAG架构设计方案.md",
        """# RAG 架构设计方案

## 1. 概述

检索增强生成（Retrieval-Augmented Generation，RAG）是一种结合信息检索与大语言模型的技术架构。本方案基于 LanceDB 作为向量数据库，Ollama 作为本地推理引擎，实现完全本地化的知识问答系统。

## 2. 整体架构

### 2.1 三层架构
- **索引层**：文档解析 → 文本切分 → Embedding 向量化 → 存入 LanceDB
- **检索层**：Query 改写 → 向量检索 + 关键词检索 → RRF 融合重排
- **生成层**：上下文拼装 → Prompt 构造 → LLM 流式生成 → SSE 输出

### 2.2 关键技术选型
- 向量数据库：LanceDB（列式存储，支持增量写入）
- Embedding 模型：bge-m3（多语言，1024 维）
- 文本切分：语义切分 + 递归字符切分混合策略
- 检索策略：向量检索 + BM25 关键词检索，RRF 融合

## 3. 索引流程

1. 文件扫描：遍历指定目录，支持过滤扩展名
2. 文本提取：PDF 使用 PyMuPDF，Office 使用 python-docx / openpyxl
3. 文本清洗：去除页眉页脚、规范化空白、提取标题层级
4. 切分策略：按标题层级优先切分，Chunk 大小 512 tokens，重叠 64 tokens
5. 向量化：批量调用 Embedding 模型，支持并发控制
6. 入库：写入 LanceDB，自动创建 HNSW 索引

## 4. 检索策略

### 4.1 查询改写
- 多轮对话上下文化：将历史对话摘要融入当前查询
- 查询扩展：生成 3-5 个语义相似的子查询并行检索

### 4.2 混合检索
- 向量检索：Top-K = 20，相似度阈值 0.65
- 全文检索：FTS5 BM25，Top-K = 20
- RRF 融合：k = 60，按排名倒数求和重排

### 4.3 重排
- 使用 bge-reranker 对 Top-10 结果精排
- 过滤掉相关性低于 0.3 的片段

## 5. 生成流程

1. 上下文拼装：按相关性从高到低拼装，Token 上限 4096
2. 引用标记：每个片段标注来源文件和页码
3. Prompt 模板：系统提示 + 上下文 + 用户问题
4. 流式输出：SSE 逐 token 返回，携带引用标签
5. 自我纠正：答案生成后自动校验引用准确性

## 6. 性能指标

| 指标 | 目标值 |
|------|--------|
| 索引速度 | ≥ 100 页/分钟 |
| 检索延迟 | < 500ms (P95) |
| 首 token 延迟 | < 1s |
| 召回率 | ≥ 90% (测试集) |
| 答案准确率 | ≥ 85% (人工评估) |

## 7. 安全与隐私

- 所有数据本地存储，不上传云端
- Embedding 模型本地运行，数据不出本机
- 可选开启云端推理，需用户显式同意
- 操作日志链式哈希，防篡改审计
""",
    ),
    (
        "技术文档",
        "Tauri与Sidecar进程管理.md",
        """# Tauri 与 Sidecar 进程管理方案

## 1. 背景

FileMind 采用 Tauri 2 + Python FastAPI Sidecar 的混合架构。Rust 层负责 UI、数据库、系统交互；Python Sidecar 负责 AI 相关计算（RAG、分类、Embedding）。

## 2. 进程生命周期管理

### 2.1 启动流程
1. Tauri 应用启动时检查 Sidecar 二进制是否存在
2. 生成随机 PSK（预共享密钥），存入操作系统钥匙串
3. 以子进程方式启动 Sidecar，通过环境变量传递端口和 PSK
4. 等待 Sidecar 健康检查接口返回 200
5. 建立握手连接，验证 HMAC 签名

### 2.2 健康监控
- 每 30 秒发送一次心跳请求
- 连续 3 次无响应判定为异常
- 异常时自动重启 Sidecar，最多重试 5 次
- 重试间隔指数退避：1s → 2s → 4s → 8s → 16s

### 2.3 关闭流程
1. 发送优雅关闭信号（SIGTERM）
2. 等待 5 秒让 Sidecar 完成当前任务
3. 超时则强制终止（SIGKILL）
4. 清理临时文件和端口占用

## 3. 通信协议

### 3.1 HTTP + SSE
- 基础通信：HTTP POST，JSON 格式
- 流式响应：SSE（Server-Sent Events）
- 端口：默认 8765，冲突时自动探测可用端口

### 3.2 安全机制
- HMAC-SHA256 请求签名
- 逐请求递增序号，防止重放攻击
- PSK 仅在启动时通过环境变量传递
- 所有请求绑定 localhost，不对外暴露

## 4. 并发控制

### 4.1 请求队列
- 单 Sidecar 实例，单任务队列
- 队列长度上限 100，超出返回 503
- 支持任务取消：客户端可主动终止耗时任务

### 4.2 资源隔离
- Sidecar 进程设置内存上限（默认 2GB）
- CPU 优先级设为 below-normal，避免影响 UI 响应
- 大文件处理分块进行，每块之间释放 GIL

## 5. 打包方案

### 5.1 Sidecar 打包
- 使用 PyInstaller 将 Python 代码打包为单二进制
- macOS：universal2（arm64 + x86_64）
- Windows：x64
- Linux：x64（glibc 静态链接）

### 5.2 集成到 Tauri
- 在 tauri.conf.json 的 resources 中配置 sidecar 二进制
- 运行时从 resources 目录复制到临时目录执行
- 签名：macOS 用开发者证书公证，Windows 用 EV 证书
""",
    ),
    (
        "技术文档",
        "SQLite与FTS5全文检索实践.md",
        """# SQLite FTS5 全文检索实践指南

## 1. FTS5 简介

FTS5 是 SQLite 内置的全文检索扩展模块，支持高性能的全文索引和查询。FileMind 使用 FTS5 实现文件名和文件内容的快速检索。

## 2. 表结构设计

### 2.1 虚拟表创建
```sql
CREATE VIRTUAL TABLE file_fts USING fts5(
    file_name,
    content,
    content_rowid UNINDEXED,
    tokenize = 'unicode61 remove_diacritics 2'
);
```

### 2.2 与主表联动
使用触发器保持 files 表与 file_fts 表同步：
```sql
CREATE TRIGGER files_ai AFTER INSERT ON files BEGIN
    INSERT INTO file_fts(rowid, file_name, content)
    VALUES (new.rowid, new.file_name, new.content);
END;

CREATE TRIGGER files_ad AFTER DELETE ON files BEGIN
    INSERT INTO file_fts(file_fts, rowid, file_name, content)
    VALUES ('delete', old.rowid, old.file_name, old.content);
END;
```

## 3. 中文分词方案

### 3.1 问题
FTS5 默认的 unicode61 分词器按空格和标点分词，不适合中文等无空格语言。

### 3.2 解决方案：N-gram 分词
使用 FTS5 的 `unicode61` + 应用层 N-gram 预处理：
- 索引时：将中文文本转换为 bigram（两字一组）
- 查询时：将查询词也转换为 bigram
- 优点：不需要额外依赖，纯 SQLite 实现
- 缺点：索引体积约增大 2 倍，精确率略低

### 3.3 优化策略
- 混合模式：英文用默认分词，中文用 bigram
- 前缀匹配：短查询词使用前缀匹配提升召回
- 结果排序：使用 bm25() 函数计算相关性分数

## 4. 查询语法

### 4.1 基础查询
```sql
-- 简单关键词
SELECT * FROM file_fts WHERE file_fts MATCH '年度报告';

-- 多词 AND
SELECT * FROM file_fts WHERE file_fts MATCH '年度 AND 报告';

-- 短语匹配
SELECT * FROM file_fts WHERE file_fts MATCH '"年度报告"';

-- 指定列
SELECT * FROM file_fts WHERE file_fts MATCH 'file_name:报告';
```

### 4.2 排序
```sql
SELECT *, bm25(file_fts) AS score
FROM file_fts
WHERE file_fts MATCH '关键词'
ORDER BY score
LIMIT 20;
```

## 5. 性能优化

### 5.1 索引优化
- 使用 `content=` 外部内容模式，减少存储冗余
- 定期执行 `optimize` 合并段
- 大数据量时分批插入，最后一次性建索引

### 5.2 查询优化
- 限制结果集大小（LIMIT）
- 使用 `bm25()` 排序而非默认排序
- 避免在大表上不带 LIMIT 的 MATCH 查询

## 6. 与向量检索融合

FileMind 采用 FTS5 + 向量检索的混合检索方案：
1. 分别执行 FTS5 和向量检索，各取 Top-20
2. 使用 RRF（Reciprocal Rank Fusion）算法融合两个结果列表
3. 融合后取 Top-10 送入重排模型精排
4. 最终返回给 LLM 作为上下文
""",
    ),
    (
        "产品文档",
        "产品需求文档PRD.md",
        """# FileMind 产品需求文档（PRD）

## 1. 产品概述

### 1.1 产品定位
FileMind 是一款本地优先的桌面文件管理 + 知识问答工具。它帮助用户自动整理混乱的桌面文件，并基于 RAG 技术对本地文档进行自然语言问答。

### 1.2 核心价值
- **零学习成本**：像使用文件管理器一样自然
- **本地优先**：所有数据和 AI 处理默认在本地完成
- **智能分类**：规则引擎 + AI 启发式，自动整理文件
- **知识检索**：自然语言问答，精准定位文档内容

### 1.3 目标用户
- 知识工作者：文档多，找文件耗时间
- 开发者：代码和资料散落在各处
- 学生：学习资料需要整理和检索
- 设计师：设计素材和参考文件管理

## 2. 功能需求

### 2.1 文件管理
- 目录扫描：选择目录后扫描所有文件
- 虚拟滚动：支持 10 万+ 文件的流畅浏览
- 分类筛选：按文件类型、分类标签筛选
- 排序：按名称、大小、时间、类型排序
- 搜索：文件名全文检索，支持模糊匹配
- 预览抽屉：右侧滑出，支持文本、图片、PDF 预览

### 2.2 智能分类
- 规则引擎：支持扩展名、文件名正则、文件大小等规则
- AI 辅助分类：基于内容的智能分类建议
- 分类预览：执行前可预览分类结果，支持手动调整
- 进度显示：分类执行时显示进度条和实时状态
- 撤销操作：24 小时内可撤销分类操作

### 2.3 RAG 知识问答
- 自然语言提问：支持中英文问答
- 流式回答：逐字输出，模拟打字效果
- 引用溯源：答案中标注来源文件，点击跳转
- 多轮对话：支持上下文关联的多轮问答
- 对话历史：保存历史对话，可继续或删除

### 2.4 规则管理
- 规则 CRUD：新建、编辑、删除分类规则
- 拖拽排序：通过拖拽调整规则优先级
- 启用/禁用：单个规则可独立开关
- 规则类型：扩展名匹配、文件名匹配、文件大小、修改时间等

### 2.5 设置中心
- 推理模式：本地模式 / 云端模式切换
- Ollama 管理：探测本地 Ollama 环境，管理模型
- Embedding 模型：下载、切换、删除 Embedding 模型
- 主题设置：亮色 / 暗色 / 跟随系统
- 数据管理：清理缓存、导出数据、重置应用

## 3. 非功能需求

### 3.1 性能
- 冷启动 < 3s
- 扫描 1 万文件 < 5s
- 搜索响应 < 200ms
- 分类 1000 文件 < 10s

### 3.2 安全
- 所有数据本地存储
- 云端推理需用户显式同意
- API 密钥存储在系统钥匙串
- 操作日志防篡改

### 3.3 兼容性
- macOS 12+（arm64 / x86_64）
- Windows 10+（x64）
- Linux（x64，主流发行版）

## 4. 交互原则

### 4.1 设计语言
- 简洁现代：留白充足，层次清晰
- 微动效：过渡自然，反馈及时
- 一致性：相同模式相同交互

### 4.2 错误处理
- 友好的错误提示，不使用技术术语
- 提供重试或替代方案
- 关键操作支持撤销
""",
    ),
    (
        "产品文档",
        "用户调研报告-第1期.md",
        """# FileMind 用户调研报告（第 1 期）

## 1. 调研背景

为了验证 FileMind 的产品方向和功能优先级，我们进行了第一轮用户调研。本次调研采用半结构化访谈形式，共访谈 20 位目标用户。

## 2. 受访者画像

| 维度 | 分布 |
|------|------|
| 职业 | 产品经理 5 人、开发者 6 人、设计师 4 人、学生 3 人、其他 2 人 |
| 年龄 | 22-30 岁 12 人、31-40 岁 6 人、40+ 岁 2 人 |
| 使用系统 | macOS 14 人、Windows 5 人、Linux 1 人 |
| 桌面文件数 | <100 2 人、100-500 8 人、500-2000 7 人、2000+ 3 人 |

## 3. 核心发现

### 3.1 用户痛点

**痛点 1：桌面文件堆积，找文件困难**
- 18/20 用户表示桌面文件经常超过 50 个
- 15/20 用户表示找一个文件平均需要 1-3 分钟
- 典型场景："我知道文件存在，但就是找不到放在哪了"

**痛点 2：手动整理太麻烦，坚持不下来**
- 16/20 用户尝试过整理文件，但很快又乱了
- 主要原因：分类标准不清晰、整理耗时、新文件懒得归类

**痛点 3：文档内容记不清，搜文件名不够用**
- 12/20 用户有过"记得内容但记不住文件名"的经历
- 开发者和研究者对全文检索需求最强
- 希望能"用我自己的话描述，找到对应的文件"

### 3.2 功能优先级

用户最感兴趣的功能排序：
1. **智能分类**（17 票）—— 最核心的痛点解决
2. **全文检索/问答**（15 票）—— 找到文件里的内容
3. **文件预览**（10 票）—— 不用打开就能看内容
4. **重复文件清理**（8 票）—— 节省磁盘空间
5. **大文件查找**（5 票）—— 辅助清理功能

### 3.3 付费意愿

- 愿意付费：11/20（55%）
- 可能付费，看价格：6/20（30%）
- 不愿意付费：3/20（15%）

可接受价格区间：
- 免费使用基础功能，高级功能付费：14 人
- 一次性买断：4 人
- 订阅制：2 人

心理价位：一次性 ¥50-150，订阅 ¥10-25/月

## 4. 用户建议精选

> "分类前最好让我预览一下，确认没问题再执行，别一下子全给我挪走了。"
> —— 某产品经理

> "我最关心的是隐私，文件都在本地处理吗？会不会上传到云端？"
> —— 某开发者

> "如果能用自然语言问问题，比如'找一下上周的项目周报'，那就太方便了。"
> —— 某研究者

> "最好能记住我的分类习惯，越用越懂我。"
> —— 某设计师

## 5. 结论与建议

1. **产品方向验证通过**：智能分类 + 全文检索是真实痛点
2. **MVP 聚焦**：先做好文件管理和智能分类，RAG 作为核心差异化
3. **本地优先是卖点**：隐私是用户非常关心的点，需要重点强调
4. **分类预览是刚需**：用户对自动操作有顾虑，预览 + 确认是必须的
5. **定价策略**：采用 Freemium 模式，基础功能免费，高级功能付费
""",
    ),
    (
        "产品文档",
        "竞品分析报告.md",
        """# 桌面文件管理工具竞品分析报告

## 1. 竞品概览

| 产品 | 定位 | 价格 | 平台 | 核心功能 |
|------|------|------|------|----------|
| Hazel | 自动文件整理 | $32 买断 | macOS | 规则引擎、自动整理 |
| DropIt | 开源文件整理 | 免费 | Win/Linux | 拖拽整理、规则配置 |
| DEVONthink | 知识库管理 | $99 起 | macOS | 文件管理 + 全文检索 |
| Everything | 文件搜索工具 | 免费 | Windows | 极速文件名搜索 |
| Obsidian | 知识库笔记 | $25/年（同步） | 全平台 | Markdown 笔记 + 双向链接 |
| FileMind | 智能整理 + RAG 问答 | TBD | 全平台 | 规则 + AI 分类 + 自然语言问答 |

## 2. 详细对比

### 2.1 Hazel（macOS 标杆）

**优点**：
- 规则引擎强大，支持各种条件组合
- 与 macOS 深度集成，稳定可靠
- 支持 AppleScript 扩展

**不足**：
- 仅 macOS 平台
- 只有规则，没有 AI 智能分类
- 没有全文检索和问答功能
- 价格偏高，学习曲线较陡

**我们的差异化**：
- 跨平台（macOS + Windows + Linux）
- AI 辅助分类，规则 + 启发式混合
- RAG 知识问答，不止于整理
- 更现代的 UI 和交互

### 2.2 DEVONthink

**优点**：
- 功能全面，文件管理 + 知识库 + OCR
- 全文检索能力强
- 生态成熟，用户基数大

**不足**：
- 价格昂贵，Pro 版 $199
- 学习成本高，功能过于复杂
- 仅 macOS
- UI 老旧，交互繁琐

**我们的差异化**：
- 轻量快速，不做重型知识库
- 自然语言问答体验更好（流式 + 引用）
- 价格更亲民
- 更现代的界面设计

### 2.3 Everything

**优点**：
- 文件名搜索速度极快
- 轻量，资源占用低
- 免费

**不足**：
- 仅文件名搜索，不支持内容搜索
- 没有分类整理功能
- 仅 Windows
- UI 非常简陋

**我们的差异化**：
- 支持全文内容搜索 + RAG 问答
- 智能分类整理功能
- 跨平台
- 现代化的 UI

### 2.4 Obsidian

**优点**：
- Markdown 笔记体验极佳
- 双向链接、图谱视图
- 丰富的插件生态
- 本地优先理念一致

**不足**：
- 仅支持 Markdown 文件
- 不是文件管理器，不能管理所有类型文件
- 没有自动分类整理功能

**我们的差异化**：
- 管理所有类型的文件，不只是 Markdown
- 自动分类整理，不需要手动组织
- RAG 问答覆盖所有文件类型

## 3. 竞争策略

### 3.1 差异化核心
- **本地 AI 双引擎**：规则引擎 + AI 启发式分类，比纯规则更智能，比纯 AI 更可控
- **整理 + 问答闭环**：先帮你整理好文件，再帮你从文件里找答案，形成完整闭环
- **极致本地化**：默认完全本地运行，隐私安全，无网络依赖

### 3.2 切入策略
- 从开发者和知识工作者群体切入
- 强调本地优先和隐私安全
- 用 RAG 问答作为核心差异化卖点
- 免费版满足基础需求，付费版解锁高级功能

### 3.3 护城河
- 本地 AI 优化的技术壁垒
- 用户使用越久，分类越准确的飞轮效应
- 跨平台一致性体验
""",
    ),
    (
        "财务文档",
        "2026年Q2财务报告.md",
        """# 2026 年 Q2 财务报告

## 1. 经营摘要

2026 年第二季度，公司整体经营状况良好，营收保持稳定增长，成本控制在预期范围内。

### 关键指标
| 指标 | Q2 实际 | Q2 预算 | 达成率 | 同比增长 |
|------|---------|---------|--------|----------|
| 总营收 | ¥2,850,000 | ¥2,600,000 | 109.6% | +28.3% |
| 毛利润 | ¥1,720,000 | ¥1,560,000 | 110.3% | +31.2% |
| 毛利率 | 60.4% | 60.0% | 100.7% | +1.3pp |
| 净利润 | ¥680,000 | ¥550,000 | 123.6% | +45.7% |
| 净利率 | 23.9% | 21.2% | 112.7% | +2.9pp |

## 2. 营收分析

### 2.1 按产品线
| 产品线 | 营收 | 占比 | 同比 |
|--------|------|------|------|
| FileMind 个人版 | ¥1,520,000 | 53.3% | +35.2% |
| FileMind 团队版 | ¥850,000 | 29.8% | +22.5% |
| 企业定制服务 | ¥380,000 | 13.3% | +15.2% |
| 其他收入 | ¥100,000 | 3.5% | +5.1% |

### 2.2 按地区
| 地区 | 营收 | 占比 |
|------|------|------|
| 中国大陆 | ¥1,980,000 | 69.5% |
| 北美 | ¥480,000 | 16.8% |
| 欧洲 | ¥260,000 | 9.1% |
| 其他 | ¥130,000 | 4.6% |

## 3. 成本分析

### 3.1 成本结构
| 成本项 | 金额 | 占营收比 |
|--------|------|----------|
| 人力成本 | ¥720,000 | 25.3% |
| 服务器 & 云服务 | ¥180,000 | 6.3% |
| 营销费用 | ¥210,000 | 7.4% |
| 研发外包 | ¥150,000 | 5.3% |
| 办公 & 行政 | ¥95,000 | 3.3% |
| 税费 | ¥195,000 | 6.8% |
| 合计成本 | ¥2,170,000 | 76.1% |

### 3.2 人力成本明细
- 研发团队（12 人）：¥420,000
- 产品 & 设计（5 人）：¥165,000
- 运营 & 市场（4 人）：¥95,000
- 行政 & 财务（2 人）：¥40,000

## 4. 现金流

- 经营活动现金流：+¥750,000
- 投资活动现金流：-¥120,000
- 筹资活动现金流：¥0
- 净现金流：+¥630,000
- 期末现金余额：¥3,250,000

## 5. 用户数据

| 指标 | Q2 末 | Q1 末 | 增长 |
|------|-------|-------|------|
| 总用户数 | 28,500 | 21,300 | +33.8% |
| 月活用户（MAU） | 15,200 | 11,800 | +28.8% |
| 付费用户 | 3,850 | 2,680 | +43.7% |
| 付费转化率 | 13.5% | 12.6% | +0.9pp |
| 客户留存率（月） | 92.3% | 91.5% | +0.8pp |

## 6. Q3 展望

### 营收预期
- 目标营收：¥3,200,000 - ¥3,500,000
- 预期增长：+12% - +23% QoQ

### 重点投入
1. 产品研发：加速企业版功能开发
2. 市场推广：海外市场拓展
3. 团队扩张：招聘 5-8 名核心岗位
4. 基础设施：提升服务稳定性和性能
""",
    ),
    (
        "财务文档",
        "产品定价策略分析.md",
        """# FileMind 产品定价策略分析

## 1. 定价目标

### 1.1 战略目标
- 短期（0-6 个月）：快速获取用户，验证产品市场契合度
- 中期（6-18 个月）：实现盈利，扩大市场份额
- 长期（18 个月+）：建立品牌壁垒，探索多元化收入

### 1.2 定价原则
- 价值导向：价格反映用户获得的价值
- 简单透明：定价清晰，无隐藏费用
- 灵活可选：多档位满足不同用户需求
- 本地友好：对不同地区有差异化定价

## 2. 竞品定价参考

| 产品 | 个人版价格 | 团队版价格 | 商业模式 |
|------|-----------|-----------|----------|
| Hazel | $32 买断 | - | 买断 |
| DEVONthink | $99 买断 | $49.95/用户/月 | 买断 + 订阅 |
| CleanMyMac | $34.95/年 | $89.95/年 | 订阅 |
| Notion | 免费 / $8/月 | $15/用户/月 | Freemium + 订阅 |
| Obsidian | 免费 / $25/年（同步） | - | 免费 + 增值服务 |

## 3. 定价方案

### 方案 A：Freemium + 订阅（推荐）

**免费版**：
- 文件管理基础功能
- 最多 500 个文件索引
- 基础规则（5 条）
- 本地推理（需自备 Ollama）
- 社区支持

**Pro 版 - ¥19/月 或 ¥168/年**：
- 无限文件索引
- 无限规则数量
- 高级 AI 分类（启发式算法）
- RAG 知识问答（本地模型）
- 优先技术支持
- 未来新功能优先体验

**团队版 - ¥29/用户/月**：
- Pro 版所有功能
- 团队共享知识库
- 管理后台
- 统一账单
- 专属客户成功

### 方案 B：一次性买断

**基础版 - ¥99**：
- 文件管理 + 规则引擎
- 终身免费更新
- 基础技术支持

**专业版 - ¥199**：
- 基础版全部功能
- AI 智能分类
- RAG 知识问答（本地）
- 2 年免费大版本更新
- 优先技术支持

### 方案 C：混合模式

- 基础功能：免费
- 高级功能：买断 ¥128
- 云端 AI 服务：按需付费或订阅 ¥15/月

## 4. 用户调研价格敏感度

根据 20 位用户访谈结果：

| 价格区间 | 接受度 | 说明 |
|----------|--------|------|
| 免费 | 100% | 都愿意尝试 |
| ¥50 以下买断 | 85% | 觉得很划算 |
| ¥50-100 买断 | 60% | 可以接受 |
| ¥100-200 买断 | 30% | 需要考虑一下 |
| ¥200+ 买断 | 10% | 觉得太贵 |
| ¥10-15/月订阅 | 45% | 愿意订阅 |
| ¥20+/月订阅 | 20% | 觉得不划算 |

## 5. 推荐方案

**推荐采用方案 A（Freemium + 订阅）**，理由：
1. 低门槛获取用户，免费版作为获客渠道
2. 订阅制带来稳定的 recurring revenue
3. 与产品持续迭代的节奏匹配
4. 便于后续扩展更多付费功能

**首年定价策略**：
- 上线首月：早鸟价 ¥99/年（5 折）
- 前 1000 名付费用户：终身 ¥99/年
- 学生优惠：凭教育邮箱 5 折

## 6. 营收预测（保守估计）

| 时间 | 总用户 | 付费率 | 付费用户 | ARPU（年） | 年营收 |
|------|--------|--------|----------|------------|--------|
| 第 1 个月 | 2,000 | 2% | 40 | ¥99 | ¥3,960 |
| 第 3 个月 | 8,000 | 4% | 320 | ¥120 | ¥38,400 |
| 第 6 个月 | 20,000 | 6% | 1,200 | ¥140 | ¥168,000 |
| 第 12 个月 | 50,000 | 8% | 4,000 | ¥160 | ¥640,000 |
""",
    ),
    (
        "学习笔记",
        "React性能优化技巧.md",
        """# React 性能优化技巧总结

## 1. 渲染优化

### 1.1 避免不必要的重渲染

**使用 React.memo**
```tsx
const ExpensiveComponent = React.memo(({ data }) => {
  // 只有 props 变化时才重新渲染
  return <div>{data.map(item => <Item key={item.id} data={item} />)}</div>;
});
```

**使用 useMemo 缓存计算结果**
```tsx
const sortedData = useMemo(() => {
  return [...data].sort((a, b) => a.value - b.value);
}, [data]);
```

**使用 useCallback 缓存函数**
```tsx
const handleClick = useCallback((id) => {
  setSelected(id);
}, []);
```

### 1.2 正确使用 key

- ❌ 不要用 index 作为 key（列表会重排时）
- ✅ 用唯一 ID 作为 key
- ✅ key 在兄弟节点中唯一即可，不需要全局唯一

### 1.3 拆分组件

将大组件拆分为小组件，让状态尽量下沉：
- 只把需要共享的状态提升到最近公共祖先
- 局部状态放在组件内部，减少重渲染范围

## 2. 列表优化

### 2.1 虚拟滚动

当列表项超过 100 条时，使用虚拟滚动：

```tsx
import { FixedSizeList } from 'react-window';

const Row = ({ index, style }) => (
  <div style={style}>
    {items[index].name}
  </div>
);

const List = () => (
  <FixedSizeList
    height={500}
    itemCount={10000}
    itemSize={50}
    width="100%"
  >
    {Row}
  </FixedSizeList>
);
```

### 2.2 分页 / 无限滚动

- 首屏只加载第一页数据
- 滚动到底部时加载更多
- 配合 `IntersectionObserver` 实现

## 3. 状态管理优化

### 3.1 Zustand 使用技巧

**拆分 store**：按领域拆分多个 store，避免单一 store 过大

**选择器订阅**：只订阅需要的状态
```tsx
// 不要这样：每次 state 变化都重渲染
const { user, settings } = useStore();

// 而是这样：只订阅需要的字段
const user = useStore(state => state.user);
const settings = useStore(state => state.settings);
```

**使用 shallow 比较**：
```tsx
import { shallow } from 'zustand/shallow';

const { name, age } = useUserStore(
  state => ({ name: state.name, age: state.age }),
  shallow
);
```

### 3.2 避免状态冗余

- 能派生的状态不要单独存
- 使用 selector 从 state 派生出需要的值

## 4. 异步优化

### 4.1 React 19 useOptimistic

乐观更新，提升交互响应速度：
```tsx
const [optimisticState, addOptimistic] = useOptimistic(
  state,
  (currentState, newItem) => [...currentState, newItem]
);
```

### 4.2 Suspense + 懒加载

```tsx
const HeavyComponent = lazy(() => import('./HeavyComponent'));

function App() {
  return (
    <Suspense fallback={<Loading />}>
      <HeavyComponent />
    </Suspense>
  );
}
```

### 4.3 数据预取

- 路由切换时预取下一页数据
- hover 时预取可能点击的内容

## 5. 性能分析工具

### 5.1 React DevTools Profiler
- 查看组件渲染耗时
- 识别不必要的重渲染
- 分析提交（commit）阶段耗时

### 5.2 Lighthouse
- 整体性能评分
- 首屏加载时间
- 交互就绪时间

### 5.3 Chrome DevTools Performance
- 录制运行时性能
- 分析长任务
- 查看帧率

## 6. 常见性能陷阱

1. **在 render 中创建新对象/数组/函数** → 用 useMemo/useCallback
2. **Context 滥用** → 大的 context 导致很多组件重渲染 → 拆分 context
3. **列表无 key 或 key 用 index** → 用唯一标识
4. **每次 render 都计算昂贵的值** → 用 useMemo 缓存
5. **组件太大，状态太多** → 拆分为小组件
""",
    ),
    (
        "学习笔记",
        "Rust基础入门笔记.md",
        """# Rust 基础入门笔记

## 1. 为什么学 Rust

### 1.1 Rust 的优势
- **内存安全**：编译时保证，无需 GC
- **并发安全**：所有权系统从根本上避免数据竞争
- **性能优秀**：与 C/C++ 同级别，零成本抽象
- **跨平台**：支持主流操作系统和架构
- **工具链完善**：Cargo、rustfmt、clippy 等

### 1.2 适用场景
- 系统编程（操作系统、驱动）
- WebAssembly
- 命令行工具
- 嵌入式开发
- 前端基础设施（Tauri、SWC 等）

## 2. 核心概念

### 2.1 所有权（Ownership）

Rust 最核心的特性：
1. 每个值有且只有一个所有者
2. 所有者离开作用域时，值被丢弃
3. 值可以被移动（move）或借用（borrow）

```rust
let s1 = String::from("hello");
let s2 = s1; // s1 的所有权移动给 s2
// println!("{}", s1); // 编译错误：s1 已失效
println!("{}", s2); // 正常
```

### 2.2 借用与引用

**不可变引用** `&T`：
```rust
fn calculate_length(s: &String) -> usize {
    s.len()
} // s 离开作用域，但因为没有所有权，什么都不做

let s1 = String::from("hello");
let len = calculate_length(&s1);
```

**可变引用** `&mut T`：
```rust
fn change(s: &mut String) {
    s.push_str(", world");
}

let mut s = String::from("hello");
change(&mut s);
```

借用规则：
- 同一时刻，要么有一个可变引用，要么有多个不可变引用
- 引用必须总是有效的（悬垂引用编译不通过）

### 2.3 生命周期

生命周期确保引用有效：
```rust
fn longest<'a>(x: &'a str, y: &'a str) -> &'a str {
    if x.len() > y.len() { x } else { y }
}
```

大多数时候生命周期可以被编译器自动推导（生命周期省略规则）。

## 3. 常用类型

### 3.1 Option<T>
表示可能存在也可能不存在的值：
```rust
enum Option<T> {
    Some(T),
    None,
}

fn find_item(items: &[Item], id: u32) -> Option<&Item> {
    items.iter().find(|item| item.id == id)
}
```

### 3.2 Result<T, E>
表示可能成功也可能失败的操作：
```rust
enum Result<T, E> {
    Ok(T),
    Err(E),
}

fn read_file(path: &str) -> Result<String, io::Error> {
    fs::read_to_string(path)
}
```

### 3.3 错误传播

使用 `?` 运算符传播错误：
```rust
fn read_and_parse() -> Result<Data, Box<dyn Error>> {
    let content = fs::read_to_string("file.txt")?;
    let data: Data = serde_json::from_str(&content)?;
    Ok(data)
}
```

## 4. 结构体与枚举

### 4.1 结构体
```rust
struct User {
    username: String,
    email: String,
    active: bool,
}

impl User {
    fn new(username: String, email: String) -> Self {
        Self { username, email, active: true }
    }

    fn deactivate(&mut self) {
        self.active = false;
    }
}
```

### 4.2 枚举
```rust
enum Message {
    Quit,
    Move { x: i32, y: i32 },
    Write(String),
    ChangeColor(i32, i32, i32),
}

impl Message {
    fn call(&self) {
        match self {
            Message::Quit => println!("退出"),
            Message::Move { x, y } => println!("移动到 {}, {}", x, y),
            Message::Write(text) => println!("写入 {}", text),
            Message::ChangeColor(r, g, b) => println!("颜色 {},{},{}", r, g, b),
        }
    }
}
```

## 5. Trait

Trait 类似于其他语言的接口：
```rust
trait Summary {
    fn summarize(&self) -> String;

    fn summarize_author(&self) -> String {
        String::from("(作者不详)")
    }
}

struct Article { title: String, content: String }

impl Summary for Article {
    fn summarize(&self) -> String {
        format!("文章：{}", self.title)
    }
}
```

## 6. 常用集合

### 6.1 Vec<T>
```rust
let mut v: Vec<i32> = Vec::new();
v.push(1);
v.push(2);
v.push(3);
```

### 6.2 HashMap<K, V>
```rust
use std::collections::HashMap;

let mut map = HashMap::new();
map.insert("key".to_string(), 42);
```

### 6.3 String
```rust
let mut s = String::from("hello");
s.push_str(", world");
println!("{}", s); // hello, world
```

## 7. 学习建议

1. 先理解所有权系统，这是 Rust 的核心
2. 多写多练，Rust 的学习曲线确实比较陡
3. 善用编译器提示，Rust 的错误信息非常友好
4. 使用 clippy 获得更多改进建议
5. 阅读标准库文档和源码
""",
    ),
    (
        "学习笔记",
        "提示词工程入门指南.md",
        """# 提示词工程入门指南

## 1. 什么是提示词工程

提示词工程（Prompt Engineering）是设计和优化输入给大语言模型（LLM）的文本提示，以获得高质量、准确、符合预期的输出的技术。

### 1.1 为什么重要
- 同一个模型，好的提示词和差的提示词输出质量差距巨大
- 是使用 AI 最直接、最高效的方式
- 不需要修改模型参数，只需要优化输入

### 1.2 核心原则
- **清晰明确**：告诉模型你想要什么，不要让它猜
- **提供上下文**：给出足够的背景信息
- **给出示例**：Few-shot 比 Zero-shot 效果好
- **指定格式**：明确输出格式要求
- **迭代优化**：根据输出不断调整提示词

## 2. 基础技巧

### 2.1 指令清晰明确

❌ 不好的例子：
```
写一篇关于 AI 的文章
```

✅ 好的例子：
```
请写一篇 800 字左右的科普文章，主题是"人工智能在医疗领域的应用"。
要求：
1. 面向普通读者，语言通俗易懂
2. 包含 3 个具体的应用案例
3. 结构：引言 → 应用场景 → 挑战与展望 → 结语
4. 避免过于专业的术语
```

### 2.2 角色扮演

让模型扮演特定角色：
```
你是一位有 10 年经验的资深产品经理，擅长 B 端 SaaS 产品设计。
现在请你帮我评审下面这个产品需求文档，从以下几个维度给出反馈：
1. 需求是否清晰明确
2. 用户场景是否真实
3. 功能优先级是否合理
4. 有哪些遗漏的考虑点

需求文档内容：
...
```

### 2.3 少样本学习（Few-shot）

给出几个例子，模型会模仿例子的模式：
```
请将以下句子转换为更正式的商务用语。

例子：
输入：这个方案不行，得改
输出：该方案存在改进空间，建议进一步优化

输入：客户说太贵了
输出：客户反馈价格方面超出其预算预期

现在请转换：
输入：这个功能我们做不了
输出：
```

### 2.4 思维链（Chain of Thought）

让模型一步步思考，而不是直接给答案：
```
请仔细思考下面这个问题，先列出你的推理过程，再给出最终答案。

问题：一个水池有两个进水管和一个出水管。单开甲管 6 小时注满，单开乙管 4 小时注满，单开出水管 8 小时放完。三管同时打开，几小时注满？

请按步骤计算：
1. 甲管每小时注入多少
2. 乙管每小时注入多少
3. 出水管每小时放出多少
4. 三管同时开每小时净注入多少
5. 注满需要多少小时
```

## 3. 进阶技巧

### 3.1 结构化输出

要求模型按指定格式输出，便于解析：
```
请分析以下用户反馈，按 JSON 格式输出分析结果：
{
  "sentiment": "positive/negative/neutral",
  "category": "功能问题/体验问题/建议/其他",
  "keywords": ["关键词1", "关键词2"],
  "summary": "一句话总结"
}

用户反馈：
"分类功能很好用，但是有时候分类不太准，希望能支持自定义分类规则。"
```

### 3.2 自我检查（Self-Correction）

让模型自己检查和修正输出：
```
请生成一个 Python 函数，计算斐波那契数列的第 n 项。
生成后，请你自己检查代码是否正确，考虑边界情况（n=0, n=1, 负数等），如有问题则修正。
```

### 3.3 多轮优化

第一轮先生成，第二轮让它优化：
```
【第一轮】
请写一份产品发布会的演讲稿，主题是 FileMind 1.0 发布。

【第二轮】
请根据以下反馈修改演讲稿：
1. 开场不够吸引人，增加一个用户痛点场景
2. 技术部分太详细，听众是非技术人士
3. 结尾没有行动号召，加上下载引导
```

## 4. RAG 中的提示词设计

### 4.1 系统提示模板
```
你是一个知识助手，基于提供的参考资料回答用户的问题。

规则：
1. 只使用参考资料中的信息回答问题
2. 如果参考资料中没有答案，明确说"根据现有资料无法回答这个问题"
3. 回答中引用的内容请标注来源，格式：[来源文件名]
4. 保持客观准确，不要编造信息
5. 语言简洁明了，重点突出

参考资料：
{context}

用户问题：{question}
```

### 4.2 查询改写
```
请将用户的问题改写成 3 个不同的搜索查询，用于从知识库中检索相关文档。
要求：
1. 3 个查询语义相似但表达方式不同
2. 提取问题中的关键概念
3. 考虑可能的同义词和相关术语

用户问题：怎么提高 RAG 系统的准确率？

改写后的查询：
1.
2.
3.
```

## 5. 常见错误

1. **提示词太模糊** —— 给出明确的要求和约束
2. **上下文不足** —— 提供足够的背景信息
3. **一次要求太多** —— 复杂任务拆分成多步
4. **不做迭代** —— 根据输出不断优化提示词
5. **不验证结果** —— 对重要内容进行事实核查

## 6. 推荐学习资源

- OpenAI Prompt Engineering Guide
- Anthropic Prompt Engineering
- 各种 Prompt 模板库
- 多实践，多尝试
""",
    ),
    (
        "会议纪要",
        "产品周会纪要-2026-W36.md",
        """# 产品周会纪要 - 2026 年第 36 周

**日期**：2026-09-02 14:00-15:30
**地点**：线上会议
**参会人**：产品组、设计组、研发组
**主持人**：李明
**记录人**：王芳

## 一、上周进展

### 1.1 产品
- 完成 FileMind 1.0 需求文档终稿
- 用户调研报告整理完毕
- 与设计组完成首页交互评审

### 1.2 设计
- 完成首页、文件列表页高保真设计
- 完成分类流程交互原型
- 启动设置页设计

### 1.3 研发
- Rust 后端：完成文件扫描和基础 CRUD
- 前端：完成项目脚手架和基础路由
- Sidecar：完成 FastAPI 基础框架和握手协议

## 二、本周计划

### 2.1 产品（负责人：李明）
- [ ] 输出规则引擎产品需求文档
- [ ] 输出 RAG 问答功能需求文档
- [ ] 准备内测用户招募文案

### 2.2 设计（负责人：张设计）
- [ ] 完成设置页、规则管理页设计
- [ ] 完成空状态、错误状态设计
- [ ] 输出设计规范文档 v1.0

### 2.3 前端（负责人：陈开发）
- [ ] 实现文件列表虚拟滚动
- [ ] 实现分类筛选和排序功能
- [ ] 实现右侧预览抽屉

### 2.4 Rust 后端（负责人：刘开发）
- [ ] 实现分类规则引擎
- [ ] 实现操作日志（链式哈希）
- [ ] 实现 FTS5 全文检索

### 2.5 Sidecar（负责人：赵开发）
- [ ] 实现文档解析和文本提取
- [ ] 实现 Embedding 接口封装
- [ ] 实现 LanceDB 向量存储

## 三、重点讨论

### 3.1 MVP 范围确认

**议题**：1.0 版本是否包含 RAG 问答功能？

**讨论**：
- 产品：RAG 是核心差异化，应该包含
- 研发：工作量较大，可能影响上线时间
- 设计：UI 不复杂，主要是后端工作量
- 结论：**MVP 包含基础版 RAG**，高级功能后续迭代

**决议**：
- MVP RAG 范围：单轮问答、引用溯源、本地模型
- 多轮对话、对话历史、云端模型放在 1.1
- 上线时间目标不变：10 月底

### 3.2 分类撤销机制

**议题**：分类操作是否需要撤销功能？

**讨论**：
- 产品：用户调研显示非常需要，担心自动分类出错
- 研发：实现有一定复杂度，需要记录操作日志
- 方案：24 小时内可撤销，采用软删除 + 日志链
- 结论：**做，24 小时撤销窗口**

**决议**：
- 分类操作记录到 operations_log 表
- 每个操作有唯一 batch_id
- 24 小时内可按 batch 撤销
- 超过 24 小时自动清理日志（保留哈希链摘要）

### 3.3 内测时间

**议题**：什么时候开始内测？

**讨论**：
- 目标：9 月底启动内测，10 月底正式发布
- 内测人数：50 人左右
- 内测渠道：用户调研受访者优先

**决议**：
- 9 月 25 日：内测版本准备好
- 9 月 26 日：发出内测邀请
- 内测周期：2 周
- 10 月 15 日：功能冻结，只修 bug

## 四、风险与阻塞

| 风险 | 影响 | 负责人 | 应对方案 |
|------|------|--------|----------|
| Ollama 在 Windows 上兼容性问题 | 影响本地推理体验 | 赵开发 | 提前测试，准备降级方案 |
| 虚拟滚动 + 筛选排序组合复杂 | 可能有性能问题 | 陈开发 | 提前做性能原型验证 |
| 设计资源紧张 | 设置页可能延期 | 张设计 | 优先级排序，非核心页面延后 |

## 五、行动项

- @李明：本周内输出 RAG 需求文档
- @刘开发：评估分类撤销实现工作量
- @赵开发：调研 Ollama Windows 兼容性
- @张设计：输出设计规范文档
- @王芳：准备内测招募文案

---
**下次会议**：2026-09-09 14:00
""",
    ),
    (
        "会议纪要",
        "技术方案评审会-分类引擎.md",
        """# 技术方案评审会 - 分类引擎

**日期**：2026-09-04 10:00-12:00
**地点**：线上会议
**参会人**：刘开发、陈开发、赵开发、李明
**主持人**：刘开发
**记录人**：陈开发

## 一、方案概述

本次评审的分类引擎技术方案，包含规则引擎 + AI 启发式分类两部分。

## 二、规则引擎方案

### 2.1 规则类型

支持以下规则类型：
1. **扩展名匹配**：文件扩展名在指定列表中
2. **文件名匹配**：文件名匹配正则表达式
3. **文件大小**：大于/小于/等于指定大小
4. **修改时间**：在指定时间范围内
5. **文件内容关键词**：文本文件包含指定关键词

### 2.2 匹配优先级

- 规则按 priority 字段排序，数字越小优先级越高
- 第一个匹配的规则生效（先匹配先赢）
- 支持规则启用/禁用开关

### 2.3 规则执行性能

**问题**：1000 个文件 + 50 条规则，执行速度？

**方案**：
- 预编译正则表达式，缓存复用
- 扩展名匹配用 HashSet 快速判断
- 内容关键词匹配只对文本类文件执行
- 预估：1000 文件 < 2 秒（不含内容匹配）

**决议**：方案可行，先实现基础 4 种规则类型，内容关键词匹配后续迭代。

## 三、AI 启发式分类

### 3.1 方案描述

对于规则没有匹配到的文件，使用 AI 启发式算法推断分类：
1. 基于文件名语义分析
2. 基于文件元数据（大小、时间、扩展名组合）
3. 基于用户历史分类行为学习

### 3.2 实现路径

**Phase 1（MVP）**：
- 纯规则引擎
- 内置一套默认规则
- 用户可自定义规则

**Phase 2（1.1）**：
- 基于文件名的启发式分类
- 相似度匹配已有文件的分类
- 分类置信度展示

**Phase 3（1.2）**：
- 基于内容的智能分类
- 用户行为学习
- 自动推荐规则

**决议**：MVP 只做规则引擎，启发式分类延后到 1.1，避免过度设计。

## 四、分类执行流程

### 4.1 执行步骤
1. 扫描目标目录，获取文件列表
2. 按优先级依次匹配规则
3. 生成分类预览（文件 → 目标分类）
4. 用户确认后执行分类
5. 记录操作日志，支持撤销

### 4.2 冲突处理

**问题**：目标目录已有同名文件怎么办？

**方案**：
- 策略 1：重命名（文件名后加序号）
- 策略 2：跳过
- 策略 3：覆盖（不推荐）
- 默认策略：重命名，用户可在设置中修改

**决议**：默认重命名，设置中可切换策略。

### 4.3 撤销机制

**实现方案**：
- 每个分类操作生成唯一 batch_id
- operations_log 表记录每条移动操作（源路径 → 目标路径）
- 撤销时按 batch_id 反向移动
- 24 小时后自动标记为不可撤销（保留日志记录）

**链式哈希**：
- 每条日志记录包含 prev_hash 和 current_hash
- current_hash = SHA256(prev_hash + 操作内容)
- 保证操作日志不可篡改
- 用于审计追踪

**决议**：方案通过，按此实现。

## 五、性能指标

| 指标 | 目标 |
|------|------|
| 1000 文件分类预览 | < 2s |
| 1000 文件执行分类 | < 5s |
| 单批次撤销 | < 3s |
| 规则匹配准确率 | 规则命中的 100% 准确 |

## 六、后续行动

- @刘开发：实现规则引擎核心逻辑
- @赵开发：配合提供文件元数据接口
- @陈开发：前端分类预览和执行 UI
- @李明：输出规则配置 UI 交互说明

---
**下次评审**：待定，等核心逻辑完成后
""",
    ),
    (
        "项目管理",
        "项目排期表-Phase2.md",
        """# FileMind 项目排期表 - Phase 2

## 项目信息

- **项目名称**：FileMind 1.0
- **版本**：Phase 2（核心功能开发）
- **开始日期**：2026-09-01
- **目标发布**：2026-10-31
- **项目经理**：李明

## 里程碑

| 里程碑 | 日期 | 交付物 |
|--------|------|--------|
| M1：核心功能完成 | 2026-09-30 | 文件管理 + 分类引擎 + 基础 RAG |
| M2：内测版本 | 2026-09-25 | 可运行的内测版本 |
| M3：功能冻结 | 2026-10-15 | 功能全部完成，只修 bug |
| M4：RC 版本 | 2026-10-25 | 发布候选版 |
| M5：正式发布 | 2026-10-31 | 1.0 正式版 |

## 任务拆解

### T1：文件管理模块（9.1 - 9.10）—— 负责人：刘开发

| 子任务 | 工时 | 状态 |
|--------|------|------|
| T1.1 目录扫描功能 | 2d | ✅ 完成 |
| T1.2 文件元数据 CRUD | 2d | ✅ 完成 |
| T1.3 FTS5 全文检索 | 2d | 进行中 |
| T1.4 分类筛选与排序 | 1d | 待开始 |
| T1.5 虚拟滚动列表（前端） | 3d | 进行中 |

### T2：分类引擎模块（9.8 - 9.20）—— 负责人：刘开发

| 子任务 | 工时 | 状态 |
|--------|------|------|
| T2.1 规则引擎核心 | 3d | 待开始 |
| T2.2 内置规则定义 | 1d | 待开始 |
| T2.3 分类预览 | 2d | 待开始 |
| T2.4 分类执行与进度 | 2d | 待开始 |
| T2.5 操作日志与撤销 | 3d | 待开始 |
| T2.6 前端规则管理 UI | 3d | 待开始 |

### T3：RAG 问答模块（9.15 - 10.10）—— 负责人：赵开发

| 子任务 | 工时 | 状态 |
|--------|------|------|
| T3.1 文档解析与文本提取 | 3d | 待开始 |
| T3.2 文本切分与向量化 | 3d | 待开始 |
| T3.3 LanceDB 向量存储 | 2d | 待开始 |
| T3.4 混合检索与重排 | 3d | 待开始 |
| T3.5 LLM 流式生成 | 2d | 待开始 |
| T3.6 前端问答界面 | 4d | 待开始 |

### T4：设置与系统（9.20 - 10.5）—— 负责人：刘开发

| 子任务 | 工时 | 状态 |
|--------|------|------|
| T4.1 设置页面 UI | 2d | 待开始 |
| T4.2 推理模式切换 | 2d | 待开始 |
| T4.3 Ollama 环境探测 | 2d | 待开始 |
| T4.4 主题切换 | 1d | 待开始 |
| T4.5 系统托盘与快捷键 | 2d | 待开始 |

### T5：测试与优化（10.1 - 10.25）—— 负责人：全员

| 子任务 | 工时 | 状态 |
|--------|------|------|
| T5.1 单元测试 | 5d | 待开始 |
| T5.2 集成测试 | 3d | 待开始 |
| T5.3 性能优化 | 5d | 待开始 |
| T5.4 Bug 修复 | 5d | 待开始 |
| T5.5 内测反馈处理 | 5d | 待开始 |

### T6：打包与发布（10.20 - 10.31）—— 负责人：陈开发

| 子任务 | 工时 | 状态 |
|--------|------|------|
| T6.1 macOS 打包与签名 | 2d | 待开始 |
| T6.2 Windows 打包与签名 | 2d | 待开始 |
| T6.3 Linux 打包 | 1d | 待开始 |
| T6.4 自动更新机制 | 3d | 待开始 |
| T6.5 发布文档与准备 | 2d | 待开始 |

## 资源分配

| 角色 | 人数 | 投入 |
|------|------|------|
| Rust 后端开发 | 1.5 | 刘开发 + 兼职 |
| 前端开发 | 1 | 陈开发 |
| Python Sidecar | 1 | 赵开发 |
| 产品经理 | 0.5 | 李明 |
| UI 设计师 | 0.5 | 张设计 |

## 关键风险

| 风险 | 概率 | 影响 | 应对措施 |
|------|------|------|----------|
| RAG 模块技术难度超预期 | 中 | 高 | MVP 砍功能，先做基础版 |
| 跨平台兼容性问题 | 中 | 中 | 提前测试，优先保证 macOS |
| 人员不足导致延期 | 高 | 中 | 优先级排序，非核心功能延后 |
| 性能不达预期 | 低 | 中 | 预留优化时间，做性能基准测试 |

## 每周节奏

- 周一上午：周会，同步进展和计划
- 周三下午：技术方案评审（按需）
- 周五下午：代码评审 + Demo
- 每日：站会（15 分钟，同步阻塞项）
""",
    ),
    (
        "英文资料",
        "Vector Database Comparison.md",
        """# Vector Database Comparison: Pinecone vs Weaviate vs Milvus vs LanceDB

## Overview

Vector databases are specialized databases designed to store and query high-dimensional vectors efficiently. They are a critical component of RAG (Retrieval-Augmented Generation) systems. This document compares four popular vector database options.

## Comparison Table

| Feature | Pinecone | Weaviate | Milvus | LanceDB |
|---------|----------|----------|--------|---------|
| Type | Managed Service | Open Source + Cloud | Open Source | Open Source (embedded) |
| License | Commercial | BSD-3 | Apache 2.0 | Apache 2.0 |
| Deployment | SaaS only | Self-hosted + Cloud | Self-hosted + Cloud | Embedded (in-process) |
| Index Types | HNSW | HNSW, IVF, Flat | HNSW, IVF, IVF-PQ, DiskANN | IVF-PQ, HNSW (beta) |
| Scalability | Fully managed | Horizontally scalable | Horizontally scalable | Single process, file-based |
| Filtering | Yes (metadata) | Yes (built-in) | Yes (scalar filtering) | Yes (SQL-like via Lance) |
| Hybrid Search | Yes (sparse-dense) | Yes (BM25 + vector) | Yes (BM25 + vector) | Yes (via Lance FTS) |
| Multi-tenancy | Yes | Yes | Yes | File-level isolation |
| Real-time updates | Yes | Yes | Yes | Yes (append-only) |

## Detailed Analysis

### 1. Pinecone

**Pros:**
- Fully managed, zero infrastructure overhead
- Excellent performance at scale
- Great developer experience
- Sparse-dense hybrid search
- Enterprise-grade reliability

**Cons:**
- Expensive at scale
- No self-hosted option
- Less control over data
- Vendor lock-in
- Limited filtering capabilities

**Best for:** Teams that want to move fast, don't want to manage infrastructure, and have budget for managed services.

### 2. Weaviate

**Pros:**
- Open source with commercial support
- Built-in BM25 for hybrid search
- Rich ecosystem and modules
- GraphQL API
- Good documentation

**Cons:**
- Resource-heavy (Java-based)
- Complex to operate at scale
- Higher memory requirements
- Steeper learning curve

**Best for:** Teams that need a full-featured vector database with hybrid search and are comfortable operating infrastructure.

### 3. Milvus

**Pros:**
- High performance at large scale
- Multiple index types supported
- Open source (LF AI & Data)
- Good Kubernetes integration
- Active community

**Cons:**
- Complex architecture (many components)
- Heavy resource requirements
- Steep learning curve
- Overkill for small datasets

**Best for:** Enterprise teams with large datasets (100M+ vectors) and dedicated infrastructure teams.

### 4. LanceDB

**Pros:**
- Embedded, zero setup
- Very lightweight
- Based on Apache Lance (columnar format)
- Good for desktop and edge applications
- SQL-like querying via Lance
- Versioning support

**Cons:**
- Single-process, not for multi-user server scenarios
- HNSW index still in beta
- Smaller community
- Less mature than alternatives

**Best for:** Desktop applications, edge devices, datasets under 10M vectors, and scenarios where zero infrastructure is desired.

## Why FileMind Chose LanceDB

FileMind is a desktop application, which means:
1. **No server infrastructure** - everything runs locally on the user's machine
2. **Lightweight footprint** - can't require heavy dependencies
3. **Dataset size** - typical user has thousands to hundreds of thousands of documents
4. **Zero setup** - user shouldn't need to install or configure a database

LanceDB is the perfect fit because:
- It's an embedded database that runs in-process
- Very lightweight (Python package, ~10MB)
- Based on Lance columnar format, efficient storage
- Supports both vector search and full-text search
- Versioning enables safe incremental updates
- Active development with good roadmap

## Performance Benchmarks (Reference)

Based on ann-benchmarks.com results for glove-100-angular dataset:

| Database | Recall@10 | QPS | Memory |
|----------|-----------|-----|--------|
| Pinecone (S1 pod) | ~0.95 | ~5000 | Managed |
| Milvus (HNSW) | ~0.95 | ~8000 | ~2GB |
| Weaviate (HNSW) | ~0.93 | ~3000 | ~4GB |
| LanceDB (IVF-PQ) | ~0.90 | ~5000 | ~500MB |

Note: Benchmarks are approximate and vary by configuration and dataset.

## Conclusion

There is no one-size-fits-all vector database. The choice depends on your scale, deployment model, team size, and budget. For FileMind's use case (desktop application with local-first philosophy), LanceDB is the clear choice. For server-side applications with high scale requirements, Milvus or Pinecone would be better options.
""",
    ),
    (
        "英文资料",
        "Local-First Software Principles.md",
        """# Local-First Software Principles

## What is Local-First Software?

Local-first software is an approach to application design where data is stored locally on the user's device by default, with cloud sync being optional. This is the opposite of cloud-first software, where data lives on a server and the client is just a view into that data.

## Core Principles

### 1. No Spinner at the Speed of Light

**Principle**: The application should respond instantly to user actions, regardless of network conditions.

**Why it matters**:
- Users hate waiting
- Network latency is unpredictable
- Offline usage should be a first-class experience

**Implementation**:
- All data reads come from local storage
- Writes go to local storage first, then sync in background
- UI never waits for network calls

### 2. Your Data Is Yours

**Principle**: Users own their data and should have full control over it.

**Why it matters**:
- Privacy is a fundamental right
- Users shouldn't be locked into a platform
- Data portability prevents vendor lock-in

**Implementation**:
- Data stored in open formats
- Export functionality built-in
- No proprietary data silos
- User can delete all their data easily

### 3. The Network is Optional

**Principle**: The application works fully offline. Cloud features are enhancements, not requirements.

**Why it matters**:
- Internet access isn't always available
- Some users prefer not to use cloud services
- Security: fewer data transfers mean fewer attack surfaces

**Implementation**:
- All core features work without internet
- Sync happens opportunistically
- Graceful degradation of cloud features

### 4. Fast by Default

**Principle**: Performance is a feature. Local storage should be fast enough that users never think about it.

**Why it matters**:
- Speed is a quality attribute
- Slow software is frustrating
- Performance affects perceived quality

**Implementation**:
- Optimize for local read/write speed
- Use appropriate storage engines (SQLite, etc.)
- Minimize serialization overhead
- Virtual scrolling for large datasets

### 5. Collaboration is Optional but Possible

**Principle**: Local-first doesn't mean solo. Real-time collaboration should be possible, but not required.

**Why it matters**:
- Many workflows are collaborative
- But not everyone needs collaboration
- Don't force multi-user complexity on single users

**Implementation**:
- CRDTs for conflict-free merging
- Optional sync servers
- Peer-to-peer sync capabilities

## Local-First vs Cloud-First

| Aspect | Local-First | Cloud-First |
|--------|-------------|-------------|
| Data location | User's device | Remote server |
| Offline support | Full | Limited or none |
| Performance | Instant (local) | Depends on network |
| Privacy | User controls data | Provider controls data |
| Data ownership | User owns data | Provider owns data |
| Collaboration | Optional, add-on | Core feature |
| Cost model | One-time / Freemium | Subscription |
| Infrastructure | Minimal (user devices) | Massive server farms |

## Why FileMind is Local-First

FileMind embraces local-first philosophy because:

### Privacy
- Files contain sensitive personal and work information
- Users shouldn't have to trust a third party with their data
- AI processing on-device means data never leaves the computer

### Performance
- File operations are instant when done locally
- No waiting for uploads or downloads
- Search and retrieval at local disk speeds

### Reliability
- Works even when internet is down
- No server outages affecting users
- No dependency on third-party service availability

### Control
- Users own their files and their data
- No lock-in - uninstalling removes everything
- No subscription required for core features

## Challenges of Local-First

### 1. Sync Complexity
- Keeping multiple devices in sync is hard
- Conflict resolution is tricky
- CRDTs help but add complexity

### 2. Multi-device Experience
- Users expect their data everywhere
- Need seamless sync across devices
- Mobile + desktop consistency

### 3. Data Safety
- Local data can be lost if device fails
- Need backup strategies
- Encryption for sensitive data

### 4. Discovery
- Cloud apps benefit from network effects
- Local-first apps need different distribution strategies

## The Future of Local-First

Local-first software is gaining momentum because:
1. Users are more concerned about privacy
2. Edge computing is becoming more powerful
3. AI models can run locally (Ollama, llama.cpp)
4. CRDTs make collaboration feasible
5. Users are tired of subscription fatigue

We believe local-first is the future of software, and FileMind is proud to be part of this movement.
""",
    ),
    (
        "个人笔记",
        "读书笔记-深入理解计算机系统.md",
        """# 《深入理解计算机系统》读书笔记

## 第 1 章：计算机系统漫游

### 1.1 信息就是位 + 上下文

- 所有信息（文件、程序、数字）都是由比特位组成的
- 相同的比特序列在不同上下文中含义不同
- 程序的生命周期：源程序（文本）→ 编译 → 可执行文件（二进制）

### 1.2 程序被其他程序翻译成不同的格式

GCC 编译过程的四个阶段：
1. **预处理阶段**：处理 `#include`、`#define` 等，生成 `.i` 文件
2. **编译阶段**：编译成汇编代码，生成 `.s` 文件
3. **汇编阶段**：汇编成机器指令，生成 `.o` 目标文件
4. **链接阶段**：链接库函数，生成可执行文件

### 1.3 处理器读取并解释存储在内存中的指令

- 总线：贯穿系统的电子管道，传送字节
- I/O 桥：连接系统总线和内存总线
- 主存：临时存储，DRAM 组成
- 处理器：CPU，执行指令

### 1.4 高速缓存至关重要

- 寄存器文件 → L1 高速缓存 → L2 → L3 → 主存 → 磁盘
- 存储器层次结构：上一层是下一层的缓存
- 利用局部性原理：时间局部性 + 空间局部性

## 第 2 章：信息的表示和处理

### 2.1 信息存储

- 字节（byte）= 8 位（bit）
- 字长（word size）：指针数据的标称大小，决定虚拟地址空间大小
- 32 位字长：4GB 地址空间
- 64 位字长：16EB 地址空间

### 2.2 整数表示

- 无符号数编码：B2U(X) = Σ xi * 2^i
- 补码（Two's Complement）：最高位是负权
- 有符号数和无符号数转换：位模式不变，解释方式变
- 扩展：零扩展（无符号）、符号扩展（有符号）
- 截断：直接丢弃高位

### 2.3 整数运算

- 无符号加法：模运算，可能溢出
- 补码加法：正溢出 → 负，负溢出 → 正
- 乘法：左移 n 位等于乘以 2^n
- 除法：右移（算术右移补符号位，逻辑右移补 0）

### 2.4 浮点数

- IEEE 754 标准：符号位 + 阶码 + 尾数
- 单精度（float）：32 位（1+8+23）
- 双精度（double）：64 位（1+11+52）
- 规格化值、非规格化值、特殊值（无穷、NaN）
- 浮点运算不满足结合律（精度问题）

## 第 3 章：程序的机器级表示

### 3.1 程序编码

- ISA（指令集体系结构）：定义了处理器状态、指令格式、指令对状态的影响
- x86-64 有 16 个 64 位通用寄存器
- 程序计数器（PC/rip）：下一条指令的地址
- 条件码寄存器：存放最近的算术或逻辑指令的状态信息

### 3.2 数据格式

- byte（1 字节）、word（2 字节）、double word（4 字节）、quad word（8 字节）
- 浮点数：单精度（4 字节）、双精度（8 字节）

### 3.3 操作数指示符

- 立即数（immediate）：常数值
- 寄存器（register）：寄存器中的值
- 内存引用：根据有效地址访问内存

### 3.4 数据传送指令

- MOV 类指令：将数据从源位置复制到目的位置
- movb、movw、movl、movq（不同大小）
- 栈：向低地址方向增长，push/pop 操作

## 第 6 章：存储器层次结构

### 6.1 存储技术

- SRAM：静态随机存储器，快、贵、用作高速缓存
- DRAM：动态随机存储器，较慢、便宜、用作主存
- 磁盘：机械结构，非常慢，但容量大、便宜
- SSD：固态硬盘，比磁盘快，比 DRAM 慢

### 6.2 局部性

**时间局部性**：最近访问过的信息很快会再次访问
**空间局部性**：最近访问过的信息附近的信息很快会被访问

### 6.3 存储器层次结构

- 每层存储设备都是下一层的缓存
- 缓存命中：需要的数据在该层
- 缓存不命中：需要的数据不在，从下一层加载
- 缓存替换策略：LRU、FIFO、随机等

## 学习心得

1. 理解底层有助于写出更高效的代码
2. 计算机系统是分层抽象的，每一层都依赖下一层
3. 性能优化要基于对硬件的理解
4. 很多"魔法"背后都是简单的物理原理
5. 持续学习底层知识，打好基础
""",
    ),
    (
        "个人笔记",
        "读书清单-2026.md",
        """# 2026 年读书清单

## 已读（上半年）

### 技术类
1. **《深入理解计算机系统》** - Randal E. Bryant ⭐⭐⭐⭐⭐
   - 读了第三遍，每次都有新收获
   - 推荐所有程序员至少读一遍
   - 重点章节：存储器层次结构、虚拟内存

2. **《代码整洁之道》** - Robert C. Martin ⭐⭐⭐⭐
   - 经典的代码质量指南
   - 虽然有些观点过时了，但核心理念不过时
   - 命名和函数两章最实用

3. **《设计数据密集型应用》** - Martin Kleppmann ⭐⭐⭐⭐⭐
   - 分布式系统圣经
   - 干货密度极高，需要慢慢读
   - 做了很多笔记，需要反复翻阅

4. **《Rust 程序设计》** - Steve Klabnik ⭐⭐⭐⭐
   - Rust 官方书，入门必读
   - 所有权系统讲得很清楚
   - 需要配合练习才能掌握

### 非技术类
5. **《原则》** - Ray Dalio ⭐⭐⭐⭐
   - 很多实用的工作和生活原则
   - 极度求真和极度透明很有启发
   - 有些内容过于理想化

6. **《思考，快与慢》** - Daniel Kahneman ⭐⭐⭐⭐⭐
   - 行为经济学经典
   - 系统 1 和系统 2 的框架很有用
   - 了解认知偏差有助于更好地决策

7. **《人类简史》** - 尤瓦尔·赫拉利 ⭐⭐⭐⭐
   - 宏大的视角看人类历史
   - 虚构的故事是人类协作的基础
   - 有些观点有争议，但值得一读

## 在读

1. **《数据库系统概念》** - Silberschatz 等
   - 进度：第 12 章（事务管理）
   - 补一下数据库理论基础
   - 计划 9 月底读完

2. **《深度工作》** - Cal Newport
   - 进度：第 3 章
   - 关于专注力的书
   - 碎片化时代很需要

## 待读（下半年计划）

### 技术类（优先级从高到低）
1. 《编译原理》（龙书）- 一直想读的经典
2. 《计算机网络：自顶向下方法》- 补网络基础
3. 《操作系统导论》- 重温 OS
4. 《分布式系统：概念与设计》- 深入分布式
5. 《重构》- 代码重构技巧
6. 《领域驱动设计》- DDD 入门
7. 《设计模式》- GoF 经典

### 非技术类
1. 《影响力》- 心理学经典
2. 《枪炮、病菌与钢铁》- 人类社会发展
3. 《创新者的窘境》- 商业经典
4. 《置身事内》- 中国政府与经济
5. 《被讨厌的勇气》- 阿德勒心理学

## 读书方法总结

### 我的方法
1. **技术书**：先通读一遍，第二遍做笔记，重要章节读第三遍
2. **非技术书**：通读 + 划线 + 写读后感
3. **电子书**：用 Kindle 或微信读书，方便做笔记和搜索
4. **纸质书**：适合需要深度思考的书

### 今年的改进
- 减少了买书的数量，更注重读完
- 开始写读书笔记，加深理解
- 每季度复盘读书进度
- 技术书和非技术书穿插着读，避免疲劳

### 目标
- 全年目标：24 本（每月 2 本）
- 上半年完成：7 本
- 下半年计划：17 本（有点紧张，争取完成 12 本）
- 调整后全年目标：20 本
""",
    ),
    (
        "个人笔记",
        "年度总结与计划-2026.md",
        """# 2026 年度总结与下半年计划

## 上半年总结

### 工作方面

**主要成就**：
1. 主导 FileMind 产品从 0 到 1 的设计和开发
2. 组建了 5 人的核心团队
3. 完成 MVP 核心功能开发
4. 完成第一轮用户调研，验证了产品方向
5. 建立了开发流程和代码规范

**不足与反思**：
1. 进度管理不够好，有几次延期
2. 需求变更控制不够严格
3. 技术选型有些地方过于理想化
4. 团队沟通效率有待提升

**技能提升**：
1. 深入学习了 Rust 和 Tauri
2. 对 RAG 技术栈有了实践经验
3. 产品设计能力有提升
4. 团队管理经验增加了

### 个人方面

**健康**：
- 坚持每周运动 3 次（跑步 + 健身）
- 体重保持稳定（减了 3kg）
- 睡眠质量有所改善（早睡了）

**学习**：
- 读完了 7 本书（技术 4 本，非技术 3 本）
- 学习了 Rust 语言
- 完成了 2 门在线课程
- 写了 12 篇技术博客

**生活**：
- 旅行 2 次（云南、日本）
- 陪伴家人的时间增加了
- 学会了做饭（简单的家常菜）

## 下半年计划

### 工作目标

**产品目标**：
- [ ] FileMind 1.0 正式发布（10 月底）
- [ ] 首月获取 10,000 用户
- [ ] 付费转化率达到 5%
- [ ] 收集 100 份有效用户反馈
- [ ] 规划 1.1 版本功能

**团队建设**：
- [ ] 招聘 2-3 名核心成员
- [ ] 建立更完善的开发流程
- [ ] 提升团队技术分享氛围
- [ ] 组织 1-2 次团建活动

**个人成长**：
- [ ] 深入学习产品设计
- [ ] 提升技术架构能力
- [ ] 学习更多商业知识
- [ ] 建立个人技术品牌

### 学习目标

**技术**：
1. 读完《编译原理》和《操作系统导论》
2. 深入学习分布式系统
3. 学习 WebAssembly
4. 写 24 篇技术博客（每月 2 篇）

**非技术**：
1. 读完 10 本非技术书
2. 学习经济学基础
3. 学习心理学（行为经济学方向）
4. 练习英语写作

### 生活目标

**健康**：
- 每周运动 3-4 次
- 体脂率降到 18%
- 养成早睡早起的习惯（11 点睡，7 点起）
- 每年体检 1 次

**旅行**：
- 国庆：新疆
- 年底：泰国或越南
- 周末短途游 4-5 次

**财务**：
- 储蓄率达到 40%
- 学习投资理财
- 记录每月支出

## 三年展望（2026-2028）

### 职业发展
- 2026：FileMind 1.0 发布，验证 PMF
- 2027：FileMind 商业化，团队扩张到 15 人
- 2028：成为细分领域头部产品，探索新方向

### 个人成长
- 成为优秀的产品技术复合型人才
- 建立个人影响力
- 财务更自由

### 生活
- 保持健康的身体和心态
- 多陪伴家人
- 多去看看世界

## 写在最后

上半年过得很充实，做了很多有意义的事情。下半年希望能继续保持这种状态，把 FileMind 做好，同时也不忘记生活。

记住最重要的几件事：
1. 健康是一切的基础
2. 持续学习，保持好奇心
3. 和优秀的人一起做事
4. 享受过程，不要只盯着结果
""",
    ),
]

# ============================================================================
# 图片测试数据 —— 生成带文字标注的纯色图片，可预览
# ============================================================================

IMAGE_SPECS = [
    # (子目录, 文件名, 尺寸, 背景色, 文字)
    ("产品截图", "首页-文件列表.png", (800, 500), (59, 130, 246), "FileMind\n文件列表页"),
    ("产品截图", "首页-分类视图.png", (800, 500), (16, 185, 129), "FileMind\n分类视图"),
    ("产品截图", "智能分类-预览.png", (800, 500), (245, 158, 11), "智能分类\n预览确认"),
    ("产品截图", "知识问答页面.png", (800, 500), (139, 92, 246), "RAG 知识问答"),
    ("产品截图", "规则管理页面.png", (800, 500), (6, 182, 212), "规则管理"),
    ("产品截图", "设置页面.png", (800, 500), (107, 114, 128), "设置中心"),
    ("设计素材", "品牌Logo-深色.png", (400, 400), (30, 30, 46), "FileMind"),
    ("设计素材", "品牌Logo-浅色.png", (400, 400), (250, 250, 252), "FileMind"),
    ("设计素材", "应用图标-512.png", (512, 512), (59, 130, 246), "FM"),
    ("设计素材", "应用图标-256.png", (256, 256), (16, 185, 129), "FM"),
    ("设计素材", "banner-主视觉.jpg", (1200, 400), (30, 41, 59), "FileMind\n你的桌面知识助手"),
    ("照片", "头像-个人照.png", (300, 300), (236, 72, 153), "头像"),
    ("照片", "团队合影.jpg", (600, 400), (107, 114, 128), "团队合影\n2026"),
    ("照片", "产品发布会.jpg", (800, 600), (59, 130, 246), "产品发布会"),
    ("照片", "办公室日常.jpg", (600, 400), (16, 185, 129), "办公室"),
]


def create_image(
    save_path: Path,
    size: tuple[int, int],
    bg_color: tuple[int, int, int],
    text: str,
) -> None:
    """生成一张带文字的纯色图片。

    Args:
        save_path: 保存路径
        size: 图片尺寸 (宽, 高)
        bg_color: 背景色 (R, G, B)
        text: 要显示的文字
    """
    img = Image.new("RGB", size, bg_color)
    draw = ImageDraw.Draw(img)

    # 尝试使用系统字体，失败则用默认字体
    font_size = min(size) // 8
    font = None
    font_paths = [
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    ]
    for fp in font_paths:
        if Path(fp).exists():
            try:
                font = ImageFont.truetype(fp, font_size)
                break
            except Exception:
                continue
    if font is None:
        font = ImageFont.load_default()

    # 计算文字位置（居中）
    lines = text.split("\n")
    total_height = len(lines) * font_size * 1.2
    y_start = (size[1] - total_height) / 2

    # 文字颜色：根据背景亮度选择黑或白
    brightness = (bg_color[0] * 299 + bg_color[1] * 587 + bg_color[2] * 114) / 1000
    text_color = (255, 255, 255) if brightness < 128 else (30, 30, 30)

    for i, line in enumerate(lines):
        bbox = draw.textbbox((0, 0), line, font=font)
        text_width = bbox[2] - bbox[0]
        x = (size[0] - text_width) / 2
        y = y_start + i * font_size * 1.2
        draw.text((x, y), line, fill=text_color, font=font)

    # 添加边框
    border_color = tuple(max(0, c - 30) for c in bg_color)
    draw.rectangle([(2, 2), (size[0] - 3, size[1] - 3)], outline=border_color, width=3)

    save_path.parent.mkdir(parents=True, exist_ok=True)
    img.save(save_path)


# ============================================================================
# 深层嵌套目录结构 —— 模拟真实工作目录
# ============================================================================

NESTED_DIR_STRUCTURE = {
    "项目-Alpha": {
        "01_需求文档": [
            "PRD-v1.0.docx",
            "PRD-v1.1.docx",
            "用户调研报告.pdf",
            "竞品分析.md",
        ],
        "02_设计稿": {
            "UI设计": [
                "首页设计稿.fig",
                "详情页设计稿.fig",
                "组件库.sketch",
            ],
            "交互原型": [
                "主流程原型.png",
                "引导流程原型.png",
            ],
        },
        "03_开发": {
            "前端": [
                "README.md",
                "package.json",
                "vite.config.ts",
            ],
            "后端": [
                "Cargo.toml",
                "src/main.rs",
                "db/schema.sql",
            ],
            "AI模型": [
                "model_config.yaml",
                "training_log.txt",
                "evaluation_report.md",
            ],
        },
        "04_测试": [
            "测试用例.xlsx",
            "Bug清单.csv",
            "性能测试报告.pdf",
        ],
        "05_运营": [
            "上线计划.md",
            "推广方案.pptx",
            "用户反馈汇总.xlsx",
        ],
    },
    "项目-Beta": {
        "文档": [
            "项目计划书.docx",
            "技术方案.md",
            "会议纪要-01.md",
            "会议纪要-02.md",
        ],
        "代码": [
            "main.py",
            "utils.py",
            "config.yaml",
            "requirements.txt",
        ],
        "数据": [
            "raw_data.csv",
            "processed_data.csv",
            "analysis_result.xlsx",
        ],
    },
    "个人文档": {
        "学习笔记": [
            "Rust学习笔记.md",
            "React进阶笔记.md",
            "产品设计思考.txt",
        ],
        "财务": [
            "2026年预算.xlsx",
            "月度支出.csv",
            "投资记录.xlsx",
        ],
        "健康": [
            "体检报告.pdf",
            "运动记录.csv",
            "饮食日记.txt",
        ],
    },
}


def create_nested_files(
    root: Path, structure: dict, rng: random.Random
) -> list[Path]:
    """递归创建嵌套目录和文件。

    Args:
        root: 根目录
        structure: 目录结构字典
        rng: 随机数生成器

    Returns:
        创建的文件路径列表
    """
    created: list[Path] = []
    for name, content in structure.items():
        dir_path = root / name
        dir_path.mkdir(parents=True, exist_ok=True)
        if isinstance(content, dict):
            created.extend(create_nested_files(dir_path, content, rng))
        elif isinstance(content, list):
            for fname in content:
                fpath = dir_path / fname
                # 文件名可能包含子路径（如 src/main.rs），需确保父目录存在
                fpath.parent.mkdir(parents=True, exist_ok=True)
                if not fpath.exists():
                    # 生成随机大小的内容（256B - 8KB）
                    size = rng.randint(256, 8 * 1024)
                    fpath.write_bytes(rng.randbytes(size))
                created.append(fpath)
    return created


# ============================================================================
# 边界情况文件
# ============================================================================

EDGE_CASES: list[tuple[str, bytes]] = [
    # (文件名, 内容) —— 内容为空 bytes 表示空文件
    # 空文件
    ("空文件_0字节.txt", b""),
    ("empty_file.dat", b""),
    ("空白文档.docx", b""),
    # 特殊字符文件名
    ("文件_带空格 测试.txt", b"This file has spaces in name."),
    ("文件-with-hyphens.md", b"hyphens in filename"),
    ("文件_with_underscores.py", b"underscores in filename"),
    ("文件.with.dots.txt", b"dots in filename stem"),
    ("【重要】紧急报告@2026.pdf", b"special chars: brackets and at sign"),
    ("文件(括号版)[方括号]{大括号}.txt", b"various brackets"),
    # 长文件名（> 100 字符）
    (
        "这是一个非常长的文件名用于测试系统对长文件名的处理能力包括显示排序搜索等各个环节是否正常工作_2026版_最终版_真的是最后一版了.txt",
        b"long filename test content",
    ),
    (
        "very-long-filename-for-testing-the-edge-case-of-file-name-length-limit-in-various-filesystems-and-applications.txt",
        b"very long filename test",
    ),
    # Unicode 混合文件名
    ("日本語_ファイル.txt", b"Japanese filename test"),
    ("한국어_파일.txt", b"Korean filename test"),
    ("файл_на_русском.txt", b"Russian filename test"),
    ("混合文件名_中文_English_日本語_한국어.txt", b"mixed unicode filename"),
    # 极端大小文件
    ("极小文件_10字节.txt", b"1234567890"),
    ("极小文件_1字节.dat", b"x"),
    # 重复内容文件（用于哈希去重测试）
    ("重复文件_A_副本1.txt", b"This is duplicate content for dedup testing. abc123"),
    ("重复文件_A_副本2.txt", b"This is duplicate content for dedup testing. abc123"),
    ("重复文件_A_副本3.txt", b"This is duplicate content for dedup testing. abc123"),
    ("重复文件_B_相同内容.md", b"# Hello\n\nThis is markdown with same content.\n\nTesting dedup."),
    ("重复文件_B_一模一样.md", b"# Hello\n\nThis is markdown with same content.\n\nTesting dedup."),
    # 没有扩展名的文件
    ("Makefile", b"all: build\n\nbuild:\n\techo building\n"),
    ("README", b"This is a readme file without extension."),
    ("LICENSE", b"MIT License\n\nCopyright (c) 2026\n"),
    # 大小写扩展名
    ("大写扩展名.PDF", b"uppercase extension test"),
    ("大写扩展名.TXT", b"uppercase extension test"),
    ("混合大小写.Png", b"mixed case extension"),
    # 隐藏文件（点开头）
    (".hidden_config", b"hidden config file content"),
    (".gitignore", b"*.log\nnode_modules/\n.env\n"),
    (".env.example", b"API_KEY=your_key_here\nDEBUG=false\n"),
]


# ============================================================================
# 分类规则测试文件 —— 各种命名模式，用于测试规则引擎
# ============================================================================

RULE_TEST_FILES: list[tuple[str, str, bytes]] = [
    # (子目录, 文件名, 内容)
    # 日期前缀模式：YYYY-MM-DD_xxx
    ("日期前缀", "2026-01-15_周报.md", b"weekly report content"),
    ("日期前缀", "2026-02-20_会议纪要.docx", b"meeting minutes"),
    ("日期前缀", "2026-03-10_项目计划.pdf", b"project plan"),
    ("日期前缀", "2026-06-01_儿童节活动方案.pptx", b"event plan"),
    ("日期前缀", "2026-09-01_开学通知.txt", b"notice content"),
    # 日期后缀模式：xxx_YYYYMMDD
    ("日期后缀", "财务报表_20260131.xlsx", b"financial report data"),
    ("日期后缀", "销售数据_20260228.csv", b"sales data"),
    ("日期后缀", "库存盘点_20260331.xlsx", b"inventory data"),
    # 版本号模式：v1, v2, v1.0, v2.3.1
    ("版本号", "产品设计稿_v1.sketch", b"v1 design"),
    ("版本号", "产品设计稿_v2.sketch", b"v2 design"),
    ("版本号", "产品设计稿_v3_final.sketch", b"v3 final design"),
    ("版本号", "API文档_v1.0.md", b"v1.0 docs"),
    ("版本号", "API文档_v1.2.md", b"v1.2 docs"),
    ("版本号", "API文档_v2.0.1.md", b"v2.0.1 docs"),
    # 项目代号模式：PROJ-xxx
    ("项目代号", "PROJ-A-需求文档.docx", b"PROJ-A req"),
    ("项目代号", "PROJ-A-设计稿.fig", b"PROJ-A design"),
    ("项目代号", "PROJ-B-技术方案.md", b"PROJ-B tech"),
    ("项目代号", "PROJ-C-测试用例.xlsx", b"PROJ-C test"),
    # 状态标签：[草稿] [待审] [已发布]
    ("状态标签", "[草稿]产品规划.docx", b"draft"),
    ("状态标签", "[待审]产品规划.docx", b"pending review"),
    ("状态标签", "[已发布]产品规划.docx", b"published"),
    ("状态标签", "[已归档]旧方案.docx", b"archived"),
    # 人名前缀
    ("人名前缀", "张三_报销单.xlsx", b"expense report"),
    ("人名前缀", "李四_请假申请.docx", b"leave request"),
    ("人名前缀", "王五_工作总结.md", b"work summary"),
    ("人名前缀", "赵六_项目进度.pptx", b"progress"),
    # 各种扩展名（测试扩展名规则）
    ("扩展名测试", "document1.pdf", b"pdf file"),
    ("扩展名测试", "document2.docx", b"docx file"),
    ("扩展名测试", "spreadsheet.xlsx", b"xlsx file"),
    ("扩展名测试", "presentation.pptx", b"pptx file"),
    ("扩展名测试", "image1.jpg", b"jpg file"),
    ("扩展名测试", "image2.png", b"png file"),
    ("扩展名测试", "video.mp4", b"mp4 file"),
    ("扩展名测试", "audio.mp3", b"mp3 file"),
    ("扩展名测试", "code.py", b"python file"),
    ("扩展名测试", "archive.zip", b"zip file"),
]


# ============================================================================
# 混乱桌面模拟 —— 混合类型文件平铺在根目录
# ============================================================================

MESSY_DESKTOP_FILES = [
    # 混合各种类型，模拟用户真实的混乱桌面
    "未命名 1.png",
    "屏幕快照 2026-09-01 上午10.30.25.png",
    "屏幕快照 2026-09-02 下午3.15.08.png",
    "微信图片_20260903_xxxxxx.jpg",
    "IMG_1234.HEIC",
    "IMG_1235.JPG",
    "新建文本文档.txt",
    "新建文件夹.zip",
    "新建 Microsoft Word 文档.docx",
    "新建 Microsoft Excel 工作表.xlsx",
    "新建 Microsoft PowerPoint 演示文稿.pptx",
    "下载 (1).pdf",
    "下载.pdf",
    "Untitled.ipynb",
    "untitled.py",
    "main copy.js",
    "package-lock.json",
    ".DS_Store",
    "Thumbs.db",
    "desktop.ini",
    "~$临时文件.docx",
    "~$报告.docx",
    "期末复习资料-副本.pdf",
    "期末复习资料-副本2.pdf",
    "期末复习资料(1).pdf",
    "合同-最终版.docx",
    "合同-最终版2.docx",
    "合同-真的最终版.docx",
    "合同-绝对是最终版.docx",
    "合同-最终版_改.docx",
    "发票.pdf",
    "发票 (1).pdf",
    "发票 (2).pdf",
    "收据.jpg",
    "转账截图.png",
    "二维码.png",
    "证件照_蓝底.jpg",
    "证件照_红底.jpg",
    "身份证正反面.pdf",
    "简历-最新版.docx",
    "简历-互联网版.pdf",
    "简历-国企版.docx",
    "个人介绍.pptx",
    "自我介绍.md",
    "待办.txt",
    "随手记.txt",
    "密码.txt",  # 测试用，非真实密码
    "读书笔记.md",
    "购物清单.xlsx",
    "旅行计划.docx",
    "健身计划.pdf",
    "书单.txt",
    "电影清单.csv",
    "通讯录.xlsx",
]


# ============================================================================
# 主流程
# ============================================================================


def generate_all(seed: int = SEED) -> dict[str, int]:
    """生成所有测试数据。

    Args:
        seed: 随机种子

    Returns:
        各类别文件数量统计
    """
    rng = random.Random(seed)
    stats: dict[str, int] = {}

    # 1. RAG 文本测试集
    rag_dir = TESTDATA_ROOT / "01_RAG文本测试集"
    count = 0
    for sub_dir, filename, content in RAG_DOCS:
        fpath = rag_dir / sub_dir / filename
        fpath.parent.mkdir(parents=True, exist_ok=True)
        if not fpath.exists():
            fpath.write_text(content, encoding="utf-8")
        count += 1
    stats["RAG文本"] = count
    logger.info(f"[1/6] RAG 文本测试集：{count} 个文件")

    # 2. 图片预览测试
    img_dir = TESTDATA_ROOT / "02_图片预览测试"
    count = 0
    for sub_dir, filename, size, bg_color, text in IMAGE_SPECS:
        fpath = img_dir / sub_dir / filename
        if not fpath.exists():
            create_image(fpath, size, bg_color, text)
        count += 1
    stats["图片预览"] = count
    logger.info(f"[2/6] 图片预览测试：{count} 个文件")

    # 3. 深层嵌套目录
    nested_dir = TESTDATA_ROOT / "03_深层嵌套目录"
    files = create_nested_files(nested_dir, NESTED_DIR_STRUCTURE, rng)
    stats["深层嵌套"] = len(files)
    logger.info(f"[3/6] 深层嵌套目录：{len(files)} 个文件")

    # 4. 边界情况文件
    edge_dir = TESTDATA_ROOT / "04_边界情况测试"
    count = 0
    for filename, content in EDGE_CASES:
        fpath = edge_dir / filename
        fpath.parent.mkdir(parents=True, exist_ok=True)
        if not fpath.exists():
            fpath.write_bytes(content)
        count += 1
    stats["边界情况"] = count
    logger.info(f"[4/6] 边界情况测试：{count} 个文件")

    # 5. 分类规则测试
    rule_dir = TESTDATA_ROOT / "05_分类规则测试"
    count = 0
    for sub_dir, filename, content in RULE_TEST_FILES:
        fpath = rule_dir / sub_dir / filename
        fpath.parent.mkdir(parents=True, exist_ok=True)
        if not fpath.exists():
            fpath.write_bytes(content)
        count += 1
    stats["规则测试"] = count
    logger.info(f"[5/6] 分类规则测试：{count} 个文件")

    # 6. 混乱桌面模拟
    messy_dir = TESTDATA_ROOT / "06_混乱桌面模拟"
    messy_dir.mkdir(parents=True, exist_ok=True)
    count = 0
    for filename in MESSY_DESKTOP_FILES:
        fpath = messy_dir / filename
        if not fpath.exists():
            # 随机生成 100B - 4KB 的内容
            size = rng.randint(100, 4 * 1024)
            fpath.write_bytes(rng.randbytes(size))
        count += 1
    stats["混乱桌面"] = count
    logger.info(f"[6/6] 混乱桌面模拟：{count} 个文件")

    return stats


def clean_testdata() -> None:
    """删除测试数据目录。"""
    if TESTDATA_ROOT.exists():
        shutil.rmtree(TESTDATA_ROOT)
        logger.info(f"已删除 {TESTDATA_ROOT}")
    else:
        logger.info("测试目录不存在，无需清理")


def main() -> int:
    parser = argparse.ArgumentParser(description="FileMind 第二批测试数据生成")
    parser.add_argument("--clean", action="store_true", help="删除测试数据目录")
    parser.add_argument("--dry-run", action="store_true", help="仅输出计划，不写入")
    parser.add_argument("--seed", type=int, default=SEED, help="随机种子")
    args = parser.parse_args()

    logging.basicConfig(level=logging.INFO, format="%(levelname)s %(message)s")

    if args.clean:
        if args.dry_run:
            logger.info(f"[dry-run] 将删除目录 {TESTDATA_ROOT}")
        else:
            clean_testdata()
        return 0

    if args.dry_run:
        logger.info(f"[plan] 测试数据将生成到 {TESTDATA_ROOT}")
        logger.info(f"  RAG 文本测试集：{len(RAG_DOCS)} 个文件")
        logger.info(f"  图片预览测试：{len(IMAGE_SPECS)} 个文件")
        logger.info(f"  深层嵌套目录：约 30+ 个文件（含子目录）")
        logger.info(f"  边界情况测试：{len(EDGE_CASES)} 个文件")
        logger.info(f"  分类规则测试：{len(RULE_TEST_FILES)} 个文件")
        logger.info(f"  混乱桌面模拟：{len(MESSY_DESKTOP_FILES)} 个文件")
        logger.info(f"  总计：约 150+ 个文件")
        return 0

    logger.info(f"开始生成测试数据到 {TESTDATA_ROOT}")
    stats = generate_all(seed=args.seed)
    total = sum(stats.values())
    logger.info(f"完成！共生成 {total} 个文件，分布如下：")
    for name, count in stats.items():
        logger.info(f"  {name}: {count} 个")
    logger.info(f"目录位置：{TESTDATA_ROOT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
