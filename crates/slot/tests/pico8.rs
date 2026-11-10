mod common;

use std::io::Write as _;
use std::path::Path;

use slot::app::{App, Phase};
use slot::input::evdev::Keymap;
use slot::pico8::{self, Launch, Pad};
use slot_input::Btn;
use slot_store::{Core, Platform, SlotState};
use tempfile::TempDir;

fn install(root: &Path) {
    let dir = pico8::install_dir(root);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("pico8_dyn"), b"\x7fELF").unwrap();
    std::fs::write(dir.join("pico8.dat"), b"dat").unwrap();
}

fn write_cart(root: &Path, name: &str, bytes: &[u8]) {
    let dir = pico8::carts_dir(root);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(name), bytes).unwrap();
}

fn p8_with_label(colour: char) -> String {
    let row: String = std::iter::repeat_n(colour, 128).collect();
    let rows = vec![row; 128].join("\n");
    format!("pico-8 cartridge // http://www.pico-8.com\nversion 41\n__lua__\nprint(1)\n__label__\n{rows}\n__map__\n")
}

fn png(width: u32, height: u32, px: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    let mut rgba = Vec::new();
    for y in 0..height {
        for x in 0..width {
            rgba.extend_from_slice(&px(x, y));
        }
    }
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, width, height);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        e.write_header().unwrap().write_image_data(&rgba).unwrap();
    }
    out
}

#[test]
fn a_hand_off_reads_back_what_was_written() {
    for launch in [
        Launch::Splore,
        Launch::Cart("/mnt/sdcard/Games/PICO8/a b.p8.png".into()),
    ] {
        assert_eq!(Launch::decode(&launch.encode()), Some(launch));
    }
    assert_eq!(Launch::decode("pico8\ncart\n\n"), None);
    assert_eq!(Launch::decode("other\nsplore\n"), None);
}

#[test]
fn a_cart_runs_and_splore_opens_splore() {
    assert_eq!(Launch::Splore.args(), ["-splore"]);
    assert_eq!(Launch::Cart("/c/x.p8".into()).args(), ["-run", "/c/x.p8"]);
    assert_eq!(pico8::launch_for(Path::new("/c/SPLORE.P8")), Launch::Splore);
    assert_eq!(
        pico8::launch_for(Path::new("/c/celeste.p8")),
        Launch::Cart("/c/celeste.p8".into())
    );
}

#[test]
fn pico8_carts_are_p8_text_and_p8_png_pictures() {
    let p = Platform::Pico8;
    assert!(p.accepts(Path::new("celeste.p8")));
    assert!(p.accepts(Path::new("Celeste.P8.PNG")));
    assert!(!p.accepts(Path::new("screenshot.png")));
    assert!(!p.accepts(Path::new(".p8")));
    assert_eq!(p.stem_of(Path::new("celeste.p8.png")), Some("celeste"));
    assert_eq!(p.stem_of(Path::new("celeste.p8")), Some("celeste"));
    assert_eq!(
        Platform::Gba.stem_of(Path::new("Emerald.v1.gba")),
        Some("Emerald.v1")
    );
    assert!(!Platform::Gb.accepts(Path::new("celeste.p8")));
}

#[test]
fn pico8_carts_play_on_fake08_unless_set_to_the_owners_pico8() {
    let d = TempDir::new().unwrap();
    let gb = slot_store::Core::Mgba;
    assert_eq!(
        Core::pair(Platform::Pico8),
        [Core::Fake08, Core::Pico8],
        "the core picker flips between fake-08 and PICO-8"
    );
    assert_eq!(
        slot_store::core_for_platform(d.path(), "celeste", Platform::Pico8, gb),
        Core::Fake08
    );
    slot_store::write_selected_core(d.path(), Platform::Pico8, "celeste", Core::Pico8).unwrap();
    assert_eq!(
        slot_store::core_for_platform(d.path(), "celeste", Platform::Pico8, gb),
        Core::Pico8
    );
    assert!(Core::Pico8.runs_outside());
    assert!(!Core::Fake08.runs_outside());
    assert!(
        !Core::Mgba.runs(Platform::Pico8),
        "mGBA does not play PICO-8 carts"
    );
    assert!(!Core::Fake08.runs(Platform::Gbc));
}

#[test]
fn the_pico8_shelf_lists_carts_by_their_names() {
    let d = TempDir::new().unwrap();
    write_cart(d.path(), "celeste.p8.png", b"x");
    write_cart(d.path(), "jelpi.p8", b"x");
    write_cart(d.path(), "notes.txt", b"x");
    let carts = slot_store::scan(d.path()).unwrap();
    let stems: Vec<_> = carts
        .iter()
        .filter(|c| c.platform == Platform::Pico8)
        .map(|c| c.stem.as_str())
        .collect();
    assert_eq!(stems, ["celeste", "jelpi"]);
}

#[test]
fn a_p8_label_is_drawn_in_the_pico8_palette() {
    let rgba = pico8::label_from_p8(&p8_with_label('8')).unwrap();
    assert_eq!(rgba.len(), 128 * 128 * 4);
    assert_eq!(&rgba[..4], &[0xff, 0x00, 0x4d, 0xff]);
    let secret = pico8::label_from_p8(&p8_with_label('g')).unwrap();
    assert_eq!(&secret[..4], &[0x29, 0x18, 0x14, 0xff]);
}

#[test]
fn a_cart_without_a_captured_label_has_none() {
    assert_eq!(pico8::label_from_p8(&p8_with_label('0')), None);
    assert_eq!(pico8::label_from_p8("__lua__\nprint(1)\n"), None);
}

#[test]
fn a_png_cart_gives_the_picture_inside_its_label_window() {
    let cart = png(160, 205, |x, y| {
        match (16..144).contains(&x) && (24..152).contains(&y) {
            true => [0x29, 0xad, 0xff, 0xff],
            false => [0, 0, 0, 0xff],
        }
    });
    let rgba = pico8::label_from_png(&cart).unwrap();
    assert_eq!(rgba.len(), 128 * 128 * 4);
    assert!(rgba.chunks_exact(4).all(|p| p == [0x29, 0xad, 0xff, 0xff]));
    assert_eq!(pico8::label_from_png(&png(64, 64, |_, _| [0; 4])), None);
}

#[test]
fn the_splore_card_carries_a_label() {
    let text = String::from_utf8_lossy(pico8::SPLORE_CART);
    assert!(pico8::label_from_p8(&text).is_some());
}

#[test]
fn splore_is_stocked_once_and_stays_deleted() {
    let d = TempDir::new().unwrap();
    pico8::stock_shelf(d.path());
    let splore = pico8::carts_dir(d.path()).join("Splore.p8");
    assert!(!splore.exists(), "Splore without PICO-8 to run it");

    install(d.path());
    pico8::stock_shelf(d.path());
    assert!(splore.is_file());
    assert!(d.path().join("Labels/PICO8/Splore.png").is_file());

    std::fs::remove_file(&splore).unwrap();
    pico8::stock_shelf(d.path());
    assert!(!splore.exists(), "a deleted Splore came back");
}

#[test]
fn labels_are_written_once_for_carts_without_one() {
    let d = TempDir::new().unwrap();
    write_cart(d.path(), "red.p8", p8_with_label('8').as_bytes());
    write_cart(d.path(), "blank.p8", p8_with_label('0').as_bytes());
    let mine = d.path().join("Labels/PICO8/mine.png");
    write_cart(d.path(), "mine.p8", p8_with_label('8').as_bytes());
    std::fs::create_dir_all(mine.parent().unwrap()).unwrap();
    std::fs::write(&mine, b"keep").unwrap();
    pico8::write_labels(d.path());
    let red = slot_ui::decode_png(&d.path().join("Labels/PICO8/red.png")).expect("red label");
    assert_eq!(&red.0[..4], &[0xff, 0x00, 0x4d, 0xff]);
    assert!(!d.path().join("Labels/PICO8/blank.png").exists());
    assert_eq!(std::fs::read(&mine).unwrap(), b"keep");
}

#[test]
fn the_raspberry_pi_zip_is_unpacked_into_bios() {
    let d = TempDir::new().unwrap();
    assert!(!pico8::installed(d.path()));
    let bios = d.path().join("BIOS");
    std::fs::create_dir_all(&bios).unwrap();
    let file = std::fs::File::create(bios.join("pico-8_0.2.6b_raspi.zip")).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default();
    for (name, body) in [
        ("pico-8/pico8_dyn", "dyn"),
        ("pico-8/pico8_64", "64"),
        ("pico-8/pico8.dat", "dat"),
        ("pico-8/../escape", "no"),
    ] {
        zip.start_file(name, opts).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
    assert!(pico8::installed(d.path()));

    let (binary, arch) = pico8::unpack(d.path()).unwrap().unwrap();
    assert_eq!(binary, pico8::install_dir(d.path()).join("pico8_dyn"));
    assert_eq!(arch, pico8::Arch::Armhf);
    for escaped in ["BIOS/escape", "escape", "BIOS/PICO-8/escape"] {
        assert!(!d.path().join(escaped).exists(), "{escaped}");
    }
}

#[test]
fn the_64_bit_build_is_only_the_last_choice() {
    let d = TempDir::new().unwrap();
    let dir = d.path().join("pico-8");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("pico8.dat"), b"").unwrap();
    std::fs::write(dir.join("pico8_64"), b"").unwrap();
    assert_eq!(
        pico8::find_binary(d.path()).unwrap().1,
        pico8::Arch::Aarch64
    );
    std::fs::write(dir.join("pico8"), b"").unwrap();
    assert_eq!(pico8::find_binary(d.path()).unwrap().0, dir.join("pico8"));
}

#[test]
fn pico8_plays_at_slots_volume() {
    let quiet = SlotState {
        volume: 50,
        volume_hp: 25,
        ..SlotState::default()
    };
    assert_eq!(pico8::volume(&quiet, false), 128);
    assert_eq!(pico8::volume(&quiet, true), 64);
    let muted = SlotState {
        muted: true,
        ..quiet
    };
    assert_eq!(pico8::volume(&muted, false), 0);
}

#[test]
fn pico8_runs_itself_so_it_finds_pico8_dat_beside_it() {
    let line = pico8::command_line(
        Path::new("/mnt/sdcard"),
        Path::new("/mnt/sdcard/BIOS/PICO-8/pico8_dyn"),
        pico8::Arch::Armhf,
        &Launch::Splore,
        200,
        true,
    );
    assert_eq!(line[0], "/mnt/sdcard/BIOS/PICO-8/pico8_dyn");
    assert_eq!(line[1], "-home");
    assert!(!line.iter().any(|a| a.contains("ld-linux")));
}

#[cfg(unix)]
#[test]
fn only_an_executable_binary_runs_directly() {
    use std::os::unix::fs::PermissionsExt as _;
    let d = TempDir::new().unwrap();
    let bin = d.path().join("pico8_dyn");
    std::fs::write(&bin, b"\x7fELF").unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(!pico8::runs_directly(&bin));
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(pico8::runs_directly(&bin));
    assert!(!pico8::runs_directly(&d.path().join("missing")));
}

#[test]
fn sdl_is_pointed_at_the_first_gles_and_egl_it_can_find() {
    let d = TempDir::new().unwrap();
    let own = d.path().join("own");
    let lib32 = d.path().join("lib32");
    std::fs::create_dir_all(&own).unwrap();
    std::fs::create_dir_all(&lib32).unwrap();
    std::fs::write(lib32.join("libGLESv2.so.2"), b"").unwrap();
    std::fs::write(lib32.join("libEGL.so.1"), b"").unwrap();
    std::fs::write(own.join("libEGL.so.1"), b"").unwrap();
    let dirs = vec![own.clone(), lib32.clone()];
    assert_eq!(
        pico8::gl_drivers(&dirs),
        vec![
            ("SDL_VIDEO_GL_DRIVER", lib32.join("libGLESv2.so.2")),
            ("SDL_VIDEO_EGL_DRIVER", own.join("libEGL.so.1")),
        ]
    );
    assert!(pico8::gl_drivers(&[d.path().join("none")]).is_empty());
}

#[test]
fn pico8_runs_through_its_loader_with_its_folders_on_the_card() {
    let root = Path::new("/mnt/sdcard");
    let line = pico8::command_line(
        root,
        Path::new("/mnt/sdcard/BIOS/PICO-8/pico8_dyn"),
        pico8::Arch::Armhf,
        &Launch::Splore,
        200,
        false,
    );
    assert_eq!(line[0], "/lib/ld-linux-armhf.so.3");
    assert_eq!(line[1], "/mnt/sdcard/BIOS/PICO-8/pico8_dyn");
    let after = |flag: &str| {
        let at = line.iter().position(|a| a == flag).unwrap();
        line[at + 1].clone()
    };
    assert_eq!(after("-home"), "/mnt/sdcard/Saves/PICO8");
    assert_eq!(after("-root_path"), "/mnt/sdcard/Games/PICO8");
    assert_eq!(after("-volume"), "200");
    assert_eq!(line.last().unwrap(), "-splore");
    assert_eq!(
        pico8::sdl_shim(root),
        Path::new("/mnt/sdcard/System/pico8_sdl.so")
    );
    assert_eq!(
        pico8::library_path(Path::new("/i"), pico8::Arch::Armhf),
        "/i/lib:/usr/lib32:/usr/lib/arm-linux-gnueabihf"
    );
}

fn h700_pad() -> Pad {
    Pad {
        name: "H700 Gamepad".into(),
        bus: 0x19,
        vendor: 0x0001,
        product: 0x0002,
        version: 0x0100,
        keys: vec![
            114, 115, 0x130, 0x131, 0x132, 0x133, 0x134, 0x135, 0x136, 0x137, 0x138, 0x13a, 0x13b,
        ],
        axes: vec![0x02, 0x03, 0x05, 0x10, 0x11],
    }
}

#[test]
fn the_guid_is_the_one_sdl_builds_from_sysfs_ids() {
    assert_eq!(h700_pad().guid(), "19000000010000000200000000010000");
    let nameless_ids = Pad {
        vendor: 0,
        name: "gpio-keys".into(),
        ..h700_pad()
    };
    assert_eq!(nameless_ids.guid(), "190000006770696f2d6b657973000000");
}

#[test]
fn the_mapping_places_buttons_where_they_sit() {
    let line = h700_pad().mapping(Keymap::BaseOs).unwrap();
    let fields: Vec<&str> = line.trim_end_matches(',').split(',').collect();
    assert_eq!(fields[0], "19000000010000000200000000010000");
    assert_eq!(fields[1], "H700 Gamepad");
    // Buttons count from BTN_JOYSTICK up, then the keys below it.
    for want in [
        "a:b1",
        "b:b0",
        "x:b2",
        "y:b3",
        "leftshoulder:b4",
        "rightshoulder:b5",
        "back:b6",
        "start:b7",
        "lefttrigger:b9",
        "righttrigger:b10",
        "dpup:h0.1",
        "dpleft:h0.8",
        "leftx:a0",
        "lefty:a1",
        "platform:Linux",
    ] {
        assert!(fields.contains(&want), "{want} missing from {line}");
    }
    assert!(!line.contains("guide"), "MENU belongs to slot: {line}");
}

#[test]
fn a_device_with_no_face_buttons_gets_no_mapping() {
    let keys = Pad {
        keys: vec![114, 115, 116],
        ..h700_pad()
    };
    assert_eq!(keys.mapping(Keymap::BaseOs), None);
}

#[test]
fn the_mapping_is_read_from_sysfs() {
    let d = TempDir::new().unwrap();
    let dev = d.path().join("dev");
    let sys = d.path().join("sys");
    std::fs::create_dir_all(&dev).unwrap();
    std::fs::write(dev.join("event1"), b"").unwrap();
    let device = sys.join("event1/device");
    std::fs::create_dir_all(device.join("id")).unwrap();
    std::fs::create_dir_all(device.join("capabilities")).unwrap();
    std::fs::write(device.join("name"), "H700 Gamepad\n").unwrap();
    for (f, v) in [
        ("bustype", "0019"),
        ("vendor", "0001"),
        ("product", "0002"),
        ("version", "0100"),
    ] {
        std::fs::write(device.join("id").join(f), format!("{v}\n")).unwrap();
    }
    // 0x130..=0x13b without 0x139, in the 64-bit words sysfs prints.
    std::fs::write(
        device.join("capabilities/key"),
        "0 dff000000000000 0 0 0 0\n",
    )
    .unwrap();
    std::fs::write(device.join("capabilities/abs"), "3002c\n").unwrap();
    let lines = pico8::controller_mappings(Keymap::BaseOs, &dev, &sys);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].starts_with("19000000010000000200000000010000,H700 Gamepad,a:b1,b:b0,"));
    assert!(lines[0].contains("leftx:a0,lefty:a1"), "{}", lines[0]);
    assert_eq!(
        pico8::joystick_devices(Keymap::BaseOs, &dev, &sys),
        dev.join("event1").display().to_string(),
    );
}

/// `SLOT_RUN` is one variable for the whole test binary: tests that hand off through it take
/// turns, or one reads the hand-off the other wrote.
static RUN_VAR: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn hand_off_through(dir: &Path) -> std::sync::MutexGuard<'static, ()> {
    let held = RUN_VAR.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    std::fs::create_dir_all(dir).unwrap();
    std::env::set_var(slot::loading_mark::RUN_VAR, dir);
    held
}

/// Plays `stem` on the owner's own PICO-8 instead of fake-08.
fn on_pico8(root: &Path, stem: &str) {
    slot_store::write_selected_core(root, Platform::Pico8, stem, Core::Pico8).unwrap();
}

fn to_pico8_shelf(a: &mut App) {
    for _ in 0..8 {
        if a.shelf_platform_name() == Some("PICO-8") {
            return;
        }
        common::through(a, Btn::R1);
        for _ in 0..60 {
            a.update(1.0 / 60.0);
        }
    }
    panic!("no PICO-8 shelf, on {:?}", a.shelf_platform_name());
}

#[test]
fn playing_a_pico8_cart_hands_the_device_over_and_comes_back_to_it() {
    let d = TempDir::new().unwrap();
    let run = d.path().join("run");
    let _turn = hand_off_through(&run);
    install(d.path());
    write_cart(d.path(), "celeste.p8", p8_with_label('c').as_bytes());
    write_cart(d.path(), "jelpi.p8", b"");
    on_pico8(d.path(), "jelpi");
    slot::root::ensure(d.path());
    // A second shelf, so there is a shelf to name.
    std::fs::write(d.path().join("Games/GBA/Emerald.gba"), [0u8; 512]).unwrap();
    common::clocked(d.path());
    let mut a = App::boot(d.path());
    to_pico8_shelf(&mut a);
    assert_eq!(a.selected_stem(), Some("celeste"));
    common::through(&mut a, Btn::Right);
    for _ in 0..30 {
        a.update(1.0 / 60.0);
    }
    assert_eq!(a.selected_stem(), Some("jelpi"));
    common::through(&mut a, Btn::A);
    assert!(
        matches!(a.phase(), Phase::Inserting { cart, .. } if cart == "jelpi"),
        "a PICO-8 cart goes into the slot like any other, not straight to PICO-8: {:?}",
        a.phase()
    );
    let mut seated = false;
    let mut frames = 0;
    while !a.relaunching() {
        assert!(
            !run.join(pico8::NEXT_FILE).exists(),
            "handed off before the insert had played"
        );
        assert_eq!(
            a.screen_power(),
            0.0,
            "the screen went dark before PICO-8 had it: the seated cart stays on screen instead"
        );
        a.update(1.0 / 60.0);
        frames += 1;
        assert!(frames < 120, "never handed off: {:?}", a.phase());
        seated |= a.seat() >= 1.0;
    }
    assert!(seated, "handed off before the cart was seated");
    let played = frames as f32 / 60.0;
    assert!(
        played >= slot::app::HAND_OFF_AT - 1.0 / 60.0,
        "handed off after {played} s, before the insert's {} s",
        slot::app::HAND_OFF_AT
    );
    let next = std::fs::read_to_string(run.join(pico8::NEXT_FILE)).unwrap();
    assert_eq!(
        Launch::decode(&next),
        Some(Launch::Cart(pico8::carts_dir(d.path()).join("jelpi.p8")))
    );
    let state = slot_store::read_slot_state(d.path());
    assert_eq!(
        (state.cart.as_deref(), state.cart_platform),
        (Some("jelpi"), Some(Platform::Pico8)),
        "the cart is in the slot while PICO-8 runs it"
    );
    assert_eq!(
        state.recents.first().map(|r| r.stem.as_str()),
        Some("jelpi")
    );

    let mut a = App::boot(d.path());
    assert!(
        matches!(a.phase(), Phase::Ejecting { cart, .. } if cart == "jelpi"),
        "coming back from PICO-8 ejects the cart instead of starting it again: {:?}",
        a.phase()
    );
    assert_eq!(
        (a.screen_power(), a.seat()),
        (0.0, 1.0),
        "slot comes back to the seated cart PICO-8 left on screen, not to the dark"
    );
    assert!(!a.relaunching());
    for _ in 0..120 {
        a.update(1.0 / 60.0);
        assert!(!a.relaunching(), "coming back handed the cart off again");
    }
    assert!(matches!(a.phase(), Phase::Shelf), "{:?}", a.phase());
    assert_eq!(a.screen_power(), 0.0);
    assert_eq!(a.shelf_platform_name(), Some("PICO-8"));
    assert_eq!(a.selected_stem(), Some("jelpi"));
    assert_eq!(slot_store::read_slot_state(d.path()).cart, None);

    let a = App::boot(d.path());
    assert!(
        matches!(a.phase(), Phase::Shelf),
        "once ejected, the next boot starts on the shelf: {:?}",
        a.phase()
    );
}

#[test]
fn ejecting_a_pico8_cart_while_it_goes_in_keeps_slot_running() {
    let d = TempDir::new().unwrap();
    let run = d.path().join("run");
    let _turn = hand_off_through(&run);
    install(d.path());
    write_cart(d.path(), "celeste.p8", b"");
    on_pico8(d.path(), "celeste");
    slot::root::ensure(d.path());
    std::fs::write(d.path().join("Games/GBA/Emerald.gba"), [0u8; 512]).unwrap();
    common::clocked(d.path());
    let mut a = App::boot(d.path());
    to_pico8_shelf(&mut a);
    common::through(&mut a, Btn::A);
    for _ in 0..10 {
        a.update(1.0 / 60.0);
    }
    assert!(matches!(a.phase(), Phase::Inserting { .. }), "{:?}", a.phase());
    a.apply(slot_input::Action::Eject);
    for _ in 0..120 {
        a.update(1.0 / 60.0);
    }
    assert!(!a.relaunching(), "an ejected cart was still handed to PICO-8");
    assert!(!run.join(pico8::NEXT_FILE).exists());
    assert!(matches!(a.phase(), Phase::Shelf), "{:?}", a.phase());
}

#[test]
fn a_cart_set_to_pico8_plays_on_fake08_while_pico8_is_not_on_the_card() {
    let d = TempDir::new().unwrap();
    let run = d.path().join("run");
    let _turn = hand_off_through(&run);
    write_cart(d.path(), "celeste.p8", b"");
    on_pico8(d.path(), "celeste");
    slot::root::ensure(d.path());
    // A second shelf, so there is a shelf to name.
    std::fs::write(d.path().join("Games/GBA/Emerald.gba"), [0u8; 512]).unwrap();
    common::clocked(d.path());
    let mut a = App::boot(d.path());
    to_pico8_shelf(&mut a);
    common::through(&mut a, Btn::A);
    assert!(
        matches!(a.phase(), Phase::Inserting { cart, .. } if cart == "celeste"),
        "{:?}",
        a.phase()
    );
    for _ in 0..120 {
        a.update(1.0 / 60.0);
        assert!(!a.relaunching(), "handed to a PICO-8 that is not there");
    }
    assert!(!a.seated_outside());
    assert!(!run.join(pico8::NEXT_FILE).exists());
}

#[test]
fn without_pico8_its_chip_cannot_be_seated() {
    let d = TempDir::new().unwrap();
    write_cart(d.path(), "celeste.p8", b"");
    on_pico8(d.path(), "celeste");
    slot::root::ensure(d.path());
    std::fs::write(d.path().join("Games/GBA/Emerald.gba"), [0u8; 512]).unwrap();
    common::clocked(d.path());
    let mut a = App::boot(d.path());
    to_pico8_shelf(&mut a);
    open_board(&mut a);
    assert_eq!(
        a.core_picker(),
        Some(Core::Fake08),
        "the board opens on the core the cart can play on"
    );
    a.apply(slot_input::Action::GbaDown(Btn::Right));
    a.update(1.0 / 60.0);
    assert_ne!(
        a.core_picker_chip().map(|c| c.shake),
        Some(0.0),
        "moving to PICO-8 did not shake"
    );
    for _ in 0..30 {
        a.update(1.0 / 60.0);
    }
    assert_eq!(a.core_picker(), Some(Core::Fake08), "PICO-8 was seated");
}

#[test]
fn with_pico8_its_chip_can_be_seated() {
    let d = TempDir::new().unwrap();
    install(d.path());
    write_cart(d.path(), "celeste.p8", b"");
    slot::root::ensure(d.path());
    std::fs::write(d.path().join("Games/GBA/Emerald.gba"), [0u8; 512]).unwrap();
    common::clocked(d.path());
    let mut a = App::boot(d.path());
    to_pico8_shelf(&mut a);
    assert_eq!(a.selected_stem(), Some("celeste"));
    open_board(&mut a);
    a.apply(slot_input::Action::GbaDown(Btn::Right));
    assert_eq!(a.core_picker(), Some(Core::Pico8));
}

/// Opens the core board and waits out its faces, which no renderer draws here.
fn open_board(a: &mut App) {
    a.apply(slot_input::Action::GbaDown(Btn::Start));
    for _ in 0..120 {
        a.update(1.0 / 60.0);
    }
}

#[test]
fn splore_leaves_the_shelf_with_pico8_and_comes_back_with_it() {
    let d = TempDir::new().unwrap();
    install(d.path());
    write_cart(d.path(), "celeste.p8", b"");
    slot::root::ensure(d.path());
    std::fs::write(d.path().join("Games/GBA/Emerald.gba"), [0u8; 512]).unwrap();
    common::clocked(d.path());
    let stems = |a: &App| -> Vec<String> {
        a.carts()
            .filter(|c| c.platform == Platform::Pico8)
            .map(|c| c.stem.clone())
            .collect()
    };
    let a = App::boot(d.path());
    assert_eq!(stems(&a), ["celeste", pico8::SPLORE]);

    std::fs::remove_dir_all(pico8::install_dir(d.path())).unwrap();
    let a = App::boot(d.path());
    assert_eq!(
        stems(&a),
        ["celeste"],
        "Splore with no PICO-8 to open it, or the carts gone with it"
    );
    assert!(
        pico8::carts_dir(d.path()).join("Splore.p8").is_file(),
        "the Splore card was deleted, not left off"
    );

    install(d.path());
    let a = App::boot(d.path());
    assert_eq!(stems(&a), ["celeste", pico8::SPLORE]);
}

#[test]
fn a_fake08_cart_goes_in_like_any_other_without_pico8_installed() {
    let d = TempDir::new().unwrap();
    let run = d.path().join("run");
    let _turn = hand_off_through(&run);
    write_cart(d.path(), "celeste.p8", b"");
    slot::root::ensure(d.path());
    std::fs::write(d.path().join("Games/GBA/Emerald.gba"), [0u8; 512]).unwrap();
    common::clocked(d.path());
    let mut a = App::boot(d.path());
    to_pico8_shelf(&mut a);
    common::through(&mut a, Btn::A);
    assert!(
        matches!(a.phase(), Phase::Inserting { cart, .. } if cart == "celeste"),
        "fake-08 needs no PICO-8 of the owner's: {:?}",
        a.phase()
    );
    for _ in 0..120 {
        a.update(1.0 / 60.0);
        assert!(!a.relaunching(), "a fake-08 cart was handed to PICO-8");
    }
    assert!(!a.seated_outside());
    assert!(!run.join(pico8::NEXT_FILE).exists());
}

#[test]
fn the_splore_card_always_opens_the_owners_pico8() {
    let d = TempDir::new().unwrap();
    let run = d.path().join("run");
    let _turn = hand_off_through(&run);
    install(d.path());
    slot::root::ensure(d.path());
    pico8::stock_shelf(d.path());
    std::fs::write(d.path().join("Games/GBA/Emerald.gba"), [0u8; 512]).unwrap();
    common::clocked(d.path());
    let mut a = App::boot(d.path());
    to_pico8_shelf(&mut a);
    assert_eq!(a.selected_stem(), Some(pico8::SPLORE));
    common::through(&mut a, Btn::A);
    let mut frames = 0;
    while !a.relaunching() {
        a.update(1.0 / 60.0);
        frames += 1;
        assert!(frames < 120, "Splore never handed off: {:?}", a.phase());
    }
    let next = std::fs::read_to_string(run.join(pico8::NEXT_FILE)).unwrap();
    assert_eq!(Launch::decode(&next), Some(Launch::Splore));
}

#[test]
fn pico8_never_wears_the_game_boy_colors_bezel_or_overlay() {
    let d = TempDir::new().unwrap();
    write_cart(d.path(), "celeste.p8", b"");
    slot::root::ensure(d.path());
    common::clocked(d.path());
    let mut state = slot_store::read_slot_state(d.path());
    state.bezel_gbc = slot_store::BezelPick::Default;
    state.overlay_gbc = Some("frame.png".into());
    slot_store::write_slot_state(d.path(), &state).unwrap();
    let a = App::boot(d.path());
    assert_eq!(
        a.resolved_bezel(Platform::Gbc),
        slot_store::BezelPick::Default,
        "the Game Boy Color keeps its own"
    );
    assert_eq!(
        a.resolved_bezel(Platform::Pico8),
        slot_store::BezelPick::None,
        "PICO-8 draws its whole screen, so its power-on has nothing around it"
    );
    assert!(a.resolved_overlay(Platform::Pico8).is_none());
}

#[test]
fn the_in_game_menu_opens_over_a_fake08_cart_on_every_row() {
    let d = TempDir::new().unwrap();
    let (mut a, _turn) = playing_on_fake08(d.path());
    assert!(matches!(a.phase(), Phase::Playing { cart } if cart == "celeste"));
    a.apply(slot_input::Action::QuickMenu);
    for _ in 0..30 {
        a.update(1.0 / 60.0);
    }
    assert_eq!(a.in_game_menu(), Some(slot_ui::QuickRow::Resume));
    let hidden = a.in_game_hidden();
    for row in [
        slot_ui::QuickRow::Bezel,
        slot_ui::QuickRow::Overlay,
        slot_ui::QuickRow::Rewind,
        slot_ui::QuickRow::ColourCorrection,
        slot_ui::QuickRow::Blending,
        slot_ui::QuickRow::Stretch,
        slot_ui::QuickRow::GameSelect,
    ] {
        assert!(hidden.contains(&row), "{row:?} shows for PICO-8");
    }
    // Drawing a row asks for its value: PICO-8 once had no Scale or Shader of its own, and
    // the row asked itself again until the stack ran out, freezing slot as the menu opened.
    for row in slot_ui::QuickRow::ALL {
        let _ = a.quick_value(row);
    }
    for _ in 0..30 {
        let mut out = Vec::new();
        a.draw(&mut out);
        assert!(!out.is_empty());
        a.apply(slot_input::Action::GbaDown(Btn::Down));
        a.apply(slot_input::Action::GbaUp(Btn::Down));
        a.update(1.0 / 60.0);
    }
}

#[test]
fn pico8_keeps_its_own_screen_settings_apart_from_the_game_boy_colors() {
    let d = TempDir::new().unwrap();
    slot::root::ensure(d.path());
    common::clocked(d.path());
    let mut state = slot_store::read_slot_state(d.path());
    state.shader_gbc = slot_store::Shader::Lcd3x;
    state.scale_gbc = slot_store::Scale::Fit;
    state.shader_pico8 = slot_store::Shader::Shimmerless;
    state.scale_pico8 = slot_store::Scale::Fit;
    slot_store::write_slot_state(d.path(), &state).unwrap();
    let back = slot_store::read_slot_state(d.path());
    assert_eq!(back.shader_pico8, slot_store::Shader::Shimmerless);
    assert_eq!(back.scale_pico8, slot_store::Scale::Fit);
    assert_eq!(back.shader_gbc, slot_store::Shader::Lcd3x);
    let a = App::boot(d.path());
    assert_ne!(
        a.quick_value(slot_ui::QuickRow::Pico8Shader),
        a.quick_value(slot_ui::QuickRow::GbcShader),
        "PICO-8's shader is its own, not the Game Boy Color's"
    );
    assert_eq!(
        a.scale_choices(Platform::Pico8),
        vec![slot_store::Scale::Integer, slot_store::Scale::Fit],
        "PICO-8 has no SP frame to fit"
    );
    assert!(a.stretch_choices(Platform::Pico8).is_empty());
    assert!(!a.colour_correction_for(Platform::Pico8));
    assert_eq!(a.blending_for(Platform::Pico8), slot_store::Blending::Off);
    assert_eq!(
        slot_store::GameButton::of(Platform::Pico8)
            .iter()
            .any(|b| *b == slot_store::GameButton::Select),
        false,
        "PICO-8 has no select button"
    );
}

/// A PICO-8 cart playing on fake-08 (the mock core stands in for it here). Holds the
/// `SLOT_RUN` turn, so no other test's loading mark lands in this one's run folder.
fn playing_on_fake08(root: &Path) -> (App, std::sync::MutexGuard<'static, ()>) {
    let turn = hand_off_through(&root.join("run"));
    write_cart(root, "celeste.p8", b"");
    slot::root::ensure(root);
    std::fs::write(root.join("Games/GBA/Emerald.gba"), [0u8; 512]).unwrap();
    common::clocked(root);
    let mut state = slot_store::read_slot_state(root);
    state.cart = Some("celeste".into());
    state.cart_platform = Some(Platform::Pico8);
    state.clock_set = true;
    slot_store::write_slot_state(root, &state).unwrap();
    let mut a = App::boot(root);
    for _ in 0..600 {
        a.on_core_ready();
        a.update(1.0 / 60.0);
        if matches!(a.phase(), Phase::Playing { .. }) {
            break;
        }
    }
    assert!(matches!(a.phase(), Phase::Playing { .. }), "{:?}", a.phase());
    (a, turn)
}

fn press(a: &mut App, btn: Btn) {
    a.apply(slot_input::Action::GbaDown(btn));
    a.apply(slot_input::Action::GbaUp(btn));
    a.update(1.0 / 60.0);
}

fn down_to(a: &mut App, row: slot_ui::QuickRow) {
    for _ in 0..20 {
        if a.in_game_menu() == Some(row) {
            return;
        }
        press(a, Btn::Down);
    }
    panic!("never reached {row:?}, on {:?}", a.in_game_menu());
}

#[test]
fn pico8s_buttons_go_by_its_own_names_in_the_in_game_menu() {
    use slot_ui::{QuickMenuFaces, QuickRow, TexId};
    let d = TempDir::new().unwrap();
    let (mut a, _turn) = playing_on_fake08(d.path());
    let id = TexId::from_raw;
    let own = QuickRow::ALL.len();
    a.set_quick_menu_faces(QuickMenuFaces {
        labels: (0..own + QuickRow::PICO8_NAMED.len())
            .map(|i| [(id(10_000 + 2 * i), 80, 40), (id(10_001 + 2 * i), 80, 40)])
            .collect(),
        pointer: (id(700), 16, 40),
        values: Vec::new(),
        legend: [(id(400), 70); 8],
        legend_light: [(id(400), 70); 8],
        descriptions: [(id(410), 300, 44), (id(411), 340, 44)],
    });
    a.apply(slot_input::Action::QuickMenu);
    for _ in 0..30 {
        a.update(1.0 / 60.0);
    }
    down_to(&mut a, QuickRow::Settings);
    press(&mut a, Btn::A);
    down_to(&mut a, QuickRow::PlayControls);
    press(&mut a, Btn::A);
    down_to(&mut a, QuickRow::GameA);
    let mut out = Vec::new();
    a.draw(&mut out);
    let drawn = |tex: TexId| {
        out.iter()
            .any(|d| matches!(d, slot_ui::Draw::Tex { tex: t, .. } if *t == tex))
    };
    let lit = |i: usize| id(10_001 + 2 * i);
    let dim = |i: usize| id(10_000 + 2 * i);
    assert_eq!(QuickRow::GameA.pico8_label(), Some("X Button"));
    assert_eq!(QuickRow::GameB.pico8_label(), Some("O Button"));
    assert_eq!(QuickRow::GameStart.pico8_label(), Some("Pause"));
    assert!(drawn(lit(own)), "the core's A is not called X on PICO-8");
    assert!(drawn(dim(own + 1)), "the core's B is not called O on PICO-8");
    assert!(drawn(dim(own + 2)), "Start is not called Pause on PICO-8");
    assert!(!drawn(lit(QuickRow::GameA.index())), "A still goes by the Game Boy's name");
    assert!(!drawn(dim(QuickRow::GameSelect.index())), "PICO-8 has no select");
}

fn favourites(root: &Path, text: &str) {
    let home = pico8::home_dir(root);
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join("favourites.txt"), text).unwrap();
}

fn bbs_cart(root: &Path, dir: &str, id: &str, bytes: &[u8]) {
    let dir = pico8::home_dir(root).join("bbs").join(dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{id}.p8.png")), bytes).unwrap();
}

/// Lines as PICO-8 writes them: a BBS cart, a numbered one, one PICO-8 took
/// for a local cart, and one no BBS id could be.
const FAVOURITES: &str = "\
|kalikan_menu-6        |kalikan_menu         |1794   |louiechapm       |                     |kalikan
|56576                 |56575                |1794   |krystman         |                     |mai-chan's sweet buns
|                     |                     |0      |                 |bbs/carts/dank_tomb-0.p8.png |dank_tomb-0
|../../escape          |x                    |0      |                 |                     |escape
";

#[test]
fn splore_favourites_are_read_with_their_titles() {
    let favs = pico8::parse_favourites(FAVOURITES);
    let got: Vec<(&str, &str)> = favs
        .iter()
        .map(|f| (f.id.as_str(), f.title.as_str()))
        .collect();
    assert_eq!(
        got,
        [
            ("kalikan_menu-6", "kalikan"),
            ("56576", "mai-chan's sweet buns"),
            ("dank_tomb-0", "dank_tomb-0"),
        ]
    );
}

#[test]
fn bbs_carts_live_under_the_start_of_their_id() {
    assert_eq!(
        pico8::bbs_url("celeste_classic-2").as_deref(),
        Some("https://www.lexaloffle.com/bbs/cposts/ce/celeste_classic-2.p8.png")
    );
    assert_eq!(
        pico8::bbs_url("15133").as_deref(),
        Some("https://www.lexaloffle.com/bbs/cposts/1/15133.p8.png")
    );
    assert_eq!(pico8::bbs_url("../x"), None);
}

#[test]
fn a_favourite_is_named_on_the_shelf_by_its_title() {
    assert_eq!(
        pico8::shelf_name("mai-chan's sweet buns", "56576"),
        "Mai-chan's Sweet Buns"
    );
    assert_eq!(pico8::shelf_name("a/b: c?", "x-1"), "Ab C");
    assert_eq!(pico8::shelf_name("...", "x-1"), "x-1");
    assert_eq!(pico8::shelf_name("splore", "x-1"), "x-1");
}

#[test]
fn splore_favourites_go_on_the_shelf_once() {
    let d = TempDir::new().unwrap();
    favourites(d.path(), FAVOURITES);
    let cart = png(160, 205, |_, _| [0xff, 0, 0x4d, 0xff]);
    bbs_cart(d.path(), "carts", "kalikan_menu-6", &cart);
    bbs_cart(d.path(), "5", "56576", &cart);
    let mut asked = Vec::new();
    let added = pico8::shelve_favourites(d.path(), |id| {
        asked.push(id.to_string());
        pico8::Fetched::Cart(cart.clone())
    });
    assert_eq!(added, 3);
    assert_eq!(asked, ["dank_tomb-0"], "asked the BBS for a cart Splore had");
    let dir = pico8::carts_dir(d.path());
    for name in ["Kalikan", "Mai-chan's Sweet Buns", "Dank_tomb-0"] {
        assert!(dir.join(format!("{name}.p8.png")).is_file(), "{name}");
    }
    let carts = slot_store::scan(d.path()).unwrap();
    assert!(carts.iter().any(|c| c.stem == "Kalikan"));

    std::fs::remove_file(dir.join("Kalikan.p8.png")).unwrap();
    let again = pico8::shelve_favourites(d.path(), |_| panic!("asked the BBS again"));
    assert_eq!(again, 0);
    assert!(
        !dir.join("Kalikan.p8.png").exists(),
        "a favourite taken off the shelf came back"
    );

    pico8::stock_shelf(d.path());
    assert!(d
        .path()
        .join("Labels/PICO8/Mai-chan's Sweet Buns.png")
        .is_file());
}

#[test]
fn offline_the_bbs_is_asked_once_and_the_rest_wait() {
    let d = TempDir::new().unwrap();
    favourites(
        d.path(),
        "|one-0 |one |0 |a | |one\n|two-0 |two |0 |a | |two\n",
    );
    let mut asked = 0;
    let added = pico8::shelve_favourites(d.path(), |_| {
        asked += 1;
        pico8::Fetched::Offline
    });
    assert_eq!((added, asked), (0, 1));
    let cart = png(160, 205, |_, _| [0, 0, 0, 0xff]);
    let added = pico8::shelve_favourites(d.path(), |_| pico8::Fetched::Cart(cart.clone()));
    assert_eq!(added, 2, "favourites missed offline go on next time");
}

#[test]
fn a_favourite_does_not_overwrite_a_cart_of_the_same_name() {
    let d = TempDir::new().unwrap();
    write_cart(d.path(), "Kalikan.p8.png", b"mine");
    favourites(d.path(), FAVOURITES);
    let cart = png(160, 205, |_, _| [0, 0, 0, 0xff]);
    bbs_cart(d.path(), "carts", "kalikan_menu-6", &cart);
    pico8::shelve_favourites(d.path(), |_| pico8::Fetched::Missing);
    let dir = pico8::carts_dir(d.path());
    assert_eq!(std::fs::read(dir.join("Kalikan.p8.png")).unwrap(), b"mine");
    assert!(dir.join("Kalikan (kalikan_menu-6).p8.png").is_file());
}

fn args(line: &[&str]) -> Vec<String> {
    line.iter().map(|s| s.to_string()).collect()
}

#[test]
fn splores_wget_lines_are_read() {
    let get = pico8::Wget::parse(&args(&[
        "https://carts.lexaloffle.com/celeste-2.p8.png",
        "-q",
        "-O",
        "/home/bbs/temp.p8.png",
    ]))
    .unwrap();
    assert_eq!(get.url, "https://carts.lexaloffle.com/celeste-2.p8.png");
    assert_eq!(get.out.as_deref(), Some(Path::new("/home/bbs/temp.p8.png")));
    assert_eq!(get.post, None);

    let post = pico8::Wget::parse(&args(&[
        "http://www.lexaloffle.com/bbs/x.php",
        "-q",
        "-O",
        "out",
        "--post-file=/tmp/body",
    ]))
    .unwrap();
    assert_eq!(post.post.as_deref(), Some(Path::new("/tmp/body")));
    assert_eq!(pico8::Wget::parse(&args(&["-q", "-O", "x"])), None);
}

#[test]
fn the_wget_script_runs_slot_without_pico8s_libraries() {
    let script = pico8::wget_script(&args(&[
        "/lib/ld-linux-aarch64.so.1",
        "/mnt/sd card/System/slot",
    ]));
    assert!(script.starts_with("#!/bin/sh\n"));
    assert!(script.contains("unset LD_PRELOAD LD_LIBRARY_PATH"));
    assert!(script.contains(
        "exec '/lib/ld-linux-aarch64.so.1' '/mnt/sd card/System/slot' --wget \"$@\""
    ));
    assert!(pico8::wget_script(&args(&["/it's"])).contains(r"'/it'\''s'"));
}

#[test]
fn wget_goes_first_on_pico8s_path() {
    let path = pico8::path_with(Path::new("/run/pico8-bin"), Some("/usr/bin:/bin".as_ref()));
    assert_eq!(path, "/run/pico8-bin:/usr/bin:/bin");
}

/// Serves one request with `status` and `body`, and hands back what was asked.
fn serve_once(status: &str, body: &'static [u8]) -> (String, std::thread::JoinHandle<String>) {
    use std::io::{BufRead as _, BufReader, Read as _};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/bbs/cpost_lister3.php?max=32", listener.local_addr().unwrap());
    let status = status.to_string();
    let served = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut head = String::new();
        let mut length = 0;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if let Some(n) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                length = n.trim().parse().unwrap();
            }
            head.push_str(&line);
            if line == "\r\n" {
                break;
            }
        }
        let mut posted = vec![0; length];
        reader.read_exact(&mut posted).unwrap();
        let mut stream = stream;
        write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .unwrap();
        stream.write_all(body).unwrap();
        head + &String::from_utf8_lossy(&posted)
    });
    (url, served)
}

#[test]
fn slot_fetches_for_splore_as_wget_would() {
    let d = TempDir::new().unwrap();
    let out = d.path().join("list.txt");
    let (url, served) = serve_once("200 OK", b"cart list");
    let code = pico8::wget(&args(&[&url, "-q", "-O", out.to_str().unwrap()]));
    assert_eq!(code, 0);
    assert_eq!(std::fs::read(&out).unwrap(), b"cart list");
    assert!(served.join().unwrap().starts_with("GET /bbs/cpost_lister3.php?max=32 "));

    let body = d.path().join("body");
    std::fs::write(&body, b"fav=1").unwrap();
    let (url, served) = serve_once("200 OK", b"ok");
    let post = format!("--post-file={}", body.display());
    assert_eq!(pico8::wget(&args(&[&url, "-q", "-O", out.to_str().unwrap(), &post])), 0);
    let asked = served.join().unwrap();
    assert!(asked.starts_with("POST "), "{asked}");
    assert!(asked.ends_with("fav=1"), "{asked}");

    let (url, served) = serve_once("404 Not Found", b"");
    assert_eq!(pico8::wget(&args(&[&url, "-O", out.to_str().unwrap()])), 8);
    served.join().unwrap();

    let gone = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", gone.local_addr().unwrap());
    drop(gone);
    assert_eq!(pico8::wget(&args(&[&url, "-O", out.to_str().unwrap()])), 4);
}

#[test]
fn a_favourite_as_pico8_0_2_7_writes_it() {
    let line = "|molemole-1           |molemole             |1794   |adam_atomic      |                     |mole mole\n";
    assert_eq!(
        pico8::parse_favourites(line),
        [pico8::Favourite {
            id: "molemole-1".into(),
            title: "mole mole".into()
        }]
    );
    assert_eq!(pico8::shelf_name("mole mole", "molemole-1"), "Mole Mole");
}

#[test]
fn a_host_that_never_resolves_is_given_up_on() {
    let wait = std::time::Duration::from_millis(600);
    let began = std::time::Instant::now();
    assert!(!pico8::resolves("https://no-such-host.invalid/x", wait));
    assert!(began.elapsed() >= wait);
    assert!(pico8::resolves("http://127.0.0.1:9/x", wait));
    assert!(pico8::resolves("http://localhost/x", wait));
}
