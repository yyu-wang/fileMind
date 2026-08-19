"""T4.6 — 评估 CLI 达标门禁单元测试。

monkeypatch ``run_eval`` 返回合成报告，验证退出码门禁（达标 0 / 不达标 1）
与 ``--min-accuracy`` 覆盖，不走真实分类。
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

import eval.__main__ as cli  # noqa: E402
from eval.metrics import EvalReport  # noqa: E402


def _report(accuracy: float) -> EvalReport:
    """构造指定准确率、其余指标达标的报告。"""
    return EvalReport(
        total=100,
        accuracy=accuracy,
        rule_coverage=0.8,
        llm_count=10,
        llm_accuracy=0.9,
        json_parse_rate=1.0,
        confusion={},
        per_category=[],
    )


def test_main_returns_zero_when_meets_targets(tmp_path, monkeypatch) -> None:
    """全部达标 → 退出码 0，测试集保留。"""

    async def fake_run(records: object, use_llm: bool = True) -> EvalReport:
        return _report(accuracy=0.95)

    monkeypatch.setattr(cli, "run_eval", fake_run)
    dataset = tmp_path / "eval.jsonl"
    assert cli.main(["--count", "20", "--dataset", str(dataset), "--keep"]) == 0
    assert dataset.exists()


def test_main_returns_one_when_below_min_accuracy(tmp_path, monkeypatch) -> None:
    """准确率低于默认门禁 0.85 → 退出码 1。"""

    async def fake_run(records: object, use_llm: bool = True) -> EvalReport:
        return _report(accuracy=0.5)

    monkeypatch.setattr(cli, "run_eval", fake_run)
    dataset = tmp_path / "eval.jsonl"
    assert cli.main(["--count", "20", "--dataset", str(dataset), "--keep"]) == 1


def test_main_min_accuracy_override(tmp_path, monkeypatch) -> None:
    """--min-accuracy 调整门禁：0.95 > 0.9 → 不达标；0.85 <= 0.9 → 达标。"""

    async def fake_run(records: object, use_llm: bool = True) -> EvalReport:
        return _report(accuracy=0.9)

    monkeypatch.setattr(cli, "run_eval", fake_run)
    dataset = tmp_path / "eval.jsonl"
    assert (
        cli.main(["--count", "20", "--dataset", str(dataset), "--keep", "--min-accuracy", "0.95"])
        == 1
    )
    assert (
        cli.main(["--count", "20", "--dataset", str(dataset), "--keep", "--min-accuracy", "0.85"])
        == 0
    )
