"""Draw the PICO-8 cart: slot's own fantasy cart, built the way the Game Boy moulds are.

PICO-8 has no real cartridge, so this one is modelled on the fan-made printed
ones: a matte slab a little taller than wide, a square window for the label
with a black lip round it, a dark strip under it for the cart's name, and grip
grooves cut into both sides. It is drawn as a height map lit from the top
left, with the same 2px grain as the Game Boy moulds, so slot tints it the
same way.

Writes, all 264x328:

  assets/pico8_cart.png         the silhouette: white, the outline in alpha
  assets/pico8_neutral.png      the shell: 50% grey plastic, its relief lit
  assets/pico8_interior.png     the back plastic a clear shell shows
  assets/pico8_board.png        the board inside, a chip seated, for clear shells
  assets/pico8_board_empty.png  the same board with empty seats, for the core
                                picker to seat its chips in

and assets/pico8_chip.png, a core's chip, its name left for slot to print.

    python tools/pico8_cart.py [--shell IMG] [--board IMG] [--interior IMG]
                               [--chip IMG] [--preview DIR]

--shell, --board, --interior and --chip each take a render of that part, made from
this tool's own drawing as a template, on a pure green (#00FF00) background at
any size of the same aspect: the green is keyed out, the shell's grey is set
so its flat face is 50% again and its strip dark (the interior's floor a
little darker), and each is scaled down to its art's size.
The shell's outline then comes from the render, so the silhouette matches it.

--preview also writes the shell tinted a few colours and the open cart.
"""

from __future__ import annotations

import argparse
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[1]
ASSETS = ROOT / "assets"
MONO = ROOT / "crates" / "slot-ui" / "assets" / "DejaVuSansMono.ttf"

W, H = 264, 328
SS = 4  # supersampling for edges
RADIUS = 12

# The square label, and the black lip of the window it sits in. Keep in step
# with PICO8_LABEL_* in slot-ui's pico8_cart.rs.
LABEL = (32, 28, 200, 200)
LIP = 5
# The dark strip under the label the cart's name is printed on. Keep in step
# with PICO8_STRIP in slot-ui's pico8_cart.rs.
STRIP = (27, 242, 210, 56)
# Grip grooves cut into each side, a third of the way down.
GROOVES = [112 + 8 * i for i in range(4)]
GROOVE = (4, 6)  # tall, deep

# The core chips' seats on the board: left edges, top, and the chip's size.
# Keep in step with PICO8_SEATS in slot-ui's pico8_cart.rs.
CHIP = (92, 52)
SEAT_X = [28, 144]
SEAT_Y = 128

NEUTRAL = 128


# ---------------------------------------------------------------- the shell


def outline_img(scale: int) -> Image.Image:
    img = Image.new("L", (W * scale, H * scale), 0)
    d = ImageDraw.Draw(img)
    d.rounded_rectangle((0, 0, W * scale - 1, H * scale - 1), RADIUS * scale, fill=255)
    gh, gd = GROOVE
    for y in GROOVES:
        for x0 in (0, W - gd):
            d.rounded_rectangle(
                (x0 * scale, y * scale, (x0 + gd) * scale - 1, (y + gh) * scale - 1),
                1 * scale,
                fill=0,
            )
    return img


def coverage() -> np.ndarray:
    big = outline_img(SS)
    return np.asarray(big.resize((W, H), Image.BOX), np.float32) / 255.0


def box_blur(a: np.ndarray, r: int) -> np.ndarray:
    if r <= 0:
        return a
    k = 2 * r + 1
    p = np.pad(a, r, mode="edge")
    c = p.cumsum(0).cumsum(1)
    c = np.pad(c, ((1, 0), (1, 0)))
    return (c[k:, k:] - c[:-k, k:] - c[k:, :-k] + c[:-k, :-k]) / (k * k)


def rect_mask(box, r: int = 0) -> np.ndarray:
    """Coverage of an x, y, w, h box with corners of radius r, antialiased."""
    x, y, w, h = box
    img = Image.new("L", (W * SS, H * SS), 0)
    ImageDraw.Draw(img).rounded_rectangle(
        (x * SS, y * SS, (x + w) * SS - 1, (y + h) * SS - 1), r * SS, fill=255
    )
    return np.asarray(img.resize((W, H), Image.BOX), np.float32) / 255.0


def heights(cover: np.ndarray) -> np.ndarray:
    h = np.zeros((H, W), np.float32)
    lx, ly, lw, lh = LABEL
    window = rect_mask((lx - LIP, ly - LIP, lw + 2 * LIP, lh + 2 * LIP), 3)
    h -= 3.0 * window
    h -= 2.6 * rect_mask(STRIP, 3)
    return h


def lit(h: np.ndarray, cover: np.ndarray) -> np.ndarray:
    soft = box_blur(h, 1)
    dy = np.zeros_like(soft)
    dx = np.zeros_like(soft)
    dy[1:-1] = soft[:-2] - soft[2:]
    dx[:, 1:-1] = soft[:, :-2] - soft[:, 2:]
    # Light from above and a little left: faces turned up are lighter.
    grey = NEUTRAL + 22 * dy + 9 * dx
    # The slab's edge rounds away: darker toward it, a lit top lip, a shaded foot.
    solid = (cover > 0.5).astype(np.float32)
    inner = box_blur(solid, 3)
    grey -= (1.0 - inner) * 60
    edge = box_blur(solid, 1)
    up = np.zeros_like(edge)
    up[1:] = edge[1:] - edge[:-1]
    down = np.zeros_like(edge)
    down[:-1] = edge[:-1] - edge[1:]
    grey += np.clip(up, 0, 1) * 36
    grey -= np.clip(down, 0, 1) * 30
    return grey


def grain(rng: np.random.Generator, amount: int = 5) -> np.ndarray:
    n = rng.integers(-amount, amount + 1, size=((H + 1) // 2, (W + 1) // 2)).astype(np.float32)
    return np.repeat(np.repeat(n, 2, 0), 2, 1)[:H, :W]


def shell(cover: np.ndarray) -> np.ndarray:
    rng = np.random.default_rng(8)
    grey = lit(heights(cover), cover) + grain(rng)
    lx, ly, lw, lh = LABEL
    # The window's black lip, and the flat the label is stuck on.
    lip = rect_mask((lx - LIP + 1, ly - LIP + 1, lw + 2 * LIP - 2, lh + 2 * LIP - 2), 2)
    grey = grey * (1 - lip) + 14 * lip
    grey[ly : ly + lh, lx : lx + lw] = 119
    # The strip: a dark insert, whatever colour the cart is, its top in shadow.
    sx, sy, sw, sh = STRIP
    inset = rect_mask((sx + 2, sy + 2, sw - 4, sh - 4), 2)
    panel = 44 + grain(np.random.default_rng(88), 2)
    panel[sy + 2 : sy + 5] -= 10
    grey = grey * (1 - inset) + panel * inset
    return grey


def interior(cover: np.ndarray) -> np.ndarray:
    rng = np.random.default_rng(80)
    h = np.zeros((H, W), np.float32)
    ys, xs = np.mgrid[0:H, 0:W]
    # Four screw posts in the corners, a rib above the contacts.
    for cx, cy in HOLES:
        d = np.hypot(xs - cx, ys - cy)
        h[d < 10] = 2.0
        h[d < 4] = -2.0
    h[H - 46 : H - 41, 16 : W - 16] = 1.2
    return lit(h, cover) - 29 + grain(rng)


# ---------------------------------------------------------------- the board

MASK = (0x3B, 0x1F, 0x5C)  # purple solder mask
TRACE = (0x58, 0x35, 0x80)  # copper under the mask
GOLD = (0xD9, 0xA4, 0x3A)
GOLD_LIT = (0xF6, 0xD3, 0x7A)
SILK = (0xEE, 0xEA, 0xF2)
BOARD_IN = 9  # how far the board stands in from the shell's outside
FINGERS_TOP = H - 38
HOLES = [(26, 26), (W - 27, 26), (26, H - 64), (W - 27, H - 64)]


class Canvas:
    """An RGBA drawing at SS times the art's size."""

    def __init__(self) -> None:
        self.img = Image.new("RGBA", (W * SS, H * SS), (0, 0, 0, 0))
        self.d = ImageDraw.Draw(self.img)

    def s(self, *v):
        return [round(x * SS) for x in v]

    def rect(self, x, y, w, h, fill, r=0, outline=None, width=0):
        box = self.s(x, y, x + w, y + h)
        box[2] -= 1
        box[3] -= 1
        self.d.rounded_rectangle(box, r * SS, fill=fill, outline=outline, width=round(width * SS))

    def circle(self, cx, cy, r, fill, outline=None, width=0):
        self.d.ellipse(self.s(cx - r, cy - r, cx + r, cy + r), fill=fill, outline=outline, width=round(width * SS))

    def line(self, pts, fill, width):
        self.d.line([tuple(self.s(*p)) for p in pts], fill=fill, width=round(width * SS), joint="curve")
        for p in (pts[0], pts[-1]):
            self.circle(*p, width / 2, fill)

    def text(self, x, y, s, size, fill, anchor="la"):
        font = ImageFont.truetype(str(MONO), round(size * SS))
        self.d.text(tuple(self.s(x, y)), s, font=font, fill=fill, anchor=anchor)

    def done(self) -> Image.Image:
        return self.img.resize((W, H), Image.LANCZOS)


def via(c: Canvas, x, y):
    c.circle(x, y, 2.6, GOLD)
    c.circle(x, y, 1.2, (0x1A, 0x0E, 0x26))


def smd(c: Canvas, x, y, body, vertical=False):
    """A 0805 part: a body between two tinned ends."""
    w, h = (6, 11) if vertical else (11, 6)
    c.rect(x, y, w, h, (0xB8, 0xBC, 0xC2), r=1)
    if vertical:
        c.rect(x, y + 3, w, h - 6, body)
    else:
        c.rect(x + 3, y, w - 6, h, body)


def board(seated: bool) -> Image.Image:
    c = Canvas()
    i = BOARD_IN
    # The board, its corners following the shell's.
    c.rect(i, i, W - 2 * i, FINGERS_TOP - i + 2, MASK, r=RADIUS - 4)
    c.rect(i + 10, FINGERS_TOP - 6, W - 2 * (i + 10), H - FINGERS_TOP + 4, MASK, r=4)
    # Copper under the mask: a ground pour's edge, then the tracks.
    c.rect(i + 4, i + 4, W - 2 * i - 8, FINGERS_TOP - i - 6, None, r=RADIUS - 6, outline=TRACE, width=1.5)
    cw, ch = CHIP
    finger_xs = [x + 3 for x in range(i + 16, W - i - 16, 9)]
    pins = [[sx + 8 + k * 11 for k in range(8)] for sx in SEAT_X]
    # Each seat's lower pins run down to the nearest free contact, one 45 degree jog each.
    taken = -1
    jog = FINGERS_TOP - 22
    for px in pins[0] + pins[1]:
        n = min(range(taken + 1, len(finger_xs)), key=lambda j: abs(finger_xs[j] - px))
        taken = n
        fx = finger_xs[n]
        c.line([(px, SEAT_Y + ch - 2), (px, jog - abs(px - fx)), (fx, jog), (fx, FINGERS_TOP + 6)], TRACE, 2)
    # The contacts left over are ground, tied to the pour.
    # The upper pins go through the board, staggered, and the inner two take the clock.
    for seat in pins:
        for k, px in enumerate(seat):
            if px in (pins[0][7], pins[1][0]):
                continue
            top = SEAT_Y - 9 - (k % 2) * 8
            c.line([(px, SEAT_Y + 2), (px, top)], TRACE, 2)
            via(c, px, top - 2)
    for px, side in ((pins[0][7], -1), (pins[1][0], 1)):
        x0 = W // 2 + side * 12
        c.line([(px, SEAT_Y + 2), (px, 100 + abs(px - x0) * 0 + 8), (x0, 96), (x0, 86)], TRACE, 2)
    # The contacts, gold, cut into the board's lower edge.
    for fx in finger_xs:
        c.rect(fx - 3, FINGERS_TOP + 4, 6, H - FINGERS_TOP - 8, GOLD, r=1)
        c.rect(fx - 2, FINGERS_TOP + 5, 2, H - FINGERS_TOP - 12, GOLD_LIT, r=1)
    # Mounting holes, plated.
    for cx, cy in HOLES:
        c.circle(cx, cy, 8, GOLD)
        c.circle(cx, cy, 6.6, GOLD_LIT)
        c.circle(cx, cy, 5, (0x14, 0x0A, 0x1E))
    # The seats: pads under every leg, a silkscreen outline, the pin-1 dot, the part's name.
    for n, seat in enumerate(pins):
        sx = SEAT_X[n]
        c.rect(sx - 3, SEAT_Y - 3, cw + 6, ch + 6, None, r=2, outline=SILK, width=1.2)
        c.circle(sx - 6, SEAT_Y + ch - 2, 1.8, SILK)
        c.text(sx - 2, SEAT_Y + ch + 14, f"U{n + 1}", 7, SILK)
        for px in seat:
            for py in (SEAT_Y, SEAT_Y + ch - 8):
                c.rect(px - 2.5, py, 5, 8, (0xC9, 0xCD, 0xD3), r=0.8)
    # Decoupling between the seats.
    smd(c, (SEAT_X[0] + cw + SEAT_X[1]) / 2 - 3, SEAT_Y + 8, (0xB0, 0x8A, 0x58), vertical=True)
    smd(c, (SEAT_X[0] + cw + SEAT_X[1]) / 2 - 3, SEAT_Y + 32, (0xB0, 0x8A, 0x58), vertical=True)
    # A crystal, a regulator, and the small parts round them.
    c.rect(W // 2 - 20, 72, 40, 15, (0xB4, 0xB8, 0xBE), r=7.5)
    c.rect(W // 2 - 17, 74, 34, 10, (0xE2, 0xE5, 0xEA), r=5)
    c.text(W // 2, 68, "Y1", 7, SILK, anchor="ms")
    c.rect(30, 70, 22, 15, (0x22, 0x22, 0x26), r=1)
    c.rect(31, 64, 20, 5, (0xC9, 0xCD, 0xD3))
    for k in range(3):
        c.rect(33 + k * 7, 85, 3, 4, (0xC9, 0xCD, 0xD3))
    c.text(30, 98, "U3", 7, SILK)
    smd(c, 64, 70, (0xB0, 0x8A, 0x58), vertical=True)
    smd(c, 82, 74, (0x1E, 0x1E, 0x22))
    smd(c, W // 2 - 34, 74, (0xB0, 0x8A, 0x58), vertical=True)
    smd(c, W // 2 + 28, 74, (0xB0, 0x8A, 0x58), vertical=True)
    smd(c, W - 80, 66, (0x1E, 0x1E, 0x22))
    smd(c, W - 80, 80, (0x1E, 0x1E, 0x22))
    c.circle(W - 50, 76, 3.4, (0xFF, 0x00, 0x4D))
    c.circle(W - 51, 75, 1.2, (0xFF, 0x9D, 0xB8))
    c.text(W - 50, 92, "D1", 6, SILK, anchor="ms")
    # Its name, in silk.
    c.text(42, 31, "PICO-8 CART", 9, SILK, anchor="lm")
    c.text(W - 42, 31, "REV A", 7, SILK, anchor="rm")
    img = c.done()
    if seated:
        chip = chip_img()
        img.alpha_composite(chip, (SEAT_X[0], SEAT_Y))
    return img


def chip_img() -> Image.Image:
    """A core's chip: a matte black package, gull-wing legs top and bottom."""
    cw, ch = CHIP
    img = Image.new("RGBA", (cw * SS, ch * SS), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    s = SS
    for k in range(8):
        px = 8 + k * 11
        for y0, y1 in ((0, 8), (ch - 8, ch)):
            d.rectangle(((px - 2) * s, y0 * s, (px + 2) * s - 1, y1 * s - 1), fill=(0xC4, 0xC8, 0xCE, 255))
            d.rectangle(((px - 2) * s, y0 * s, (px - 1) * s - 1, y1 * s - 1), fill=(0xE8, 0xEB, 0xEF, 255))
    d.rounded_rectangle((0, 6 * s, cw * s - 1, (ch - 6) * s - 1), 2 * s, fill=(0x16, 0x16, 0x1A, 255))
    d.rounded_rectangle((1 * s, 7 * s, (cw - 1) * s - 1, (ch - 7) * s - 1), 2 * s, fill=(0x22, 0x22, 0x27, 255))
    d.rectangle((2 * s, 7 * s, (cw - 2) * s - 1, 8 * s - 1), fill=(0x38, 0x38, 0x40, 255))
    d.ellipse((5 * s, (ch - 15) * s, 9 * s, (ch - 11) * s), fill=(0x12, 0x12, 0x15, 255))
    return img.resize((cw, ch), Image.LANCZOS)


# ---------------------------------------------------------------- renders


def keyed(path: Path, size=(W, H), slack=0.01) -> tuple[np.ndarray, np.ndarray]:
    """A render on pure green, scaled to `size`: its colour and its coverage."""
    w_out, h_out = size
    im = np.asarray(Image.open(path).convert("RGB"), np.float32) / 255.0
    r, g, b = im[..., 0], im[..., 1], im[..., 2]
    # Every pixel is the render over green: a * colour + (1 - a) * (0, 1, 0).
    a = np.clip(1.0 - np.clip(g - np.maximum(r, b), 0.0, 1.0), 0.0, 1.0)
    a[a < 0.02] = 0.0
    g = np.minimum(g - (1.0 - a), np.maximum(r, b) + 0.08 * a)
    pre = np.dstack([r, np.clip(g, 0.0, 1.0), b])  # premultiplied by a already
    big_h, big_w = a.shape
    if abs(big_w / big_h - w_out / h_out) > slack * w_out / h_out:
        raise SystemExit(f"{path}: {big_w}x{big_h} is not the art's {w_out}:{h_out} shape")

    def down(chan: np.ndarray) -> np.ndarray:
        img = Image.fromarray(chan.astype(np.float32), "F")
        return np.asarray(img.resize((w_out, h_out), Image.BOX))

    cover = np.clip(down(a), 0.0, 1.0)
    rgb = np.dstack([down(pre[..., k]) for k in range(3)])
    rgb = rgb / np.maximum(cover, 1e-4)[..., None]
    return np.clip(rgb * 255.0, 0, 255), cover


def feathered(box, r: int) -> np.ndarray:
    return box_blur(rect_mask(box), r)


def shell_from(path: Path) -> tuple[np.ndarray, np.ndarray]:
    """A rendered shell as slot's neutral grey: its flat face 50%, its strip dark."""
    rgb, cover = keyed(path)
    grey = rgb @ np.array([0.299, 0.587, 0.114], np.float32)
    # The flat face, beside the window, sets the grey: there it must be 50% exactly.
    lx, ly, lw, lh = LABEL
    face = grey[ly + 20 : ly + lh - 20, 10 : lx - LIP - 4]
    grey *= NEUTRAL / float(np.median(face))
    # The strip's floor as dark as the drawn one, whatever the render made of it.
    sx, sy, sw, sh = STRIP
    floor = (sx + 8, sy + 8, sw - 16, sh - 16)
    fx, fy, fw, fh = floor
    k = 44.0 / float(np.median(grey[fy : fy + fh, fx : fx + fw]))
    m = feathered((sx + 4, sy + 4, sw - 8, sh - 8), 2)
    grey = grey * (1.0 + (k - 1.0) * m)
    grey[ly : ly + lh, lx : lx + lw] = 119  # the flat the label is stuck on
    return grey, cover


def interior_from(path: Path) -> tuple[np.ndarray, np.ndarray]:
    """A rendered back shell as slot's grey: its floor as dark as the drawn one's."""
    rgb, cover = keyed(path)
    grey = rgb @ np.array([0.299, 0.587, 0.114], np.float32)
    floor = grey[H // 3 : 2 * H // 3, W // 4 : 3 * W // 4]
    grey *= (NEUTRAL - 29) / float(np.median(floor))
    return grey, cover


def board_from(path: Path, size=(W, H), slack=0.01) -> Image.Image:
    rgb, cover = keyed(path, size, slack)
    a = np.clip(np.rint(cover * 255), 0, 255).astype(np.uint8)
    return Image.fromarray(np.dstack([np.clip(np.rint(rgb), 0, 255).astype(np.uint8), a]), "RGBA")


# ---------------------------------------------------------------- output


def rgba(grey: np.ndarray, cover: np.ndarray) -> Image.Image:
    g = np.clip(np.rint(grey), 0, 255).astype(np.uint8)
    a = np.clip(np.rint(cover * 255), 0, 255).astype(np.uint8)
    return Image.fromarray(np.dstack([g, g, g, a]), "RGBA")


def overlay(base: np.ndarray, colour: np.ndarray) -> np.ndarray:
    base = base.astype(np.int32)
    c = colour.astype(np.int32)
    lighter = c + ((255 - c) * (base - NEUTRAL) + 63) // 127
    darker = c - (c * (NEUTRAL - base) + 64) // NEUTRAL
    return np.clip(np.where(base >= NEUTRAL, lighter, darker), 0, 255)


def tinted(img: Image.Image, colour) -> Image.Image:
    a = np.array(img).astype(np.int32)
    out = a.copy()
    for c in range(3):
        out[..., c] = overlay(a[..., c], np.full(a.shape[:2], colour[c]))
    return Image.fromarray(out.astype(np.uint8), "RGBA")


def preview(neutral, inner, full, empty, out: Path) -> None:
    out.mkdir(parents=True, exist_ok=True)
    tiles = []
    for name, colour in [("charcoal", (0x33, 0x33, 0x37)), ("blue", (0x1D, 0x2B, 0x53)), ("red", (0xFF, 0x00, 0x4D)), ("peach", (0xFF, 0xCC, 0xAA))]:
        t = tinted(neutral, colour)
        t.save(out / f"pico8_{name}.png")
        tiles.append(t)
    for b in (full, empty):
        back = tinted(inner, (0x33, 0x33, 0x37))
        back.alpha_composite(b)
        tiles.append(back)
    sheet = Image.new("RGBA", (len(tiles) * (W + 16), H), (40, 40, 60, 255))
    for i, t in enumerate(tiles):
        sheet.alpha_composite(t, (i * (W + 16), 0))
    sheet.save(out / "pico8_sheet.png")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--shell", type=Path)
    ap.add_argument("--board", type=Path)
    ap.add_argument("--interior", type=Path)
    ap.add_argument("--chip", type=Path)
    ap.add_argument("--preview", type=Path)
    args = ap.parse_args()
    if args.shell:
        grey, cover = shell_from(args.shell)
    else:
        cover = coverage()
        grey = shell(cover)
    rgba(np.full((H, W), 255.0), cover).save(ASSETS / "pico8_cart.png", optimize=True)
    neutral = rgba(grey, cover)
    neutral.save(ASSETS / "pico8_neutral.png", optimize=True)
    if args.interior:
        inner = rgba(*interior_from(args.interior))
    else:
        inner = rgba(interior(cover), cover)
    inner.save(ASSETS / "pico8_interior.png", optimize=True)
    empty = board_from(args.board) if args.board else board(False)
    empty.save(ASSETS / "pico8_board_empty.png", optimize=True)
    # A chip's render may be a little off its shape: it is only squeezed to fit.
    chip = board_from(args.chip, CHIP, 0.03) if args.chip else chip_img()
    chip.save(ASSETS / "pico8_chip.png", optimize=True)
    full = empty.copy()
    full.alpha_composite(chip, (SEAT_X[0], SEAT_Y))
    full.save(ASSETS / "pico8_board.png", optimize=True)
    if args.preview:
        preview(neutral, inner, full, empty, args.preview)


if __name__ == "__main__":
    main()
