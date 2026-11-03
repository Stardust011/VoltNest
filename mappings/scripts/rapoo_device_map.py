#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
rapoo_device_map.py

从 RAPOO HUB 网页驱动自动抓取并解析设备映射表（manifest），
转换为精简 JSON：{config_version, source, devices}。

数据来源：网页首页 -> 自动发现 index-*.js 打包文件 -> 解析内嵌的
设备 manifest（model / type / hardware.vendorId / productId / transport 等）。

仅使用 Python 标准库，无第三方依赖。

用法：
    python rapoo_device_map.py                      # 抓取线上最新，输出到 stdout
    python rapoo_device_map.py -o rapoo_devices.json
    python rapoo_device_map.py --url https://hub.rapoo.cn/ -o out.json
    python rapoo_device_map.py --file ./index-xxx.js -o out.json   # 离线解析本地 JS
    python rapoo_device_map.py --key-format dec      # 键用十进制（默认十六进制）
"""

from __future__ import annotations

import argparse
import datetime
import gzip
import io
import json
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from collections import Counter
from typing import Iterable

DEFAULT_INDEX_URL = "https://hub.rapoo.cn/"
USER_AGENT = (
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
    "(KHTML, like Gecko) Chrome/124.0 Safari/537.36"
)

RAPOO_VENDOR_IDS = [0x24AE, 0x2E3C]

KNOWN_DEVICE_TYPES = {
    "mouse",
    "keyboard",
    "base",
    "dock",
    "headset",
    "gamepad",
    "joystick",
    "mousepad",
    "receiver",
    "keyboard_base",
}


# --------------------------------------------------------------------------
# 网络
# --------------------------------------------------------------------------
def http_get(url: str, timeout: float = 30.0, retries: int = 3) -> str:
    """GET 文本内容，支持 gzip、重试与 UA。"""
    last_err: Exception | None = None
    for attempt in range(1, retries + 1):
        try:
            req = urllib.request.Request(
                url,
                headers={
                    "User-Agent": USER_AGENT,
                    "Accept": "text/html,application/javascript,*/*",
                    "Accept-Encoding": "gzip, identity",
                },
            )
            with urllib.request.urlopen(req, timeout=timeout) as resp:
                raw = resp.read()
                if resp.headers.get("Content-Encoding", "").lower() == "gzip":
                    raw = gzip.GzipFile(fileobj=io.BytesIO(raw)).read()
                return raw.decode("utf-8", errors="replace")
        except (urllib.error.URLError, urllib.error.HTTPError, TimeoutError) as exc:
            last_err = exc
            if attempt < retries:
                time.sleep(0.8 * attempt)
    raise RuntimeError(f"请求失败: {url} ({last_err})")


def discover_bundle_url(index_html: str, index_url: str) -> str:
    """从首页 HTML 中找出主 JS 打包文件地址。"""
    patterns = [
        r'(?:src|href)\s*=\s*["\']([^"\']*assets/js/index-[^"\']+?\.js)["\']',
        r'(?:src|href)\s*=\s*["\']([^"\']*index-[^"\']+?\.js)["\']',
    ]
    for pat in patterns:
        m = re.search(pat, index_html)
        if m:
            return urllib.parse.urljoin(index_url, m.group(1))
    raise RuntimeError("未能在首页中找到 index-*.js 打包文件")


# --------------------------------------------------------------------------
# 解析
# --------------------------------------------------------------------------
def _brace_end(text: str, start: int) -> int:
    """从 text[start] == '{' 开始，返回配对 '}' 的下标（忽略字符串内括号）。"""
    depth = 0
    i = start
    quote: str | None = None
    escaped = False
    while i < len(text):
        ch = text[i]
        if quote is not None:
            if escaped:
                escaped = False
            elif ch == "\\":
                escaped = True
            elif ch == quote:
                quote = None
        else:
            if ch in ('"', "'"):
                quote = ch
            elif ch == "{":
                depth += 1
            elif ch == "}":
                depth -= 1
                if depth == 0:
                    return i
        i += 1
    return -1


def _first_int(text: str, key: str) -> int | None:
    m = re.search(rf"\b{key}\s*:\s*(\d+)", text)
    return int(m.group(1)) if m else None


def _pid_list(text: str) -> list[int]:
    m = re.search(r"\bproductId\s*:\s*\[([0-9,\s]*)\]", text)
    if m:
        return [int(x) for x in re.findall(r"\d+", m.group(1))]
    m = re.search(r"\bproductId\s*:\s*(\d+)", text)
    return [int(m.group(1))] if m else []


def extract_manifests(js: str) -> list[dict]:
    """从打包 JS 中提取所有设备 manifest 的关键字段。"""
    model_re = re.compile(r'model:"([^"\\]*)"')
    matches = list(model_re.finditer(js))
    rows: list[dict] = []

    for idx, m in enumerate(matches):
        name = m.group(1).strip()
        if not name:
            continue

        window_end = (
            matches[idx + 1].start() if idx + 1 < len(matches) else min(len(js), m.end() + 1200)
        )
        window = js[m.end() : window_end]

        if "hardware:{" not in window or "vendorId:" not in window:
            continue

        hw_pos = window.index("hardware:{")
        pre = window[:hw_pos]

        vid = _first_int(window, "vendorId")
        pids = _pid_list(window)
        if vid is None or not pids:
            continue

        type_m = re.search(r'\btype\s*:\s*"([^"]+)"', pre)
        if type_m:
            device_type = type_m.group(1)
        else:
            cand = [
                t
                for t in re.findall(r'\btype\s*:\s*"([^"]+)"', window)
                if t not in ("default", "update")
            ]
            device_type = cand[0] if cand else "unknown"

        ver_m = re.search(r'\bversion\s*:\s*"([^"]+)"', pre)
        transport_m = re.search(r'\btransport\s*:\s*"([^"]+)"', window)
        dongle_m = re.search(r'\bdongleModel\s*:\s*"([^"]+)"', pre)
        main_m = re.search(r'\bmainControl\s*:\s*"([^"]+)"', pre)
        driver_m = re.search(r'\bdriver\s*:\s*"([^"]+)"', window)

        head = js[max(0, m.start() - 300) : m.start()]
        ch_m = re.search(r"\bchannel\s*:\s*\[([^\]]*)\]", head)
        channels = re.findall(r'"([^"]+)"', ch_m.group(1)) if ch_m else []

        rows.append(
            {
                "name": name,
                "deviceType": device_type,
                "driver": driver_m.group(1) if driver_m else None,
                "version": ver_m.group(1) if ver_m else None,
                "vendorId": vid,
                "productIds": pids,
                "transport": (transport_m.group(1) if transport_m else "").upper() or None,
                "dongleModel": dongle_m.group(1) if dongle_m else None,
                "mainControl": main_m.group(1) if main_m else None,
                "channels": channels,
            }
        )

    return rows


# --------------------------------------------------------------------------
# 转换
# --------------------------------------------------------------------------
def resolve_mode(row: dict) -> str:
    """连接方式：BLE 直连记为 BLE；有接收器记为 2.4G；否则记为 USB。"""
    transport = (row.get("transport") or "").upper()
    if transport == "BLE":
        return "BLE"
    if row.get("dongleModel"):
        return "2.4G"
    return "USB"


def make_key(vid: int, pid: int, key_format: str) -> str:
    if key_format == "dec":
        return f"{vid}:{pid}"
    return f"{vid:04X}:{pid:04X}"


def build_output(rows: Iterable[dict], *, key_format: str, source: dict) -> dict:
    devices: dict[str, dict] = {}
    collisions: list[str] = []

    for row in rows:
        if row["deviceType"] == "base":
            continue
        mode = resolve_mode(row)
        for pid in row["productIds"]:
            key = make_key(row["vendorId"], pid, key_format)
            entry = {
                "name": row["name"],
                "deviceType": row["deviceType"],
                "mode": mode,
            }
            if row.get("driver"):
                entry["driver"] = row["driver"]
            if row.get("channels"):
                entry["channels"] = row["channels"]

            if key in devices:
                collisions.append(key)
                if devices[key].get("mode") == "USB" and mode in ("2.4G", "BLE"):
                    devices[key] = entry
                continue
            devices[key] = entry

    if collisions:
        uniq = sorted(set(collisions))
        print(
            f"[warn] 发现 {len(uniq)} 个重复设备标识，已合并: {', '.join(uniq[:5])}"
            + (" ..." if len(uniq) > 5 else ""),
            file=sys.stderr,
        )

    config_version = fetch_stamp()

    return {
        "config_version": config_version,
        "source": source,
        "devices": dict(sorted(devices.items())),
    }


def fetch_stamp() -> str:
    """抓取时刻（UTC，精确到小时），作为 config_version。"""
    return datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H")


def derive_web_version(bundle_url: str) -> str:
    m = re.search(r"index-([A-Za-z0-9_-]+)\.js", bundle_url)
    return m.group(1) if m else ""


# --------------------------------------------------------------------------
# 主流程
# --------------------------------------------------------------------------
def run(args: argparse.Namespace) -> dict:
    if args.file:
        js = open(args.file, "r", encoding="utf-8", errors="replace").read()
        source = {"file": args.file, "url": "", "web_version": derive_web_version(args.file)}
        bundle_url = args.file
    else:
        index_url = args.url
        index_html = http_get(index_url, timeout=args.timeout)
        bundle_url = discover_bundle_url(index_html, index_url)
        if not args.quiet:
            print(f"[info] 打包文件: {bundle_url}", file=sys.stderr)
        js = http_get(bundle_url, timeout=args.timeout)
        source = {"file": "", "url": index_url, "web_version": derive_web_version(bundle_url)}

    rows = extract_manifests(js)
    total = len(rows)
    rows = [r for r in rows if r["vendorId"] in RAPOO_VENDOR_IDS]
    if not args.quiet and len(rows) != total:
        allow = ", ".join(f"{v:04X}" for v in RAPOO_VENDOR_IDS)
        print(
            f"[info] vendorId 过滤：丢弃 {total - len(rows)} 个非 {allow} 设备，"
            f"保留 {len(rows)} 个",
            file=sys.stderr,
        )
    if args.channel:
        rows = [r for r in rows if args.channel in (r.get("channels") or [])]
    if not rows:
        raise RuntimeError("未解析到任何设备 manifest，页面结构可能已变更")

    if not args.quiet:
        types = Counter(r["deviceType"] for r in rows)
        print(f"[info] 解析设备 {len(rows)} 个，类型分布: {dict(types)}", file=sys.stderr)

    return build_output(rows, key_format=args.key_format, source=source)


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    p = argparse.ArgumentParser(description="抓取并转换 RAPOO HUB 设备映射表")
    p.add_argument("--url", default=DEFAULT_INDEX_URL, help=f"首页地址（默认 {DEFAULT_INDEX_URL}）")
    p.add_argument("--file", help="离线解析本地打包 JS（如 index-xxx.js），与 --url 二选一")
    p.add_argument("-o", "--output", help="输出 JSON 文件路径（默认打印到 stdout）")
    p.add_argument(
        "--key-format",
        choices=["hex", "dec"],
        default="hex",
        help="设备标识键格式：hex 如 24AE:4814（默认），dec 如 9390:18452",
    )
    p.add_argument("--channel", help="仅保留包含指定 channel 的设备，如 china_game")
    p.add_argument("--timeout", type=float, default=30.0, help="请求超时秒数（默认 30）")
    p.add_argument("--compact", action="store_true", help="压缩输出（默认缩进美化）")
    p.add_argument("-q", "--quiet", action="store_true", help="不输出进度信息")
    return p.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    try:
        result = run(args)
    except Exception as exc:  # noqa: BLE001
        print(f"[error] {exc}", file=sys.stderr)
        return 1

    text = json.dumps(
        result,
        ensure_ascii=False,
        indent=None if args.compact else 2,
        separators=(",", ":") if args.compact else None,
    )
    if args.output:
        with open(args.output, "w", encoding="utf-8", newline="\n") as fh:
            fh.write(text)
            fh.write("\n")
        if not args.quiet:
            print(
                f"[info] 已写入 {args.output}（{len(result['devices'])} 个设备标识）",
                file=sys.stderr,
            )
    else:
        print(text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
