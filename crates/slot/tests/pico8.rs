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
fn a_cart_set_to_pico8_is_refused_until_pico8_is_installed() {
    let d = TempDir::new().unwrap();
    write_cart(d.path(), "celeste.p8", b"");
    on_pico8(d.path(), "celeste");
    slot::root::ensure(d.path());
    // A second shelf, so there is a shelf to name.
    std::fs::write(d.path().join("Games/GBA/Emerald.gba"), [0u8; 512]).unwrap();
    common::clocked(d.path());
    let mut a = App::boot(d.path());
    to_pico8_shelf(&mut a);
    common::through(&mut a, Btn::A);
    assert!(!a.relaunching());
    assert!(matches!(a.phase(), Phase::Shelf), "{:?}", a.phase());
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
    write_cart(d.path(), "celeste.p8", b"");
    slot::root::ensure(d.path());
    std::fs::write(d.path().join("Games/GBA/Emerald.gba"), [0u8; 512]).unwrap();
    common::clocked(d.path());
    let mut state = slot_store::read_slot_state(d.path());
    state.cart = Some("celeste".into());
    state.cart_platform = Some(Platform::Pico8);
    state.clock_set = true;
    slot_store::write_slot_state(d.path(), &state).unwrap();
    let mut a = App::boot(d.path());
    for _ in 0..600 {
        a.on_core_ready();
        a.update(1.0 / 60.0);
        if matches!(a.phase(), Phase::Playing { .. }) {
            break;
        }
    }
    assert!(
        matches!(a.phase(), Phase::Playing { cart } if cart == "celeste"),
        "{:?}",
        a.phase()
    );
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
