"""Embedding 模型切换：预检查 + 版本分配 + 新表保障。

范围（T2.6）：``precheck_switch`` 纯函数计算需重建文件数与预估耗时；
``next_version`` 扫描 LanceDB 中同模型已有 documents_* 表分配下一个版本号；
``ensure_target_table`` 调 LanceDBManager.ensure_table 建新表（若不存在）。

非范围：真实 embedding 重算与分批写表（T3.x 长任务串联实现）。
"""

from __future__ import annotations

import math
import re
from typing import TYPE_CHECKING

from app.core import embedding_models
from app.models import EmbeddingSwitchResult

if TYPE_CHECKING:
    from app.db.lancedb_repo import LanceDBManager

# 复用 LanceDB 表名中的版本号提取正则：documents_{safe_model}_v{N}
# LanceDB 内部用 _sanitize_model_name，但 service 层只需要"从 table 名反推出 N"，
# 这里用宽匹配即可：末尾 _v(\d+) 前面跟任意字符
_TABLE_VERSION_RE = re.compile(r"_v(\d+)$")

# 合法模型名字符白名单（与 lancedb_repo._MODEL_NAME_FORBIDDEN_RE 一致）
# 保留副本：避免 service → db/__init__.py 双向 import 噪声
_SAFE_MODEL_FORBIDDEN_RE = re.compile(r"[^A-Za-z0-9._-]")


def _sanitize_model_name(model: str) -> str:
    """与 LanceDBManager.table_name 同名的 sanitize（本地副本避免循环 import）。"""
    return _SAFE_MODEL_FORBIDDEN_RE.sub("_", model)


def next_version(lancedb: LanceDBManager | None, model: str) -> int:
    """分配下一个 Embedding 版本号：documents_{model}_v{N}。

    扫描 LanceDB 里所有 documents_ 开头的表，挑出模型名匹配的最大 N，返回 N+1。
    完全没有匹配表 → 返回 MODEL_REGISTRY 中该模型的 default_version。

    说明：T2.6 预检查阶段可能还没建任何表（lancedb 传入 None 也可用）；
    生产切换时必须传真实 LanceDBManager，否则版本号回落到 initial 值，
    后续 ``ensure_table`` 冲突会抛错。
    """
    default = embedding_models.get_model_info(model).default_version

    if lancedb is None:
        return default

    safe = _sanitize_model_name(model)
    prefix = f"documents_{safe}_v"

    existing_max = 0
    for tname in lancedb.list_document_tables():
        if not tname.startswith(prefix):
            continue
        m = _TABLE_VERSION_RE.search(tname)
        if m is None:
            continue
        v = int(m.group(1))
        if v > existing_max:
            existing_max = v

    if existing_max == 0:
        return default
    return existing_max + 1


def _est_minutes(indexed_files: int) -> int:
    """按 ``files_per_minute`` 粗估耗时（无上限时）。"""
    if indexed_files <= 0:
        return 0
    minutes = math.ceil(indexed_files / embedding_models.EST_FILES_PER_MINUTE)
    if minutes > embedding_models.EST_MINUTES_CAP:
        return embedding_models.EST_MINUTES_CAP
    return int(minutes)


def precheck_switch(
    *,
    new_model: str,
    current_model: str,
    indexed_files: int,
    lancedb: LanceDBManager | None,
    current_version: int,
) -> EmbeddingSwitchResult:
    """切换预检查（POST /embedding/switch 背后的核心逻辑）。

    纯函数：读取 LanceDB 表名列表（不写），返回结果对象。调用方把它包成
    ``{"success": True, "data": result}`` 回给 Rust 端。

    业务规则（对齐 T2.6 开发计划 §3.2）：
      1. new_model 合法性校验（未知模型抛 ValueError → 路由转 400）
      2. current_model 同时合法性校验（当前模型名不合法意味着状态异常，
         允许走预检查，但 current_dim 用 0 占位避免 crash）
      3. **模型名变了（即使 dim 相同）→ 判定 dim_changed=True，
         files_to_rebuild = indexed_files（向量空间不同，必须重 embedding）**
      4. 模型名相同 → dim_changed=False，files_to_rebuild=0
         （版本 bump；下次增量索引会按 embedding_version 判断，不需要全量）
      5. old_table_preserved 固定 True（rules/sql.md：旧表保留便于回滚）
    """
    # Step 1：目标模型合法性（严格校验，不允许未知模型）
    new_info = embedding_models.get_model_info(new_model)  # 抛 ValueError

    # Step 2：当前模型 dim（不阻塞预检查；非法时用 0 占位，结果展示不依赖它）
    try:
        current_dim = embedding_models.get_model_dim(current_model)
    except ValueError:
        current_dim = 0

    # Step 3：版本号分配（扫描 documents_* 表）
    assigned_version = next_version(lancedb, new_model)

    # Step 4：构造表名——直接用 LanceDBManager.table_name 的静态方法避免再拼一遍
    # （这里没有 lancedb 实例也能调用，因为它是 @staticmethod）
    from app.db.lancedb_repo import LanceDBManager

    new_table_name = LanceDBManager.table_name(new_model, assigned_version)

    # Step 5：判断是否全量重建
    model_changed = new_model != current_model
    dim_changed = model_changed or (current_dim != new_info.dim)

    files_to_rebuild = max(indexed_files, 0) if dim_changed else 0

    est_minutes = _est_minutes(files_to_rebuild)

    return EmbeddingSwitchResult(
        dim_changed=dim_changed,
        current_dim=current_dim,
        new_dim=new_info.dim,
        files_to_rebuild=files_to_rebuild,
        est_minutes=est_minutes,
        new_table_name=new_table_name,
        new_version=assigned_version,
        old_table_preserved=True,
    )


def ensure_target_table(
    *,
    lancedb: LanceDBManager,
    new_model: str,
    new_version: int,
) -> str:
    """预检查通过后、T3.x 启动重建前调用：确保目标表存在。

    若表不存在：写入一条零向量作为 schema 锚点并立即删除；
    若表已存在：直接返回表名（幂等，断点恢复场景会命中）。

    错误透传：LanceDBManager.ensure_table 抛 ValueError/RuntimeError 直接向上；
    调用方（T3.x）捕获后对用户报错即可。
    """
    new_dim = embedding_models.get_model_dim(new_model)
    return lancedb.ensure_table(new_model, new_version, new_dim)
