use slot::pico8_menu::{frame_rgba, hidden, Menu, Step, ROWS};
use slot_input::{Action, Btn, Context, Gestures, RawEvent, MENU_HOLD_MS};
use slot_ui::QuickRow;

fn press(g: &mut Gestures, m: &mut Menu, btn: Btn, at: u64, held: u64) -> Vec<Step> {
    let mut steps = Vec::new();
    for action in g.feed(RawEvent::Down(btn), at) {
        steps.push(m.act(action));
    }
    for action in g.tick(at + held) {
        steps.push(m.act(action));
    }
    for action in g.feed(RawEvent::Up(btn), at + held) {
        steps.push(m.act(action));
    }
    steps.retain(|s| *s != Step::Nothing);
    steps
}

#[test]
fn pico8_only_gets_resume_and_exit_game() {
    assert_eq!(QuickRow::Resume.shown(&hidden()), ROWS.to_vec());
}

#[test]
fn a_menu_tap_opens_on_resume_and_b_carries_on() {
    let (mut g, mut m) = (Gestures::new(), Menu::default());
    assert_eq!(press(&mut g, &mut m, Btn::Menu, 0, 100), [Step::Open]);
    assert_eq!(m.row(), Some(QuickRow::Resume));
    g.set_context(Context::Menu);
    assert_eq!(press(&mut g, &mut m, Btn::B, 1000, 80), [Step::Resume]);
    assert!(!m.is_open());
}

#[test]
fn exit_game_is_one_down_and_a_leaves() {
    let (mut g, mut m) = (Gestures::new(), Menu::default());
    press(&mut g, &mut m, Btn::Menu, 0, 100);
    g.set_context(Context::Menu);
    assert_eq!(press(&mut g, &mut m, Btn::Down, 1000, 80), [Step::Moved]);
    assert_eq!(m.row(), Some(QuickRow::ExitGame));
    assert_eq!(press(&mut g, &mut m, Btn::A, 2000, 80), [Step::Quit]);
}

#[test]
fn a_on_resume_and_a_second_menu_tap_both_carry_on() {
    let (mut g, mut m) = (Gestures::new(), Menu::default());
    press(&mut g, &mut m, Btn::Menu, 0, 100);
    g.set_context(Context::Menu);
    assert_eq!(press(&mut g, &mut m, Btn::A, 1000, 80), [Step::Resume]);
    g.set_context(Context::Game);
    press(&mut g, &mut m, Btn::Menu, 2000, 100);
    g.set_context(Context::Menu);
    assert_eq!(press(&mut g, &mut m, Btn::Menu, 3000, 100), [Step::Resume]);
}

#[test]
fn holding_menu_goes_back_to_the_shelf_from_the_game_or_the_menu() {
    let (mut g, mut m) = (Gestures::new(), Menu::default());
    assert_eq!(
        press(&mut g, &mut m, Btn::Menu, 0, MENU_HOLD_MS + 10),
        [Step::Quit]
    );
    press(&mut g, &mut m, Btn::Menu, 5000, 100);
    assert!(m.is_open());
    g.set_context(Context::Menu);
    assert_eq!(
        press(&mut g, &mut m, Btn::Menu, 6000, MENU_HOLD_MS + 10),
        [Step::Quit]
    );
    assert!(!m.is_open());
}

#[test]
fn buttons_do_nothing_while_the_game_runs() {
    let mut m = Menu::default();
    for btn in [Btn::A, Btn::B, Btn::Up, Btn::Down] {
        assert_eq!(m.act(Action::GbaDown(btn)), Step::Nothing);
    }
    assert!(!m.is_open());
}

#[test]
fn the_frame_is_read_from_the_panned_buffer_as_rgba() {
    let (w, h, stride) = (2u32, 2u32, 12usize);
    let mut fb = vec![0u8; stride * 4];
    for y in 0..4 {
        for x in 0..2 {
            let o = y * stride + x * 4;
            fb[o..o + 4].copy_from_slice(&[10 * y as u8 + x as u8, 1, 200, 0]);
        }
    }
    let rgba = frame_rgba(&fb, stride, 2, w, h).unwrap();
    assert_eq!(
        rgba,
        [
            200, 1, 20, 255, 200, 1, 21, 255, //
            200, 1, 30, 255, 200, 1, 31, 255,
        ]
    );
    assert_eq!(frame_rgba(&fb, stride, 3, w, h), None);
    assert_eq!(frame_rgba(&fb, 4, 0, w, h), None);
}

#[test]
fn a_frame_goes_back_into_fb0_as_it_came_out() {
    let rgba = [10u8, 20, 30, 255, 200, 100, 0, 255];
    let xrgb = slot::pico8_menu::to_xrgb(&rgba);
    assert_eq!(xrgb, [30, 20, 10, 255, 0, 100, 200, 255]);
    assert_eq!(frame_rgba(&xrgb, 8, 0, 2, 1).unwrap(), rgba);
}

#[test]
fn the_shims_tint_comes_off_within_a_level() {
    let gain = slot_gfx::blue_light_gain(slot_store::BLUE_LIGHT_MAX);
    let factor = gain.map(|g| (g * 255.0).round() as u32);
    for c in [0u8, 1, 37, 128, 200, 255] {
        let tinted: Vec<u8> = (0..3)
            .map(|i| ((u32::from(c) * factor[i] + 127) / 255) as u8)
            .chain([255])
            .collect();
        let back = slot::pico8_menu::untint(&tinted, gain);
        for i in 0..3 {
            assert!(
                (i32::from(back[i]) - i32::from(c)).abs() <= 2,
                "channel {i}: {c} came back as {}",
                back[i]
            );
        }
    }
    assert_eq!(
        slot::pico8_menu::untint(&[9, 9, 9, 255], [1.0; 3]),
        [9, 9, 9, 255]
    );
}

#[test]
fn the_menu_draws_into_the_buffer_not_on_screen() {
    use slot::pico8_menu::other_buffer;
    // The RG34XXSP's fb0: two 480-row buffers, one over the other.
    assert_eq!(other_buffer(0, 480, 960), Some(480));
    assert_eq!(other_buffer(480, 480, 960), Some(0));
    assert_eq!(
        other_buffer(0, 480, 480),
        None,
        "with one buffer the menu has nowhere else to draw"
    );
    assert_eq!(other_buffer(0, 0, 960), None);
}
