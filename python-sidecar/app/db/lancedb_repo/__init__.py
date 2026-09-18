"""LanceDB 连接管理与 schema 定义。

表名规范：``documents_{embedding_model}_v{version}``
（模型名中非法字符做安全替换，参考 ``TableOpsMixin.table_name``。）

Schema 来源：`02_总体实施计划.html §4.2`：
    vector      FLOAT[dim]
    chunk_id    TEXT
    file_path   TEXT
    chunk_text  TEXT
    page        INTEGER (默认 0)

安全约束（`07_安全合规设计.html` 威胁分析）：
    - 存储目录 ~/.filemind/data/lancedb 权限 0600
    - 模型版本切换 = 建新表，不原地修改旧表（旧表保留便于回滚）

模块划分（原单文件 420 行的单个 ``LanceDBManager``，逼近 Python 模块 500 行强制
阈值，见 `rules/complexity.md`）：本包同名保留原导入路径
（``from app.db.lancedb_repo import LanceDBManager`` 的 11+ 处调用方零改动），
内部按职责拆为混入：``tables`` 表名规范与表生命周期 / ``chunks`` 向量行增删 /
``search`` ANN 检索，``manager`` 只放字段与连接初始化。
"""

from app.db.lancedb_repo.manager import LanceDBManager
from app.db.lancedb_repo.types import DocumentChunk, VectorHit

__all__ = ["DocumentChunk", "LanceDBManager", "VectorHit"]
