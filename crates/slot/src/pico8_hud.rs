//! Volume and brightness while PICO-8 plays, with slot's own HUD.
//!
//! slot is not running then, so `slot --pico8` keeps its rules: the volume
//! keys, mute, and the brightness shortcuts change the same levels in the
//! same steps as in slot's games, and the bar is drawn by slot's `Hud`.
//!
//! PICO-8 only reads a volume when it starts, so it starts at full and the
//! shim slot loads into it (cores/pico8-sdl) applies slot's volume to its
//! audio, with slot's curve. The same shim tints what PICO-8 draws by slot's
//! blue light and dim gain, as slot's compositor tints its frames, and puts
//! the HUD strip over PICO-8's frames. All of it is read from a small file
//! in shared memory this module writes.

use slot_input::{Action, Millis, MUTE_CHORD_MS};
use slot_store::{SlotState, BLUE_LIGHT_MAX, BRIGHTNESS_MAX, DIM_MAX, VOLUME_MAX};
use slot_ui::{HudKind, HUD_MS};

/// The gain slot's compositor puts on every frame: the blue light's warm
/// tint times the dim steps below the backlight.
pub fn screen_gain(s: &SlotState) -> [f32; 3] {
    let dim = slot_gfx::dim_gain(s.dim);
    slot_gfx::blue_light_gain(s.blue_light).map(|g| g * dim)
}

/// slot's volume step, as in its games.
pub const VOLUME_STEP: u8 = 5;
/// The last stretch of `HUD_MS` the HUD fades out over, as slot-ui's.
pub const FADE_MS: Millis = 250;

/// What to put on the HUD after a change.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Shown {
    pub kind: HudKind,
    pub value: u8,
    pub muted: bool,
}

fn up(v: u8, step: u8, max: u8) -> u8 {
    v.saturating_add(step).min(max)
}

pub fn level(s: &SlotState, headphones: bool) -> u8 {
    match headphones {
        true => s.volume_hp,
        false => s.volume,
    }
}

fn level_mut(s: &mut SlotState, headphones: bool) -> &mut u8 {
    match headphones {
        true => &mut s.volume_hp,
        false => &mut s.volume,
    }
}

pub fn muted(s: &SlotState, headphones: bool) -> bool {
    match headphones {
        true => s.muted_hp,
        false => s.muted,
    }
}

fn muted_mut(s: &mut SlotState, headphones: bool) -> &mut bool {
    match headphones {
        true => &mut s.muted_hp,
        false => &mut s.muted,
    }
}

/// The volume the speaker or headphones play at, 0 to 100.
pub fn output_volume(s: &SlotState, headphones: bool) -> u8 {
    match muted(s, headphones) {
        true => 0,
        false => level(s, headphones),
    }
}

/// What the brightness bar shows: the dim steps below the backlight, then
/// the backlight's own.
pub fn brightness_level(s: &SlotState) -> u8 {
    DIM_MAX - s.dim.min(DIM_MAX) + s.brightness
}

/// slot's rules for the volume keys and the brightness and blue light
/// shortcuts, as `App::adjust` has them.
#[derive(Default, Debug)]
pub struct Levels {
    vol_before: Vec<(u8, bool, Millis)>,
}

impl Levels {
    pub fn act(
        &mut self,
        s: &mut SlotState,
        headphones: bool,
        action: Action,
        now: Millis,
    ) -> Option<Shown> {
        match action {
            Action::MuteToggle => {
                if let Some((volume, was_muted, _)) = self
                    .vol_before
                    .iter()
                    .find(|(_, _, at)| now.saturating_sub(*at) <= MUTE_CHORD_MS)
                    .copied()
                {
                    *level_mut(s, headphones) = volume;
                    *muted_mut(s, headphones) = was_muted;
                }
                self.vol_before.clear();
                let to = !muted(s, headphones);
                *muted_mut(s, headphones) = to;
                Some(Shown {
                    kind: HudKind::Volume,
                    value: output_volume(s, headphones),
                    muted: to,
                })
            }
            Action::BlueLightUp | Action::BlueLightDown => {
                s.blue_light = match action {
                    Action::BlueLightUp => up(s.blue_light, 1, BLUE_LIGHT_MAX),
                    _ => s.blue_light.saturating_sub(1),
                };
                Some(Shown {
                    kind: HudKind::BlueLight,
                    value: s.blue_light,
                    muted: muted(s, headphones),
                })
            }
            Action::BrightnessUp | Action::BrightnessDown => {
                step_brightness(s, action == Action::BrightnessUp);
                Some(Shown {
                    kind: HudKind::Brightness,
                    value: brightness_level(s),
                    muted: muted(s, headphones),
                })
            }
            Action::VolumeUp | Action::VolumeDown => {
                let was = level(s, headphones);
                let to = match action {
                    Action::VolumeUp => up(was, VOLUME_STEP, VOLUME_MAX),
                    _ => was.saturating_sub(VOLUME_STEP),
                };
                if self.vol_before.len() == 2 {
                    self.vol_before.remove(0);
                }
                self.vol_before.push((was, muted(s, headphones), now));
                *muted_mut(s, headphones) = false;
                *level_mut(s, headphones) = to;
                Some(Shown {
                    kind: HudKind::Volume,
                    value: output_volume(s, headphones),
                    muted: false,
                })
            }
            _ => None,
        }
    }
}

fn step_brightness(s: &mut SlotState, brighter: bool) {
    let dim = match brighter {
        true => s.dim.saturating_sub(1),
        false if s.brightness == 0 => up(s.dim, 1, DIM_MAX),
        false => s.dim,
    };
    if dim != s.dim {
        s.dim = dim;
        return;
    }
    s.brightness = match brighter {
        true => up(s.brightness, 1, BRIGHTNESS_MAX),
        false => s.brightness.saturating_sub(1),
    };
}

/// The HUD's opacity `now`, as slot-ui's `fade`: full, then the last
/// `FADE_MS` of `HUD_MS` down to nothing.
pub fn hud_alpha(shown_at: Millis, now: Millis) -> f32 {
    let age = now.saturating_sub(shown_at);
    if age >= HUD_MS {
        return 0.0;
    }
    ((HUD_MS - age) as f32 / FADE_MS as f32).min(1.0)
}

/// The strip as premultiplied RGBA, from the same HUD drawn once over black
/// and once over white: whatever the white shows through is what the strip
/// lets through, so `alpha = 1 - (white - black)`, and over black the colour
/// is already multiplied by it.
pub fn premultiplied(over_black: &[u8], over_white: &[u8]) -> Vec<u8> {
    over_black
        .chunks_exact(4)
        .zip(over_white.chunks_exact(4))
        .flat_map(|(b, w)| {
            let through = (0..3)
                .map(|c| u32::from(w[c].saturating_sub(b[c])))
                .sum::<u32>()
                / 3;
            let alpha = 255 - through.min(255) as u8;
            [b[0].min(alpha), b[1].min(alpha), b[2].min(alpha), alpha]
        })
        .collect()
}

/// The file the shim reads, laid out as cores/pico8-sdl/sdl_init.c's
/// `struct hud`: eight little-endian words, then the strip.
pub mod layout {
    pub const MAGIC: u32 = 0x4448_4c53;
    pub const HEADER: usize = 64;
    pub const MAX_W: u32 = 1024;
    pub const MAX_H: u32 = 128;
    pub const LEN: usize = HEADER + (MAX_W * MAX_H * 4) as usize;
    pub const W_MAGIC: usize = 0;
    /// Odd while the strip is being written.
    pub const W_SEQ: usize = 1;
    /// slot's output volume, 0 to 100.
    pub const W_VOLUME: usize = 2;
    /// 1 while the strip should be on screen.
    pub const W_SHOWN: usize = 3;
    pub const W_WIDTH: usize = 4;
    pub const W_HEIGHT: usize = 5;
    /// `screen_gain`'s red, green and blue, times `GAIN_ONE`.
    pub const W_GAIN: usize = 6;
    pub const GAIN_ONE: u32 = 65535;
}

#[cfg(unix)]
pub use shared::Shared;

#[cfg(unix)]
mod shared {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{fence, AtomicU32, Ordering};

    use super::layout::*;

    /// The shared file, mapped.
    pub struct Shared {
        path: PathBuf,
        base: *mut u8,
    }

    impl Shared {
        pub fn create(path: &Path) -> std::io::Result<Shared> {
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(true)
                .open(path)?;
            file.set_len(LEN as u64)?;
            let base = unsafe {
                libc::mmap(
                    std::ptr::null_mut(),
                    LEN,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_SHARED,
                    std::os::unix::io::AsRawFd::as_raw_fd(&file),
                    0,
                )
            };
            if base == libc::MAP_FAILED {
                return Err(std::io::Error::last_os_error());
            }
            let shared = Shared {
                path: path.to_path_buf(),
                base: base as *mut u8,
            };
            shared.word(W_VOLUME).store(100, Ordering::Relaxed);
            shared.set_gain([1.0; 3]);
            shared.word(W_MAGIC).store(MAGIC, Ordering::Release);
            Ok(shared)
        }

        pub fn path(&self) -> &Path {
            &self.path
        }

        fn word(&self, i: usize) -> &AtomicU32 {
            unsafe { &*(self.base as *const AtomicU32).add(i) }
        }

        pub fn set_volume(&self, volume: u8) {
            self.word(W_VOLUME)
                .store(u32::from(volume.min(100)), Ordering::Release);
        }

        /// The tint PICO-8's frames and the strip get, `screen_gain`.
        pub fn set_gain(&self, gain: [f32; 3]) {
            for (i, g) in gain.into_iter().enumerate() {
                let fixed = (g.clamp(0.0, 1.0) * GAIN_ONE as f32).round() as u32;
                self.word(W_GAIN + i).store(fixed, Ordering::Release);
            }
        }

        /// Puts `rgba`, premultiplied and `w` by `h`, on screen.
        pub fn show(&self, rgba: &[u8], w: u32, h: u32) {
            if w > MAX_W || h > MAX_H || rgba.len() != (w * h * 4) as usize {
                return;
            }
            let seq = self.word(W_SEQ);
            seq.fetch_add(1, Ordering::AcqRel);
            fence(Ordering::Release);
            unsafe {
                std::ptr::copy_nonoverlapping(rgba.as_ptr(), self.base.add(HEADER), rgba.len());
            }
            self.word(W_WIDTH).store(w, Ordering::Relaxed);
            self.word(W_HEIGHT).store(h, Ordering::Relaxed);
            self.word(W_SHOWN).store(1, Ordering::Relaxed);
            fence(Ordering::Release);
            seq.fetch_add(1, Ordering::AcqRel);
        }

        pub fn hide(&self) {
            self.word(W_SHOWN).store(0, Ordering::Release);
        }
    }

    impl Drop for Shared {
        fn drop(&mut self) {
            unsafe { libc::munmap(self.base as *mut libc::c_void, LEN) };
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
