#!/usr/bin/env python3
"""从 CI 产物生成 updater 清单 latest.json 与 GitHub Release 正文。

用法：
    python3 scripts/gen-latest-json.py \\
        --dist dist --out-dir release \\
        --version 1.0.0 --tag v1.0.0 \\
        --repo yyu-wang/fileMind --check-version

产物：
    <out-dir>/latest.json       上传到 GitHub Release，作为 updater 端点数据源
    <out-dir>/release-notes.md  GitHub Release 正文（取自 CHANGELOG.md 对应版本小节）

清单里的 `signature` 直接读取构建阶段生成的 `.sig` 文件内容（纯文本）。

⚠️ 若将来接入构建后签名（如 SignPath），签名会改变安装包字节，
必须在调用本脚本**之前**用 `tauri signer sign` 重新生成 `.sig`，
否则清单里的签名与安装包不匹配，用户端更新会直接失败。
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

# 平台键与 Tauri updater 的 platform 标识一致；值是安装包在产物目录里的 glob。
# 只列实际分发的平台：macOS Intel / Linux 本期不分发（见 README「分发范围」）。
PLATFORMS: dict[str, str] = {
    "darwin-aarch64": "*.app.tar.gz",
    "windows-x86_64": "*-setup.exe",
}

# 版本号必须与 tag 一致的文件（release.md 要求多处同步）
VERSION_FILES: tuple[tuple[str, str], ...] = (
    ("package.json", "version"),
    ("src-tauri/tauri.conf.json", "version"),
)

HEADING_RE = re.compile(r"^##\s+\\?\[(?P<version>[^\]]+)\\?\]")


def parse_args() -> argparse.Namespace:
    """解析命令行参数。"""
    parser = argparse.ArgumentParser(description="生成 latest.json 与 Release notes")
    parser.add_argument("--dist", required=True, type=Path, help="CI 产物根目录")
    parser.add_argument("--out-dir", required=True, type=Path, help="输出目录")
    parser.add_argument("--version", required=True, help="应用版本号，如 1.0.0")
    parser.add_argument("--tag", required=True, help="Git tag，如 v1.0.0")
    parser.add_argument("--repo", required=True, help="GitHub 仓库 owner/name")
    parser.add_argument(
        "--changelog",
        type=Path,
        default=Path("CHANGELOG.md"),
        help="Changelog 路径（默认 CHANGELOG.md）",
    )
    parser.add_argument(
        "--check-version",
        action="store_true",
        help="校验 version 与 package.json / tauri.conf.json / Cargo.toml 一致",
    )
    return parser.parse_args()


def check_version(version: str) -> None:
    """校验版本号在多处声明文件中一致。

    Args:
        version: 期望的版本号。

    Raises:
        SystemExit: 任一文件缺失或版本号不一致。
    """
    mismatched: list[str] = []
    for path, key in VERSION_FILES:
        if not path or not Path(path).is_file():
            mismatched.append(f"{path}: 文件不存在")
            continue
        value = json.loads(Path(path).read_text(encoding="utf-8")).get(key)
        if value != version:
            mismatched.append(f"{path}: {value}")
    cargo = Path("src-tauri/Cargo.toml")
    if cargo.is_file():
        matched = re.search(r'^version\s*=\s*"([^"]+)"', cargo.read_text(encoding="utf-8"), re.MULTILINE)
        cargo_version = matched.group(1) if matched else "未找到 version"
        if cargo_version != version:
            mismatched.append(f"src-tauri/Cargo.toml: {cargo_version}")
    else:
        mismatched.append("src-tauri/Cargo.toml: 文件不存在")
    if mismatched:
        raise SystemExit(
            f"版本号与 {version} 不一致（release.md 要求同步更新）：\n  - "
            + "\n  - ".join(mismatched)
        )
    print(f"版本号一致：{version}")


def find_installer(dist: Path, platform: str, pattern: str) -> Path:
    """在产物目录中定位某平台的安装包。

    Args:
        dist: CI 产物根目录。
        platform: 平台键，用于报错信息。
        pattern: 安装包文件名的 glob。

    Returns:
        匹配到的安装包路径。

    Raises:
        SystemExit: 匹配结果不是恰好 1 个。
    """
    matches = sorted(dist.rglob(pattern))
    if len(matches) != 1:
        raise SystemExit(
            f"平台 {platform} 期望恰好 1 个 `{pattern}` 产物，实际 {len(matches)} 个："
            f"{[str(p) for p in matches]}"
        )
    return matches[0]


def read_signature(installer: Path) -> str:
    """读取安装包对应的 `.sig` 内容。

    Args:
        installer: 安装包路径。

    Returns:
        签名文本（去掉首尾空白）。

    Raises:
        SystemExit: `.sig` 文件不存在或为空。
    """
    sig_path = Path(f"{installer}.sig")
    if not sig_path.is_file():
        raise SystemExit(
            f"缺少 updater 签名文件：{sig_path}（构建时需设置 TAURI_SIGNING_PRIVATE_KEY，"
            "且 merge-build 的 artifact 清单需包含 *.sig）"
        )
    signature = sig_path.read_text(encoding="utf-8").strip()
    if not signature:
        raise SystemExit(f"updater 签名文件为空：{sig_path}")
    return signature


def extract_notes(changelog: Path, version: str) -> str:
    """从 Changelog 中抽取指定版本的正文，作为 Release 说明。

    Args:
        changelog: Changelog 文件路径。
        version: 版本号，如 1.0.0。

    Returns:
        该版本小节正文（Markdown）。

    Raises:
        SystemExit: Changelog 不存在，或没有该版本的小节。
    """
    if not changelog.is_file():
        raise SystemExit(f"Changelog 不存在：{changelog}")
    lines = changelog.read_text(encoding="utf-8").splitlines()
    start = next(
        (
            index
            for index, line in enumerate(lines)
            if (matched := HEADING_RE.match(line)) and matched.group("version").strip() == version
        ),
        None,
    )
    if start is None:
        raise SystemExit(f"CHANGELOG.md 中没有 [{version}] 小节（发布前需把 [Unreleased] 定版）")
    end = next(
        (index for index in range(start + 1, len(lines)) if HEADING_RE.match(lines[index])),
        len(lines),
    )
    body = lines[start + 1 : end]
    # 去掉收尾的分隔标记与多余空行（CHANGELOG 尾部有 <br />）
    while body and body[-1].strip() in {"", "<br />", "---"}:
        body.pop()
    return "\n".join(body).strip()


def build_manifest(dist: Path, version: str, tag: str, repo: str, notes: str) -> dict[str, Any]:
    """组装 latest.json 内容。

    Args:
        dist: CI 产物根目录。
        version: 应用版本号。
        tag: Git tag。
        repo: GitHub 仓库 owner/name。
        notes: 更新说明。

    Returns:
        可直接序列化为 latest.json 的字典。
    """
    platforms: dict[str, dict[str, str]] = {}
    for platform, pattern in PLATFORMS.items():
        installer = find_installer(dist, platform, pattern)
        url = f"https://github.com/{repo}/releases/download/{tag}/{installer.name}"
        platforms[platform] = {"signature": read_signature(installer), "url": url}
        print(f"  {platform}: {installer.name}")
    return {
        "version": version,
        "notes": notes,
        "pub_date": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platforms": platforms,
    }


def main() -> None:
    """入口：校验版本 → 组装清单 → 写出 latest.json 与 release-notes.md。"""
    args = parse_args()
    if not args.dist.is_dir():
        raise SystemExit(f"产物目录不存在：{args.dist}")
    if args.check_version:
        check_version(args.version)
    print(f"扫描产物：{args.dist}")
    notes = extract_notes(args.changelog, args.version)
    manifest = build_manifest(args.dist, args.version, args.tag, args.repo, notes)

    args.out_dir.mkdir(parents=True, exist_ok=True)
    latest = args.out_dir / "latest.json"
    latest.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    release_notes = args.out_dir / "release-notes.md"
    release_notes.write_text(f"{notes}\n", encoding="utf-8")
    print(f"已写出 {latest}（版本 {manifest['version']}，{len(manifest['platforms'])} 个平台）")
    print(f"已写出 {release_notes}")


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        sys.exit(130)
