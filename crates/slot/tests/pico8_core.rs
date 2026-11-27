mod common;

use std::path::Path;

use common::{app_booting_at, tmp_root_with_carts, CLOCK_IS_SET};
use slot::app::App;
use slot::pico8;
use slot_input::{Action, Btn};
use slot_store::{read_slot_state, write_slot_state, Core, Platform, SlotState};
use slot_ui::{QuickRow, QuickValue};

fn install(root: &Path) {
    let dir = pico8::install_dir(root);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("pico8_dyn"), b"\x7fELF").unwrap();
    std::fs::write(dir.join("pico8.dat"), b"dat").unwrap();
}

fn with_pico8_cart(root: &Path) {
    let dir = pico8::carts_dir(root);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("celeste.p8"), b"").unwrap();
}

fn boot(root: &Path) -> App {
    write_slot_state(
        root,
        &SlotState {
            clock_set: true,
            ..SlotState::default()
        },
    )
    .expect("write slot.state");
    app_booting_at(root, CLOCK_IS_SET).0
}

fn press(a: &mut App, btn: Btn) {
    a.apply(Action::GbaDown(btn));
    a.apply(Action::GbaUp(btn));
}

/// Opens Settings > Gameplay and walks down to the PICO-8 Core row.
fn to_core_row(a: &mut App) {
    a.apply(Action::QuickMenu);
    for _ in 0..QuickRow::Game.position(&[]) {
        press(a, Btn::Down);
    }
    press(a, Btn::A);
    for _ in 0..QuickRow::Pico8Core.position(&a.hidden_rows()) {
        press(a, Btn::Down);
    }
    assert_eq!(a.quick_menu(), Some(QuickRow::Pico8Core));
}

#[test]
fn the_row_hides_without_pico8_carts_and_in_game() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let a = boot(d.path());
    assert!(a.hidden_rows().contains(&QuickRow::Pico8Core));
    assert!(a.in_game_hidden().contains(&QuickRow::Pico8Core));
    with_pico8_cart(d.path());
    let a = boot(d.path());
    assert!(!a.hidden_rows().contains(&QuickRow::Pico8Core));
}

#[test]
fn without_pico8_on_the_card_the_default_stays_fake08() {
    let d = tmp_root_with_carts(&["Emerald"]);
    with_pico8_cart(d.path());
    let mut a = boot(d.path());
    to_core_row(&mut a);
    assert_eq!(a.quick_value(QuickRow::Pico8Core), Some(QuickValue::Fake08));
    press(&mut a, Btn::Right);
    assert_eq!(
        a.quick_value(QuickRow::Pico8Core),
        Some(QuickValue::Fake08),
        "PICO-8 was picked without being on the card"
    );
    assert_eq!(read_slot_state(d.path()).pico8_core, Core::Fake08);
}

#[test]
fn with_pico8_on_the_card_new_carts_default_to_it() {
    let d = tmp_root_with_carts(&["Emerald"]);
    with_pico8_cart(d.path());
    install(d.path());
    let mut a = boot(d.path());
    to_core_row(&mut a);
    press(&mut a, Btn::Right);
    assert_eq!(a.quick_value(QuickRow::Pico8Core), Some(QuickValue::Pico8));
    assert_eq!(read_slot_state(d.path()).pico8_core, Core::Pico8);
    assert_eq!(a.default_core(Platform::Pico8), Core::Pico8);
    // A cart added from Splore has no core of its own yet, so it takes the default.
    assert_eq!(
        slot_store::core_for_platform(
            d.path(),
            "jelpi",
            Platform::Pico8,
            a.default_core(Platform::Pico8)
        ),
        Core::Pico8
    );
    // A core picked for one cart still wins over the default.
    slot_store::write_selected_core(d.path(), Platform::Pico8, "celeste", Core::Fake08).unwrap();
    assert_eq!(
        slot_store::core_for_platform(
            d.path(),
            "celeste",
            Platform::Pico8,
            a.default_core(Platform::Pico8)
        ),
        Core::Fake08
    );
    // The Game Boy keeps its own default.
    assert_eq!(a.default_core(Platform::Gb), a.gb_core());
    press(&mut a, Btn::Left);
    assert_eq!(a.quick_value(QuickRow::Pico8Core), Some(QuickValue::Fake08));
}

#[test]
fn the_default_reads_back_and_ignores_a_core_that_cannot_play_pico8() {
    let d = tempfile::TempDir::new().unwrap();
    slot::root::ensure(d.path());
    let s = SlotState {
        pico8_core: Core::Pico8,
        ..SlotState::default()
    };
    write_slot_state(d.path(), &s).unwrap();
    assert_eq!(read_slot_state(d.path()).pico8_core, Core::Pico8);
    let text = std::fs::read_to_string(d.path().join("Config/slot.state")).unwrap();
    let text = text.replace("pico8_core=pico8", "pico8_core=mgba");
    std::fs::write(d.path().join("Config/slot.state"), text).unwrap();
    assert_eq!(read_slot_state(d.path()).pico8_core, Core::Fake08);
}
