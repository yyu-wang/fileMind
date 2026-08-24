"""T2.6 — services.rebuild_status_service 单元测试。

覆盖：create_task/get_task 往返；update_progress 边界；set_status + can_pause/can_resume；
est_remaining_minutes（done>0 / done=0 两条路径）；clear_finished。
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.services import rebuild_status_service as svc  # noqa: E402


def setup_function(_func) -> None:
    """每个测试函数开始前，清空内存任务状态（避免用例间耦合）。"""
    svc._reset_all_for_tests()


def teardown_function(_func) -> None:
    """结束后再清一次，避免污染。"""
    svc._reset_all_for_tests()


# ------------------------------------------------------------------
# CRUD 基础
# ------------------------------------------------------------------


def test_create_task_returns_id_and_get_task() -> None:
    """create_task 返回 hex uuid，get_task 拿到结构正确的 RebuildTask。"""
    tid = svc.create_task(
        new_model="bge-m3",
        new_version=2,
        new_table_name="documents_bge-m3_v2",
        total=1000,
    )
    assert len(tid) == 32 and all(c in "0123456789abcdef" for c in tid), (
        f"task_id 应该是 uuid4 hex（32 字符），实际={tid!r}"
    )

    t = svc.get_task(tid)
    assert t is not None
    assert t.task_id == tid
    assert t.new_model == "bge-m3"
    assert t.new_version == 2
    assert t.new_table_name == "documents_bge-m3_v2"
    assert t.total == 1000
    assert t.done == 0
    assert t.status == svc.STATUS_PENDING
    assert t.current_file is None
    assert t.error is None


def test_get_task_unknown_returns_none() -> None:
    """不存在的 task_id → None（调用方抛 404）。"""
    assert svc.get_task("not-a-task") is None


def test_update_progress_increments_and_clamps() -> None:
    """update_progress: done 被 clamp 到 [0, total]，current_file 更新。"""
    tid = svc.create_task(new_model="x", new_version=1, new_table_name="t1", total=100)
    # 中间值
    svc.update_progress(tid, done=42, current_file="report.pdf")
    t = svc.get_task(tid)
    assert t is not None
    assert t.done == 42
    assert t.current_file == "report.pdf"

    # done 超 total → clamp 到 total
    svc.update_progress(tid, done=9999)
    assert svc.get_task(tid).done == 100

    # done 负数 → clamp 到 0
    svc.update_progress(tid, done=-5)
    assert svc.get_task(tid).done == 0


def test_update_progress_nonexistent_id_silent() -> None:
    """更新不存在的任务——静默 no-op（不抛异常，不新增假任务）。"""
    svc.update_progress("nope", done=10, current_file="x.pdf")
    assert svc.get_task("nope") is None


# ------------------------------------------------------------------
# set_status + can_pause / can_resume
# ------------------------------------------------------------------


def test_set_status_in_progress_records_start_ts() -> None:
    """进入 in_progress 时刷 start_ts（做 est_remaining 的基准）。"""
    tid = svc.create_task(new_model="x", new_version=1, new_table_name="t1", total=100)
    before = time.time()
    svc.set_status(tid, svc.STATUS_IN_PROGRESS)
    after = time.time()
    t = svc.get_task(tid)
    assert t is not None
    assert t.status == svc.STATUS_IN_PROGRESS
    # 时间戳应在 [before, after] 范围内
    assert before - 1 <= t.start_ts <= after + 1
    # can_pause = True, can_resume = False
    assert svc.can_pause(t) is True
    assert svc.can_resume(t) is False


def test_set_status_paused_cannot_pause_but_can_resume() -> None:
    """paused → can_pause=False, can_resume=True。"""
    tid = svc.create_task(new_model="x", new_version=1, new_table_name="t1", total=100)
    svc.set_status(tid, svc.STATUS_PAUSED)
    t = svc.get_task(tid)
    assert t is not None
    assert svc.can_pause(t) is False
    assert svc.can_resume(t) is True


def test_set_status_failed_with_error_and_can_resume() -> None:
    """失败状态：error 保存，status=failed，can_resume=True（允许断点续做）。"""
    tid = svc.create_task(new_model="x", new_version=1, new_table_name="t1", total=100)
    svc.set_status(tid, svc.STATUS_FAILED, error="Ollama 连接超时")
    t = svc.get_task(tid)
    assert t is not None
    assert t.status == svc.STATUS_FAILED
    assert t.error == "Ollama 连接超时"
    assert svc.can_pause(t) is False
    assert svc.can_resume(t) is True


def test_set_status_done_cannot_pause_or_resume() -> None:
    """done 是终态：can_pause=False, can_resume=False。"""
    tid = svc.create_task(new_model="x", new_version=1, new_table_name="t1", total=100)
    svc.set_status(tid, svc.STATUS_DONE)
    t = svc.get_task(tid)
    assert t is not None
    assert svc.can_pause(t) is False
    assert svc.can_resume(t) is False


def test_set_status_cancelled_terminal() -> None:
    """cancelled 终态：不可暂停不可恢复。"""
    tid = svc.create_task(new_model="x", new_version=1, new_table_name="t1", total=100)
    svc.set_status(tid, svc.STATUS_CANCELLED)
    t = svc.get_task(tid)
    assert t is not None
    assert t.status == svc.STATUS_CANCELLED
    assert svc.can_pause(t) is False
    assert svc.can_resume(t) is False


def test_set_status_unknown_falls_back_to_pending() -> None:
    """未知状态值：安全兜底写 pending + error 说明，避免 UI 崩溃。"""
    tid = svc.create_task(new_model="x", new_version=1, new_table_name="t1", total=100)
    svc.set_status(tid, "weird_status")
    t = svc.get_task(tid)
    assert t is not None
    assert t.status == svc.STATUS_PENDING
    assert t.error is not None and "weird_status" in t.error


def test_set_status_nonexistent_id_is_silent() -> None:
    """未知 id 写状态——静默 no-op。"""
    svc.set_status("ghost", svc.STATUS_DONE)
    assert svc.get_task("ghost") is None


# ------------------------------------------------------------------
# est_remaining_minutes
# ------------------------------------------------------------------


def test_est_remaining_done_zero_is_none() -> None:
    """done=0 时（未开始或卡住），算不出剩余时间 → None。"""
    tid = svc.create_task(new_model="x", new_version=1, new_table_name="t1", total=100)
    t = svc.get_task(tid)
    assert t is not None
    assert svc.est_remaining_minutes(t) is None


def test_est_remaining_done_gt_zero_has_value() -> None:
    """done>0 且 start_ts>0 → 给出剩余分钟（下界 1）。"""
    tid = svc.create_task(new_model="x", new_version=1, new_table_name="t1", total=100)
    svc.set_status(tid, svc.STATUS_IN_PROGRESS)
    # 人工把 start_ts 调到 10 秒前，模拟已工作 10 秒做了 10 个文件 = 速率 1/s
    t = svc.get_task(tid)
    assert t is not None
    t.start_ts = time.time() - 10
    svc.update_progress(tid, done=10)
    # 剩 90 文件，按 1/s → 90 秒 → ceil(90/60) = 2 分钟
    remaining = svc.est_remaining_minutes(t)
    assert remaining is not None
    # 速率 = 10/10 = 1 file/s；剩余 90 → 90 s → 2 分钟；
    # 但受测试环境抖动影响（实际 time.time() 与我们 set 略有差），用宽松区间
    assert 1 <= remaining <= 3


def test_est_remaining_complete_returns_at_least_one() -> None:
    """极端：done=total（100% 完了但还没改状态 done）。剩余时间 ceil(0/...) ≥ 1。"""
    tid = svc.create_task(new_model="x", new_version=1, new_table_name="t1", total=50)
    svc.set_status(tid, svc.STATUS_IN_PROGRESS)
    t = svc.get_task(tid)
    assert t is not None
    t.start_ts = time.time() - 5
    svc.update_progress(tid, done=50)
    r = svc.est_remaining_minutes(t)
    # 已完成所有 → 剩 0 文件。按公式 max(1, ceil(0/60)) = 至少 1（或者 0？实现里
    # remaining_files = 0 且 rate>0 → seconds_left=0 → max(1, ceil(0)) = 1）
    assert r == 1


# ------------------------------------------------------------------
# clear_finished
# ------------------------------------------------------------------


def test_clear_finished_removes_old_terminal_tasks() -> None:
    """老终态任务被清；年轻的终态 + 所有非终态保留。"""
    # 任务 1：done，且 48 小时前更新（超过默认 max_age_hours=24 → 清掉）
    t1 = svc.create_task(new_model="x", new_version=1, new_table_name="t1", total=10)
    svc.set_status(t1, svc.STATUS_DONE)
    svc.get_task(t1).last_updated_ts = time.time() - 48 * 3600  # 48h 前

    # 任务 2：failed，仅 1 小时前（保留）
    t2 = svc.create_task(new_model="x", new_version=1, new_table_name="t1", total=10)
    svc.set_status(t2, svc.STATUS_FAILED)
    svc.get_task(t2).last_updated_ts = time.time() - 3600

    # 任务 3：in_progress，1 年以前（非终态 → 不清理）
    t3 = svc.create_task(new_model="x", new_version=1, new_table_name="t1", total=10)
    svc.set_status(t3, svc.STATUS_IN_PROGRESS)
    svc.get_task(t3).last_updated_ts = time.time() - 365 * 24 * 3600

    removed = svc.clear_finished()
    assert removed == 1, "只清了 1 个老 done"
    assert svc.get_task(t1) is None
    assert svc.get_task(t2) is not None
    assert svc.get_task(t3) is not None


def test_clear_finished_no_candidates_is_noop() -> None:
    """没什么可清 → return 0。"""
    svc.create_task(new_model="x", new_version=1, new_table_name="t1", total=10)
    assert svc.clear_finished() == 0
