#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
mchose_device_map.py

抓取 MCHOSE（M HUB 网页驱动）设备映射表，转换为：
{"config_version": str, "source": {file,url,web_version}, "devices": {"VID:PID": {...}}}

数据来源（自动发现，无需硬编码 hash）：
  1. 站点首页 -> /assets/index-*.js、/assets/main-*.js 及其依赖 chunk
     - "MODEL$$$vid$$$pid" 形式的 productKey 三元组（音频/键鼠）
     - 共享 chunk 内按型号聚合的鼠标 pids 表
  2. /cizhou/CZ_SHARED_DATA/asset-manifest.json -> main.<hash>.js
     - _identity{vendorId,productId} + name + type + isWireless（键盘全量）
  3. https://cdn.mchose.com.cn/configCenter/custom/cardList.json
     - identities + desc + webdriverEnum（键盘补充身份）
  4. https://cdn.mchose.com.cn/configCenter/custom/keyboardConfig.json
     - 0xVID_0xPID_[型号名] 形式的键，判断接收器 PID 的连接方式
  5. https://cdn.mchose.com.cn/configCenter/config/global.json
     - webVersion / 配置版本

共享接收器 PID（0x3837_0x3033 / 0x2010 / 0x1015 / 0x1017）的处理：
网页驱动靠 WebHID 上报的 productName 消歧，设备表的 key 写成
"0xVID_0xPID_型号名"，且碰撞 PID 故意不建基键。本脚本对这类键不再丢弃，
而是输出：
    {"name": null, "deviceType": "keyboard", "mode": "2.4G",
     "ambiguous": true, "receiverMatch": "contains", "fallback": "K87S",
     "candidates": [{"name": "K87S", "receiverProductName": "MCHOSE K87S 2.4G"}, ...]}
candidates 顺序与官方白名单一致；receiverMatch/fallback 对应
pickUsefulProductName 的匹配方式与兜底型号（认不出时官方会直接按兜底型号处理）。
非歧义条目若已知接收器名，也会带上 receiverProductName。

仅使用 Python 标准库。

用法：
    python mchose_device_map.py                       # 输出到 stdout
    python mchose_device_map.py -o mchose_devices.json
    python mchose_device_map.py --key-format dec      # 键用十进制（默认十六进制）
    python mchose_device_map.py --pick-name 3837:3033 "MCHOSE G75 V2 2.4G"
"""

from __future__ import annotations

import argparse
import base64
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
import zipfile
from collections import Counter
from typing import Iterable

DEFAULT_SITE_URL = "https://www.mchose.com.cn/"
CONFIG_BASE = "https://cdn.mchose.com.cn/configCenter"
GLOBAL_CONFIG_URL = f"{CONFIG_BASE}/config/global.json"
CARD_LIST_URL = f"{CONFIG_BASE}/custom/cardList.json"
KEYBOARD_CONFIG_URL = f"{CONFIG_BASE}/custom/keyboardConfig.json"
CZ_MANIFEST_PATH = "/cizhou/CZ_SHARED_DATA/asset-manifest.json?v=1"
USER_AGENT = (
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
    "(KHTML, like Gecko) Chrome/124.0 Safari/537.36"
)

# 打包代码中的 deviceType 枚举名 -> 输出取值
ENUM_TYPE_MAP = {
    "MOUSE": "mouse",
    "KEYBOARD": "keyboard",
    "HEADSET": "headset",
    "HEADSET_DOCK": "headset",
    "SPEAKER": "speaker",
    "DOCK": "dock",
    "GAMEPAD": "gamepad",
}

# 型号名关键字 -> 设备类型（未能从枚举取到类型时兜底）
NAME_TYPE_HINTS = [
    ("HEADSET", "headset"),
    ("SPEAKER", "speaker"),
    ("DOCK", "dock"),
]

KEYBOARD_NAME_RE = re.compile(r"^(Ace|Jet|Mix|God|ZERO|K\d|G\d{2,3}|GX|Z75|UT98|X75)", re.I)

DONGLE_PIDS = {4106, 4107, 4128, 4129, 8219, 16401, 0x1015, 0x1017}

# --------------------------------------------------------------------------
# 共享接收器 PID 消歧规则
# --------------------------------------------------------------------------
# 网页驱动不按 VID:PID 认型号：VID:PID 只是第一层粗筛，真正区分靠 WebHID 上报的
# productName 字符串，设备表把这些条目的 key 写成 "0xVID_0xPID_型号名"，并且
# 对碰撞 PID 故意不建基键（"0xVID_0xPID"），所以查表时必须带名字。
#
# 下面的表是打包代码里 pickUsefulProductName（cz_main.js，Le 函数）的逐条移植：
#   - 0x3033 / 0x2010：按顺序做 substring 匹配，全部不中则回落到 K87S 2.4G
#   - 0x1015 / 0x1017：正则 MCHOSE.+2.4G 抽取，名字里含 "Upgrader" 时视为升级模式排除
# 每组两个 PID 是同一物理设备的正常运行 / 升级模式孪生 PID，规则相同。
_KB_RECEIVER_3033 = {
    "match": "contains",
    "order": [
        "MCHOSE K87S 2.4G",
        "MCHOSE Jet 75 2.4G",
        "MCHOSE G87 V2 2.4G",
        "MCHOSE K99 V3 2.4G",
        "MCHOSE G75 V2 2.4G",
    ],
    "fallback": "MCHOSE K87S 2.4G",
}
_KB_RECEIVER_1015 = {
    "match": "regex",
    "pattern": r"MCHOSE.+?2\.4G",
    "exclude": ["Upgrader"],
    "fallback": "MCHOSE G98 V3 2.4G",
}
SHARED_RECEIVER_RULES: dict[tuple[int, int], dict] = {
    (0x3837, 0x3033): _KB_RECEIVER_3033,
    (0x3837, 0x2010): _KB_RECEIVER_3033,
    (0x3837, 0x1015): _KB_RECEIVER_1015,
    (0x3837, 0x1017): _KB_RECEIVER_1015,
}

# 同一 VID:PID 被多个型号登记时，保留哪一条：数字越小越可信
SOURCE_RANK = {"cardList": 0, "cz": 1, "bundle": 2, "mouse-table": 3}


# --------------------------------------------------------------------------
# 网络
# --------------------------------------------------------------------------
def http_get_bytes(url: str, timeout: float = 30.0, retries: int = 3) -> bytes:
    last: Exception | None = None
    for attempt in range(1, retries + 1):
        try:
            req = urllib.request.Request(
                url,
                headers={
                    "User-Agent": USER_AGENT,
                    "Accept": "*/*",
                    "Accept-Encoding": "gzip, identity",
                },
            )
            with urllib.request.urlopen(req, timeout=timeout) as resp:
                raw = resp.read()
                if resp.headers.get("Content-Encoding", "").lower() == "gzip":
                    raw = gzip.GzipFile(fileobj=io.BytesIO(raw)).read()
                return raw
        except (urllib.error.URLError, urllib.error.HTTPError, TimeoutError) as exc:
            last = exc
            if attempt < retries:
                time.sleep(0.8 * attempt)
    raise RuntimeError(f"请求失败: {url} ({last})")


def http_get(url: str, timeout: float = 30.0) -> str:
    return http_get_bytes(url, timeout=timeout).decode("utf-8", errors="replace")


def load_cdn_json(url: str, timeout: float = 30.0):
    obj = json.loads(http_get(url, timeout=timeout))
    if isinstance(obj, dict) and "__compressBase64__" in obj:
        buf = base64.b64decode(obj["__compressBase64__"])
        archive = zipfile.ZipFile(io.BytesIO(buf))
        return json.loads(archive.read(archive.namelist()[0]).decode("utf-8"))
    return obj


# --------------------------------------------------------------------------
# 资源发现
# --------------------------------------------------------------------------
def discover_entry_bundles(html: str, base: str) -> list[str]:
    urls: list[str] = []
    for m in re.finditer(r'<script[^>]+src="([^"]+\.js[^"]*)"', html):
        urls.append(urllib.parse.urljoin(base, m.group(1)))
    for m in re.finditer(r'<link[^>]+href="([^"]+\.js)"', html):
        urls.append(urllib.parse.urljoin(base, m.group(1)))
    return list(dict.fromkeys(urls))


def discover_related_chunks(js: str, bundle_url: str) -> list[str]:
    names = set()
    names.update(re.findall(r'from"\.{1,2}/([A-Za-z0-9_\-.]+\.js)"', js))
    names.update(re.findall(r'import\("\.{1,2}/([A-Za-z0-9_\-.]+\.js)"\)', js))
    names.update(re.findall(r"assets/([A-Za-z0-9_\-.]+\.js)", js))
    return [urllib.parse.urljoin(bundle_url, n) for n in sorted(names)]


def discover_cz_bundle(site_url: str, timeout: float) -> str | None:
    try:
        manifest = json.loads(
            http_get(urllib.parse.urljoin(site_url, CZ_MANIFEST_PATH), timeout=timeout)
        )
        entry = manifest.get("main.js")
        if entry:
            return urllib.parse.urljoin(
                urllib.parse.urljoin(site_url, "/cizhou/CZ_SHARED_DATA/"), entry
            )
    except Exception:
        return None
    return None


# --------------------------------------------------------------------------
# 解析
# --------------------------------------------------------------------------
def _clean_name(name: str) -> str:
    name = re.sub(r"^MCHOSE\s+", "", name.strip(), flags=re.I)
    return name.strip()


WIRELESS_PREFIX_RE = re.compile(r"^2\.4G\s*Wireless[-\s]+", re.I)


def _normalize_product_name(raw: str) -> tuple[str, bool]:
    name = _clean_name(raw)
    wireless = bool(WIRELESS_PREFIX_RE.match(name))
    if wireless:
        name = f"{WIRELESS_PREFIX_RE.sub('', name).replace('_', ' ')} (2.4G)"
    return name, wireless


def _model_fingerprint(name: str) -> str:
    """用于把 "MCHOSE G75 V2 2.4G" 和型号名 "G75 V2" 归一到同一个比较键。"""
    stripped = re.sub(r"\s*\(?2\.4G\)?\s*$", "", _clean_name(name), flags=re.I)
    return re.sub(r"[^a-z0-9]", "", stripped.lower())


def pick_useful_product_name(vid: int, pid: int, product_name: str | None) -> str | None:
    """官方 pickUsefulProductName 的移植：把 HID productName 归一到可查表的型号名。

    非碰撞 PID 原样返回；碰撞 PID 按硬编码白名单消歧，匹配不上时返回兜底型号
    （这与网页驱动行为一致——认不出来也不会报错，而是按主力型号处理）。
    """
    rule = SHARED_RECEIVER_RULES.get((vid, pid))
    raw = product_name or ""
    if rule is None:
        return product_name or None
    if rule["match"] == "contains":
        for candidate in rule["order"]:
            if candidate in raw:
                return candidate
    else:
        if not any(bad in raw for bad in rule.get("exclude", [])):
            m = re.search(rule["pattern"], raw)
            if m:
                return m.group(0)
    return rule["fallback"]


def _rule_canonical_name(vid: int, pid: int, model_name: str) -> str | None:
    """从规则白名单里反查该型号对应的接收器名（抓取缺字段时兜底）。"""
    rule = SHARED_RECEIVER_RULES.get((vid, pid))
    if not rule:
        return None
    target = _model_fingerprint(model_name)
    for candidate in rule.get("order", []):
        if _model_fingerprint(candidate) == target:
            return candidate
    return None


def _desc_to_name(desc: str) -> str:
    name = re.split(r"[（(]", desc, maxsplit=1)[0].strip()
    name = re.sub(r"(磁轴|机械|无线|有线)?(键盘|鼠标|耳机|音箱)\s*$", "", name).strip()
    name = re.sub(r"\s*配置$", "", name).strip()
    return name or desc.strip()


def parse_product_keys(js: str) -> list[dict]:
    rows: list[dict] = []
    for m in re.finditer(r'"([^"$]{2,64})\$\$\$(\d{1,6})\$\$\$(\d{1,6})"', js):
        vid, pid = int(m.group(2)), int(m.group(3))
        name, wireless = _normalize_product_name(m.group(1))
        window = js[m.end() : m.end() + 160]
        enum = re.match(r",deviceType:\w+\.([A-Z_]+)", window)
        rows.append(
            {
                "name": name,
                "deviceType": ENUM_TYPE_MAP.get(enum.group(1)) if enum else None,
                "identities": [(vid, pid)],
                "identity_names": {},
                "isWireless": True if wireless else None,
                "source": "bundle",
            }
        )
    return rows


def _identity_matches(js: str):
    pat = re.compile(
        r'_identity:\{vendorId:(\d+),productId:(\d+),productName:(null|"[^"]*"),key:"([^"]*)"\}'
    )
    return list(pat.finditer(js))


def parse_identity_devices(js: str) -> list[dict]:
    rows: list[dict] = []
    last: dict | None = None
    for m in _identity_matches(js):
        vid, pid = int(m.group(1)), int(m.group(2))
        raw_name = m.group(3)
        receiver_name = raw_name.strip('"') if raw_name and raw_name != "null" else None
        window = js[m.end() : m.end() + 1500]
        head = js[max(0, m.start() - 40) : m.start()]
        in_spread = "spread:[" in head

        name_m = re.search(r'name:"([^"]{1,64})"', window)
        type_m = re.search(r"type:(\d+)", window)
        wired = re.search(r"isWireless:(!0|!1|true|false)", window)

        if in_spread and last is not None:
            last["identities"].append((vid, pid))
            if receiver_name:
                last["identity_names"][(vid, pid)] = receiver_name
            if wired and last["isWireless"] is None:
                last["isWireless"] = wired.group(1) in ("!0", "true")
            continue

        if name_m is None:
            continue
        record = {
            "name": _clean_name(name_m.group(1)),
            "deviceType": "keyboard",
            "identities": [(vid, pid)],
            "identity_names": {(vid, pid): receiver_name} if receiver_name else {},
            "isWireless": (wired.group(1) in ("!0", "true")) if wired else None,
            "source": "cz",
            "typeCode": int(type_m.group(1)) if type_m else None,
        }
        rows.append(record)
        last = record
    return rows


def parse_mouse_table(js: str) -> list[dict]:
    rows: list[dict] = []
    pat = re.compile(r'"([A-Za-z0-9][^"]{1,63})":\{[^{}]{0,400}?pids:\[([^\]]{1,200})\]')
    for m in pat.finditer(js):
        name, pid_blob = m.group(1), m.group(2)
        if not re.search(r"\d", pid_blob):
            continue
        block = m.group(0)
        if "dpi" not in block and "rate" not in block:
            continue
        pids = []
        for token in re.findall(r'"(\d{1,6})"|(\d{1,6})', pid_blob):
            value = int(token[0] or token[1])
            if 0 < value <= 0xFFFF:
                pids.append(value)
        if not pids:
            continue
        rows.append(
            {
                "name": _clean_name(name),
                "deviceType": "mouse",
                "identities": [(0, pid) for pid in pids],
                "identity_names": {},
                "pids": pids,
                "isWireless": None,
                "source": "mouse-table",
            }
        )
    return rows


def parse_card_list(data) -> list[dict]:
    rows: list[dict] = []
    items = data.get("data") if isinstance(data, dict) else None
    for item in items or []:
        desc = str(item.get("desc") or "").strip()
        enum = item.get("webdriverEnum") or {}
        identities = []
        identity_names: dict[tuple[int, int], str] = {}
        for ident in item.get("identities") or []:
            try:
                vid = int(str(ident["vendorId"]), 16)
                pid = int(str(ident["productId"]), 16)
            except (KeyError, ValueError):
                continue
            identities.append((vid, pid))
            product_name = str(ident.get("productName") or "").strip()
            if product_name:
                identity_names[(vid, pid)] = product_name
        if not identities:
            continue
        device_type = "keyboard" if enum.get("deviceType") == 1 else "keyboard"
        rows.append(
            {
                "name": _desc_to_name(desc),
                "deviceType": device_type,
                "identities": identities,
                "identity_names": identity_names,
                "isWireless": None,
                "desc": desc,
                "source": "cardList",
            }
        )
    return rows


def parse_keyboard_config(data) -> dict[tuple[int, int], str]:
    """键盘配置键形如 0x3837_0x3033_MCHOSE G87 V2 2.4G，带 2.4G 后缀即接收器 PID。"""
    overrides: dict[tuple[int, int], str] = {}
    if not isinstance(data, dict):
        return overrides
    for key in data:
        m = re.match(r"^0x([0-9A-Fa-f]{4})_0x([0-9A-Fa-f]{4})(?:_(.+))?$", str(key))
        if not m:
            continue
        vid, pid = int(m.group(1), 16), int(m.group(2), 16)
        suffix = (m.group(3) or "").upper()
        if "2.4G" in suffix:
            overrides[(vid, pid)] = "2.4G"
    return overrides


# --------------------------------------------------------------------------
# 合并
# --------------------------------------------------------------------------
def _merge_key(name: str) -> str:
    return re.sub(r"[^a-z0-9]", "", name.lower())


def resolve_type(record: dict) -> str:
    if record.get("deviceType"):
        return record["deviceType"]
    upper = record["name"].upper()
    for hint, device_type in NAME_TYPE_HINTS:
        if hint in upper:
            return device_type
    if KEYBOARD_NAME_RE.match(record["name"]):
        return "keyboard"
    return "mouse"


def type_confidence(record: dict) -> int:
    """2=来源明确给出类型；1=按型号名推断；0=兜底当成鼠标。"""
    if record.get("deviceType"):
        return 2
    upper = record["name"].upper()
    if any(hint in upper for hint, _ in NAME_TYPE_HINTS):
        return 1
    return 1 if KEYBOARD_NAME_RE.match(record["name"]) else 0


def resolve_mode(record: dict) -> str:
    if record.get("isWireless") is True:
        return "2.4G"
    if record.get("isWireless") is False:
        return "USB"
    desc = record.get("desc") or ""
    if "2.4G" in desc or "无线" in desc:
        return "2.4G"
    if record.get("deviceType") == "mouse":
        return "2.4G"
    return "USB"


def merge_records(records: Iterable[dict]) -> tuple[list[dict], list[str]]:
    models: dict[str, dict] = {}
    warnings: list[str] = []

    for record in records:
        key = _merge_key(record["name"])
        if not key:
            continue
        model = models.get(key)
        if model is None:
            model = {
                "name": record["name"],
                "deviceType": record.get("deviceType"),
                "isWireless": record.get("isWireless"),
                "desc": record.get("desc"),
                "identities": [],
                "identity_names": {},
                "sources": set(),
            }
            models[key] = model
        else:
            if not model["deviceType"] and record.get("deviceType"):
                model["deviceType"] = record["deviceType"]
            if model["isWireless"] is None and record.get("isWireless") is not None:
                model["isWireless"] = record["isWireless"]
            if not model.get("desc") and record.get("desc"):
                model["desc"] = record["desc"]
        for ident in record["identities"]:
            if ident not in model["identities"]:
                model["identities"].append(ident)
        for ident, product_name in (record.get("identity_names") or {}).items():
            model["identity_names"].setdefault(ident, product_name)
        model["sources"].add(record.get("source", "?"))

    return list(models.values()), warnings


def _ambiguous_entry(vid: int, pid: int, items: list[dict]) -> dict:
    """多个型号共用同一 VID:PID 时的条目：不猜型号，把候选与官方规则一起交出去。"""
    rule = SHARED_RECEIVER_RULES.get((vid, pid))
    order = (
        {n: i for i, n in enumerate(rule["order"])} if rule and rule["match"] == "contains" else {}
    )

    def sort_key(item: dict) -> tuple[int, str]:
        return (order.get(item["receiverProductName"] or "", len(order)), item["name"])

    items = sorted(items, key=sort_key)
    types = {item["deviceType"] for item in items}
    modes = Counter(item["mode"] for item in items)

    entry: dict = {
        "name": None,
        "deviceType": types.pop() if len(types) == 1 else None,
        "mode": modes.most_common(1)[0][0],
        "ambiguous": True,
        "receiverMatch": (rule["match"] if rule else None),
        "candidates": [
            {
                "name": item["name"],
                "receiverProductName": item["receiverProductName"]
                or _rule_canonical_name(vid, pid, item["name"]),
            }
            for item in items
        ],
        "fallback": None,
    }
    if rule:
        fallback_fp = _model_fingerprint(rule["fallback"])
        for candidate in entry["candidates"]:
            if _model_fingerprint(candidate["name"]) == fallback_fp:
                entry["fallback"] = candidate["name"]
                break
    return entry


def build_output(
    models: list[dict],
    *,
    key_format: str,
    source: dict,
    mouse_vid: int,
    mode_overrides: dict[tuple[int, int], str] | None = None,
) -> dict:
    overrides = mode_overrides or {}
    grouped: dict[str, list[dict]] = {}

    for model in models:
        device_type = resolve_type(model)
        mode = resolve_mode(model)
        names = model.get("identity_names") or {}
        rank = min((SOURCE_RANK.get(s, 9) for s in model.get("sources") or ()), default=9)
        for raw_vid, pid in model["identities"]:
            vid = mouse_vid if raw_vid == 0 else raw_vid
            key = f"{vid}:{pid}" if key_format == "dec" else f"{vid:04X}:{pid:04X}"
            entry_mode = overrides.get((vid, pid), mode)
            if (vid, pid) in SHARED_RECEIVER_RULES:
                entry_mode = "2.4G"  # 规则命中的 PID 都是接收器侧接口
            grouped.setdefault(key, []).append(
                {
                    "vid": vid,
                    "pid": pid,
                    "name": model["name"],
                    "deviceType": device_type,
                    "typeConfidence": type_confidence(model),
                    "mode": entry_mode,
                    "rank": rank,
                    "receiverProductName": names.get((vid, pid))
                    or names.get((raw_vid, pid))
                    or _rule_canonical_name(vid, pid, model["name"]),
                }
            )

    devices: dict[str, dict] = {}
    ambiguous_keys: list[str] = []
    conflicted_keys: list[str] = []

    for key in sorted(grouped):
        # 同一型号可能被多个来源重复登记，先按型号名去重
        uniq: dict[str, dict] = {}
        for item in grouped[key]:
            uniq.setdefault(_merge_key(item["name"]), item)
        items = list(uniq.values())
        vid, pid = items[0]["vid"], items[0]["pid"]

        if len(items) == 1:
            item = items[0]
            entry = {"name": item["name"], "deviceType": item["deviceType"], "mode": item["mode"]}
            if item["receiverProductName"]:
                entry["receiverProductName"] = item["receiverProductName"]
            devices[key] = entry
            continue

        if (vid, pid) in SHARED_RECEIVER_RULES:
            # 官方明确按 productName 消歧的共享接收器 PID：不猜，交出候选与规则
            devices[key] = _ambiguous_entry(vid, pid, items)
            ambiguous_keys.append(key)
            continue

        # 其余碰撞（多为鼠标接收器 PID 池共用，官方靠 productName 细分）：
        # 保留可信度最高的型号，同时列出冲突型号，避免静默丢弃
        items.sort(key=lambda it: (it["rank"], -it["typeConfidence"], it["name"]))
        best = items[0]
        # 同一台模型的有线/无线 PID 混在同一列表里，取多数派更能反映该 PID 的真实用途
        modes = Counter(item["mode"] for item in items)
        entry = {
            "name": best["name"],
            "deviceType": best["deviceType"],
            "mode": modes.most_common(1)[0][0],
        }
        if best["receiverProductName"]:
            entry["receiverProductName"] = best["receiverProductName"]
        entry["ambiguous"] = True
        entry["conflicts"] = sorted({item["name"] for item in items[1:]})
        devices[key] = entry
        conflicted_keys.append(key)

    if ambiguous_keys:
        print(
            f"[warn] {len(ambiguous_keys)} 个共享接收器 PID 存在型号歧义，"
            f"已输出 candidates: {', '.join(ambiguous_keys)}",
            file=sys.stderr,
        )
    if conflicted_keys:
        print(
            f"[warn] {len(conflicted_keys)} 个设备标识被多个型号登记，"
            f"已保留可信度最高者并标注 conflicts: {', '.join(conflicted_keys[:5])}"
            + (" ..." if len(conflicted_keys) > 5 else ""),
            file=sys.stderr,
        )

    return {
        "config_version": fetch_stamp(),
        "source": source,
        "devices": dict(sorted(devices.items())),
    }


def fetch_stamp() -> str:
    """抓取时刻（UTC，精确到小时），作为 config_version。"""
    return datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H")


# --------------------------------------------------------------------------
# 主流程
# --------------------------------------------------------------------------
def collect(
    site_url: str, timeout: float, max_chunks: int, quiet: bool
) -> tuple[list[dict], str, dict[tuple[int, int], str]]:
    html = http_get(site_url, timeout=timeout)
    bundles = discover_entry_bundles(html, site_url)

    queue = list(bundles)
    seen: set[str] = set()
    texts: list[str] = []
    while queue and len(seen) < max_chunks:
        url = queue.pop(0)
        if url in seen:
            continue
        seen.add(url)
        try:
            text = http_get(url, timeout=timeout * 2)
        except Exception as exc:
            print(f"[warn] 跳过 {url}: {exc}", file=sys.stderr)
            continue
        texts.append(text)
        if not quiet:
            print(f"[info] 已加载 {url} ({len(text)} 字符)", file=sys.stderr)
        queue.extend(u for u in discover_related_chunks(text, url) if u not in seen)

    cz_url = discover_cz_bundle(site_url, timeout)
    if cz_url:
        try:
            texts.append(http_get(cz_url, timeout=timeout * 2))
            if not quiet:
                print(f"[info] 已加载 {cz_url}", file=sys.stderr)
        except Exception as exc:
            print(f"[warn] CZ 共享数据不可用: {exc}", file=sys.stderr)

    records: list[dict] = []
    for text in texts:
        records.extend(parse_product_keys(text))
        records.extend(parse_identity_devices(text))
        records.extend(parse_mouse_table(text))

    try:
        records.extend(parse_card_list(load_cdn_json(CARD_LIST_URL, timeout=timeout)))
    except Exception as exc:
        print(f"[warn] cardList 不可用: {exc}", file=sys.stderr)

    overrides: dict[tuple[int, int], str] = {}
    try:
        overrides = parse_keyboard_config(load_cdn_json(KEYBOARD_CONFIG_URL, timeout=timeout))
    except Exception as exc:
        print(f"[warn] keyboardConfig 不可用: {exc}", file=sys.stderr)

    web_version = ""
    try:
        global_cfg = load_cdn_json(GLOBAL_CONFIG_URL, timeout=timeout)
        web_version = str((global_cfg.get("webVersion") or {}).get("tag") or "")
    except Exception as exc:
        print(f"[warn] 全局配置不可用: {exc}", file=sys.stderr)

    return records, web_version, overrides


def run(args: argparse.Namespace) -> dict:
    records, web_version, overrides = collect(args.url, args.timeout, args.max_chunks, args.quiet)
    if not records:
        raise RuntimeError("未解析到任何设备记录，页面结构可能已变更")

    models, _ = merge_records(records)
    if not args.quiet:
        types = Counter(resolve_type(m) for m in models)
        print(f"[info] 型号 {len(models)} 个，类型分布: {dict(types)}", file=sys.stderr)

    source = {"file": "", "url": args.url, "web_version": web_version}
    return build_output(
        models,
        key_format=args.key_format,
        source=source,
        mouse_vid=args.mouse_vid,
        mode_overrides=overrides,
    )


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    p = argparse.ArgumentParser(description="抓取并转换 MCHOSE 设备映射表")
    p.add_argument("--url", default=DEFAULT_SITE_URL, help=f"站点地址（默认 {DEFAULT_SITE_URL}）")
    p.add_argument("-o", "--output", help="输出 JSON 文件路径（默认打印到 stdout）")
    p.add_argument(
        "--key-format",
        choices=["hex", "dec"],
        default="hex",
        help="设备标识键格式：hex 如 3837:300C（默认），dec 如 14391:12300",
    )
    p.add_argument(
        "--mouse-vid",
        type=lambda v: int(v, 0),
        default=14391,
        help="鼠标型号表未带 vendorId 时使用的默认值（默认 14391 / 0x3837）",
    )
    p.add_argument("--max-chunks", type=int, default=40, help="最多加载的打包文件数（默认 40）")
    p.add_argument("--timeout", type=float, default=30.0, help="请求超时秒数（默认 30）")
    p.add_argument("--compact", action="store_true", help="压缩输出（默认缩进美化）")
    p.add_argument(
        "--pick-name",
        nargs=2,
        metavar=("KEY", "PRODUCT_NAME"),
        help="调试：按官方规则把 HID productName 解析成型号名，KEY 形如 3837:3033",
    )
    p.add_argument("-q", "--quiet", action="store_true", help="不输出进度信息")
    return p.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)

    if args.pick_name:
        key, product_name = args.pick_name
        try:
            vid_s, pid_s = key.split(":")
            vid, pid = int(vid_s, 16), int(pid_s, 16)
        except ValueError:
            print(f"[error] KEY 需形如 3837:3033，收到 {key!r}", file=sys.stderr)
            return 1
        print(pick_useful_product_name(vid, pid, product_name))
        return 0

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
            fh.write(text + "\n")
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
