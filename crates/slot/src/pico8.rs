//! PICO-8 runs as Lexaloffle's own program, not as a core inside slot.
//!
//! slot cannot bundle PICO-8: it is paid software. The owner copies the
//! Raspberry Pi build they bought into `BIOS/PICO-8` (either the unzipped
//! `pico-8` folder or the zip itself). A cart on the PICO-8 shelf, or the
//! Splore card, is launched by handing the device over: slot writes what to
//! play to `$SLOT_RUN/slot.next`, saves and exits, and `launch_frontend.sh`
//! runs `slot --pico8`. That runs PICO-8, watches the buttons PICO-8 does not
//! use (MENU leaves, POWER sleeps), and when PICO-8 quits the launcher starts
//! slot again on the same shelf.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use slot_input::Btn;

use crate::input::evdev::Keymap;

/// Where the owner puts their copy of PICO-8, under the card's `BIOS`.
pub const INSTALL_DIR: &str = "PICO-8";
/// The folder carts go in, and the shelf's name in `Games`, `Labels`, `Saves`.
pub const PLATFORM_DIR: &str = "PICO8";
/// What slot leaves in `$SLOT_RUN` for the launcher to hand to `slot --pico8`.
pub const NEXT_FILE: &str = "slot.next";
/// The stem of the card on the PICO-8 shelf that opens Splore.
pub const SPLORE: &str = "Splore";

/// Binaries from the Raspberry Pi zip, most likely to work on BaseOS first.
/// `pico8_dyn` is 32-bit ARM and links SDL2 dynamically: BaseOS ships the
/// stock 32-bit Mali SDL2 in `/usr/lib32` for exactly this kind of program.
/// `pico8` links its own SDL2 statically and `pico8_64` needs an AArch64
/// SDL2, which BaseOS does not ship, so either only runs with libraries the
/// owner adds to `BIOS/PICO-8/lib`.
pub const BINARIES: [(&str, Arch); 3] = [
    ("pico8_dyn", Arch::Armhf),
    ("pico8", Arch::Armhf),
    ("pico8_64", Arch::Aarch64),
];

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Arch {
    Armhf,
    Aarch64,
}

impl Arch {
    pub fn loader(self) -> &'static str {
        match self {
            Arch::Armhf => "/lib/ld-linux-armhf.so.3",
            Arch::Aarch64 => "/lib/ld-linux-aarch64.so.1",
        }
    }

    pub fn library_path(self) -> &'static [&'static str] {
        match self {
            Arch::Armhf => &["/usr/lib32", "/usr/lib/arm-linux-gnueabihf"],
            Arch::Aarch64 => &["/usr/lib", "/usr/lib/aarch64-linux-gnu"],
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Launch {
    Cart(PathBuf),
    Splore,
}

impl Launch {
    pub fn encode(&self) -> String {
        match self {
            Launch::Cart(path) => format!("pico8\ncart\n{}\n", path.display()),
            Launch::Splore => "pico8\nsplore\n".to_string(),
        }
    }

    pub fn decode(text: &str) -> Option<Launch> {
        let mut lines = text.lines();
        if lines.next()? != "pico8" {
            return None;
        }
        match lines.next()? {
            "splore" => Some(Launch::Splore),
            "cart" => {
                let path = lines.next().filter(|p| !p.is_empty())?;
                Some(Launch::Cart(PathBuf::from(path)))
            }
            _ => None,
        }
    }

    pub fn args(&self) -> Vec<String> {
        match self {
            Launch::Cart(path) => vec!["-run".into(), path.display().to_string()],
            Launch::Splore => vec!["-splore".into()],
        }
    }
}

/// The Splore card: a real cart, so it can be favourited, labelled and
/// deleted like any other. Playing it opens Splore instead of the cart.
pub const SPLORE_CART: &[u8] = include_bytes!("../assets/Splore.p8");
const SPLORE_FILE: &str = "Splore.p8";
/// Set once the Splore card is first put on the shelf, so a deleted one stays
/// deleted.
const SPLORE_STOCKED: &str = "Config/pico8_splore_stocked";

pub fn is_splore(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(SPLORE_FILE))
}

/// What playing a cart on the PICO-8 shelf runs.
pub fn launch_for(rom: &Path) -> Launch {
    match is_splore(rom) {
        true => Launch::Splore,
        false => Launch::Cart(rom.to_path_buf()),
    }
}

/// Run on every boot: puts the Splore card on the shelf the first time
/// PICO-8 is installed, and draws labels for carts that have none.
pub fn stock_shelf(root: &Path) {
    let stocked = root.join(SPLORE_STOCKED);
    if !stocked.exists() && installed(root) {
        let cart = carts_dir(root).join(SPLORE_FILE);
        let _ = std::fs::create_dir_all(carts_dir(root));
        match cart.exists() || slot_store::atomic_write(&cart, SPLORE_CART).is_ok() {
            true => {
                let _ = std::fs::create_dir_all(root.join("Config"));
                let _ = std::fs::write(&stocked, b"");
            }
            false => eprintln!("slot: pico8: could not write {}", cart.display()),
        }
    }
    write_labels(root);
}

pub fn install_dir(root: &Path) -> PathBuf {
    crate::root::bios_dir(root).join(INSTALL_DIR)
}

pub fn carts_dir(root: &Path) -> PathBuf {
    root.join("Games").join(PLATFORM_DIR)
}

/// PICO-8's home: config.txt, cart data saves, Splore's downloads and
/// favourites. It sits with the other saves so it is backed up with them.
pub fn home_dir(root: &Path) -> PathBuf {
    crate::root::saves_dir(root).join(PLATFORM_DIR)
}

pub fn screenshots_dir(root: &Path) -> PathBuf {
    root.join("Screenshots").join(PLATFORM_DIR)
}

pub fn is_cart(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    name.ends_with(".p8") || name.ends_with(".p8.png")
}

/// The name a cart is listed under: `celeste.p8.png` is `celeste`.
pub fn cart_stem(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    let lower = name.to_ascii_lowercase();
    let cut = if lower.ends_with(".p8.png") {
        name.len() - ".p8.png".len()
    } else if lower.ends_with(".p8") {
        name.len() - ".p8".len()
    } else {
        return None;
    };
    Some(name[..cut].to_string())
}

pub fn find_binary(dir: &Path) -> Option<(PathBuf, Arch)> {
    let dirs = [dir.to_path_buf(), dir.join("pico-8")];
    BINARIES.iter().find_map(|(name, arch)| {
        dirs.iter()
            .map(|d| d.join(name))
            .find(|p| p.is_file() && p.with_file_name("pico8.dat").is_file())
            .map(|p| (p, *arch))
    })
}

/// The Raspberry Pi zip as it comes from Lexaloffle, left whole in `BIOS`
/// or `BIOS/PICO-8`.
pub fn find_zip(root: &Path) -> Option<PathBuf> {
    let bios = crate::root::bios_dir(root);
    [install_dir(root), bios].iter().find_map(|dir| {
        let mut zips: Vec<PathBuf> = std::fs::read_dir(dir)
            .ok()?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                let name = p
                    .file_name()
                    .map(|n| n.to_string_lossy().to_ascii_lowercase())
                    .unwrap_or_default();
                name.starts_with("pico-8") && name.ends_with(".zip") && !name.starts_with("._")
            })
            .collect();
        zips.sort();
        zips.pop()
    })
}

/// True when there is a PICO-8 to run, or a zip slot can unpack into one.
pub fn installed(root: &Path) -> bool {
    find_binary(&install_dir(root)).is_some() || find_zip(root).is_some()
}

/// Unpacks the Raspberry Pi zip into `BIOS/PICO-8`, dropping its top
/// `pico-8/` folder, when no binary is there yet. Returns the binary.
pub fn unpack(root: &Path) -> std::io::Result<Option<(PathBuf, Arch)>> {
    let dir = install_dir(root);
    if let Some(found) = find_binary(&dir) {
        return Ok(Some(found));
    }
    let Some(zip) = find_zip(root) else {
        return Ok(None);
    };
    eprintln!("slot: pico8: unpacking {}", zip.display());
    let file = std::fs::File::open(&zip)?;
    let mut archive = zip::ZipArchive::new(file).map_err(std::io::Error::other)?;
    std::fs::create_dir_all(&dir)?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(std::io::Error::other)?;
        let Some(inside) = inside_install(entry.name()) else {
            continue;
        };
        let out = dir.join(&inside);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut bytes)?;
        slot_store::atomic_write(&out, &bytes)?;
    }
    Ok(find_binary(&dir))
}

/// Where a zip entry lands under `BIOS/PICO-8`: plain names only, so nothing
/// can climb out, and without the zip's own `pico-8/` folder.
fn inside_install(name: &str) -> Option<PathBuf> {
    let mut parts = Vec::new();
    for part in Path::new(name).components() {
        match part {
            std::path::Component::Normal(p) => parts.push(p),
            std::path::Component::CurDir => {}
            _ => return None,
        }
    }
    if parts.first().is_some_and(|p| *p == "pico-8") {
        parts.remove(0);
    }
    (!parts.is_empty()).then(|| parts.iter().collect())
}

pub const LABEL: u32 = 128;

const PALETTE: [[u8; 3]; 32] = [
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
    [0x29, 0x18, 0x14],
    [0x11, 0x1d, 0x35],
    [0x42, 0x21, 0x36],
    [0x12, 0x53, 0x59],
    [0x74, 0x2f, 0x29],
    [0x49, 0x33, 0x3b],
    [0xa2, 0x88, 0x79],
    [0xf3, 0xef, 0x7d],
    [0xbe, 0x12, 0x50],
    [0xff, 0x6c, 0x24],
    [0xa8, 0xe7, 0x2e],
    [0x00, 0xb5, 0x43],
    [0x06, 0x5a, 0xb5],
    [0x75, 0x46, 0x65],
    [0xff, 0x6e, 0x59],
    [0xff, 0x9d, 0x81],
];

/// A cart's 128x128 label as RGBA: the `__label__` section of a `.p8`, or the
/// label window of a `.p8.png` cart image.
pub fn label(path: &Path) -> Option<Vec<u8>> {
    let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    let bytes = std::fs::read(path).ok()?;
    if name.ends_with(".p8.png") {
        label_from_png(&bytes)
    } else {
        label_from_p8(&String::from_utf8_lossy(&bytes))
    }
}

pub fn label_from_p8(text: &str) -> Option<Vec<u8>> {
    let mut lines = text.lines().skip_while(|l| l.trim() != "__label__").skip(1);
    let mut out = Vec::with_capacity((LABEL * LABEL * 4) as usize);
    for _ in 0..LABEL {
        let line = lines.next()?.trim();
        if line.starts_with("__") {
            return None;
        }
        let mut row = line.chars();
        for _ in 0..LABEL {
            let index = row.next().and_then(|c| c.to_digit(32)).unwrap_or(0) as usize;
            let [r, g, b] = PALETTE[index.min(PALETTE.len() - 1)];
            out.extend_from_slice(&[r, g, b, 0xff]);
        }
    }
    // An all-black label is what PICO-8 writes before one is captured.
    out.chunks_exact(4)
        .any(|px| px[..3] != [0, 0, 0])
        .then_some(out)
}

/// The label window inside the 160x205 cart picture PICO-8 saves.
const PNG_LABEL_AT: (u32, u32) = (16, 24);
const PNG_SIZE: (u32, u32) = (160, 205);

pub fn label_from_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    if (info.width, info.height) != PNG_SIZE {
        return None;
    }
    let channels = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        _ => return None,
    };
    let (x0, y0) = PNG_LABEL_AT;
    let mut out = Vec::with_capacity((LABEL * LABEL * 4) as usize);
    for y in y0..y0 + LABEL {
        for x in x0..x0 + LABEL {
            let at = (y as usize * info.line_size) + x as usize * channels;
            let px = buf.get(at..at + 3)?;
            out.extend_from_slice(&[px[0], px[1], px[2], 0xff]);
        }
    }
    Some(out)
}

/// Writes a label PNG for every cart in `Games/PICO8` that has none in
/// `Labels/PICO8`, so the shelf shows the art the cart carries.
pub fn write_labels(root: &Path) {
    let labels = root.join("Labels").join(PLATFORM_DIR);
    let Ok(entries) = std::fs::read_dir(carts_dir(root)) else {
        return;
    };
    for path in entries.filter_map(|e| e.ok().map(|e| e.path())) {
        if !is_cart(&path) {
            continue;
        }
        let Some(stem) = cart_stem(&path) else {
            continue;
        };
        let out = labels.join(format!("{stem}.png"));
        if out.exists() {
            continue;
        }
        let Some(rgba) = label(&path) else {
            continue;
        };
        let _ = std::fs::create_dir_all(&labels);
        match encode_png(&rgba, LABEL, LABEL) {
            Some(png) => {
                if let Err(e) = slot_store::atomic_write(&out, &png) {
                    eprintln!("slot: pico8: {}: {e}", out.display());
                }
            }
            None => eprintln!("slot: pico8: could not encode a label for {stem}"),
        }
    }
}

fn encode_png(rgba: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(rgba).ok()?;
    }
    Some(out)
}

/// What SDL learns about a gamepad from sysfs, enough to name it in a
/// `SDL_GAMECONTROLLERCONFIG` mapping.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Pad {
    pub name: String,
    pub bus: u16,
    pub vendor: u16,
    pub product: u16,
    pub version: u16,
    /// Key codes the device reports, lowest first.
    pub keys: Vec<u16>,
    /// Absolute axes the device reports, lowest first.
    pub axes: Vec<u16>,
}

const BTN_JOYSTICK: u16 = 0x120;
const KEY_MAX: u16 = 0x2ff;
const ABS_MAX: u16 = 0x3f;
const ABS_HAT0X: u16 = 0x10;
const ABS_HAT3Y: u16 = 0x17;

impl Pad {
    pub fn read(sys_device: &Path) -> Option<Pad> {
        let text = |p: &str| {
            std::fs::read_to_string(sys_device.join(p))
                .ok()
                .map(|s| s.trim().to_string())
        };
        let hex = |p: &str| text(p).and_then(|s| u16::from_str_radix(&s, 16).ok());
        let bits = |p: &str, max: u16| {
            let mask = text(p).unwrap_or_default();
            (0..=max)
                .filter(|b| slot_power::has_bit(&mask, *b))
                .collect::<Vec<u16>>()
        };
        Some(Pad {
            name: text("name")?,
            bus: hex("id/bustype").unwrap_or(0),
            vendor: hex("id/vendor").unwrap_or(0),
            product: hex("id/product").unwrap_or(0),
            version: hex("id/version").unwrap_or(0),
            keys: bits("capabilities/key", KEY_MAX),
            axes: bits("capabilities/abs", ABS_MAX),
        })
    }

    /// SDL's joystick GUID for this device, as SDL 2.0 builds before 2.26
    /// write it. Later SDL adds a name checksum in bytes 2 and 3 but still
    /// matches a mapping that leaves them zero.
    pub fn guid(&self) -> String {
        let mut g = [0u8; 16];
        g[0..2].copy_from_slice(&self.bus.to_le_bytes());
        if self.vendor != 0 && self.product != 0 {
            g[4..6].copy_from_slice(&self.vendor.to_le_bytes());
            g[8..10].copy_from_slice(&self.product.to_le_bytes());
            g[12..14].copy_from_slice(&self.version.to_le_bytes());
        } else {
            let name = self.name.as_bytes();
            let n = name.len().min(12);
            g[4..4 + n].copy_from_slice(&name[..n]);
        }
        g.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// SDL numbers buttons from BTN_JOYSTICK up, then the codes below it.
    fn button(&self, code: u16) -> Option<usize> {
        let order = self
            .keys
            .iter()
            .filter(|&&k| k >= BTN_JOYSTICK)
            .chain(self.keys.iter().filter(|&&k| k < BTN_JOYSTICK));
        order.enumerate().find(|(_, &k)| k == code).map(|(i, _)| i)
    }

    /// SDL numbers every absolute axis but the hats in code order.
    fn axis(&self, code: u16) -> Option<usize> {
        self.axes
            .iter()
            .filter(|&&a| !(ABS_HAT0X..=ABS_HAT3Y).contains(&a))
            .position(|&a| a == code)
    }

    fn has_hat(&self) -> bool {
        self.axes.contains(&ABS_HAT0X) && self.axes.contains(&(ABS_HAT0X + 1))
    }

    /// One `SDL_GAMECONTROLLERCONFIG` line. Buttons are placed by where they
    /// sit, as SDL expects: the bottom face button is SDL's `a`, which
    /// PICO-8 reads as O, and the right one is `b`, PICO-8's X.
    pub fn mapping(&self, keymap: Keymap) -> Option<String> {
        let code_of = |btn: Btn| {
            self.keys
                .iter()
                .copied()
                .find(|&code| keymap.btn(code) == Some(btn))
        };
        let mut fields = vec![self.guid(), self.name.replace(',', " ")];
        let buttons = [
            ("a", Btn::B),
            ("b", Btn::A),
            ("x", Btn::Y),
            ("y", Btn::X),
            ("back", Btn::Select),
            ("start", Btn::Start),
            ("leftshoulder", Btn::L1),
            ("rightshoulder", Btn::R1),
            ("lefttrigger", Btn::L2),
            ("righttrigger", Btn::R2),
        ];
        let mut mapped = 0;
        for (field, btn) in buttons {
            if let Some(index) = code_of(btn).and_then(|code| self.button(code)) {
                fields.push(format!("{field}:b{index}"));
                mapped += 1;
            }
        }
        if mapped < 2 {
            return None;
        }
        if self.has_hat() {
            fields.extend(
                ["dpup:h0.1", "dpright:h0.2", "dpdown:h0.4", "dpleft:h0.8"].map(String::from),
            );
        } else {
            for (field, btn) in [
                ("dpup", Btn::Up),
                ("dpdown", Btn::Down),
                ("dpleft", Btn::Left),
                ("dpright", Btn::Right),
            ] {
                if let Some(index) = code_of(btn).and_then(|code| self.button(code)) {
                    fields.push(format!("{field}:b{index}"));
                }
            }
        }
        let [x, y] = keymap.stick_axes();
        if let (Some(x), Some(y)) = (self.axis(x), self.axis(y)) {
            fields.push(format!("leftx:a{x}"));
            fields.push(format!("lefty:a{y}"));
        }
        fields.push("platform:Linux".into());
        Some(fields.join(",") + ",")
    }
}

/// Every gamepad the keymap reads, as SDL mapping lines.
pub fn controller_mappings(keymap: Keymap, dev: &Path, sys: &Path) -> Vec<String> {
    let mut lines: Vec<String> = keymap
        .pick_devices(dev, sys)
        .iter()
        .filter_map(|node| node.file_name())
        .filter_map(|name| Pad::read(&sys.join(name).join("device")))
        .filter_map(|pad| pad.mapping(keymap))
        .collect();
    lines.dedup();
    lines
}

/// PICO-8's volume runs 0 to 256; slot's runs 0 to 100.
pub fn volume(s: &slot_store::SlotState, headphones: bool) -> u32 {
    let (level, muted) = match headphones {
        true => (s.volume_hp, s.muted_hp),
        false => (s.volume, s.muted),
    };
    match muted {
        true => 0,
        false => {
            u32::from(level.min(slot_store::VOLUME_MAX)) * 256 / u32::from(slot_store::VOLUME_MAX)
        }
    }
}

/// The program and arguments that run PICO-8. The binary runs through its
/// loader, as slot does, because the card's FAT has no executable bit.
pub fn command_line(
    root: &Path,
    binary: &Path,
    arch: Arch,
    launch: &Launch,
    volume: u32,
) -> Vec<String> {
    let mut args = vec![
        arch.loader().to_string(),
        binary.display().to_string(),
        "-home".into(),
        home_dir(root).display().to_string(),
        "-root_path".into(),
        carts_dir(root).display().to_string(),
        "-desktop".into(),
        screenshots_dir(root).display().to_string(),
        "-windowed".into(),
        "0".into(),
        "-volume".into(),
        volume.to_string(),
    ];
    args.extend(launch.args());
    args
}

pub fn library_path(install: &Path, arch: Arch) -> String {
    std::iter::once(install.join("lib").display().to_string())
        .chain(arch.library_path().iter().map(|s| s.to_string()))
        .collect::<Vec<_>>()
        .join(":")
}

/// Loaded ahead of the 32-bit PICO-8: BaseOS's stock SDL2 has no haptics,
/// and PICO-8 cannot start SDL while it asks for them. See cores/pico8-sdl.
pub const SDL_SHIM: &str = "pico8_sdl.so";

pub fn sdl_shim(root: &Path) -> PathBuf {
    root.join("System").join(SDL_SHIM)
}

pub fn read_next(run: &Path) -> Option<Launch> {
    let file = run.join(NEXT_FILE);
    let text = std::fs::read_to_string(&file).ok()?;
    let _ = std::fs::remove_file(&file);
    Launch::decode(&text)
}

pub fn write_next(run: &Path, launch: &Launch) -> std::io::Result<()> {
    std::fs::write(run.join(NEXT_FILE), launch.encode())
}

#[cfg(unix)]
pub use runner::run;

#[cfg(unix)]
mod runner {
    use std::fs::File;
    use std::io::Read;
    use std::os::unix::io::AsRawFd;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command};
    use std::time::{Duration, Instant};

    use slot_input::Btn;
    use slot_power::Platform as _;

    use super::*;
    use crate::input::evdev::{decode, EVENT_BYTES, EV_KEY, EV_SW, SW_LID};

    const DEV: &str = "/dev/input";
    const SYS: &str = "/sys/class/input";
    const QUIT_GRACE: Duration = Duration::from_secs(3);

    /// `slot --pico8`: runs what slot asked for in `$SLOT_RUN/slot.next`.
    pub fn run(root: &Path, run: &Path) -> i32 {
        let Some(launch) = read_next(run) else {
            eprintln!(
                "slot: pico8: nothing to run in {}",
                run.join(NEXT_FILE).display()
            );
            return 2;
        };
        let (binary, arch) = match unpack(root) {
            Ok(Some(found)) => found,
            Ok(None) => {
                eprintln!(
                    "slot: pico8: no PICO-8 in {}; copy the Raspberry Pi zip there",
                    install_dir(root).display()
                );
                return 1;
            }
            Err(e) => {
                eprintln!("slot: pico8: unpack: {e}");
                return 1;
            }
        };
        for dir in [home_dir(root), carts_dir(root), screenshots_dir(root)] {
            let _ = std::fs::create_dir_all(dir);
        }
        let keymap = Keymap::of_system();
        let mappings = controller_mappings(keymap, Path::new(DEV), Path::new(SYS));
        if !mappings.is_empty() {
            let text = mappings.join("\n") + "\n";
            let _ = std::fs::write(home_dir(root).join("sdl_controllers.txt"), &text);
        }
        let mut platform = slot_power::DevicePlatform::new(root.to_path_buf());
        let state = slot_store::read_slot_state(root);
        let volume = volume(&state, platform.headphones());
        let line = command_line(root, &binary, arch, &launch, volume);
        eprintln!("slot: pico8: {}", line.join(" "));
        let install = binary.parent().unwrap_or(root).to_path_buf();
        let mut command = Command::new(&line[0]);
        command
            .args(&line[1..])
            .current_dir(&install)
            .env("LD_LIBRARY_PATH", library_path(&install, arch))
            .env("HOME", home_dir(root))
            .env("SDL_GAMECONTROLLERCONFIG", mappings.join("\n"));
        let shim = sdl_shim(root);
        if arch == Arch::Armhf && shim.is_file() {
            command.env("LD_PRELOAD", &shim);
        }
        let wifi = match launch {
            Launch::Splore => Wifi::up(root, run),
            Launch::Cart(_) => Wifi::Untouched,
        };
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) => {
                eprintln!("slot: pico8: {}: {e}", line[0]);
                wifi.down(root);
                return 1;
            }
        };
        let brightness = state.brightness;
        let code = watch(&mut child, keymap, &mut platform, brightness);
        platform.set_backlight(brightness);
        wifi.down(root);
        eprintln!("slot: pico8: exited {code}");
        0
    }

    const UDHCPC: &str = "/sbin/udhcpc";
    const DHCP_PID: &str = "/run/udhcpc.wlan0.pid";

    /// Splore browses the BBS, so it needs the network slot only brings up
    /// on screens that use it. It joins the networks saved in slot's Wi-Fi
    /// settings and leaves again after, unless slot already had it up.
    enum Wifi {
        Untouched,
        Started,
    }

    impl Wifi {
        fn up(root: &Path, run: &Path) -> Wifi {
            let conf = root.join("Config/wifi.conf");
            if !conf.is_file() || run.join("slotwifi.on").exists() {
                return Wifi::Untouched;
            }
            let started = Command::new("/bin/sh")
                .arg(root.join("System/slotlink.sh"))
                .args(["wifi", "start"])
                .arg(&conf)
                .status()
                .is_ok_and(|s| s.success());
            if !started {
                eprintln!("slot: pico8: wifi did not start, Splore will be offline");
                return Wifi::Untouched;
            }
            // udhcpc forks itself away once it has a lease or gives up on
            // the first try; PICO-8 starts meanwhile and Splore retries.
            match Command::new(UDHCPC)
                .args([
                    "-i", "wlan0", "-S", "-t", "5", "-T", "7", "-b", "-p", DHCP_PID,
                ])
                .spawn()
            {
                Ok(mut child) => {
                    std::thread::spawn(move || child.wait());
                }
                Err(e) => eprintln!("slot: pico8: {UDHCPC}: {e}"),
            }
            Wifi::Started
        }

        fn down(&self, root: &Path) {
            if let Wifi::Started = self {
                let _ = Command::new("/bin/sh")
                    .arg(root.join("System/slotlink.sh"))
                    .args(["wifi", "stop"])
                    .status();
            }
        }
    }

    /// Waits for PICO-8, reading the buttons it leaves alone: MENU asks it to
    /// quit, POWER turns the screen off and stops it until POWER again.
    /// Closing the lid of a clamshell does the same until it opens.
    fn watch(
        child: &mut Child,
        keymap: Keymap,
        platform: &mut slot_power::DevicePlatform,
        brightness: u8,
    ) -> i32 {
        let nodes: Vec<PathBuf> = keymap.pick_devices(Path::new(DEV), Path::new(SYS));
        let files: Vec<File> = nodes.iter().filter_map(|n| File::open(n).ok()).collect();
        for f in &files {
            set_nonblocking(f);
        }
        let hall = crate::input::device::find_hall(Path::new(PSY));
        let mut lid = Lid::default();
        let mut sleep = Sleep {
            pid: child.id() as libc::pid_t,
            brightness,
            asleep: false,
        };
        let mut quitting: Option<Instant> = None;
        let mut buf = [0u8; EVENT_BYTES * 16];
        loop {
            if let Ok(Some(status)) = child.try_wait() {
                return status.code().unwrap_or(-1);
            }
            if let Some(at) = quitting {
                if at.elapsed() > QUIT_GRACE {
                    eprintln!("slot: pico8: did not quit, killing it");
                    let _ = child.kill();
                }
            }
            if let Some(shut) = hall.as_deref().and_then(read_hall) {
                if let Some(shut) = lid.moved(shut) {
                    sleep.set(shut, platform);
                }
            }
            let mut fds: Vec<libc::pollfd> = files
                .iter()
                .map(|f| libc::pollfd {
                    fd: f.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                })
                .collect();
            let ready = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, 100) };
            if ready <= 0 {
                continue;
            }
            for (fd, mut file) in fds.iter().zip(files.iter()) {
                if fd.revents & libc::POLLIN == 0 {
                    continue;
                }
                let Ok(read) = file.read(&mut buf) else {
                    continue;
                };
                for ev in buf[..read].chunks_exact(EVENT_BYTES).filter_map(decode) {
                    if ev.kind == EV_SW && ev.code == SW_LID {
                        if let Some(shut) = lid.moved(ev.value != 0) {
                            sleep.set(shut, platform);
                        }
                        continue;
                    }
                    if ev.kind != EV_KEY || ev.value != 1 {
                        continue;
                    }
                    match keymap.btn(ev.code) {
                        Some(Btn::Menu) if !sleep.asleep && quitting.is_none() => {
                            eprintln!("slot: pico8: MENU, back to slot");
                            unsafe { libc::kill(sleep.pid, libc::SIGTERM) };
                            quitting = Some(Instant::now());
                        }
                        Some(Btn::Power) => {
                            let to = !sleep.asleep;
                            sleep.set(to, platform);
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    const PSY: &str = "/sys/class/power_supply";

    /// The SP-style hall sensor: 0 is shut, 1 is open.
    fn read_hall(path: &Path) -> Option<bool> {
        match std::fs::read_to_string(path).ok()?.trim() {
            "0" => Some(true),
            "1" => Some(false),
            _ => None,
        }
    }

    #[derive(Default)]
    struct Lid {
        shut: Option<bool>,
    }

    impl Lid {
        /// The new state when the lid has moved since it was last seen.
        fn moved(&mut self, shut: bool) -> Option<bool> {
            let was = self.shut.replace(shut);
            (was.is_some_and(|w| w != shut) || (was.is_none() && shut)).then_some(shut)
        }
    }

    struct Sleep {
        pid: libc::pid_t,
        brightness: u8,
        asleep: bool,
    }

    impl Sleep {
        fn set(&mut self, asleep: bool, platform: &mut slot_power::DevicePlatform) {
            if asleep == self.asleep {
                return;
            }
            self.asleep = asleep;
            eprintln!("slot: pico8: {}", if asleep { "sleep" } else { "wake" });
            match asleep {
                true => {
                    unsafe { libc::kill(self.pid, libc::SIGSTOP) };
                    platform.set_backlight(0);
                }
                false => {
                    platform.set_backlight(self.brightness);
                    unsafe { libc::kill(self.pid, libc::SIGCONT) };
                }
            }
        }
    }

    fn set_nonblocking(file: &File) {
        let fd = file.as_raw_fd();
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFL);
            libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }
    }
}
