"""``LanceDBManager`` 组装与连接初始化（原 ``lancedb_repo.py`` 拆出）。

安全约束（`07_安全合规设计.html` 威胁分析）：
    - 存储目录 ~/.filemind/data/lancedb 权限 0600
    - 模型版本切换 = 建新表，不原地修改旧表（旧表保留便于回滚）
"""

from __future__ import annotations

import contextlib
import os
from dataclasses import dataclass, field
from typing import TYPE_CHECKING

from app.db.lancedb_repo.chunks import ChunkOpsMixin
from app.db.lancedb_repo.search import VectorSearchMixin

if TYPE_CHECKING:
    from pathlib import Path

    # 惰性导入（P2-1）：lancedb 冷导入约 0.7s（连带 lance_namespace / pyarrow 扩展），
    # 仅在真正建立连接时加载，避免拖慢 Sidecar 启动（/health 可服务时间）。
    # 运行时导入见本模块 ``LanceDBManager.connect``。
    import lancedb
    from lancedb.table import Table as LanceTable


@dataclass
class LanceDBManager(ChunkOpsMixin, VectorSearchMixin):
    """LanceDB 生命周期管理器（FastAPI lifespan 里单例初始化）。

    方法按职责拆到三个混入（见包 docstring）；本类只放字段与连接初始化。
    """

    db_path: Path
    # 惰性求值：``from __future__ import annotations`` 下此为字符串注解，
    # 不在导入期解析 lancedb.DBConnection（该导入在 TYPE_CHECKING 分支）
    _db: lancedb.DBConnection | None = None
    #: 表句柄缓存：open_table 命中后跳过列目录/读元数据（T10.2 检索首 token 优化）。
    #: 表被删除重建时须调 invalidate_table/invalidate_all 使旧句柄失效。
    _table_cache: dict[str, LanceTable] = field(default_factory=dict, init=False)

    def connect(self) -> None:
        """创建/打开 LanceDB。

        行为：
            1. 确保父目录存在，权限 0700（owner rwx 仅自己）
            2. 目录不存在时 mkdir(parents=True)
            3. lancedb.connect(self.db_path)
            4. 对生成的 LanceDB 数据文件/目录设 0600（owner rw only）

        Raises:
            RuntimeError: LanceDB 连接失败时，抛带路径上下文的异常
        """
        # P2-1：惰性导入（模块级不导入 lancedb，见文件头 TYPE_CHECKING 说明）
        import lancedb

        try:
            self.db_path.parent.mkdir(parents=True, exist_ok=True)
            # 父目录 0700：仅 owner 可进入/读/写
            os.chmod(self.db_path.parent, 0o700)
            self._db = lancedb.connect(str(self.db_path))
            # 连接成功后，为 db_path 目录再降权：0700 即可（不阻止子文件写入）
            if self.db_path.exists():
                # 某些文件系统不支持 chmod（samba、tmpfs...），不阻塞启动
                with contextlib.suppress(OSError):
                    os.chmod(self.db_path, 0o700)
        except Exception as exc:  # noqa: BLE001
            raise RuntimeError(f"LanceDB 初始化失败（path={self.db_path}）: {exc}") from exc
