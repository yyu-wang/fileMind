"""Embedding 重建任务状态服务：进程内内存字典（无持久化）。

重建执行主体还没实现（T3.x Rust 驱动分批、Tauri Event 推进度给前端），
本模块先把**状态结构体 + CRUD** 铺好，让 `GET /embedding/rebuild/status`
先可用；长任务上线时复用同一套状态 API 即可。

说明：
- Sidecar 进程重启 = 内存任务状态丢失（设计如此）。如果进程重启，Rust 侧
  需要重新 `create_task`；但此时 LanceDB 新表已经写入一部分，重建长任务
  需要能从断点续做（T3.x 实现）。
- 为了避免内存膨胀，超过 ``max_age_hours`` 的 done/cancelled/failed 老任务
  通过 :func:`clear_finished` 定期清理（生命周期或路由侧钩子触发）。
"""

from __future__ import annotations

import math
import time
import uuid
from dataclasses import dataclass, field

# 状态枚举值常量（API 规格书 §s3-6b 响应体 status 字段）
STATUS_PENDING = "pending"
STATUS_IN_PROGRESS = "in_progress"
STATUS_PAUSED = "paused"
STATUS_DONE = "done"
STATUS_FAILED = "failed"
STATUS_CANCELLED = "cancelled"

_ALL_STATUSES = {
    STATUS_PENDING,
    STATUS_IN_PROGRESS,
    STATUS_PAUSED,
    STATUS_DONE,
    STATUS_FAILED,
    STATUS_CANCELLED,
}


@dataclass
class RebuildTask:
    """单个重建任务的完整状态快照（内存里可变，读写无锁）。"""

    task_id: str
    """UUID 任务 ID（API 返回给调用方的句柄）。"""

    new_model: str
    """目标模型名（用于 est 或展示）。"""

    new_version: int
    """目标模型版本。"""

    new_table_name: str
    """目标 LanceDB 表名。"""

    total: int
    """预计处理的文件总数。"""

    done: int = 0
    """已处理文件数（更新进度用）。"""

    current_file: str | None = None
    """当前处理的文件名（展示用，None 表示尚未开始或卡住）。"""

    status: str = STATUS_PENDING
    """pending / in_progress / paused / done / failed / cancelled。"""

    start_ts: float = 0
    """进入 in_progress 的墙钟时间戳（time.time()）；未开始为 0。"""

    error: str | None = None
    """失败原因（status=failed 时有值，其他状态为 None）。"""

    last_updated_ts: float = field(default_factory=time.time)
    """最后一次状态更新的时间戳（用于 clear_finished 判断过期）。"""


# 模块级存储：task_id -> RebuildTask
# 注：不用线程锁——Sidecar 是单进程，FastAPI 默认 asyncio 单线程跑 endpoints；
# 如果未来换成多 worker（uvicorn workers>1），此服务不可用，需要改为 Redis/DB。
_TASKS: dict[str, RebuildTask] = {}


def create_task(
    *,
    new_model: str,
    new_version: int,
    new_table_name: str,
    total: int,
) -> str:
    """创建一个新的重建任务（初始 status=pending）。返回 task_id。"""
    task_id = uuid.uuid4().hex
    _TASKS[task_id] = RebuildTask(
        task_id=task_id,
        new_model=new_model,
        new_version=new_version,
        new_table_name=new_table_name,
        total=max(total, 0),
    )
    return task_id


def get_task(task_id: str) -> RebuildTask | None:
    """按 ID 查询任务；不存在返回 None。"""
    return _TASKS.get(task_id)


def update_progress(
    task_id: str,
    *,
    done: int,
    current_file: str | None = None,
) -> None:
    """更新单个任务的进度 + 当前文件名（不改变状态）。

    - done 自动 clamp 到 [0, task.total]
    - 不存在的 task_id 静默忽略（调用方用 ``get_task`` 先确认再更新也行）
    - 更新时同时刷 ``last_updated_ts``（便于清理过期任务）
    """
    task = _TASKS.get(task_id)
    if task is None:
        return
    task.done = max(0, min(done, task.total))
    task.current_file = current_file
    task.last_updated_ts = time.time()


def set_status(task_id: str, status: str, *, error: str | None = None) -> None:
    """设置任务状态。合法性校验但不抛异常（非法值回落到 pending 并提示）。"""
    task = _TASKS.get(task_id)
    if task is None:
        return
    # 先保存原始 status，用于 error 信息里引用非法值
    original_status = status
    if status not in _ALL_STATUSES:
        # 安全兜底：未知状态写为 pending + error 里给说明（原始非法值），避免 UI 崩溃
        status = STATUS_PENDING
        error = error or f"unknown status: {original_status}"
    task.status = status
    if error is not None:
        task.error = error
    # 进入 in_progress 时记录开始时间
    if status == STATUS_IN_PROGRESS and task.start_ts == 0:
        task.start_ts = time.time()
    task.last_updated_ts = time.time()


def can_pause(task: RebuildTask) -> bool:
    """是否允许暂停：仅在进行中。"""
    return task.status == STATUS_IN_PROGRESS


def can_resume(task: RebuildTask) -> bool:
    """是否允许恢复：paused（用户手动暂停断点续做）/ failed（失败了从断点重试）。"""
    return task.status in {STATUS_PAUSED, STATUS_FAILED}


def est_remaining_minutes(task: RebuildTask) -> int | None:
    """按已耗时间粗估剩余分钟。done=0 时估算不出，返回 None。"""
    if task.done <= 0 or task.start_ts <= 0:
        return None
    elapsed = time.time() - task.start_ts  # seconds
    if elapsed <= 0:
        return None
    rate = task.done / elapsed  # files per second
    remaining_files = max(task.total - task.done, 0)
    if rate <= 0:
        return None
    seconds_left = remaining_files / rate
    return max(1, int(math.ceil(seconds_left / 60)))


def clear_finished(*, max_age_hours: int = 24) -> int:
    """清理超过 max_age_hours 的终态任务（done/failed/cancelled）。

    返回被清理的任务数（便于日志监控）。不清理 pending/in_progress/paused，
    因为它们仍在工作流中。
    """
    cutoff = time.time() - max_age_hours * 3600
    to_remove: list[str] = []
    for tid, task in _TASKS.items():
        if task.status not in {STATUS_DONE, STATUS_FAILED, STATUS_CANCELLED}:
            continue
        if task.last_updated_ts < cutoff:
            to_remove.append(tid)
    for tid in to_remove:
        _TASKS.pop(tid, None)
    return len(to_remove)


# 暴露给测试与清理：别直接改 _TASKS，用这个清干净（测试 fixture 用）
def _reset_all_for_tests() -> None:
    """单元测试调用。生产代码禁止。"""
    _TASKS.clear()
