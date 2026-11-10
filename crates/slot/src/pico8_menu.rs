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
    let lut = untint_lut(gain);
    rgba.chunks_exact(4)
        .flat_map(|p| {
            [
                lut[0][p[0] as usize],
                lut[1][p[1] as usize],
                lut[2][p[2] as usize],
                p[3],
            ]
        })
        .collect()
}

/// `untint` for each value of each channel, worked out once a frame instead
/// of once a pixel.
fn untint_lut(gain: [f32; 3]) -> [[u8; 256]; 3] {
    gain.map(|g| {
        let factor = (g.clamp(0.0, 1.0) * 255.0).round();
        let mut lut = [0u8; 256];
        if factor > 0.0 {
            for (v, out) in lut.iter_mut().enumerate() {
                *out = (v as f32 * 255.0 / factor).round().min(255.0) as u8;
            }
        }
        lut
    })
}

/// `untint(frame_rgba(..))` in one pass over the copy, as the menu needs it
/// each time it opens.
pub fn frame_rgba_untinted(
    fb: &[u8],
    stride: usize,
    y0: usize,
    w: u32,
    h: u32,
    gain: [f32; 3],
) -> Option<Vec<u8>> {
    let (w, h) = (w as usize, h as usize);
    let end = (y0 + h).checked_mul(stride)?;
    if w * 4 > stride || end > fb.len() {
        return None;
    }
    let lut = untint_lut(gain);
    let mut rgba = vec![0u8; w * h * 4];
    for (y, out) in rgba.chunks_exact_mut(w * 4).enumerate() {
        let row = &fb[(y0 + y) * stride..][..w * 4];
        for (px, o) in row.chunks_exact(4).zip(out.chunks_exact_mut(4)) {
            o[0] = lut[0][px[2] as usize];
            o[1] = lut[1][px[1] as usize];
            o[2] = lut[2][px[0] as usize];
            o[3] = 0xff;
        }
    }
    Some(rgba)
}

/// The first row of the buffer not on screen, when fb0's `all` rows hold two
/// buffers of `rows` and the one on screen starts at row `front`: where the
/// menu writes its next frame before showing it. `None` with one buffer.
pub fn other_buffer(front: usize, rows: usize, all: usize) -> Option<usize> {
    if rows == 0 || all < rows * 2 {
        return None;
    }
    Some(if front >= rows { front - rows } else { front + rows })
}

/// RGBA as fb0's 32-bit little-endian XRGB, the other way from `frame_rgba`.
pub fn to_xrgb(rgba: &[u8]) -> Vec<u8> {
    rgba.chunks_exact(4)
        .flat_map(|p| [p[2], p[1], p[0], 0xff])
        .collect()
}

#[cfg(unix)]
pub use screen::{blank, grab, power_off, Kept, Overlay, Saved, Still};

#[cfg(unix)]
mod screen {
    use std::io::Write as _;
    use std::os::unix::io::AsRawFd;

    use std::path::Path;

    use slot_gfx::{Canvas, Compositor, Draw, FbdevSurface, Surface, TexId};
    use slot_input::Millis;
    use slot_store::{Cart, Platform, SlotState};
    use slot_ui::{
        draw_corners, ease, footer_ink, icon_face, quick_label_face, quick_legend_faces,
        quick_pointer_face, word_ink, Hud, Icon, Printed, QuickMenu, QuickMenuFaces, QuickRow,
        SlotChrome, UndoFace, BOLT_PX, HINT_EDGE, HUD_ICON_PX, HUD_INK, MOUTH_H, PLATE_H,
    };

    use crate::panel_turn::{panel_turn, PanelTurn};

    const FB: &str = "/dev/fb0";
    const FB_SYS: &str = "/sys/class/graphics/fb0";
    /// How long `Saved::settled` waits between copies, a refresh and a bit,
    /// and how many times it looks before taking the last.
    const SETTLE_GAP: std::time::Duration = std::time::Duration::from_millis(20);
    const SETTLE_TRIES: usize = 5;
    /// The rows in a tile the GPU writes at once.
    const TILE: usize = 16;
    const EVIOCGRAB: u64 = 0x4004_4590;
    const FBIOGET_VSCREENINFO: u64 = 0x4600;
    const FBIOPAN_DISPLAY: u64 = 0x4606;
    /// How long the mouth takes to slide in or out: slot's footer slide.
    const SLIDE_S: f32 = crate::app::POWER_ON_S;

    /// fb0 mapped into memory, every row of its buffers.
    ///
    /// Everything that reads or writes fb0 goes through this rather than
    /// read(2) and write(2). On this display a write(2) to /dev/fb0 lands in
    /// the CPU's cache while the display reads memory, so lines not yet
    /// written back showed an older frame as 16-pixel dashes until they
    /// were, a couple of seconds later. fb0's own mapping is write-combined:
    /// what goes into it is what the display shows.
    struct Mapped {
        ptr: *mut u8,
        len: usize,
    }

    impl Mapped {
        fn open() -> Option<Mapped> {
            let stride: usize = std::fs::read_to_string(format!("{FB_SYS}/stride"))
                .ok()?
                .trim()
                .parse()
                .ok()?;
            let len = stride.checked_mul(virtual_rows()?)?;
            if len == 0 {
                return None;
            }
            let fb = std::fs::OpenOptions::new().read(true).write(true).open(FB).ok()?;
            let ptr = unsafe {
                libc::mmap(
                    std::ptr::null_mut(),
                    len,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_SHARED,
                    fb.as_raw_fd(),
                    0,
                )
            };
            if ptr == libc::MAP_FAILED {
                eprintln!("slot: pico8: menu: could not map fb0");
                return None;
            }
            Some(Mapped {
                ptr: ptr.cast(),
                len,
            })
        }

        fn bytes(&self) -> &[u8] {
            unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
        }

        fn bytes_mut(&mut self) -> &mut [u8] {
            unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
        }
    }

    impl Drop for Mapped {
        fn drop(&mut self) {
            unsafe { libc::munmap(self.ptr.cast(), self.len) };
        }
    }

    /// Clears every buffer of fb0 to black, so nothing of PICO-8 or the menu
    /// is left on screen while slot starts again.
    pub fn blank() {
        match Mapped::open() {
            Some(mut fb) => fb.bytes_mut().fill(0),
            None => {
                let len = (|| {
                    let stride: usize = std::fs::read_to_string(format!("{FB_SYS}/stride"))
                        .ok()?
                        .trim()
                        .parse()
                        .ok()?;
                    Some(stride * virtual_rows()?)
                })();
                let blanked = len.map(|len| {
                    std::fs::OpenOptions::new()
                        .write(true)
                        .open(FB)
                        .and_then(|mut f| f.write_all(&vec![0u8; len]))
                });
                if let Some(Err(e)) = blanked {
                    eprintln!("slot: pico8: could not blank the screen: {e}");
                }
            }
        }
    }

    /// All of fb0's buffers as PICO-8 left them.
    pub struct Saved {
        bytes: Vec<u8>,
        /// The buffer PICO-8 had on screen: its first row and fb0's stride.
        front: Option<(usize, usize)>,
    }

    impl Saved {
        pub fn take() -> Option<Saved> {
            let front = front_buffer();
            Mapped::open().map(|fb| Saved {
                bytes: fb.bytes().to_vec(),
                front,
            })
        }

        /// The frame PICO-8 had on screen, as fb0 held it.
        pub fn still(&self) -> Option<Still> {
            let (stride, y0) = self.front?;
            let (w, h) = visible_size()?;
            let rows = self.bytes.get(y0 * stride..(y0 + h) * stride)?.to_vec();
            Some(Still { rows, stride, w, h })
        }

        /// A copy of fb0 once it has stopped changing. Stopping PICO-8 does
        /// not stop the GPU, which can still be drawing PICO-8's last frame
        /// into its other buffer. A copy taken mid-frame and put back later
        /// leaves stale tiles the GPU will not draw again, since it skips
        /// tiles it believes it already wrote: they show as lines until the
        /// game redraws there.
        ///
        /// The GPU is done with a frame within a refresh, so the copy is
        /// taken a refresh after PICO-8 stopped, and then looked over once
        /// more, a row in each row of tiles, which is quick. Reading fb0 is
        /// slow, being uncached, so it is read whole as few times as can be.
        pub fn settled() -> Option<Saved> {
            std::thread::sleep(SETTLE_GAP);
            let mut last = Saved::take()?;
            for tries in 0..SETTLE_TRIES {
                if last.still_there()? {
                    if tries > 0 {
                        eprintln!("slot: pico8: menu: the screen settled after {} copies", tries + 1);
                    }
                    return Some(last);
                }
                std::thread::sleep(SETTLE_GAP);
                last = Saved::take()?;
            }
            eprintln!("slot: pico8: menu: the screen kept changing, using the last copy");
            Some(last)
        }

        /// Whether fb0 still matches this copy, looked at in one row of each
        /// row of tiles: a tile the GPU wrote since would differ there,
        /// unless only its other rows changed. Without the stride, all of it.
        fn still_there(&self) -> Option<bool> {
            let fb = Mapped::open()?;
            let now = fb.bytes();
            if now.len() != self.bytes.len() {
                return Some(false);
            }
            let Some((stride, _)) = self.front else {
                return Some(now == &self.bytes[..]);
            };
            let rows = now.len() / stride;
            Some((TILE / 2..rows).step_by(TILE).all(|y| {
                let at = y * stride..(y + 1) * stride;
                now[at.clone()] == self.bytes[at]
            }))
        }

        /// Writes the copy back, so the screen is what PICO-8 believes it is.
        pub fn put_back(&self) {
            let Some(mut fb) = Mapped::open() else {
                eprintln!("slot: pico8: menu: could not put the screen back");
                return;
            };
            let n = fb.len.min(self.bytes.len());
            fb.bytes_mut()[..n].copy_from_slice(&self.bytes[..n]);
        }

        /// The frame on screen, as RGBA without `gain`, when fb0 is 32-bit.
        fn front_untinted(&self, w: u32, h: u32, gain: [f32; 3]) -> Option<Vec<u8>> {
            let (stride, y0) = self.front?;
            super::frame_rgba_untinted(&self.bytes, stride, y0, w, h, gain)
        }
    }

    /// One frame as fb0 holds it: the rows of a buffer, graded the way they
    /// were drawn, ready to go back into fb0 as they are.
    pub struct Still {
        rows: Vec<u8>,
        stride: usize,
        w: usize,
        h: usize,
    }

    impl Still {
        /// The frame on screen now.
        pub fn take() -> Option<Still> {
            let (stride, y0) = front_buffer()?;
            let (w, h) = visible_size()?;
            let fb = Mapped::open()?;
            let rows = fb.bytes().get(y0 * stride..(y0 + h) * stride)?.to_vec();
            Some(Still { rows, stride, w, h })
        }

        /// Puts this frame in every buffer of fb0, so it stays on screen
        /// whichever buffer is shown and whatever PICO-8 last drew.
        pub fn show(&self) {
            let Some(mut fb) = Mapped::open() else {
                return;
            };
            let mem = fb.bytes_mut();
            let len = self.rows.len();
            let mut at = 0;
            while at + len <= mem.len() {
                mem[at..at + len].copy_from_slice(&self.rows);
                at += len;
            }
        }
    }

    /// Leaves a game the way slot leaves one: `game` powers off over
    /// `under`, squashing to a line and then a dot, a little bright at the
    /// end, on slot's own curve and time. `under` is the frame slot showed
    /// as it handed over, the cart seated in the slot; it stays on screen
    /// after, so slot can start again on it and eject the cart.
    pub fn power_off(game: &Still, under: &Still) {
        let same = (game.stride, game.w, game.h) == (under.stride, under.w, under.h)
            && game.rows.len() == under.rows.len();
        if !same {
            return under.show();
        }
        let (w, h, stride) = (under.w, under.h, under.stride);
        let mut frame = vec![0u8; under.rows.len()];
        let began = std::time::Instant::now();
        loop {
            let k = (began.elapsed().as_secs_f32() / crate::app::POWER_OFF_S).min(1.0);
            let power = 1.0 - k;
            if power <= 0.0 {
                break;
            }
            frame.copy_from_slice(&under.rows);
            let (sx, sy) = slot_gfx::screen_squash(w as f32, h as f32, power);
            let rw = ((w as f32 * sx).round() as usize).clamp(1, w);
            let rh = ((h as f32 * sy).round() as usize).clamp(1, h);
            let (rx, ry) = ((w - rw) / 2, (h - rh) / 2);
            let gain = slot_gfx::screen_brightness(power);
            for y in 0..rh {
                let src = &game.rows[(y * h / rh) * stride..];
                let dst = (ry + y) * stride + rx * 4;
                for x in 0..rw {
                    let s = (x * w / rw) * 4;
                    let d = dst + x * 4;
                    for c in 0..3 {
                        frame[d + c] = (f32::from(src[s + c]) * gain).min(255.0) as u8;
                    }
                    frame[d + 3] = src[s + 3];
                }
            }
            flip_rows(&frame, h);
        }
        under.show();
    }

    /// fb0's visible size, xres by yres.
    fn visible_size() -> Option<(usize, usize)> {
        let fb = std::fs::OpenOptions::new().read(true).open(FB).ok()?;
        let mut info = [0u32; 40];
        let got = unsafe { libc::ioctl(fb.as_raw_fd(), FBIOGET_VSCREENINFO as _, info.as_mut_ptr()) };
        (got == 0 && info[0] > 0 && info[1] > 0).then(|| (info[0] as usize, info[1] as usize))
    }

    /// Writes `rows`, fb0's own layout, into the buffer not on screen and
    /// shows it; over the buffer on screen when there is only one.
    fn flip_rows(rows: &[u8], h: usize) {
        let Some((stride, front)) = front_buffer() else {
            return;
        };
        let Some(mut fb) = Mapped::open() else {
            return;
        };
        let target = back_buffer(front, h).unwrap_or(front);
        let at = target * stride;
        let Some(dst) = fb.bytes_mut().get_mut(at..at + rows.len()) else {
            return;
        };
        dst.copy_from_slice(rows);
        if target != front {
            pan_to(target);
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

    /// How many rows fb0 has in all, its buffers stacked one under another.
    fn virtual_rows() -> Option<usize> {
        let text = std::fs::read_to_string(format!("{FB_SYS}/virtual_size")).ok()?;
        text.trim().split(',').nth(1)?.parse().ok()
    }

    /// The first row of the buffer that is not on screen, when fb0 has two
    /// buffers of `rows`: where the next frame is written before it is shown.
    fn back_buffer(front: usize, rows: usize) -> Option<usize> {
        super::other_buffer(front, rows, virtual_rows()?)
    }

    /// Puts the buffer starting at row `y0` on screen. The display changes
    /// buffers between refreshes, so a frame is never shown half written.
    fn pan_to(y0: usize) -> bool {
        let Ok(fb) = std::fs::OpenOptions::new().read(true).write(true).open(FB) else {
            return false;
        };
        // struct fb_var_screeninfo: 40 words, yoffset the sixth.
        let mut info = [0u32; 40];
        let got = unsafe { libc::ioctl(fb.as_raw_fd(), FBIOGET_VSCREENINFO as _, info.as_mut_ptr()) };
        if got != 0 {
            return false;
        }
        info[4] = 0;
        info[5] = y0 as u32;
        unsafe { libc::ioctl(fb.as_raw_fd(), FBIOPAN_DISPLAY as _, info.as_ptr()) == 0 }
    }

    /// Writes `rgba`, `w` by `h`, into the buffer starting at row `y0`.
    fn write_rows(rgba: &[u8], w: u32, h: u32, y0: usize) -> bool {
        let Some((stride, _)) = front_buffer() else {
            return false;
        };
        let row_bytes = w as usize * 4;
        if row_bytes > stride || rgba.len() < row_bytes * h as usize {
            return false;
        }
        let Some(mut fb) = Mapped::open() else {
            return false;
        };
        if (y0 + h as usize) * stride > fb.len {
            return false;
        }
        // A row at a time through a small buffer: fb0 takes whole rows
        // best, and the frame is not copied whole first.
        let mem = fb.bytes_mut();
        let mut row = vec![0u8; row_bytes];
        for (y, src) in rgba.chunks_exact(row_bytes).take(h as usize).enumerate() {
            for (px, o) in src.chunks_exact(4).zip(row.chunks_exact_mut(4)) {
                o.copy_from_slice(&[px[2], px[1], px[0], 0xff]);
            }
            let at = (y0 + y) * stride;
            mem[at..at + row_bytes].copy_from_slice(&row);
        }
        true
    }

    /// Takes the input devices for the menu while it is open, so the stopped
    /// PICO-8 does not find the presses queued up when it carries on.
    pub fn grab(fd: &impl AsRawFd, on: bool) {
        unsafe { libc::ioctl(fd.as_raw_fd(), EVIOCGRAB as _, libc::c_int::from(on)) };
    }

    /// The cart PICO-8 is running, seated in slot's mouth along the bottom of
    /// the menu, as slot shows a seated cart under its menu over any game.
    struct Mouth {
        cart: Cart,
        face: TexId,
        behind: [Option<TexId>; 2],
        slot: (TexId, TexId),
    }

    /// slot's own window on the screen: slot's, lent while PICO-8 plays, or
    /// one of the runner's own when it runs as `slot --pico8`.
    pub struct Overlay {
        // Fields drop in this order: the compositor's GL objects need the
        // surface's context, so it must go first.
        compositor: Compositor,
        surface: Box<dyn Surface>,
        panel: (u32, u32),
        window: (u32, u32),
        turned: bool,
        kept: Kept,
        show_backdrop: bool,
        /// The tint the shim gave PICO-8's frames, taken off the backdrop
        /// so the compositor's grade puts it on once, as in slot.
        tint: [f32; 3],
        /// The mouth of the cart PICO-8 is running, in `kept.mouths`.
        mouth: Option<usize>,
        /// The first row of the buffer PICO-8 had on screen when it was
        /// stopped. The menu draws into the other buffer and shows it, so
        /// this one goes back on screen before PICO-8 carries on.
        home: Option<usize>,
    }

    /// What the menu uploads into the compositor, kept for the next time
    /// slot lends it: slot's compositor has no way to free a texture, and
    /// uploading it all again would only make the menu slower to come up.
    pub struct Kept {
        faces: QuickMenuFaces,
        icons: Vec<TexId>,
        backdrop: Option<(TexId, u32, u32)>,
        /// Each cart seated so far, with whether slot was light.
        mouths: Vec<(String, bool, Mouth)>,
        status: Option<Status>,
    }

    impl Kept {
        fn upload(compositor: &mut Compositor) -> Kept {
            let faces = upload_faces(compositor);
            let icons = Icon::ALL
                .iter()
                .map(|i| {
                    let f = icon_face(*i, HUD_ICON_PX, HUD_INK);
                    compositor.create_texture(f.w, f.h, &f.rgba)
                })
                .collect();
            Kept {
                faces,
                icons,
                backdrop: None,
                mouths: Vec::new(),
                status: None,
            }
        }
    }

    /// The battery and the clock in the top corners, as slot shows them over
    /// any game while its menu is open.
    struct Status {
        battery: Option<slot_power::Battery>,
        percent: (TexId, u32),
        clock: (TexId, u32),
        bolt: TexId,
    }

    fn turn_of(panel: (u32, u32)) -> PanelTurn {
        panel_turn(
            panel,
            crate::baseos::var("BASEOS_PANEL_ROTATION_CCW").as_deref(),
            std::env::var("SLOT_ROTATE").ok().as_deref(),
        )
    }

    impl Overlay {
        /// A window of the runner's own, for `slot --pico8`.
        pub fn open() -> Result<Overlay, String> {
            let surface = FbdevSurface::new().map_err(|e| e.to_string())?;
            let mut compositor = Compositor::new(&surface).map_err(|e| e.to_string())?;
            let panel = surface.window_size();
            let turn = turn_of(panel);
            if turn != PanelTurn::None {
                compositor
                    .set_panel_turn(panel, turn == PanelTurn::Ccw)
                    .map_err(|e| e.to_string())?;
            }
            Ok(Overlay::adopt(compositor, Box::new(surface), None))
        }

        /// slot's own window, lent while PICO-8 plays, already turned the way
        /// the panel is. `kept` is what the menu uploaded the last time.
        pub fn adopt(
            mut compositor: Compositor,
            surface: Box<dyn Surface>,
            kept: Option<Kept>,
        ) -> Overlay {
            let panel = surface.window_size();
            let turned = turn_of(panel) != PanelTurn::None;
            let window = if turned { (panel.1, panel.0) } else { panel };
            let kept = kept.unwrap_or_else(|| Kept::upload(&mut compositor));
            Overlay {
                surface,
                compositor,
                panel,
                window,
                turned,
                kept,
                show_backdrop: false,
                tint: [1.0; 3],
                mouth: None,
                home: None,
            }
        }

        /// Gives slot its window back, with what the menu uploaded into it.
        pub fn give_back(self) -> (Compositor, Box<dyn Surface>, Kept) {
            (self.compositor, self.surface, self.kept)
        }

        /// slot's HUD icons, for `Hud::set_icons`.
        pub fn hud_icons(&self) -> Vec<TexId> {
            self.kept.icons.clone()
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
            self.home = saved.front.map(|(_, y0)| y0);
            let (w, h) = self.panel;
            let frame = match self.turned {
                true => None,
                false => saved.front_untinted(w, h, self.tint),
            };
            self.show_backdrop = frame.is_some();
            let Some(rgba) = frame else {
                return;
            };
            match self.kept.backdrop {
                Some((tex, bw, bh)) if (bw, bh) == (w, h) => {
                    self.compositor.update_texture(tex, w, h, &rgba)
                }
                _ => {
                    let tex = self.compositor.create_texture(w, h, &rgba);
                    self.kept.backdrop = Some((tex, w, h));
                }
            }
        }

        /// Seats the cart slot handed to PICO-8 in the mouth under the menu:
        /// its face, the plastic behind a clear one, and the slot in slot's
        /// light or dark. Built once per cart and kept, so it is cheap to call
        /// before the menu is ever opened. Without the cart, the menu goes on
        /// without the mouth.
        pub fn seat(&mut self, root: &Path, state: &SlotState) {
            if self.mouth.is_some() {
                return;
            }
            let Some(stem) = state.cart.as_deref() else {
                return;
            };
            let light = state.appearance.light();
            if let Some(i) = self
                .kept
                .mouths
                .iter()
                .position(|(s, l, _)| s == stem && *l == light)
            {
                self.mouth = Some(i);
                return;
            }
            let cart = slot_store::scan_platform(root, Platform::Pico8)
                .ok()
                .and_then(|carts| carts.into_iter().find(|c| c.stem == stem));
            let Some(cart) = cart else {
                eprintln!("slot: pico8: menu: {stem} is not on the shelf, no mouth");
                return;
            };
            let c = &mut self.compositor;
            let mut up = |f: slot_ui::CartFace| c.create_texture(f.w, f.h, &f.rgba);
            let face = up(slot_ui::cart_face(&cart));
            let behind = match slot_ui::shows_interior(&cart) {
                true => [
                    Some(up(slot_ui::cart_interior(&cart))),
                    Some(up(slot_ui::cart_backing(slot_ui::shelf_shape(&cart).0))),
                ],
                false => [None, None],
            };
            // The slot itself is the same under every cart.
            let slot = match self.kept.mouths.iter().find(|(_, l, _)| *l == light) {
                Some((_, _, m)) => m.slot,
                None => {
                    let (back, front) = match light {
                        true => (slot_ui::slot_back_light(), slot_ui::slot_front_light()),
                        false => (slot_ui::slot_back(), slot_ui::slot_front()),
                    };
                    (
                        c.create_texture(slot_ui::SLOT_W, slot_ui::SLOT_H, back),
                        c.create_texture(slot_ui::SLOT_W, slot_ui::SLOT_H, front),
                    )
                }
            };
            self.kept.mouths.push((
                stem.to_string(),
                light,
                Mouth {
                    cart,
                    face,
                    behind,
                    slot,
                },
            ));
            self.mouth = Some(self.kept.mouths.len() - 1);
        }

        /// The battery and the time for the top corners, in the ink slot
        /// gives them over a game.
        pub fn set_status(&mut self, battery: Option<slot_power::Battery>, clock: &str) {
            let ink = footer_ink(false);
            let c = &mut self.compositor;
            let mut put = |face: UndoFace, old: Option<(TexId, u32)>| match old {
                Some((tex, _)) => {
                    c.update_texture(tex, face.w, face.h, &face.rgba);
                    (tex, face.w)
                }
                None => (c.create_texture(face.w, face.h, &face.rgba), face.w),
            };
            let old = self.kept.status.take();
            let percent = battery.map_or_else(String::new, |b| format!("{}%", b.percent));
            let percent = put(word_ink(&percent, ink), old.as_ref().map(|s| s.percent));
            let clock = put(word_ink(clock, ink), old.as_ref().map(|s| s.clock));
            let bolt = match old {
                Some(s) => s.bolt,
                None => {
                    let f = icon_face(Icon::Charging, BOLT_PX, ink);
                    c.create_texture(f.w, f.h, &f.rgba)
                }
            };
            self.kept.status = Some(Status {
                battery,
                percent,
                clock,
                bolt,
            });
        }

        /// Opens the menu as slot does over a game: its rows over the
        /// dimmed game at once, the mouth sliding up along the bottom.
        pub fn show(&mut self, row: QuickRow, hidden: &[QuickRow]) {
            self.slide(Some((row, hidden)), 0.0, 1.0);
        }

        /// The open menu, drawn again after a move.
        pub fn draw(&mut self, row: QuickRow, hidden: &[QuickRow]) {
            self.frame(Some((row, hidden)), 1.0);
        }

        /// Closes the menu as slot does: the game back at once, the mouth
        /// sliding down over it. The last frame is the game alone, as
        /// PICO-8 left it, so putting its buffers back changes nothing seen.
        pub fn hide(&mut self) {
            self.slide(None, 1.0, 0.0);
        }

        /// Puts PICO-8's own buffer back on screen, after the menu showed
        /// the other one. Called once fb0 holds PICO-8's frames again.
        pub fn restore(&mut self) {
            let Some(home) = self.home.take() else {
                return;
            };
            if front_buffer().is_some_and(|(_, y0)| y0 != home) && !pan_to(home) {
                eprintln!("slot: pico8: menu: could not put PICO-8's buffer back on screen");
            }
        }

        fn slide(&mut self, menu: Option<(QuickRow, &[QuickRow])>, from: f32, to: f32) {
            let began = std::time::Instant::now();
            loop {
                let k = (began.elapsed().as_secs_f32() / SLIDE_S).min(1.0);
                self.frame(menu, from + (to - from) * k);
                if k >= 1.0 {
                    return;
                }
            }
        }

        /// One frame: the game, the menu's rows when it is open, and the
        /// mouth with the cart and the legends `footer` of the way up, as
        /// slot draws its menu over any game.
        fn frame(&mut self, menu: Option<(QuickRow, &[QuickRow])>, footer: f32) {
            let canvas = Canvas::of(self.window);
            let dy = (1.0 - ease(footer.clamp(0.0, 1.0))) * MOUTH_H;
            let mut out = Vec::new();
            if let (true, Some((tex, _, _))) = (self.show_backdrop, self.kept.backdrop) {
                out.push(Draw::Tex {
                    x: 0.0,
                    y: 0.0,
                    w: canvas.w,
                    h: canvas.h,
                    tex,
                    alpha: 1.0,
                });
            }
            if let Some((row, hidden)) = menu {
                QuickMenu {
                    row,
                    hidden,
                    values: [None; QuickRow::ALL.len()],
                    clock: None,
                    wallpaper: None,
                    networks: None,
                    romm: [None; 4],
                    kid_shelf: None,
                    faces: Some(&self.kept.faces),
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
                    pico8: true,
                    // The menu's shade stops where the mouth begins, as in slot.
                    footer_top: canvas.h - MOUTH_H + dy,
                    canvas,
                }
                .draw(&mut out);
                if let Some(s) = &self.kept.status {
                    draw_corners(
                        canvas,
                        s.battery,
                        Printed::new(s.percent.0, s.percent.1),
                        Some(s.bolt),
                        Printed::new(s.clock.0, s.clock.1),
                        &mut out,
                    );
                }
            }
            if footer > 0.0 {
                let start = out.len();
                if let Some((_, _, m)) = self.mouth.and_then(|i| self.kept.mouths.get(i)) {
                    SlotChrome {
                        cart: &m.cart,
                        face: Some(m.face),
                        rest: 0.0,
                        scale: 1.0,
                        seat: 1.0,
                        alert: None,
                        dim: 0.0,
                        screen: 0.0,
                        game: false,
                        over: true,
                        behind: m.behind,
                        slot: Some(m.slot),
                        canvas,
                    }
                    .draw(&mut out);
                }
                let [_, _, open, resume, ..] = self.kept.faces.legend;
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
                crate::app::shift_down(&mut out[start..], dy);
            }
            self.present(&out);
        }

        fn present(&mut self, out: &[Draw]) {
            self.compositor.fit(self.window);
            self.compositor.begin_frame();
            self.compositor.draw_list(out);
            // The frame goes into fb0 by hand rather than through a swap: the
            // GPU skips writing tiles that match what it last drew into a
            // buffer, and PICO-8 and the copy put back have drawn there since,
            // so swapped frames came out with holes. It is written into the
            // buffer not on screen and then shown, so it never tears.
            if !self.turned {
                let (w, h) = self.panel;
                let frame = self.compositor.read_window(self.panel);
                if self.flip(&frame, w, h) {
                    return;
                }
            }
            self.compositor.end_frame(self.window);
            if let Err(e) = self.surface.swap() {
                eprintln!("slot: pico8: menu: {e}");
            }
        }

        /// Writes a frame into the buffer not on screen and shows it. With
        /// one buffer, or if the display will not change buffers, it goes
        /// over the buffer on screen instead.
        fn flip(&mut self, rgba: &[u8], w: u32, h: u32) -> bool {
            let Some((_, front)) = front_buffer() else {
                return false;
            };
            if let Some(back) = back_buffer(front, h as usize) {
                if write_rows(rgba, w, h, back) && pan_to(back) {
                    return true;
                }
            }
            write_rows(rgba, w, h, front)
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
