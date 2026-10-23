#!/usr/bin/env python3
"""生成 VoltNest 的应用图标位图。

标识是一个「V + 闪电」的合体字：V 是产品名首字母，右臂上扬接闪电，呼应电量
主题；外围一圈断口朝下的圆环，借用仪表盘刻度盘的语义。三者共用一条从亮黄绿
到底部青绿的渐变，与界面里标题栏的徽标同色系。

不引入 Pillow / cairosvg 等图形依赖：形状只有圆角矩形、圆环、粗折线与一个
六边形闪电，用解析式 SDF + 多边形内外判定 + 超采样直接合成 RGBA，再手写
PNG / ICO 容器即可。渐变在同一像素内按子采样点取色，所以抗锯齿不会把渐变
拉出灰边。

用法（仓库根目录）：

    python apps/desktop/src-tauri/icons/generate.py

产物（全部为已提交的静态资源）：
    icon.png            1024×1024，打包与商店展示用
    32x32.png           托盘与任务栏
    128x128.png         资源管理器 / Toast 头图
    128x128@2x.png      256×256，高 DPI
    icon.ico            多尺寸 ICO（16/24/32/48/64/128/256）

所有几何量都是 0..1 的归一化坐标（原点左上，y 向下），改尺寸不需要改数字。
"""

from __future__ import annotations

import math
import pathlib
import struct
import zlib

HERE = pathlib.Path(__file__).resolve().parent

# ---------- 配色 ----------
TILE = (0x14, 0x15, 0x18)        # 近黑底
TILE_R = 0.232                   # 圆角半径（占边长）
RING = (0x4B, 0x51, 0x5C)        # 圆环：比底色亮两档，不与标记抢焦点

GRAD_FROM = (0xC8, 0xF4, 0x6E)   # 上端：亮黄绿
GRAD_TO = (0x22, 0xB2, 0xAC)     # 下端：青绿
GRAD_Y0, GRAD_Y1 = 0.17, 0.74    # 渐变在竖直方向的作用区间

# ---------- 圆环 ----------
RING_R = 0.312                   # 环心半径
RING_W = 0.036                   # 环宽
RING_GAP_DEG = 22.0              # 开口半角，方向朝正下

# ---------- 标记 ----------
# V：两条粗折线，谷底圆角连接，顶端水平切平。
V_APEX = (0.500, 0.688)          # 折线拐点（墨迹最低点还要再加半宽）
V_LEFT = (0.296, 0.300)
V_RIGHT = (0.704, 0.300)
V_HALF_W = 0.056                 # 笔画半宽
# 两臂的顶端切口高度不同：左臂平切；右臂切在闪电折口带之内，切边被闪电盖住，
# 于是「V 的右臂」与闪电在视觉上是同一笔，不会出现台阶。
V_LEFT_CUT = 0.322
V_RIGHT_CUT = 0.295

# 闪电：局部 0..1 轮廓（左上翼、右下翼的经典六边形，取自 Heroicons bolt 的顶点），
# 按 BOLT_SIZE 贴到右上，下尖端落在 V 的右臂上，两者连成「一笔画成」的观感。
BOLT_LOCAL = (
    (0.648, 0.000),  # 上尖端
    (0.557, 0.394),  # 左翼内侧
    (0.972, 0.394),  # 左翼外沿 → 右翼上沿
    (1.000, 0.454),  # 右翼尖
    (0.403, 1.000),  # 下尖端
    (0.330, 0.961),  # 下尖端左侧
    (0.443, 0.611),  # 中折口
    (0.031, 0.611),  # 左翼尖
)
BOLT_ORIGIN = (0.500, 0.150)     # 闪电左上角在图标中的位置
BOLT_SIZE = (0.280, 0.360)       # 闪电宽、高占边长的比例


def _rounded_rect_sdf(x: float, y: float, half: float, radius: float) -> float:
    dx = abs(x - half) - (half - radius)
    dy = abs(y - half) - (half - radius)
    ax, ay = max(dx, 0.0), max(dy, 0.0)
    return math.hypot(ax, ay) + min(max(dx, dy), 0.0) - radius


def _seg_dist(x: float, y: float, ax: float, ay: float, bx: float, by: float) -> float:
    """点到线段的距离。"""
    dx, dy = bx - ax, by - ay
    t = ((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy)
    t = 0.0 if t < 0.0 else (1.0 if t > 1.0 else t)
    return math.hypot(x - (ax + t * dx), y - (ay + t * dy))


def _inside(poly, x: float, y: float) -> bool:
    """射线法（even-odd）判点是否落在多边形内。"""
    inside = False
    n = len(poly)
    j = n - 1
    for i in range(n):
        xi, yi = poly[i]
        xj, yj = poly[j]
        if (yi > y) != (yj > y):
            if x < (xj - xi) * (y - yi) / (yj - yi) + xi:
                inside = not inside
        j = i
    return inside


def _bolt_polygon():
    ox, oy = BOLT_ORIGIN
    sx, sy = BOLT_SIZE
    return tuple((ox + px * sx, oy + py * sy) for px, py in BOLT_LOCAL)


def _gradient(x: float, y: float):
    """竖直为主、略向右偏暖的线性渐变。"""
    t = (y - GRAD_Y0) / (GRAD_Y1 - GRAD_Y0)
    t += (x - 0.5) * 0.18
    t = 0.0 if t < 0.0 else (1.0 if t > 1.0 else t)
    return tuple(
        int(a + (b - a) * t + 0.5) for a, b in zip(GRAD_FROM, GRAD_TO)
    )


def render(size: int) -> bytes:
    """渲染一张 size×size 的 RGBA 图（返回原始字节）。

    整个函数在归一化坐标（0..1）里算形状，只在写像素时按 size 折算采样步长：
    几何常量与尺寸解耦，改分辨率不需要改任何一个数字。
    """
    half = 0.5
    radius = TILE_R
    bolt = _bolt_polygon()

    ring_hw = RING_W / 2.0
    gap = math.radians(RING_GAP_DEG)

    # 小尺寸多取样：16px 的斜边只靠 4×4 会留下阶梯感。
    ss = 8 if size <= 48 else 4
    inv = 1.0 / (ss * ss)

    px = bytearray(size * size * 4)
    for py in range(size):
        for pxi in range(size):
            acc_r = acc_g = acc_b = 0.0
            acc_a = 0.0
            for sy in range(ss):
                for sx in range(ss):
                    x = (pxi + (sx + 0.5) / ss) / size
                    y = (py + (sy + 0.5) / ss) / size

                    # 圆角方块之外透明：图标不是满幅背景，边缘要能贴合任何主题色。
                    if _rounded_rect_sdf(x, y, half, radius) > 0:
                        continue

                    r, g, b = TILE

                    # 标记优先于圆环：闪电压在环上，环只做背景。
                    on_left = y >= V_LEFT_CUT and _seg_dist(x, y, *V_LEFT, *V_APEX) <= V_HALF_W
                    on_right = y >= V_RIGHT_CUT and _seg_dist(x, y, *V_APEX, *V_RIGHT) <= V_HALF_W
                    if on_left or on_right or _inside(bolt, x, y):
                        r, g, b = _gradient(x, y)
                    elif size >= 20:
                        # 16px 下图里的环只剩一圈噪点，小尺寸直接不画。
                        dx, dy = x - 0.5, y - 0.5
                        if abs(math.hypot(dx, dy) - RING_R) <= ring_hw:
                            # 开口朝正下：angle 从正上方顺时针计，180° 指向正下。
                            ang = math.atan2(dx, -dy)
                            if abs(abs(ang) - math.pi) > gap:
                                r, g, b = RING

                    acc_r += r
                    acc_g += g
                    acc_b += b
                    acc_a += 1.0

            a = acc_a * inv
            idx = (py * size + pxi) * 4
            if a <= 0:
                px[idx : idx + 4] = b"\x00\x00\x00\x00"
            else:
                # 按覆盖率还原颜色：标记与底色之间的像素得到正确的中间色，
                # 而不是被当成「半透明标记」压暗。
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
        entries += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(blob), offset)
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
