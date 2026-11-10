//! The in-game menu over PICO-8.
//!
//! PICO-8 owns the screen while it runs, so `slot --pico8` draws the menu
//! itself: a MENU tap stops PICO-8, keeps a copy of the framebuffer, draws
//! slot's menu over that frame, and puts the copy back before PICO-8 runs
//! again. PICO-8 only repaints what changes, so anything left of the menu
//! would stay on screen. Holding MENU leaves for the shelf, as it ejects a
//! cart everywhere else.

use slot_input::{Action, Btn};
use slot_ui::QuickRow;

/// The rows PICO-8 gets. slot keeps no save states for it, its screenshots
/// are PICO-8's own, and its settings are fixed when it starts.
pub const ROWS: [QuickRow; 2] = [QuickRow::Resume, QuickRow::ExitGame];

/// The rest of the in-game page, hidden.
pub fn hidden() -> Vec<QuickRow> {
    QuickRow::PLAY
        .into_iter()
        .filter(|r| !ROWS.contains(r))
        .collect()
}

/// What an action asks of the runner.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Step {
    Nothing,
    /// Stop PICO-8 and show the menu.
    Open,
    /// The menu changed: draw it again.
    Moved,
    /// Put the screen back and let PICO-8 carry on.
    Resume,
    /// Leave PICO-8 for the shelf.
    Quit,
}

#[derive(Default, Debug)]
pub struct Menu {
    row: Option<QuickRow>,
}

impl Menu {
    pub fn row(&self) -> Option<QuickRow> {
        self.row
    }

    pub fn is_open(&self) -> bool {
        self.row.is_some()
    }

    pub fn act(&mut self, action: Action) -> Step {
        let hidden = hidden();
        match (self.row, action) {
            (_, Action::Eject) => {
                self.row = None;
                Step::Quit
            }
            (None, Action::QuickMenu) => {
                self.row = Some(QuickRow::Resume);
                Step::Open
            }
            (None, _) => Step::Nothing,
            (Some(_), Action::QuickMenu | Action::GbaDown(Btn::B)) => {
                self.row = None;
                Step::Resume
            }
            (Some(row), Action::GbaDown(Btn::Up)) => self.move_to(row.up(&hidden)),
            (Some(row), Action::GbaDown(Btn::Down)) => self.move_to(row.down(&hidden)),
            (Some(QuickRow::ExitGame), Action::GbaDown(Btn::A)) => {
                self.row = None;
                Step::Quit
            }
            (Some(_), Action::GbaDown(Btn::A)) => {
                self.row = None;
                Step::Resume
            }
            (Some(_), _) => Step::Nothing,
        }
    }

    fn move_to(&mut self, row: QuickRow) -> Step {
        match self.row.replace(row) == Some(row) {
            true => Step::Nothing,
            false => Step::Moved,
        }
    }
}

/// The visible frame of a framebuffer copy as RGBA: `h` rows of `w`
/// 32-bit little-endian XRGB pixels, starting at row `y0` of `stride`
/// bytes each. `None` when the copy is too short.
pub fn frame_rgba(fb: &[u8], stride: usize, y0: usize, w: u32, h: u32) -> Option<Vec<u8>> {
    let (w, h) = (w as usize, h as usize);
    let end = (y0 + h).checked_mul(stride)?;
    if w * 4 > stride || end > fb.len() {
        return None;
    }
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        let row = &fb[(y0 + y) * stride..][..w * 4];
        for px in row.chunks_exact(4) {
            rgba.extend_from_slice(&[px[2], px[1], px[0], 0xff]);
        }
    }
    Some(rgba)
}

/// Takes `gain` back off RGBA that the shim tinted with it. The shim hands
/// SDL the gain as an 8-bit factor, so this divides by the same factor.
pub fn untint(rgba: &[u8], gain: [f32; 3]) -> Vec<u8> {
    let factor = gain.map(|g| (g.clamp(0.0, 1.0) * 255.0).round());
    rgba.chunks_exact(4)
        .flat_map(|p| {
            let back = |c: usize| match factor[c] {
                f if f <= 0.0 => 0,
                f => (f32::from(p[c]) * 255.0 / f).round().min(255.0) as u8,
            };
            [back(0), back(1), back(2), p[3]]
        })
        .collect()
}

/// RGBA as fb0's 32-bit little-endian XRGB, the other way from `frame_rgba`.
pub fn to_xrgb(rgba: &[u8]) -> Vec<u8> {
    rgba.chunks_exact(4)
        .flat_map(|p| [p[2], p[1], p[0], 0xff])
        .collect()
}

#[cfg(unix)]
pub use screen::{grab, Overlay, Saved};

#[cfg(unix)]
mod screen {
    use std::io::Write as _;
    use std::os::unix::io::AsRawFd;

    use slot_gfx::{Canvas, Compositor, Draw, FbdevSurface, Surface, TexId};
    use slot_input::Millis;
    use slot_ui::{
        footer_ink, icon_face, quick_label_face, quick_legend_faces, quick_pointer_face, Hud, Icon,
        QuickMenu, QuickMenuFaces, QuickRow, UndoFace, HINT_EDGE, HUD_ICON_PX, HUD_INK, MOUTH_H,
        PLATE_H,
    };

    use crate::panel_turn::{panel_turn, PanelTurn};

    const FB: &str = "/dev/fb0";
    const FB_SYS: &str = "/sys/class/graphics/fb0";
    const EVIOCGRAB: u64 = 0x4004_4590;

    /// All of fb0's memory as PICO-8 left it, both of its buffers included.
    pub struct Saved {
        bytes: Vec<u8>,
    }

    impl Saved {
        pub fn take() -> Option<Saved> {
            std::fs::read(FB).ok().map(|bytes| Saved { bytes })
        }

        /// Writes the copy back, so the screen is what PICO-8 believes it is.
        pub fn put_back(&self) {
            let written = std::fs::OpenOptions::new()
                .write(true)
                .open(FB)
                .and_then(|mut f| f.write_all(&self.bytes));
            if let Err(e) = written {
                eprintln!("slot: pico8: menu: could not put the screen back: {e}");
            }
        }

        /// The frame on screen, as RGBA, when fb0 is 32-bit.
        fn front(&self, w: u32, h: u32) -> Option<Vec<u8>> {
            let (stride, y0) = front_buffer()?;
            super::frame_rgba(&self.bytes, stride, y0, w, h)
        }
    }

    /// fb0's row stride, and the first row of the buffer on screen, when it
    /// is 32-bit.
    fn front_buffer() -> Option<(usize, usize)> {
        let sys = |name: &str| std::fs::read_to_string(format!("{FB_SYS}/{name}")).ok();
        if sys("bits_per_pixel")?.trim() != "32" {
            return None;
        }
        let stride = sys("stride")?.trim().parse().ok()?;
        let y0 = sys("pan")?.trim().split(',').nth(1)?.parse().ok()?;
        Some((stride, y0))
    }

    /// Writes `rgba`, `w` by `h`, over the buffer on screen.
    fn write_front(rgba: &[u8], w: u32, h: u32) -> bool {
        use std::os::unix::fs::FileExt as _;
        let Some((stride, y0)) = front_buffer() else {
            return false;
        };
        let Ok(fb) = std::fs::OpenOptions::new().write(true).open(FB) else {
            return false;
        };
        let row_bytes = w as usize * 4;
        if row_bytes > stride || rgba.len() < row_bytes * h as usize {
            return false;
        }
        let rows = super::to_xrgb(rgba);
        rows.chunks_exact(row_bytes)
            .enumerate()
            .all(|(y, row)| fb.write_all_at(row, ((y0 + y) * stride) as u64).is_ok())
    }

    /// Takes the input devices for the menu while it is open, so the stopped
    /// PICO-8 does not find the presses queued up when it carries on.
    pub fn grab(fd: &impl AsRawFd, on: bool) {
        unsafe { libc::ioctl(fd.as_raw_fd(), EVIOCGRAB as _, libc::c_int::from(on)) };
    }

    /// slot's own window on the screen, opened the first time the menu is.
    pub struct Overlay {
        // Fields drop in this order: the compositor's GL objects need the
        // surface's context, so it must go first.
        compositor: Compositor,
        surface: FbdevSurface,
        panel: (u32, u32),
        window: (u32, u32),
        turned: bool,
        faces: QuickMenuFaces,
        icons: Vec<TexId>,
        backdrop: Option<(TexId, u32, u32)>,
        show_backdrop: bool,
        /// The tint the shim gave PICO-8's frames, taken off the backdrop
        /// so the compositor's grade puts it on once, as in slot.
        tint: [f32; 3],
    }

    impl Overlay {
        pub fn open() -> Result<Overlay, String> {
            let surface = FbdevSurface::new().map_err(|e| e.to_string())?;
            let mut compositor = Compositor::new(&surface).map_err(|e| e.to_string())?;
            let panel = surface.window_size();
            let turn = panel_turn(
                panel,
                crate::baseos::var("BASEOS_PANEL_ROTATION_CCW").as_deref(),
                std::env::var("SLOT_ROTATE").ok().as_deref(),
            );
            let turned = turn != PanelTurn::None;
            let window = if turned { (panel.1, panel.0) } else { panel };
            if turned {
                compositor
                    .set_panel_turn(panel, turn == PanelTurn::Ccw)
                    .map_err(|e| e.to_string())?;
            }
            let faces = upload_faces(&mut compositor);
            let icons = Icon::ALL
                .iter()
                .map(|i| {
                    let f = icon_face(*i, HUD_ICON_PX, HUD_INK);
                    compositor.create_texture(f.w, f.h, &f.rgba)
                })
                .collect();
            Ok(Overlay {
                surface,
                compositor,
                panel,
                window,
                turned,
                faces,
                icons,
                backdrop: None,
                show_backdrop: false,
                tint: [1.0; 3],
            })
        }

        /// slot's HUD icons, for `Hud::set_icons`.
        pub fn hud_icons(&self) -> Vec<TexId> {
            self.icons.clone()
        }

        /// The HUD's strip as it would look over a game, premultiplied RGBA
        /// and as wide as the frame, drawn with slot's own `Hud` but never
        /// put on screen: PICO-8's shim lays it over its frames. `None` on a
        /// turned panel, whose framebuffer is not the way up it is drawn.
        pub fn hud_strip(&mut self, hud: &Hud, now: Millis) -> Option<(Vec<u8>, u32, u32)> {
            if self.turned {
                return None;
            }
            let canvas = Canvas::of(self.window);
            let rows = PLATE_H as u32;
            let mut pass = |under: Option<[f32; 4]>| {
                let mut out = Vec::new();
                if let Some(colour) = under {
                    out.push(Draw::Rect {
                        x: 0.0,
                        y: 0.0,
                        w: canvas.w,
                        h: canvas.h,
                        colour,
                    });
                }
                hud.draw(canvas, now, &mut out);
                self.compositor.fit(self.window);
                self.compositor.begin_frame();
                self.compositor.draw_list(&out);
                self.compositor.read_top(rows)
            };
            let black = pass(None);
            let white = pass(Some([1.0, 1.0, 1.0, 1.0]));
            let w = self.compositor.frame_size().0;
            Some((crate::pico8_hud::premultiplied(&black, &white), w, rows))
        }

        /// Shows `saved` behind the menu. A turned panel's framebuffer is
        /// not the way up the menu is drawn, so it gets slot's plain ground.
        pub fn set_backdrop(&mut self, saved: &Saved) {
            let (w, h) = self.panel;
            let frame = match self.turned {
                true => None,
                false => saved.front(w, h).map(|f| super::untint(&f, self.tint)),
            };
            self.show_backdrop = frame.is_some();
            let Some(rgba) = frame else {
                return;
            };
            match self.backdrop {
                Some((tex, bw, bh)) if (bw, bh) == (w, h) => {
                    self.compositor.update_texture(tex, w, h, &rgba)
                }
                _ => {
                    let tex = self.compositor.create_texture(w, h, &rgba);
                    self.backdrop = Some((tex, w, h));
                }
            }
        }

        pub fn draw(&mut self, row: QuickRow, hidden: &[QuickRow]) {
            let canvas = Canvas::of(self.window);
            let mut out = Vec::new();
            if let (true, Some((tex, _, _))) = (self.show_backdrop, self.backdrop) {
                out.push(Draw::Tex {
                    x: 0.0,
                    y: 0.0,
                    w: canvas.w,
                    h: canvas.h,
                    tex,
                    alpha: 1.0,
                });
            }
            QuickMenu {
                row,
                hidden,
                values: [None; QuickRow::ALL.len()],
                clock: None,
                wallpaper: None,
                networks: None,
                romm: [None; 4],
                kid_shelf: None,
                faces: Some(&self.faces),
                ground: None,
                palette: None,
                gba_bezel: None,
                gb_bezel: None,
                gbc_bezel: None,
                bezel: None,
                gba_overlay: None,
                gb_overlay: None,
                gbc_overlay: None,
                overlay: None,
                bound: None,
                description: None,
                over_game: self.show_backdrop,
                footer_top: canvas.h,
                canvas,
            }
            .draw(&mut out);
            let [_, _, open, resume, ..] = self.faces.legend;
            let y = canvas.h - MOUTH_H;
            for (face, right) in [(resume, false), (open, true)] {
                let seen = face.1.saturating_sub(HINT_EDGE) as f32;
                let x = if right {
                    (canvas.w - seen).round()
                } else {
                    0.0
                };
                out.push(Draw::Tex {
                    x,
                    y,
                    w: face.1 as f32,
                    h: MOUTH_H,
                    tex: face.0,
                    alpha: 1.0,
                });
            }
            self.compositor.fit(self.window);
            self.compositor.begin_frame();
            self.compositor.draw_list(&out);
            // The frame goes straight into the buffer on screen rather than
            // through a swap: the GPU skips writing tiles that match what it
            // last drew into a buffer, and PICO-8 and the copy put back have
            // drawn there since, so swapped frames came out with holes.
            if !self.turned {
                let (w, h) = self.panel;
                let frame = self.compositor.read_window(self.panel);
                if write_front(&frame, w, h) {
                    return;
                }
            }
            self.compositor.end_frame(self.window);
            if let Err(e) = self.surface.swap() {
                eprintln!("slot: pico8: menu: {e}");
            }
        }

        /// slot's blue light and dim, which its compositor puts on every
        /// frame, the menu's included.
        pub fn set_grade(&mut self, blue_light: u8, dim: u8) {
            self.compositor.set_blue_light(blue_light);
            self.compositor.set_dim(dim);
            let dim = slot_gfx::dim_gain(dim);
            self.tint = slot_gfx::blue_light_gain(blue_light).map(|g| g * dim);
        }

        /// Waits until the last menu frame is on the screen, before the
        /// framebuffer is written back under it.
        pub fn settle(&self) {
            self.compositor.finish();
        }
    }

    /// The menu's faces: real labels for the in-game page, which sets the
    /// menu's width, and a blank for every other row it never shows.
    fn upload_faces(c: &mut Compositor) -> QuickMenuFaces {
        let blank = c.create_texture(1, 1, &[0, 0, 0, 0]);
        let mut up = |f: UndoFace| (c.create_texture(f.w, f.h, &f.rgba), f.w, f.h);
        let labels = QuickRow::ALL
            .iter()
            .map(|row| match QuickRow::PLAY.contains(row) {
                true => [false, true].map(|lit| up(quick_label_face(*row, lit))),
                false => [(blank, 1, 1); 2],
            })
            .collect();
        let pointer = up(quick_pointer_face());
        let legend = quick_legend_faces(footer_ink(false)).map(|f| {
            let (tex, w, _) = up(f);
            (tex, w)
        });
        QuickMenuFaces {
            labels,
            pointer,
            values: Vec::new(),
            legend,
            legend_light: legend,
            descriptions: [(blank, 1, 1); 2],
        }
    }
}
