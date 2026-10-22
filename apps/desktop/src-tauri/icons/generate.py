#!/usr/bin/env python3
"""生成 VoltNest 的应用图标与托盘图标位图。

不引入 Pillow / cairosvg 等图形依赖：图标只有「圆角方块 + 电量环」两个形状，
用解析式 SDF + 4×4 超采样直接合成 RGBA，再手写 PNG / ICO 容器即可。

用法（仓库根目录）：

    python apps/desktop/src-tauri/icons/generate.py

产物（全部为已提交的静态资源）：
    icon.png            1024×1024，打包与商店展示用
    32x32.png           托盘与任务栏
    128x128.png         资源管理器
    128x128@2x.png      256×256，高 DPI
    icon.ico            多尺寸 ICO（16/24/32/48/64/128/256）
"""

from __future__ import annotations

import math
import pathlib
import struct
import zlib

HERE = pathlib.Path(__file__).resolve().parent

# 与前端 `batteryColor` 保持同一套语义色。
BG = (15, 23, 42)          # slate-900，圆角方块底色
RING = (16, 185, 129)      # emerald-500，正常电量
RING_DIM = (71, 85, 105)   # slate-600，环的未填充部分
SS = 4                     # 超采样倍率
ARC_RATIO = 0.72           # 图标固定显示 72%，不绑定真实电量


def _rounded_rect_sdf(x: float, y: float, half: float, radius: float) -> float:
    dx = abs(x - half) - (half - radius)
    dy = abs(y - half) - (half - radius)
    ax, ay = max(dx, 0.0), max(dy, 0.0)
    return math.hypot(ax, ay) + min(max(dx, dy), 0.0) - radius


def render(size: int) -> bytes:
    """渲染一张 size×size 的 RGBA 图（返回原始字节）。"""
    half = size / 2.0
    radius = size * 0.22
    r_out = size * 0.30
    r_in = size * 0.30 - max(1.5, size * 0.085)
    px = bytearray(size * size * 4)
    inv = 1.0 / (SS * SS)
    step = 1.0 / SS

    for py in range(size):
        for pxi in range(size):
            acc_r = acc_g = acc_b = acc_a = 0.0
            for sy in range(SS):
                for sx in range(SS):
                    x = pxi + (sx + 0.5) * step
                    y = py + (sy + 0.5) * step

                    # 底色圆角方块
                    if _rounded_rect_sdf(x, y, half, radius) > 0:
                        continue
                    r, g, b = BG

                    d = math.hypot(x - half, y - half)
                    if r_in <= d <= r_out:
                        # 12 点方向顺时针的角度比例
                        ang = math.degrees(math.atan2(x - half, half - y)) % 360.0
                        if ang / 360.0 <= ARC_RATIO:
                            r, g, b = RING
                        else:
                            r, g, b = RING_DIM

                    acc_r += r
                    acc_g += g
                    acc_b += b
                    acc_a += 1.0

            a = acc_a * inv
            idx = (py * size + pxi) * 4
            if a <= 0:
                px[idx : idx + 4] = b"\x00\x00\x00\x00"
            else:
                px[idx] = int(acc_r / acc_a + 0.5)
                px[idx + 1] = int(acc_g / acc_a + 0.5)
                px[idx + 2] = int(acc_b / acc_a + 0.5)
                px[idx + 3] = int(a * 255 + 0.5)
    return bytes(px)


def png(size: int, rgba: bytes) -> bytes:
    stride = size * 4
    raw = bytearray()
    for y in range(size):
        raw.append(0)  # filter: None
        raw += rgba[y * stride : (y + 1) * stride]

    def chunk(tag: bytes, data: bytes) -> bytes:
        body = tag + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    ihdr = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
        + chunk(b"IEND", b"")
    )


def ico(images: list[tuple[int, bytes]]) -> bytes:
    """ICO 容器：每个条目直接内嵌 PNG（Vista 起支持）。"""
    header = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    entries = bytearray()
    blobs = bytearray()
    for size, blob in images:
        dim = 0 if size >= 256 else size
        entries += struct.pack(
            "<BBBBHHII", dim, dim, 0, 0, 1, 32, len(blob), offset
        )
        blobs += blob
        offset += len(blob)
    return header + bytes(entries) + bytes(blobs)


def main() -> None:
    targets = {
        "icon.png": 1024,
        "32x32.png": 32,
        "128x128.png": 128,
        "128x128@2x.png": 256,
    }
    cache: dict[int, bytes] = {}
    for name, size in targets.items():
        blob = png(size, render(size))
        (HERE / name).write_bytes(blob)
        cache[size] = blob
        print(f"{name:20s} {size:>4}px  {len(blob):>7} bytes")

    ico_sizes = [16, 24, 32, 48, 64, 128, 256]
    images = []
    for size in ico_sizes:
        blob = cache.get(size)
        if blob is None:
            blob = png(size, render(size))
        images.append((size, blob))
    (HERE / "icon.ico").write_bytes(ico(images))
    print(f"{'icon.ico':20s} {ico_sizes}  {(HERE / 'icon.ico').stat().st_size:>7} bytes")


if __name__ == "__main__":
    main()
