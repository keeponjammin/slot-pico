//! fake-08, the core PICO-8 carts play on inside slot, driven through slot's own libretro host.
//! Each test skips when there is no fake-08 built for this host in `vendor/`.

mod common;

use std::path::{Path, PathBuf};

use slot_retro::{ButtonMask, RetroCore};
use slot_store::{Core, Platform};

fn dylib() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../vendor")
        .join(slot::core::dylib_name(Core::Fake08))
}

/// A cart whose whole screen is the PICO-8 colour of how many frames it has drawn, mod 16.
const COUNTING_CART: &str = "pico-8 cartridge // http://www.pico-8.com
version 41
__lua__
t={n=0}
step=function() t.n+=1 end
function _update60() step() end
function _draw() cls(t.n%16) end
";

fn open(root: &Path) -> Option<Box<dyn RetroCore>> {
    let path = dylib();
    if !path.exists() {
        eprintln!("no fake-08 on this host at {}, skipping", path.display());
        return None;
    }
    Some(slot::core::open_core_for_palette(
        root,
        Core::Fake08,
        Platform::Pico8,
        "auto",
        false,
        None,
        &[path],
    ))
}

fn cart(root: &Path) -> PathBuf {
    let rom = root.join("count.p8");
    std::fs::write(&rom, COUNTING_CART).unwrap();
    rom
}

/// slot keeps every core's picture centred in a Game Boy Advance sized frame.
fn pixel(core: &dyn RetroCore, x: u32, y: u32) -> [u8; 3] {
    let (w, h) = core.video_size();
    let (ox, oy) = ((w - 128) / 2, (h - 128) / 2);
    let at = (((y + oy) * w + x + ox) * 4) as usize;
    let px = &core.video_xrgb8888()[at..at + 4];
    [px[2], px[1], px[0]]
}

fn centre(core: &dyn RetroCore) -> [u8; 3] {
    pixel(core, 64, 64)
}

fn run(core: &mut dyn RetroCore, frames: usize) {
    for _ in 0..frames {
        core.run_frame(ButtonMask(0));
        let _ = core.take_audio();
    }
}

/// PICO-8's colour `i` as fake-08 draws it, in RGB565 and back.
fn pico_colour(i: usize) -> [u8; 3] {
    const PALETTE: [[u8; 3]; 16] = [
        [0x00, 0x00, 0x00],
        [0x1d, 0x2b, 0x53],
        [0x7e, 0x25, 0x53],
        [0x00, 0x87, 0x51],
        [0xab, 0x52, 0x36],
        [0x5f, 0x57, 0x4f],
        [0xc2, 0xc3, 0xc7],
        [0xff, 0xf1, 0xe8],
        [0xff, 0x00, 0x4d],
        [0xff, 0xa3, 0x00],
        [0xff, 0xec, 0x27],
        [0x00, 0xe4, 0x36],
        [0x29, 0xad, 0xff],
        [0x83, 0x76, 0x9c],
        [0xff, 0x77, 0xa8],
        [0xff, 0xcc, 0xaa],
    ];
    PALETTE[i]
}

fn close_to(got: [u8; 3], want: [u8; 3]) -> bool {
    got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= 8)
}

#[test]
fn a_cart_plays_at_pico8s_own_size_and_rate() {
    let _g = common::core_lock();
    let d = tempfile::tempdir().unwrap();
    let Some(mut core) = open(d.path()) else {
        return;
    };
    core.load(&cart(d.path()))
        .expect("fake-08 loads a .p8 cart");
    let av = core.av_info();
    assert_eq!(av.fps, 60.0);
    assert_eq!(av.sample_rate, 22050.0);
    // The cart has drawn 5 frames: cls(5), across the whole 128 by 128 screen.
    run(core.as_mut(), 5);
    for (x, y) in [(0, 0), (127, 0), (64, 64), (0, 127), (127, 127)] {
        let got = pixel(core.as_ref(), x, y);
        assert!(
            close_to(got, pico_colour(5)),
            "({x}, {y}) is {got:?}, not PICO-8's colour 5"
        );
    }
}

#[test]
fn a_state_restored_straight_after_loading_lands_on_the_cart() {
    let _g = common::core_lock();
    let d = tempfile::tempdir().unwrap();
    let Some(mut core) = open(d.path()) else {
        return;
    };
    let rom = cart(d.path());
    core.load(&rom).unwrap();
    run(core.as_mut(), 5);
    let saved_at = centre(core.as_ref());
    let state = core.serialize().expect("fake-08 saves states");
    drop(core);

    // As slot resumes a cart: load it and restore the state before any frame has run.
    let mut core = open(d.path()).unwrap();
    core.load(&rom).unwrap();
    core.unserialize(&state).expect("the state is taken");
    run(core.as_mut(), 1);
    let resumed = centre(core.as_ref());
    assert!(close_to(saved_at, pico_colour(5)), "saved at {saved_at:?}");
    assert!(
        close_to(resumed, pico_colour(6)),
        "the restored cart did not carry on from frame 5: it drew {resumed:?}"
    );
}

#[test]
fn a_file_that_is_no_cart_is_refused() {
    let _g = common::core_lock();
    let d = tempfile::tempdir().unwrap();
    let Some(mut core) = open(d.path()) else {
        return;
    };
    let rom = d.path().join("junk.p8");
    std::fs::write(&rom, b"this is not a cart").unwrap();
    assert!(
        core.load(&rom).is_err(),
        "fake-08 took a file with no Lua in it"
    );
}

#[test]
fn cart_data_is_kept_where_the_owners_pico8_keeps_it() {
    let _g = common::core_lock();
    let d = tempfile::tempdir().unwrap();
    let Some(mut core) = open(d.path()) else {
        return;
    };
    let rom = d.path().join("saver.p8");
    std::fs::write(
        &rom,
        "pico-8 cartridge // http://www.pico-8.com
version 41
__lua__
cartdata(\"slot_fake08_test\")
dset(0, 42)
function _draw() cls(1) end
",
    )
    .unwrap();
    core.load(&rom).unwrap();
    run(core.as_mut(), 3);
    drop(core);
    let saved = slot::pico8::home_dir(d.path()).join("cdata/slot_fake08_test.p8d.txt");
    assert!(saved.exists(), "no cart data at {}", saved.display());
}
