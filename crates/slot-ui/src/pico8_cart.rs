//! The PICO-8 cart: its own mould, apart from the Game Boy's paks.
//!
//! PICO-8 is a fantasy console with no cartridge of its own, so slot's is modelled on the
//! printed carts PICO-8 fans make: a matte slab a little taller than wide, a square label
//! window with a black lip, a dark strip under it with the cart's name, grip grooves in both
//! sides, and inside a purple board with gold contacts. All of it is drawn by
//! `tools/pico8_cart.py`: `assets/pico8_cart.png` (outline), `assets/pico8_neutral.png` (the
//! shell slot recolours), `assets/pico8_interior.png` (the back plastic of a clear one),
//! `assets/pico8_board.png` and `assets/pico8_board_empty.png` (the board, with a chip and
//! without) and `assets/pico8_chip.png` (a core's chip). Everything here belongs to PICO-8
//! alone; none of the Game Boy code decides its shape.

use std::sync::OnceLock;

use crate::bitmaps;

/// The PICO-8 cart's size, in the pixels of its art.
pub const PICO8_CART_W: u32 = 264;
pub const PICO8_CART_H: u32 = 328;

/// The PICO-8 cart's label box: the square inside the window drawn in
/// `assets/pico8_neutral.png`. Square, as every PICO-8 label is. Keep in step with `LABEL` in
/// `tools/pico8_cart.py`.
pub const PICO8_LABEL_X: u32 = 32;
pub const PICO8_LABEL_Y: u32 = 28;
pub const PICO8_LABEL_W: u32 = 200;
pub const PICO8_LABEL_H: u32 = 200;

/// The dark strip under the label the cart's name is printed on: x, y, w, h. Keep in step with
/// `STRIP` in `tools/pico8_cart.py`.
pub const PICO8_STRIP: (u32, u32, u32, u32) = (27, 242, 210, 56);

/// How tall a PICO-8 cart stands seated in the slot: as tall as the paks stand, so the cart
/// beside them in the slot reads as a cart of the same handheld.
pub const PICO8_SEATED_H: u32 = 259;

/// A core's chip, as drawn in `assets/pico8_chip.png`.
pub const PICO8_CHIP_W: u32 = 92;
pub const PICO8_CHIP_H: u32 = 52;

/// Where the two chips seat on the board, in its art's pixels: the left edge of each seat and
/// their top. Keep in step with `SEAT_X` and `SEAT_Y` in `tools/pico8_cart.py`.
pub const PICO8_SEAT_X: [f32; 2] = [28.0, 144.0];
pub const PICO8_SEAT_Y: f32 = 128.0;

/// Where the open cart rests in the core picker: the board's height and top, the lid's height
/// and centre, and how far the lid slides up before it lifts.
pub const PICO8_BOARD_Y: f32 = 128.0;
pub const PICO8_BOARD_REST_H: f32 = 284.0;
pub const PICO8_LID_REST_H: f32 = 92.0;
pub const PICO8_LID_REST_Y: f32 = 56.0;
pub const PICO8_SLIDE_UP: f32 = 36.0;

/// The shelf's box behind a PICO-8 cart, and the size of the label art made for one.
pub const PICO8_BOX: (u32, u32) = (312, 277);
pub const PICO8_HOUSE_LABEL: (u32, u32) = (1024, 901);

/// The PICO-8 cart's outline as coverage, one byte a pixel.
pub fn pico8_silhouette() -> Vec<u8> {
    let rgba = bitmaps::pico8_cart();
    debug_assert_eq!(rgba.len(), (PICO8_CART_W * PICO8_CART_H * 4) as usize);
    rgba.iter().skip(3).step_by(4).copied().collect()
}

pub(crate) fn pico8_cart_mask() -> &'static [u8] {
    static MASK: OnceLock<Vec<u8>> = OnceLock::new();
    MASK.get_or_init(pico8_silhouette)
}

/// PICO-8's own 3x5 letters, each row three bits, the leftmost the highest. Lower case is
/// printed as upper case, as PICO-8 itself prints it.
pub fn glyph(c: char) -> [u8; 5] {
    match c.to_ascii_uppercase() {
        'A' => [7, 5, 7, 5, 5],
        'B' => [7, 5, 6, 5, 7],
        'C' => [7, 4, 4, 4, 7],
        'D' => [6, 5, 5, 5, 6],
        'E' => [7, 4, 6, 4, 7],
        'F' => [7, 4, 6, 4, 4],
        'G' => [7, 4, 5, 5, 7],
        'H' => [5, 5, 7, 5, 5],
        'I' => [7, 2, 2, 2, 7],
        'J' => [7, 2, 2, 2, 6],
        'K' => [5, 5, 6, 5, 5],
        'L' => [4, 4, 4, 4, 7],
        'M' => [7, 7, 5, 5, 5],
        'N' => [6, 5, 5, 5, 5],
        'O' | '0' => [7, 5, 5, 5, 7],
        'P' => [7, 5, 7, 4, 4],
        'Q' => [7, 5, 5, 6, 3],
        'R' => [7, 5, 6, 5, 5],
        'S' => [7, 4, 7, 1, 7],
        'T' => [7, 2, 2, 2, 2],
        'U' => [5, 5, 5, 5, 7],
        'V' => [5, 5, 5, 5, 2],
        'W' => [5, 5, 5, 7, 7],
        'X' => [5, 5, 2, 5, 5],
        'Y' => [5, 5, 7, 2, 2],
        'Z' => [7, 1, 2, 4, 7],
        '1' => [6, 2, 2, 2, 7],
        '2' => [7, 1, 7, 4, 7],
        '3' => [7, 1, 3, 1, 7],
        '4' => [5, 5, 7, 1, 1],
        '5' => [7, 4, 7, 1, 7],
        '6' => [4, 4, 7, 5, 7],
        '7' => [7, 1, 1, 1, 1],
        '8' => [7, 5, 7, 5, 7],
        '9' => [7, 5, 7, 1, 1],
        '-' => [0, 0, 7, 0, 0],
        '.' => [0, 0, 0, 0, 2],
        ',' => [0, 0, 0, 2, 4],
        ':' => [0, 2, 0, 2, 0],
        '!' => [2, 2, 2, 0, 2],
        '?' => [7, 1, 3, 0, 2],
        '\'' => [2, 2, 0, 0, 0],
        '&' => [6, 5, 6, 5, 7],
        '+' => [0, 2, 7, 2, 0],
        '/' => [1, 1, 2, 4, 4],
        '(' => [2, 4, 4, 4, 2],
        ')' => [2, 1, 1, 1, 2],
        _ => [0; 5],
    }
}

const STRIP_PX: i32 = 2;
const STRIP_PAD: i32 = 9;
const STRIP_LINE: i32 = 7 * STRIP_PX;
const STRIP_TITLE_INK: [u8; 3] = [0xff, 0xf1, 0xe8];
const STRIP_INK: [u8; 3] = [0xc2, 0xc3, 0xc7];
/// The last line on every strip, as on the printed carts.
const STRIP_FOOT: &str = "PICO-8 CARTRIDGE";

/// The lines printed on a cart's strip: its title, wrapped to two lines at most and cut short
/// past that, then [`STRIP_FOOT`].
pub fn strip_lines(title: &str) -> Vec<String> {
    let room = ((PICO8_STRIP.2 as i32 - 2 * STRIP_PAD + 1) / (4 * STRIP_PX)).max(1) as usize;
    let mut lines: Vec<String> = Vec::new();
    for word in title.split_whitespace() {
        let word: String = word.chars().take(room).collect();
        match lines.last_mut() {
            Some(line) if line.chars().count() + 1 + word.chars().count() <= room => {
                line.push(' ');
                line.push_str(&word);
            }
            _ => lines.push(word),
        }
    }
    if lines.len() > 2 {
        lines.truncate(2);
        let last = &mut lines[1];
        while last.chars().count() + 2 > room {
            last.pop();
        }
        last.push_str("..");
    }
    lines.push(STRIP_FOOT.to_string());
    lines
}

/// Prints `title` and the strip's last line on a PICO-8 cart's face (`w` wide), in PICO-8's
/// own letters, as the printed carts carry them.
pub fn print_strip(rgba: &mut [u8], w: u32, title: &str) {
    let (sx, sy, _, sh) = PICO8_STRIP;
    let lines = strip_lines(title);
    let block = lines.len() as i32 * STRIP_LINE - 2 * STRIP_PX;
    let mut y = sy as i32 + (sh as i32 - block) / 2;
    let last = lines.len() - 1;
    for (i, line) in lines.iter().enumerate() {
        let (ink, alpha) = match i == last {
            true => (STRIP_INK, 0xc0),
            false => (STRIP_TITLE_INK, 0xff),
        };
        print_pixels(
            rgba,
            w,
            line,
            sx as i32 + STRIP_PAD,
            y,
            STRIP_PX,
            ink,
            alpha,
        );
        y += STRIP_LINE;
    }
}

/// How many letter cells wide `text` prints in PICO-8's letters: four a letter, less the gap
/// after the last.
pub fn pixel_text_w(text: &str) -> u32 {
    (text.chars().count() as u32 * 4).saturating_sub(1)
}

/// Prints `text` in PICO-8's letters onto `rgba` (`w` wide), each letter pixel `px` square,
/// from `(x, y)`, in `ink` at `alpha`.
#[allow(clippy::too_many_arguments)]
pub fn print_pixels(
    rgba: &mut [u8],
    w: u32,
    text: &str,
    x: i32,
    y: i32,
    px: i32,
    ink: [u8; 3],
    alpha: u8,
) {
    let h = (rgba.len() / 4) as u32 / w.max(1);
    let mut put = |cx: i32, cy: i32| {
        if cx < 0 || cy < 0 || cx >= w as i32 || cy >= h as i32 {
            return;
        }
        let at = ((cy as u32 * w + cx as u32) * 4) as usize;
        let a = alpha as u32;
        for k in 0..3 {
            rgba[at + k] = ((ink[k] as u32 * a + rgba[at + k] as u32 * (255 - a)) / 255) as u8;
        }
        rgba[at + 3] = rgba[at + 3].max(alpha);
    };
    for (i, c) in text.chars().enumerate() {
        let rows = glyph(c);
        for (r, bits) in rows.iter().enumerate() {
            for col in 0..3 {
                if bits & (4 >> col) == 0 {
                    continue;
                }
                let (gx, gy) = (x + (i as i32 * 4 + col) * px, y + r as i32 * px);
                for dy in 0..px {
                    for dx in 0..px {
                        put(gx + dx, gy + dy);
                    }
                }
            }
        }
    }
}
