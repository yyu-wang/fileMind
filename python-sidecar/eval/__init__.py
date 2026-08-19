"""FileMind 分类准确率评估包（T4.5）。

子模块：
- dataset: 评估集生成与加载
- metrics: 分类归一化、混淆矩阵与指标计算
- runner: 三层漏斗评估编排
- __main__: CLI 入口（``python -m eval``）
"""

from __future__ import annotations
