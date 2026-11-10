//! The PICO-8 cart: its own mould, apart from the Game Boy's paks.
//!
//! PICO-8 is a fantasy console, so its cart is slot's own design rather than a copy of any
//! real cartridge: drawn by `tools/pico8_cart.py` into `assets/pico8_cart.png` (outline),
//! `assets/pico8_neutral.png` (the shell slot recolours) and `assets/pico8_interior.png`
//! (the back plastic of a clear one). Everything here belongs to PICO-8 alone; it sits the
//! size of a Game Boy pak in the slot, so it uses the same seats on the board, but none of the
//! Game Boy code decides its shape.

use std::sync::OnceLock;

use crate::bitmaps;

/// The PICO-8 cart's size, in the pixels of its art.
pub const PICO8_CART_W: u32 = 312;
pub const PICO8_CART_H: u32 = 337;

/// The PICO-8 cart's label box: the square well drawn in `assets/pico8_neutral.png`, x 50..262,
/// y 78..290 of 312x337. Square, as every PICO-8 label is. Keep in step with `LABEL` in
/// `tools/pico8_cart.py`.
pub const PICO8_LABEL_X: u32 = 50;
pub const PICO8_LABEL_Y: u32 = 78;
pub const PICO8_LABEL_W: u32 = 212;
pub const PICO8_LABEL_H: u32 = 212;

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
