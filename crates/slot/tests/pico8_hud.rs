use slot::pico8_hud::{brightness_level, hud_alpha, output_volume, premultiplied, Levels, Shown};
use slot_input::Action;
use slot_store::{SlotState, BRIGHTNESS_MAX, DIM_MAX};
use slot_ui::HudKind;

fn state(volume: u8) -> SlotState {
    SlotState {
        volume,
        volume_hp: 30,
        brightness: 3,
        dim: 0,
        ..SlotState::default()
    }
}

#[test]
fn the_volume_keys_step_by_five_within_0_and_100() {
    let (mut l, mut s) = (Levels::default(), state(97));
    let shown = l.act(&mut s, false, Action::VolumeUp, 0).unwrap();
    assert_eq!(
        shown,
        Shown {
            kind: HudKind::Volume,
            value: 100,
            muted: false
        }
    );
    l.act(&mut s, false, Action::VolumeUp, 1000);
    assert_eq!(s.volume, 100);
    s.volume = 3;
    l.act(&mut s, false, Action::VolumeDown, 2000);
    assert_eq!(s.volume, 0);
    assert_eq!(
        s.volume_hp, 30,
        "the speaker's keys left the headphones alone"
    );
}

#[test]
fn headphones_have_their_own_level_and_mute() {
    let (mut l, mut s) = (Levels::default(), state(50));
    l.act(&mut s, true, Action::VolumeUp, 0);
    assert_eq!((s.volume, s.volume_hp), (50, 35));
    l.act(&mut s, true, Action::MuteToggle, 5000);
    assert!(s.muted_hp && !s.muted);
    assert_eq!(output_volume(&s, true), 0);
    assert_eq!(output_volume(&s, false), 50);
}

#[test]
fn a_volume_key_unmutes() {
    let (mut l, mut s) = (Levels::default(), state(40));
    s.muted = true;
    let shown = l.act(&mut s, false, Action::VolumeDown, 0).unwrap();
    assert!(!s.muted);
    assert_eq!((shown.value, shown.muted), (35, false));
}

#[test]
fn both_keys_mute_and_undo_the_step_the_first_one_took() {
    let (mut l, mut s) = (Levels::default(), state(40));
    l.act(&mut s, false, Action::VolumeUp, 1000);
    assert_eq!(s.volume, 45);
    let shown = l.act(&mut s, false, Action::MuteToggle, 1100).unwrap();
    assert_eq!(s.volume, 40, "the mute chord put the level back");
    assert!(s.muted);
    assert_eq!(
        (shown.kind, shown.value, shown.muted),
        (HudKind::Volume, 0, true)
    );
    l.act(&mut s, false, Action::MuteToggle, 5000);
    assert!(!s.muted);
    assert_eq!(s.volume, 40);
}

#[test]
fn a_late_mute_keeps_the_last_step() {
    let (mut l, mut s) = (Levels::default(), state(40));
    l.act(&mut s, false, Action::VolumeUp, 1000);
    l.act(&mut s, false, Action::MuteToggle, 1500);
    assert_eq!(s.volume, 45);
    assert!(s.muted);
}

#[test]
fn brightness_goes_into_the_dim_steps_below_the_backlight_and_back() {
    let (mut l, mut s) = (Levels::default(), state(50));
    s.brightness = 1;
    let shown = l.act(&mut s, false, Action::BrightnessDown, 0).unwrap();
    assert_eq!((s.brightness, s.dim), (0, 0));
    assert_eq!((shown.kind, shown.value), (HudKind::Brightness, DIM_MAX));
    l.act(&mut s, false, Action::BrightnessDown, 100);
    assert_eq!((s.brightness, s.dim), (0, 1));
    assert_eq!(brightness_level(&s), DIM_MAX - 1);
    l.act(&mut s, false, Action::BrightnessUp, 200);
    assert_eq!((s.brightness, s.dim), (0, 0));
    for t in 0..20 {
        l.act(&mut s, false, Action::BrightnessUp, 300 + t);
    }
    assert_eq!(s.brightness, BRIGHTNESS_MAX);
}

#[test]
fn other_actions_are_left_to_the_menu() {
    let (mut l, mut s) = (Levels::default(), state(50));
    assert_eq!(l.act(&mut s, false, Action::QuickMenu, 0), None);
    assert_eq!(l.act(&mut s, false, Action::Eject, 0), None);
}

#[test]
fn the_hud_holds_then_fades_like_slots() {
    assert_eq!(hud_alpha(1000, 1000), 1.0);
    assert_eq!(hud_alpha(1000, 2250), 1.0);
    assert!((hud_alpha(1000, 2375) - 0.5).abs() < 1e-6);
    assert_eq!(hud_alpha(1000, 2500), 0.0);
    assert_eq!(hud_alpha(1000, 9000), 0.0);
}

#[test]
fn drawn_over_black_and_white_gives_the_strip_premultiplied() {
    let black = [
        255, 255, 255, 255, // the bar, opaque
        0, 0, 0, 255, // the plate, 72% black
        0, 0, 0, 255, // outside the strip
    ];
    let white = [
        255, 255, 255, 255, //
        71, 71, 71, 255, //
        255, 255, 255, 255,
    ];
    assert_eq!(
        premultiplied(&black, &white),
        [255, 255, 255, 255, 0, 0, 0, 184, 0, 0, 0, 0]
    );
}

#[cfg(unix)]
#[test]
fn the_shared_file_is_laid_out_as_the_shim_reads_it() {
    use slot::pico8_hud::{layout, Shared};
    let d = tempfile::TempDir::new().unwrap();
    let path = d.path().join("hud");
    let shared = Shared::create(&path).unwrap();
    let word =
        |bytes: &[u8], i: usize| u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(bytes.len(), layout::LEN);
    assert_eq!(word(&bytes, layout::W_MAGIC), layout::MAGIC);
    assert_eq!(word(&bytes, layout::W_VOLUME), 100);
    shared.set_volume(35);
    shared.show(&[1, 2, 3, 4, 5, 6, 7, 8], 2, 1);
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(word(&bytes, layout::W_VOLUME), 35);
    assert_eq!(word(&bytes, layout::W_SHOWN), 1);
    assert_eq!(
        (
            word(&bytes, layout::W_WIDTH),
            word(&bytes, layout::W_HEIGHT)
        ),
        (2, 1)
    );
    assert_eq!(word(&bytes, layout::W_SEQ) % 2, 0, "left mid-write");
    assert_eq!(
        &bytes[layout::HEADER..layout::HEADER + 8],
        &[1, 2, 3, 4, 5, 6, 7, 8]
    );
    shared.hide();
    assert_eq!(word(&std::fs::read(&path).unwrap(), layout::W_SHOWN), 0);
    drop(shared);
    assert!(!path.exists(), "the file outlived the runner");
}
