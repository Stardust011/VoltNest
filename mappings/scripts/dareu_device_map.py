#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
dareu_device_map.py

从 DAREU 网页配置工具（https://dr.dareu.com/）自动抓取并解析设备映射表，
转换为精简 JSON：{config_version, source, devices}。

数据来源与判定链（全部来自站点静态资源，无需硬件）：
  1. index.html              -> 版本号（脚本查询串 ?v=1.3.26）
  2. js/global.js            -> DEVICE_FILTERS 的 vendorId(0x260D)
                                AVAILABLE_PID_LIST（支持的 PID 白名单 + 行尾注释）
                                JM_WIRELESS_MAP（Dongle PID -> 配对设备 PID/CID/MID）
  3. products/<pid>/<pid>.json -> Device.{PID,Model,ProductName,Type,FwType,HasBattery}

字段语义（依据站点 js/device.js 与实测 JSON）：
  Type: 0=mouse  1=keyboard  2=2.4G 接收器（device.js:419-489 按 Type 分派 Dongle/Keyboard/Mouse 类）
  FwType: 0=自研 1=海思 2=匠盟 3=群峰 4=合盈OHID 5=合盈v1 6=自研磁轴 7=自研Dongle配对合盈 8=匠盟键盘
  WirelessFlag: 0x00=有线 0x01=2.4G 0x02=蓝牙（运行时由 HID descriptor / Dongle 配对态推导，JSON 中无此字段）

输出字段：
  name          对外可读型号（ProductName，如 Z82 / A980 / CK98 PRO RT）
  internalModel 厂家内部型号（Model，如 TK51A / TM238），与 name 相同则省略
  deviceType    mouse / keyboard / receiver
  mode          2.4G / USB / BLE
  其余可选：modes(能力集) fwType hasBattery note productName pairs

仅使用 Python 标准库，无第三方依赖。

用法：
    python dareu_device_map.py                          # 抓取线上最新，输出到 stdout
    python dareu_device_map.py -o dareu_devices.json
    python dareu_device_map.py --url https://dr.dareu.com/ -o out.json
    python dareu_device_map.py --cache-dir .cache -o out.json   # 缓存 global.js 与 products
    python dareu_device_map.py --cache-dir .cache --offline     # 完全离线，仅用缓存
    python dareu_device_map.py --key-format dec         # 键用十进制（默认十六进制）
    python dareu_device_map.py --include-receiver       # 保留 2.4G 接收器条目（默认过滤）
"""

from __future__ import annotations

import argparse
import gzip
import hashlib
import io
import json
import os
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from typing import Iterable

DEFAULT_BASE_URL = "https://dr.dareu.com/"
# 输出数据规范版本（与站点构建号无关，站点版本走 source.web_version）
CONFIG_VERSION = "1.0.0"
USER_AGENT = (
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
    "(KHTML, like Gecko) Chrome/124.0 Safari/537.36"
)

# products/<pid>.json 中 Device.Type 的取值含义（device.js CreateHidDevice 分派 + 实测）
TYPE_MAP = {0: "mouse", 1: "keyboard", 2: "receiver"}


# --------------------------------------------------------------------------
# 网络与缓存
# --------------------------------------------------------------------------
class Fetcher:
    """带重试、gzip、可选磁盘缓存的取数器。"""

    def __init__(self, timeout: float = 20.0, retries: int = 3,
                 cache_dir: str | None = None, offline: bool = False) -> None:
        self.timeout = timeout
        self.retries = retries
        self.cache_dir = cache_dir
        self.offline = offline
        if cache_dir:
            os.makedirs(cache_dir, exist_ok=True)

    def _cache_path(self, url: str) -> str:
        digest = hashlib.sha1(url.encode("utf-8")).hexdigest()[:16]
        tail = urllib.parse.urlparse(url).path.rstrip("/").split("/")[-1] or "root"
        safe = re.sub(r"[^A-Za-z0-9._-]", "_", tail)
        return os.path.join(self.cache_dir or "", f"{safe}.{digest}.bin")

    def get(self, url: str) -> bytes | None:
        """返回响应字节；HTTP 404 返回 None；其它失败抛异常。"""
        path = self._cache_path(url) if self.cache_dir else None
        if path and os.path.exists(path):
            with open(path, "rb") as fh:
                return fh.read()
        if self.offline:
            return None

        last_err: Exception | None = None
        for attempt in range(1, self.retries + 1):
            try:
                req = urllib.request.Request(
                    url,
                    headers={
                        "User-Agent": USER_AGENT,
                        "Accept": "text/html,application/javascript,application/json,*/*",
                        "Accept-Encoding": "gzip, identity",
                        "Cache-Control": "no-cache",
                    },
                )
                with urllib.request.urlopen(req, timeout=self.timeout) as resp:
                    raw = resp.read()
                    if resp.headers.get("Content-Encoding", "").lower() == "gzip":
                        raw = gzip.GzipFile(fileobj=io.BytesIO(raw)).read()
                if path:
                    with open(path, "wb") as fh:
                        fh.write(raw)
                return raw
            except urllib.error.HTTPError as exc:
                if exc.code in (403, 404, 410):
                    return None
                last_err = exc
            except (urllib.error.URLError, TimeoutError, OSError) as exc:
                last_err = exc
            if attempt < self.retries:
                time.sleep(0.5 * attempt)
        raise RuntimeError(f"请求失败: {url} ({last_err})")

    def text(self, url: str) -> str | None:
        raw = self.get(url)
        return None if raw is None else raw.decode("utf-8", errors="replace")

    def json(self, url: str) -> dict | None:
        raw = self.get(url)
        if raw is None:
            return None
        try:
            return json.loads(raw.decode("utf-8", errors="replace"))
        except json.JSONDecodeError:
            return None


# --------------------------------------------------------------------------
# JS 子集解析工具
# --------------------------------------------------------------------------
def strip_js_comments(src: str) -> str:
    """去掉 // 与 /* */ 注释，保留字符串字面量原样（含 // 的 URL 不被误伤）。"""
    out: list[str] = []
    i, n, quote, esc = 0, len(src), "", False
    while i < n:
        ch = src[i]
        if quote:
            out.append(ch)
            if esc:
                esc = False
            elif ch == "\\":
                esc = True
            elif ch == quote:
                quote = ""
            i += 1
            continue
        if ch in ('"', "'", "`"):
            quote = ch
            out.append(ch)
            i += 1
            continue
        if ch == "/" and i + 1 < n and src[i + 1] == "/":
            while i < n and src[i] != "\n":
                i += 1
            continue
        if ch == "/" and i + 1 < n and src[i + 1] == "*":
            j = src.find("*/", i + 2)
            i = n if j < 0 else j + 2
            continue
        out.append(ch)
        i += 1
    return "".join(out)


def match_pair(text: str, start: int, open_ch: str, close_ch: str) -> int:
    """text[start] == open_ch，返回配对 close_ch 的下标，失败返回 -1。"""
    depth = 0
    i, n = start, len(text)
    quote, esc = "", False
    while i < n:
        ch = text[i]
        if quote:
            if esc:
                esc = False
            elif ch == "\\":
                esc = True
            elif ch == quote:
                quote = ""
        elif ch in ('"', "'", "`"):
            quote = ch
        elif ch == open_ch:
            depth += 1
        elif ch == close_ch:
            depth -= 1
            if depth == 0:
                return i
        i += 1
    return -1


def split_top_level_objects(body: str) -> list[str]:
    """把 '[...]' 内部文本按顶层 '{...}' 切开。"""
    objs: list[str] = []
    i, n = 0, len(body)
    while i < n:
        if body[i] == "{":
            end = match_pair(body, i, "{", "}")
            if end < 0:
                break
            objs.append(body[i:end + 1])
            i = end + 1
        else:
            i += 1
    return objs


def _hex(text: str, key: str) -> int | None:
    m = re.search(rf"\b{key}\s*:\s*0x([0-9A-Fa-f]+)", text)
    return int(m.group(1), 16) if m else None


def parse_vendor_id(js_clean: str) -> int:
    """取 DEVICE_FILTERS 中第一个 vendorId，兜底 0x260D。"""
    m = re.search(r"DEVICE_FILTERS\s*=\s*\[", js_clean)
    if m:
        vid = _hex(js_clean[m.start(): m.start() + 800], "vendorId")
        if vid:
            return vid
    return 0x260D


def parse_pid_list(js_raw: str) -> dict[int, str]:
    """解析 AVAILABLE_PID_LIST -> {pid: 行尾注释}。

    只收未被注释掉的 PID —— 站点用 `//0x1117, // 无样品调试` 这种写法临时下线机型。
    """
    key = "AVAILABLE_PID_LIST"
    a = js_raw.find(key)
    if a < 0:
        raise RuntimeError("global.js 中未找到 AVAILABLE_PID_LIST")
    b = js_raw.find("[", a)
    if b < 0:
        raise RuntimeError("AVAILABLE_PID_LIST 结构不完整")
    end = match_pair(js_raw, b, "[", "]")
    if end < 0:
        raise RuntimeError("AVAILABLE_PID_LIST 未正确闭合")
    body = js_raw[b + 1:end]

    result: dict[int, str] = {}
    for line in body.splitlines():
        m = re.search(r"0x([0-9A-Fa-f]{1,4})", line)
        if not m:
            continue
        cmt_at = line.find("//")
        if cmt_at >= 0 and m.start() > cmt_at:   # PID 落在注释里 -> 未启用
            continue
        pid = int(m.group(1), 16)
        comment = line[cmt_at + 2:].strip() if cmt_at >= 0 else ""
        result.setdefault(pid, comment)
    return result


def parse_wireless_map(js_clean: str) -> dict[int, dict]:
    """解析 JM_WIRELESS_MAP -> {dongle_pid: {dongleType, pairs: [{pid, cid, mid, type}]}}。"""
    a = js_clean.find("JM_WIRELESS_MAP")
    if a < 0:
        return {}
    b = js_clean.find("[", a)
    end = match_pair(js_clean, b, "[", "]")
    if b < 0 or end < 0:
        return {}
    body = js_clean[b + 1:end]

    result: dict[int, dict] = {}
    for obj in split_top_level_objects(body):
        pid = _hex(obj, "PID")
        if pid is None:
            continue
        entry = {"dongleType": _hex(obj, "Type"), "pairs": []}
        p = obj.find("PairInfo")
        if p >= 0:
            ab = obj.find("[", p)
            ae = match_pair(obj, ab, "[", "]") if ab >= 0 else -1
            if ab >= 0 and ae > ab:
                for item in split_top_level_objects(obj[ab + 1:ae]):
                    pair_pid = _hex(item, "PID")
                    if pair_pid is None:
                        continue
                    entry["pairs"].append({
                        "pid": pair_pid,
                        "cid": _hex(item, "CID"),
                        "mid": _hex(item, "MID"),
                        "type": _hex(item, "Type"),
                    })
        result.setdefault(pid, entry)
    return result


def discover_web_version(index_html: str) -> str:
    """从 index.html 的静态资源查询串中取版本号，如 ?v=1.3.26。"""
    for pat in (r"\?v=([0-9][0-9A-Za-z._-]*)", r"version\s*[:=]\s*[\"']([0-9][0-9A-Za-z._-]*)"):
        m = re.search(pat, index_html)
        if m:
            return m.group(1)
    return ""


# --------------------------------------------------------------------------
# 抓取产品元数据
# --------------------------------------------------------------------------
def product_url(base: str, pid: int) -> str:
    h = f"{pid:04x}"
    return urllib.parse.urljoin(base, f"products/{h}/{h}.json")


def fetch_products(fetcher: Fetcher, base: str, pids: Iterable[int],
                   workers: int = 8) -> tuple[dict[int, dict], list[int]]:
    pids = sorted(set(pids))

    def one(pid: int):
        data = fetcher.json(product_url(base, pid))
        dev = (data or {}).get("Device") if isinstance(data, dict) else None
        return pid, dev if isinstance(dev, dict) else None

    with ThreadPoolExecutor(max_workers=max(1, workers)) as pool:
        pairs = list(pool.map(one, pids))

    metas = {pid: dev for pid, dev in pairs if dev}
    missing = [pid for pid, dev in pairs if not dev]
    return metas, missing


# --------------------------------------------------------------------------
# 转换
# --------------------------------------------------------------------------
def guess_type(comment: str, model: str) -> str:
    """元数据缺失时，用 PID 注释 / 型号前缀兜底推断设备类型。"""
    blob = f"{comment} {model}".lower()
    if "dongle" in blob or "receiver" in blob or "接收器" in blob:
        return "receiver"
    m = re.match(r"\s*(TK|TM|KB|MS)", model.upper() or comment.upper())
    if m:
        return {"TK": "keyboard", "KB": "keyboard", "TM": "mouse", "MS": "mouse"}[m.group(1)]
    if re.search(r"\bTK\d", comment.upper()):
        return "keyboard"
    if re.search(r"\bTM\d", comment.upper()):
        return "mouse"
    return "unknown"


def derive_modes(pid: int, device_type: str, comment: str,
                 dongle_map: dict[int, dict], pair_pids: set[int],
                 dongle_pid: int | None) -> set[str]:
    """推导该 PID 支持的连接方式集合。"""
    modes: set[str] = {"2.4G"} if device_type == "receiver" else {"USB"}

    # JM_WIRELESS_MAP 配对表 或 白名单注释里的 "Dongle: 0xXXXX"
    if pid in dongle_map or pid in pair_pids or dongle_pid is not None:
        modes.add("2.4G")

    # 注释里的“三模 / 蓝牙”是站点内唯一可静态获得的 BLE 线索
    if "三模" in comment:
        modes.update({"USB", "2.4G", "BLE"})
    elif re.search(r"蓝牙|BLE", comment, re.IGNORECASE):
        modes.add("BLE")

    return modes


def pick_mode(modes: set[str], priority: str) -> str:
    if priority == "wired":
        order = ("USB", "2.4G", "BLE")
    else:
        # 三模设备优先记 2.4G（站点里 BLE 只是同 PID 的另一条运行时链路）
        order = ("2.4G", "BLE", "USB")
    for m in order:
        if m in modes:
            return m
    return "USB"


def clean_comment(comment: str) -> str:
    c = re.sub(r"^dongle\s*[:：]\s*", "", comment.strip(), flags=re.IGNORECASE)
    c = re.sub(r"\s*-\s*(有线|无线)$", "", c).strip()
    return c


def collect_dongle_hints(comments: dict[int, str]) -> dict[int, int]:
    """从白名单注释 "Dongle: 0x1092" 抽出 {设备PID: 接收器PID}。"""
    hints: dict[int, int] = {}
    for pid, comment in comments.items():
        m = re.search(r"Dongle\s*[:：]\s*0x([0-9A-Fa-f]{2,4})", comment, re.IGNORECASE)
        if m:
            hints[pid] = int(m.group(1), 16)
    return hints


def split_fallback_name(comment: str) -> tuple[str, str]:
    """元数据缺失时把注释拆成 (型号, 别名)，如 "TK51A - Z82" -> ("TK51A", "Z82")。"""
    c = clean_comment(comment)
    if " - " in c:
        head, _, tail = c.partition(" - ")
        return head.strip(), tail.strip()
    return c, ""


def resolve_readable(stem: str, model_index: dict[str, str]) -> str:
    """把只含内部型号的串（如 "TK52L三模"）换成可读型号；有歧义则不换。"""
    up = stem.upper()
    hits = [ (k, v) for k, v in model_index.items()
             if up.startswith(k.upper())
             and (len(up) == len(k) or not up[len(k)].isalnum()) ]
    if not hits:
        return stem
    longest = max(len(k) for k, _ in hits)
    cands = {v for k, v in hits if len(k) == longest}
    return cands.pop() if len(cands) == 1 else stem


def receiver_name(pid: int, model: str, comment: str,
                  dongle_map: dict[int, dict], readable: dict[int, str],
                  model_index: dict[str, str] | None = None) -> str:
    """接收器的可读名：优先用其配对设备的可读型号，其次从注释里剥掉 Dongle 字样。"""
    pairs = dongle_map.get(pid, {}).get("pairs", [])
    paired = sorted({readable.get(p["pid"], "") for p in pairs} - {""})
    if paired:
        head = " / ".join(paired[:2]) + ("等" if len(paired) > 2 else "")
        return f"{head} 接收器"

    stem = re.sub(r"\b(dongle|receiver|reciver|接收器)\b", " ", clean_comment(comment),
                  flags=re.IGNORECASE)
    stem = re.sub(r"\s+", " ", stem).strip(" -/")
    generic = {"common", "wireless", "2.4g", "usb", "dareu", "receiver", "reciver", ""}
    if stem and stem.lower() not in generic:
        if model_index:
            stem = resolve_readable(stem, model_index)
        return f"{stem} 接收器"

    base = re.sub(r"\s*(dongle|receiver|reciver|接收器)\s*$", "", model or "",
                  flags=re.IGNORECASE).strip()
    if base and base.upper() != (model or "").upper():
        return f"{base} 接收器"
    return model or f"DAREU_{pid:04X}"


def build_output(*, metas: dict[int, dict], comments: dict[int, str],
                 dongle_map: dict[int, dict], vendor_id: int,
                 key_format: str, mode_priority: str, include_receiver: bool,
                 config_version: str, source: dict) -> tuple[dict, Counter]:
    hints = collect_dongle_hints(comments)

    # 用 "Dongle: 0xXXXX" 注释补全 JM_WIRELESS_MAP 之外的配对关系
    for dev_pid, dongle_pid in hints.items():
        entry = dongle_map.setdefault(dongle_pid, {"dongleType": None, "pairs": []})
        if all(p["pid"] != dev_pid for p in entry["pairs"]):
            entry["pairs"].append({"pid": dev_pid, "cid": None, "mid": None, "type": None})

    pair_pids = {p["pid"] for e in dongle_map.values() for p in e["pairs"]}
    all_pids = set(comments) | set(metas) | set(dongle_map) | pair_pids

    # 第一遍：整理每个 PID 的型号信息
    # 站点约定：ProductName 是对外可读型号（如 Z82 / A980），Model 是厂家内部型号（如 TK51A / TM238）
    info: dict[int, dict] = {}
    for pid in all_pids:
        meta = metas.get(pid) or {}
        model = str(meta.get("Model") or "").strip()
        product = str(meta.get("ProductName") or "").strip()
        comment = comments.get(pid, "")
        raw_type = meta.get("Type")
        device_type = TYPE_MAP.get(raw_type) if isinstance(raw_type, int) else None
        if device_type is None:
            device_type = guess_type(comment, model)

        internal, alias = (model, "") if model else split_fallback_name(comment)
        internal = internal or ""
        # 接收器的 ProductName 多是 "USB 2.4G Receiver" 这类通用串，不当作可读型号
        readable = "" if device_type == "receiver" else (product or alias)
        info[pid] = {
            "meta": meta, "comment": comment, "rawType": raw_type,
            "deviceType": device_type, "model": internal,
            "product": product, "readable": readable,
        }

    readable_names = {pid: it["readable"] for pid, it in info.items() if it["readable"]}

    # 内部型号 -> 可读型号（仅保留一对一映射，用于给只留内部型号的备注消歧）
    seen: dict[str, set[str]] = {}
    for it in info.values():
        if it["deviceType"] == "receiver" or not it["model"] or not it["readable"]:
            continue
        seen.setdefault(it["model"], set()).add(it["readable"])
    model_index = {m: next(iter(r)) for m, r in seen.items() if len(r) == 1}

    devices: dict[str, dict] = {}
    stats: Counter = Counter()

    for pid in sorted(all_pids):
        it = info[pid]
        meta, comment = it["meta"], it["comment"]
        device_type = it["deviceType"]

        if device_type == "receiver" and not include_receiver:
            stats["skipped_receiver"] += 1
            continue

        # name = 可读型号；internalModel = 厂家内部型号
        name = it["readable"]
        if not name:
            if device_type == "receiver":
                name = receiver_name(pid, it["model"], comment, dongle_map,
                                     readable_names, model_index)
            else:
                name, alias = split_fallback_name(comment)
                name = name or it["model"] or f"DAREU_{pid:04X}"
                it["readable"] = name

        modes = derive_modes(pid, device_type, comment, dongle_map, pair_pids,
                             hints.get(pid))
        entry: dict = {
            "name": name,
            "deviceType": device_type,
            "mode": pick_mode(modes, mode_priority),
        }

        # 附加信息（可选字段，取不到即省略）
        if it["model"] and it["model"] != name:
            entry["internalModel"] = it["model"]
        if device_type != "receiver" and it["product"] and it["product"] != name:
            entry["productName"] = it["product"]
        packed = sorted(modes, key=("USB", "2.4G", "BLE").index)
        if len(packed) > 1:
            entry["modes"] = packed
        if meta.get("FwType") is not None:
            entry["fwType"] = meta.get("FwType")
        if isinstance(meta.get("HasBattery"), bool):
            entry["hasBattery"] = meta["HasBattery"]
        if comment:
            entry["note"] = comment

        # 接收器 <-> 设备 双向配对关系
        if pid in dongle_map:
            pairs = sorted({f"{vendor_id:04X}:{p['pid']:04X}" for p in dongle_map[pid]["pairs"]})
            if pairs:
                entry["pairs"] = pairs

        key = f"{vendor_id}:{pid}" if key_format == "dec" else f"{vendor_id:04X}:{pid:04X}"
        if key in devices:
            stats["collision"] += 1
            continue
        devices[key] = entry
        stats[f"type:{device_type}"] += 1

    return {
        "config_version": config_version or "1.0.0",
        "source": source,
        "devices": dict(sorted(devices.items())),
    }, stats


# --------------------------------------------------------------------------
# 主流程
# --------------------------------------------------------------------------
def run(args: argparse.Namespace) -> dict:
    base = args.url if args.url.endswith("/") else args.url + "/"
    fetcher = Fetcher(args.timeout, args.retries, args.cache_dir, args.offline)

    index_html = fetcher.text(urllib.parse.urljoin(base, "index.html"))
    if index_html is None and not args.offline:
        raise RuntimeError("未能获取站点首页，请检查网络或 --url")
    web_version = discover_web_version(index_html or "")

    js_url = urllib.parse.urljoin(base, "js/global.js")
    if web_version:
        js_url += f"?v={web_version}"
    js_raw = fetcher.text(js_url) or fetcher.text(urllib.parse.urljoin(base, "js/global.js"))
    if not js_raw:
        raise RuntimeError("未能获取 js/global.js（站点结构可能已变更，或缓存缺失）")
    js_clean = strip_js_comments(js_raw)

    vendor_id = args.vid or parse_vendor_id(js_clean)
    comments = parse_pid_list(js_raw)
    dongle_map = parse_wireless_map(js_clean)
    pair_pids = {p["pid"] for e in dongle_map.values() for p in e["pairs"]}
    hints = collect_dongle_hints(comments)

    if not args.quiet:
        print(f"[info] web_version={web_version or '-'} vendorId=0x{vendor_id:04X} "
              f"PID白名单={len(comments)} 无线配对表={len(dongle_map)} "
              f"Dongle注释={len(hints)}", file=sys.stderr)

    all_pids = set(comments) | set(dongle_map) | pair_pids | set(hints.values())
    metas, missing = fetch_products(fetcher, base, all_pids, args.workers)
    if not metas:
        raise RuntimeError("未获取到任何 products/*.json，无法生成映射表")

    if not args.quiet:
        print(f"[info] 产品元数据 {len(metas)}/{len(all_pids)} 成功"
              + (f"，缺失 {len(missing)} 个（用注释兜底）" if missing else ""), file=sys.stderr)
        if missing:
            print("[warn] 缺失 PID: " + ", ".join(f"{p:04X}" for p in sorted(missing)),
                  file=sys.stderr)

    source = {
        "file": args.cache_dir if (args.offline and args.cache_dir) else "",
        "url": base,
        "web_version": web_version,
    }
    result, stats = build_output(
        metas=metas, comments=comments, dongle_map=dongle_map,
        vendor_id=vendor_id, key_format=args.key_format,
        mode_priority=args.mode_priority, include_receiver=args.include_receiver,
        config_version=args.config_version, source=source,
    )

    if not args.quiet:
        dist = {k.split(":", 1)[1]: v for k, v in stats.items() if k.startswith("type:")}
        print(f"[info] 输出 {len(result['devices'])} 个设备标识，类型分布 {dist}", file=sys.stderr)
        if stats["collision"]:
            print(f"[warn] 键冲突 {stats['collision']} 次（已保留先到者）", file=sys.stderr)
    return result


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    p = argparse.ArgumentParser(description="抓取并转换 DAREU 网页配置工具的设备映射表")
    p.add_argument("--url", default=DEFAULT_BASE_URL, help=f"站点根地址（默认 {DEFAULT_BASE_URL}）")
    p.add_argument("-o", "--output", help="输出 JSON 路径（默认打印到 stdout）")
    p.add_argument("--key-format", choices=["hex", "dec"], default="hex",
                   help="设备标识键格式：hex 如 260D:0030（默认），dec 如 9739:48")
    p.add_argument("--vid", type=lambda s: int(s, 0), help="覆盖 vendorId（默认从 DEVICE_FILTERS 读取）")
    p.add_argument("--config-version", default=CONFIG_VERSION,
                   help=f"输出数据规范版本（默认 {CONFIG_VERSION}；站点构建号写在 source.web_version）")
    p.add_argument("--mode-priority", choices=["wireless", "wired"], default="wireless",
                   help="多模设备 mode 取值优先级：wireless=2.4G>BLE>USB（默认），wired=USB 优先")
    p.add_argument("--include-receiver", action="store_true",
                   help="保留 2.4G 接收器（Type=2）条目；默认过滤掉")
    p.add_argument("--no-receiver", dest="include_receiver", action="store_false", default=False,
                   help="（默认行为）过滤接收器条目，保留此开关以兼容旧命令")
    p.set_defaults(include_receiver=False)
    p.add_argument("--cache-dir", help="缓存目录（global.js 与 products/*.json 落盘复用）")
    p.add_argument("--offline", action="store_true", help="只用缓存，不发起网络请求")
    p.add_argument("--workers", type=int, default=8, help="并发抓取数（默认 8）")
    p.add_argument("--timeout", type=float, default=20.0, help="单请求超时秒数（默认 20）")
    p.add_argument("--retries", type=int, default=3, help="失败重试次数（默认 3）")
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

    text = json.dumps(result, ensure_ascii=False,
                      indent=None if args.compact else 2,
                      separators=(",", ":") if args.compact else None)
    if args.output:
        with open(args.output, "w", encoding="utf-8") as fh:
            fh.write(text)
            fh.write("\n")
        if not args.quiet:
            print(f"[info] 已写入 {args.output}（{len(result['devices'])} 个设备标识）", file=sys.stderr)
    else:
        print(text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
