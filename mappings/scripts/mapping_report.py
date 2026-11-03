#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
mapping_report.py

校验一份型号码表并输出单行摘要（每品牌一行，直接作为 PR 正文与 run summary）。
校验项与 `voltnest-hid` 的 table.rs 解析口径一致，不额外收紧。

用法：
    python mapping_report.py 新表.json --old 旧表.json
    python mapping_report.py 新表.json --old 旧表.json --require-change
    python mapping_report.py 新表.json            # 不比对旧表

退出码：0 = devices 有变化（或未比对），1 = 表非法，3 = devices 与旧表一致（仅 --require-change）。
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

KEY_RE = re.compile(r"^[0-9A-Fa-f]{4}:[0-9A-Fa-f]{4}$")


def load_devices(path: Path) -> dict:
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        raise SystemExit(f"{path}: JSON 解析失败：{exc}")
    if not isinstance(data, dict):
        raise SystemExit(f"{path}: 顶层必须是对象")

    version = data.get("config_version")
    if not isinstance(version, str) or not version.strip():
        raise SystemExit(f"{path}: config_version 缺失或为空")

    devices = data.get("devices")
    if not isinstance(devices, dict) or not devices:
        raise SystemExit(f"{path}: devices 缺失或为空")

    for key, entry in devices.items():
        if not KEY_RE.match(key):
            raise SystemExit(f"{path}: 非法设备标识 {key!r}（应为 VVVV:PPPP）")
        if not isinstance(entry, dict):
            raise SystemExit(f"{path}: 设备 {key} 的条目必须是对象")

    source = data.get("source")
    if source is not None and not isinstance(source, dict):
        raise SystemExit(f"{path}: source 必须是对象")

    return {"version": version, "devices": devices}


def main() -> int:
    parser = argparse.ArgumentParser(description="校验型号码表并输出摘要")
    parser.add_argument("new", type=Path, help="新抓取的型号表")
    parser.add_argument("--old", type=Path, help="仓库内旧表，用于统计差异")
    parser.add_argument(
        "--require-change",
        action="store_true",
        help="devices 与旧表一致时以退出码 3 结束（config_version 变化不算）",
    )
    args = parser.parse_args()

    new = load_devices(args.new)
    devices = new["devices"]
    keys = set(devices)

    same = False
    delta = ""
    if args.old and args.old.is_file():
        old_devices = load_devices(args.old)["devices"]
        old_keys = set(old_devices)
        added = len(keys - old_keys)
        removed = len(old_keys - keys)
        same = old_devices == devices
        if added or removed:
            delta = f"，+{added} / −{removed}"
        elif not same:
            delta = "，条目数不变但内容有变化"

    if same:
        print(f"{args.new.stem}：{len(keys)} 台，devices 无变化，跳过")
        return 3 if args.require_change else 0

    print(f"{args.new.stem}：{len(keys)} 台{delta}（版本 {new['version']}）")
    return 0


if __name__ == "__main__":
    sys.exit(main())
