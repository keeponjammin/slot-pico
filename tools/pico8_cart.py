"""Draw the PICO-8 cart mould slot tints, the way the Game Boy moulds are drawn.

Writes, all 312x337 like the Game Boy paks so the PICO-8 cart seats the same:

  assets/pico8_cart.png      the silhouette: white, the outline in alpha
  assets/pico8_neutral.png   the shell: 50% grey plastic, its relief lit from above
  assets/pico8_interior.png  the back plastic a clear shell shows

The shell is built as a height map (raised name plate, recessed square label
well, side grips, a stepped arrow) and lit from the top left, then given the
same 2px grain as gbc_neutral.png, so `overlay` tints it the same way.

    python tools/pico8_cart.py [--preview DIR]

--preview also writes the shell tinted a few colours, for looking at.
"""

from __future__ import annotations

import argparse
from pathlib import Path

import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
ASSETS = ROOT / "assets"

W, H = 312, 337
SS = 4  # supersampling for the outline's edge

# The body, inside the canvas like the Game Boy Color pak's.
BODY = (9, 0, 303, 337)
# Each corner is rounded the way pixel art rounds one: a staircase, how far
# each PIXEL-tall row of the corner is cut in.
PIXEL = 4
CORNER = [10, 5, 2]

# The square label well. Keep in step with PICO8_LABEL_* in slot-ui's cart.rs.
LABEL = (50, 78, 212, 212)

# The raised name plate across the top.
PLATE = (40, 16, 272, 64)

NEUTRAL = 128


def outline(scale: int) -> np.ndarray:
    """Coverage of the body at `scale`x, 1 inside."""
    w, h = W * scale, H * scale
    ys, xs = np.mgrid[0:h, 0:w]
    x = (xs + 0.5) / scale
    y = (ys + 0.5) / scale
    x0, y0, x1, y1 = BODY
    inside = (x >= x0) & (x < x1) & (y >= y0) & (y < y1)
    for i, cut in enumerate(CORNER):
        top = (y >= y0 + i * PIXEL) & (y < y0 + (i + 1) * PIXEL)
        bottom = (y < y1 - i * PIXEL) & (y >= y1 - (i + 1) * PIXEL)
        side = (x < x0 + cut) | (x >= x1 - cut)
        inside &= ~((top | bottom) & side)
    return inside.astype(np.float32)


def coverage() -> np.ndarray:
    big = outline(SS)
    return big.reshape(H, SS, W, SS).mean(axis=(1, 3))


def box_blur(a: np.ndarray, r: int) -> np.ndarray:
    if r <= 0:
        return a
    k = 2 * r + 1
    p = np.pad(a, r, mode="edge")
    c = p.cumsum(0).cumsum(1)
    c = np.pad(c, ((1, 0), (1, 0)))
    return (c[k:, k:] - c[:-k, k:] - c[k:, :-k] + c[:-k, :-k]) / (k * k)


def rect(h: np.ndarray, box, value: float, r: int = 0) -> None:
    x0, y0, x1, y1 = box
    ys, xs = np.mgrid[0:H, 0:W]
    inside = (xs >= x0) & (xs < x1) & (ys >= y0) & (ys < y1)
    if r:
        for cx, cy in [(x0 + r, y0 + r), (x1 - r - 1, y0 + r), (x0 + r, y1 - r - 1), (x1 - r - 1, y1 - r - 1)]:
            near = ((xs < x0 + r) | (xs > x1 - r - 1)) & ((ys < y0 + r) | (ys > y1 - r - 1))
            far = (xs - cx) ** 2 + (ys - cy) ** 2 > r * r
            inside &= ~(near & far & (abs(xs - cx) <= r) & (abs(ys - cy) <= r))
    h[inside] = value


# A pixel face for the plate's name, five pixels tall like the console's own.
GLYPHS = {
    "P": ["1110", "1001", "1110", "1000", "1000"],
    "I": ["111", "010", "010", "010", "111"],
    "C": ["0111", "1000", "1000", "1000", "0111"],
    "O": ["0110", "1001", "1001", "1001", "0110"],
    "-": ["000", "000", "111", "000", "000"],
    "8": ["0110", "1001", "0110", "1001", "0110"],
}


def lettering(h: np.ndarray, text: str, cx: int, cy: int, px: int, depth: float) -> None:
    cols = sum(len(GLYPHS[ch][0]) for ch in text) + len(text) - 1
    x = cx - cols * px // 2
    y = cy - 5 * px // 2
    for ch in text:
        for row, bits in enumerate(GLYPHS[ch]):
            for col, bit in enumerate(bits):
                if bit == "1":
                    h[y + row * px : y + (row + 1) * px, x + col * px : x + (col + 1) * px] += depth
        x += (len(GLYPHS[ch][0]) + 1) * px


def heights() -> np.ndarray:
    h = np.zeros((H, W), np.float32)
    # The raised plate, its name pressed in.
    rect(h, PLATE, 3.0, r=6)
    lettering(h, "PICO-8", (PLATE[0] + PLATE[2]) // 2, (PLATE[1] + PLATE[3]) // 2 + 1, 5, -2.2)
    # The label well, sunk.
    lx, ly, lw, lh = LABEL
    rect(h, (lx - 4, ly - 4, lx + lw + 4, ly + lh + 4), -1.5, r=4)
    # Grips down both sides under the plate.
    x0, _, x1, _ = BODY
    for i in range(6):
        y = 22 + i * 9
        h[y : y + 4, x0 : x0 + 7] -= 1.4
        h[y : y + 4, x1 - 7 : x1] -= 1.4
    # The arrow the cart goes in by: a stepped triangle, pressed in.
    cx, top = W // 2, 302
    for i, half in enumerate([16, 12, 8, 4]):
        h[top + i * 4 : top + (i + 1) * 4, cx - half : cx + half] -= 1.4
    return h


def lit(h: np.ndarray, cover: np.ndarray) -> np.ndarray:
    soft = box_blur(h, 1)
    dy = np.zeros_like(soft)
    dx = np.zeros_like(soft)
    dy[1:-1] = soft[:-2] - soft[2:]
    dx[:, 1:-1] = soft[:, :-2] - soft[:, 2:]
    # Light from above and a little left: faces turned up are lighter.
    grey = NEUTRAL + 26 * dy + 10 * dx
    # The wall of the shell darkens toward its edge, as the Game Boy paks' do.
    inner = box_blur((cover > 0.5).astype(np.float32), 3)
    grey -= (1.0 - inner) * 70
    # A lit top edge and a shaded bottom edge on the body.
    edge = box_blur((cover > 0.5).astype(np.float32), 1)
    up = np.zeros_like(edge)
    up[1:] = edge[1:] - edge[:-1]
    grey += np.clip(up, 0, 1) * 40
    return grey


def grain(rng: np.random.Generator) -> np.ndarray:
    n = rng.integers(-5, 6, size=((H + 1) // 2, (W + 1) // 2)).astype(np.float32)
    return np.repeat(np.repeat(n, 2, 0), 2, 1)[:H, :W]


def rgba(grey: np.ndarray, cover: np.ndarray) -> Image.Image:
    g = np.clip(np.rint(grey), 0, 255).astype(np.uint8)
    a = np.clip(np.rint(cover * 255), 0, 255).astype(np.uint8)
    return Image.fromarray(np.dstack([g, g, g, a]), "RGBA")


def shell(cover: np.ndarray) -> np.ndarray:
    rng = np.random.default_rng(8)
    grey = lit(heights(), cover) + grain(rng)
    # The plate bulges: lit along its top, falling into shade at its foot.
    x0, y0, x1, y1 = PLATE
    t = np.linspace(0.0, 1.0, y1 - y0)[:, None]
    grey[y0:y1, x0 + 2 : x1 - 2] += 26 - 40 * t
    lx, ly, lw, lh = LABEL
    grey[ly : ly + lh, lx : lx + lw] = 119  # the flat well the label is stuck in
    return grey


def interior(cover: np.ndarray) -> np.ndarray:
    rng = np.random.default_rng(80)
    h = np.zeros((H, W), np.float32)
    # One screw boss in the middle, a rib across the bottom.
    ys, xs = np.mgrid[0:H, 0:W]
    d = np.hypot(xs - W / 2, ys - 236)
    h[d < 13] = 2.0
    h[d < 6] = -2.0
    h[284:290, 20:292] = 1.2
    grey = lit(h, cover) - 29 + grain(rng)
    return grey


def overlay(base: np.ndarray, colour: np.ndarray) -> np.ndarray:
    base = base.astype(np.int32)
    c = colour.astype(np.int32)
    lighter = c + ((255 - c) * (base - NEUTRAL) + 63) // 127
    darker = c - (c * (NEUTRAL - base) + 64) // NEUTRAL
    return np.clip(np.where(base >= NEUTRAL, lighter, darker), 0, 255)


def preview(img: Image.Image, colour, out: Path) -> None:
    a = np.array(img).astype(np.int32)
    tinted = a.copy()
    lx, ly, lw, lh = LABEL
    for c in range(3):
        tinted[..., c] = overlay(a[..., c], np.full(a.shape[:2], colour[c]))
    tinted[ly : ly + lh, lx : lx + lw, :3] = a[ly : ly + lh, lx : lx + lw, :3]
    Image.fromarray(tinted.astype(np.uint8), "RGBA").save(out)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--preview", type=Path)
    args = ap.parse_args()
    cover = coverage()
    silhouette = np.full((H, W), 255.0)
    rgba(silhouette, cover).save(ASSETS / "pico8_cart.png", optimize=True)
    neutral = rgba(shell(cover), cover)
    neutral.save(ASSETS / "pico8_neutral.png", optimize=True)
    rgba(interior(cover), cover).save(ASSETS / "pico8_interior.png", optimize=True)
    if args.preview:
        args.preview.mkdir(parents=True, exist_ok=True)
        for name, colour in [("blue", (0x1D, 0x2B, 0x53)), ("red", (0xFF, 0x00, 0x4D)), ("peach", (0xFF, 0xCC, 0xAA)), ("green", (0x00, 0xE4, 0x36))]:
            preview(neutral, colour, args.preview / f"pico8_{name}.png")


if __name__ == "__main__":
    main()
