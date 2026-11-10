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
    shelve_favourites(root, |_| Fetched::Offline);
    write_labels(root);
}

/// The core a cart plays on: the one chosen for it, except that a cart set to
/// the owner's PICO-8 plays on fake-08 while there is no PICO-8 on the card.
pub fn usable(root: &Path, core: slot_store::Core) -> slot_store::Core {
    match core == slot_store::Core::Pico8 && !installed(root) {
        true => slot_store::Core::Fake08,
        false => core,
    }
}

/// The shelf as it can be played: without PICO-8 on the card the Splore card
/// is left off, though its file stays, so it is back once PICO-8 is.
pub fn playable(root: &Path, mut carts: Vec<slot_store::Cart>) -> Vec<slot_store::Cart> {
    if carts.iter().any(|c| is_splore(&c.rom)) && !installed(root) {
        carts.retain(|c| !is_splore(&c.rom));
    }
    carts
}

/// Where PICO-8 lists the carts favourited in Splore, in its home.
const FAVOURITES: &str = "favourites.txt";
/// The favourites slot has put on the shelf, one id a line, so each goes on
/// once and one taken off the shelf stays off.
const SHELVED: &str = "Config/pico8_favourites.txt";

/// A cart favourited in Splore.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Favourite {
    /// The BBS cart id with its revision, as in `celeste_classic-2` or `15133`.
    pub id: String,
    pub title: String,
}

/// Reads PICO-8's `favourites.txt`: one cart a line, fields between `|`, the
/// cart id second and its title seventh. A cart PICO-8 took for a local one
/// has no id, only its path in `bbs`, which names it just as well.
pub fn parse_favourites(text: &str) -> Vec<Favourite> {
    let mut out: Vec<Favourite> = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split('|').map(str::trim).collect();
        if fields.len() < 7 {
            continue;
        }
        let id = match fields[1] {
            "" => Path::new(fields[5])
                .starts_with("bbs")
                .then(|| cart_stem(Path::new(fields[5])))
                .flatten()
                .unwrap_or_default(),
            id => id.to_string(),
        };
        if !is_bbs_id(&id) || out.iter().any(|f| f.id == id) {
            continue;
        }
        let title = match fields[6] {
            "" => id.clone(),
            t => t.to_string(),
        };
        out.push(Favourite { id, title });
    }
    out
}

/// BBS ids are letters, digits, `_` and `-`: nothing that reaches outside a
/// folder or a URL path.
fn is_bbs_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Splore's own copy of a cart it has run: `bbs/carts/<id>.p8.png`, or, for
/// carts with a number for an id, `bbs/<n>/<id>.p8.png`.
pub fn bbs_cart(home: &Path, id: &str) -> Option<PathBuf> {
    let name = format!("{id}.p8.png");
    let bbs = home.join("bbs");
    let mut dirs = vec![bbs.join("carts")];
    if let Ok(entries) = std::fs::read_dir(&bbs) {
        let mut more: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_dir() && !p.ends_with("carts"))
            .collect();
        more.sort();
        dirs.extend(more);
    }
    dirs.into_iter().map(|d| d.join(&name)).find(|p| p.is_file())
}

/// Where the BBS keeps a cart: under the first character of a numbered id,
/// the first two of any other.
pub fn bbs_url(id: &str) -> Option<String> {
    if !is_bbs_id(id) || id.len() < 2 {
        return None;
    }
    let numbered = id.as_bytes()[..2].iter().all(u8::is_ascii_digit);
    let folder = &id[..if numbered { 1 } else { 2 }];
    Some(format!(
        "https://www.lexaloffle.com/bbs/cposts/{folder}/{id}.p8.png"
    ))
}

/// What became of asking the BBS for a cart.
pub enum Fetched {
    Cart(Vec<u8>),
    /// The BBS answered, without the cart.
    Missing,
    /// No answer: asking for more now would only wait as long again.
    Offline,
}

/// The name a favourite gets on the shelf: its title in capitals where words
/// start, as Splore shows it, without what a FAT card cannot name a file.
pub fn shelf_name(title: &str, id: &str) -> String {
    let mut out = String::new();
    let mut start = true;
    for c in title.chars() {
        if c.is_control() || "<>:\"/\\|?*".contains(c) {
            continue;
        }
        match start {
            true => out.extend(c.to_uppercase()),
            false => out.push(c),
        }
        start = c.is_whitespace();
    }
    let words: Vec<&str> = out.split_whitespace().collect();
    let mut name: String = words.join(" ");
    if name.chars().count() > 60 {
        name = name.chars().take(60).collect::<String>().trim_end().to_string();
    }
    let name = name.trim_end_matches(['.', ' ']).to_string();
    match name.is_empty() || name.starts_with('.') || is_splore(Path::new(&format!("{name}.p8"))) {
        true => id.to_string(),
        false => name,
    }
}

/// Puts every cart favourited in Splore on the PICO-8 shelf, once each, as a
/// `.p8.png` under its title. The cart is Splore's own copy when it has one,
/// or whatever `fetch` gets from the BBS. Returns how many went on; their
/// labels are drawn with the others' at the next boot.
pub fn shelve_favourites(root: &Path, mut fetch: impl FnMut(&str) -> Fetched) -> usize {
    let home = home_dir(root);
    let Ok(text) = std::fs::read_to_string(home.join(FAVOURITES)) else {
        return 0;
    };
    let record = root.join(SHELVED);
    let mut shelved: Vec<String> = std::fs::read_to_string(&record)
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    let mut offline = false;
    let mut added = 0;
    for fav in parse_favourites(&text) {
        if shelved.contains(&fav.id) {
            continue;
        }
        let bytes = match bbs_cart(&home, &fav.id).and_then(|p| std::fs::read(p).ok()) {
            Some(bytes) => bytes,
            None if offline => continue,
            None => match fetch(&fav.id) {
                Fetched::Cart(bytes) => bytes,
                Fetched::Missing => {
                    eprintln!("slot: pico8: {} is not on the BBS", fav.id);
                    continue;
                }
                Fetched::Offline => {
                    offline = true;
                    continue;
                }
            },
        };
        if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            eprintln!("slot: pico8: {} is not a cart", fav.id);
            continue;
        }
        let dir = carts_dir(root);
        let name = shelf_name(&fav.title, &fav.id);
        let cart = [name.clone(), format!("{name} ({})", fav.id)]
            .into_iter()
            .map(|n| dir.join(format!("{n}.p8.png")))
            .find(|p| !p.exists() || std::fs::read(p).is_ok_and(|b| b == bytes));
        let Some(cart) = cart else {
            eprintln!("slot: pico8: no free name for {}", fav.id);
            continue;
        };
        let _ = std::fs::create_dir_all(&dir);
        if let Err(e) = slot_store::atomic_write(&cart, &bytes) {
            eprintln!("slot: pico8: {}: {e}", cart.display());
            continue;
        }
        eprintln!("slot: pico8: favourite {} on the shelf as {}", fav.id, cart.display());
        shelved.push(fav.id);
        added += 1;
        let _ = std::fs::create_dir_all(root.join("Config"));
        let text = shelved.join("\n") + "\n";
        if let Err(e) = slot_store::atomic_write(&record, text.as_bytes()) {
            eprintln!("slot: pico8: {}: {e}", record.display());
        }
    }
    added
}

/// Asks the BBS for a cart.
pub fn fetch_bbs(agent: &ureq::Agent, id: &str) -> Fetched {
    let Some(url) = bbs_url(id) else {
        return Fetched::Missing;
    };
    let response = match agent.get(&url).call() {
        Ok(r) => r,
        Err(ureq::Error::StatusCode(code)) => {
            eprintln!("slot: pico8: {url}: {code}");
            return Fetched::Missing;
        }
        Err(e) => {
            eprintln!("slot: pico8: {url}: {e}");
            return Fetched::Offline;
        }
    };
    let mut bytes = Vec::new();
    match response
        .into_body()
        .into_with_config()
        .limit(BBS_CART_LIMIT)
        .reader()
        .read_to_end(&mut bytes)
    {
        Ok(_) => Fetched::Cart(bytes),
        Err(e) => {
            eprintln!("slot: pico8: {url}: {e}");
            Fetched::Offline
        }
    }
}

/// A `.p8.png` is 160x205 pixels; nothing honest is near this size.
const BBS_CART_LIMIT: u64 = 1 << 20;

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

/// Every gamepad the keymap reads, with its device node and SDL mapping line.
fn controllers(keymap: Keymap, dev: &Path, sys: &Path) -> Vec<(PathBuf, String)> {
    keymap
        .pick_devices(dev, sys)
        .into_iter()
        .filter_map(|node| {
            let pad = Pad::read(&sys.join(node.file_name()?).join("device"))?;
            let line = pad.mapping(keymap)?;
            Some((node, line))
        })
        .collect()
}

/// Every gamepad the keymap reads, as SDL mapping lines.
pub fn controller_mappings(keymap: Keymap, dev: &Path, sys: &Path) -> Vec<String> {
    let mut lines: Vec<String> = controllers(keymap, dev, sys)
        .into_iter()
        .map(|(_, line)| line)
        .collect();
    lines.dedup();
    lines
}

/// The gamepads for `SDL_JOYSTICK_DEVICE`, colon separated. BaseOS runs no
/// udevd, so nothing tags a device as a joystick and SDL 2.0.12's udev scan
/// finds none. Named here, SDL opens each and checks its buttons itself.
pub fn joystick_devices(keymap: Keymap, dev: &Path, sys: &Path) -> String {
    controllers(keymap, dev, sys)
        .iter()
        .map(|(node, _)| node.display().to_string())
        .collect::<Vec<_>>()
        .join(":")
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

/// The program and arguments that run PICO-8. With `direct` the binary runs
/// as itself, which it should whenever the card allows: PICO-8 looks for
/// `pico8.dat` beside `/proc/self/exe`, and through the loader that is `/lib`,
/// on BaseOS the read-only `/usr/lib`. A card mounted without an executable
/// bit still gets the loader, as slot does.
pub fn command_line(
    root: &Path,
    binary: &Path,
    arch: Arch,
    launch: &Launch,
    volume: u32,
    direct: bool,
) -> Vec<String> {
    let mut args = Vec::new();
    if !direct {
        args.push(arch.loader().to_string());
    }
    args.extend([
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
    ]);
    args.extend(launch.args());
    args
}

/// Whether the card lets `binary` run as a program of its own.
pub fn runs_directly(binary: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::metadata(binary).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        let _ = binary;
        false
    }
}

/// Where PICO-8 looks for libraries, the owner's own first.
pub fn library_dirs(install: &Path, arch: Arch) -> Vec<PathBuf> {
    std::iter::once(install.join("lib"))
        .chain(arch.library_path().iter().map(PathBuf::from))
        .collect()
}

pub fn library_path(install: &Path, arch: Arch) -> String {
    library_dirs(install, arch)
        .iter()
        .map(|d| d.display().to_string())
        .collect::<Vec<_>>()
        .join(":")
}

/// The GLES and EGL libraries SDL should load, as `(variable, path)`. Left
/// to itself, BaseOS's 32-bit SDL asks for `libGLESv2.so`, which no folder
/// has, and PICO-8 cannot open its window. Each is the first of `dirs` that
/// has it, so libraries the owner adds win.
pub fn gl_drivers(dirs: &[PathBuf]) -> Vec<(&'static str, PathBuf)> {
    [
        ("SDL_VIDEO_GL_DRIVER", "libGLESv2.so.2"),
        ("SDL_VIDEO_EGL_DRIVER", "libEGL.so.1"),
    ]
    .into_iter()
    .filter_map(|(var, name)| {
        dirs.iter()
            .map(|d| d.join(name))
            .find(|p| p.is_file())
            .map(|p| (var, p))
    })
    .collect()
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

    use slot_input::{Btn, Context, Gestures, Millis, RawEvent};
    use slot_power::Platform as _;

    use super::*;
    use crate::input::evdev::{decode, Hat, EVENT_BYTES, EV_SW, SW_LID};
    use crate::pico8_hud::{output_volume, Levels, Shared};
    use crate::pico8_menu::{grab, power_off, Menu, Overlay, Saved, Step, Still};
    use slot_ui::{Hud, HudKind};

    const DEV: &str = "/dev/input";
    const SYS: &str = "/sys/class/input";
    const QUIT_GRACE: Duration = Duration::from_secs(3);
    /// How long one cart may take to come from the BBS.
    const BBS_TIMEOUT: Duration = Duration::from_secs(15);

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
        let mut state = slot_store::read_slot_state(root);
        let headphones = platform.headphones();
        let shim = sdl_shim(root);
        // With the shim in, PICO-8 plays at full and the shim applies slot's
        // volume, so the volume keys work while it plays.
        let shared = match arch == Arch::Armhf && shim.is_file() {
            true => Shared::create(Path::new(HUD_FILE))
                .map_err(|e| eprintln!("slot: pico8: {HUD_FILE}: {e}"))
                .ok(),
            false => None,
        };
        let volume = match &shared {
            Some(s) => {
                s.set_volume(crate::pico8_hud::output_volume(&state, headphones));
                s.set_gain(crate::pico8_hud::screen_gain(&state));
                256
            }
            None => volume(&state, headphones),
        };
        let line = command_line(root, &binary, arch, &launch, volume, runs_directly(&binary));
        eprintln!("slot: pico8: {}", line.join(" "));
        let install = binary.parent().unwrap_or(root).to_path_buf();
        let mut command = Command::new(&line[0]);
        command
            .args(&line[1..])
            .current_dir(&install)
            .env("LD_LIBRARY_PATH", library_path(&install, arch))
            .env("HOME", home_dir(root))
            .env("SDL_GAMECONTROLLERCONFIG", mappings.join("\n"));
        for (var, path) in gl_drivers(&library_dirs(&install, arch)) {
            eprintln!("slot: pico8: {var}={}", path.display());
            command.env(var, path);
        }
        let pads = joystick_devices(keymap, Path::new(DEV), Path::new(SYS));
        if !pads.is_empty() {
            eprintln!("slot: pico8: SDL_JOYSTICK_DEVICE={pads}");
            command.env("SDL_JOYSTICK_DEVICE", pads);
        }
        if arch == Arch::Armhf && shim.is_file() {
            command.env("LD_PRELOAD", &shim);
        }
        if let Some(s) = &shared {
            command.env(HUD_ENV, s.path());
        }
        // slot handed over with the cart seated in the slot, and that frame is
        // still on screen. Leaving PICO-8 powers off into it, and slot starts
        // again on it to eject the cart, so there is no black between them.
        let seated = Still::take();
        // The menu and the HUD draw with slot's own window, opened before
        // PICO-8 opens its own, as slot's is when it hands over.
        let overlay = match shared.is_some() {
            true => Overlay::open()
                .map_err(|e| eprintln!("slot: pico8: no window for the menu: {e}"))
                .ok(),
            false => None,
        };
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
        let code = watch(
            &mut child,
            keymap,
            &mut platform,
            root,
            &mut state,
            shared.as_ref(),
            overlay,
            seated,
        );
        platform.set_backlight(state.brightness + 1);
        // Carts favourited in Splore go on the shelf while the network is
        // still up for the ones Splore never downloaded.
        if launch == Launch::Splore {
            let agent = crate::update::agent(BBS_TIMEOUT);
            let added = shelve_favourites(root, |id| fetch_bbs(&agent, id));
            if added > 0 {
                eprintln!("slot: pico8: {added} favourite(s) put on the shelf");
            }
        }
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

    /// Waits for PICO-8, reading the buttons it leaves alone. A MENU tap
    /// stops it under slot's in-game menu; holding MENU, or Exit Game, asks
    /// it to quit. POWER turns the screen off and stops it until POWER
    /// again. Closing the lid of a clamshell does the same until it opens.
    /// The volume keys and the brightness shortcuts work as in slot, with
    /// slot's HUD over the game, when `shared` reaches the shim.
    fn watch(
        child: &mut Child,
        keymap: Keymap,
        platform: &mut slot_power::DevicePlatform,
        root: &Path,
        state: &mut slot_store::SlotState,
        shared: Option<&Shared>,
        mut overlay: Option<Overlay>,
        seated: Option<Still>,
    ) -> i32 {
        let nodes: Vec<PathBuf> = keymap.pick_devices(Path::new(DEV), Path::new(SYS));
        let files: Vec<File> = nodes.iter().filter_map(|n| File::open(n).ok()).collect();
        for f in &files {
            set_nonblocking(f);
        }
        let mut hats: Vec<Hat> = files.iter().map(|_| Hat::new(keymap)).collect();
        let hall = crate::input::device::find_hall(Path::new(PSY));
        let mut lid = Lid::default();
        let mut sleep = Sleep {
            pid: child.id() as libc::pid_t,
            backlight: state.brightness + 1,
            asleep: false,
            held: false,
        };
        let mut gestures = Gestures::new();
        gestures.set_entries(
            crate::input::Table::new(
                &slot_store::read_controls(root),
                &slot_store::read_shortcuts(root),
                &slot_store::read_carousel(root),
                state.turbo,
            )
            .entries(),
        );
        let mut menu = Menu::default();
        let mut saved: Option<Saved> = None;
        let hidden = crate::pico8_menu::hidden();
        let mut levels = Levels::default();
        let mut hud = Hud::new();
        let mut hud_drawn: Option<f32> = None;
        let mut unsaved: Option<Millis> = None;
        let began = Instant::now();
        let mut quitting: Option<Instant> = None;
        let mut buf = [0u8; EVENT_BYTES * 16];
        loop {
            if let Ok(Some(status)) = child.try_wait() {
                if menu.is_open() {
                    for f in &files {
                        grab(f, false);
                    }
                    if let Some(o) = overlay.as_mut() {
                        o.restore();
                    }
                }
                if unsaved.is_some() {
                    save_state(root, state);
                }
                match (quitting.is_some(), &seated) {
                    // Already powered off into the seated cart: put it back
                    // over anything PICO-8 drew on its way out.
                    (true, Some(s)) => s.show(),
                    // PICO-8 quit by itself: power its last frame off now.
                    (false, Some(s)) => match Still::take() {
                        Some(game) => power_off(&game, s),
                        None => s.show(),
                    },
                    (_, None) => blank_screen(),
                }
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
            let wait = match hud_drawn.is_some_and(|a| a > 0.0) {
                true => 16,
                false => 50,
            };
            let ready = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, wait) };
            let now = began.elapsed().as_millis() as Millis;
            let mut actions = gestures.tick(now);
            for (i, fd) in fds.iter().enumerate().filter(|_| ready > 0) {
                if fd.revents & libc::POLLIN == 0 {
                    continue;
                }
                let Ok(read) = (&files[i]).read(&mut buf) else {
                    continue;
                };
                for ev in buf[..read].chunks_exact(EVENT_BYTES).filter_map(decode) {
                    if ev.kind == EV_SW && ev.code == SW_LID {
                        if let Some(shut) = lid.moved(ev.value != 0) {
                            sleep.set(shut, platform);
                        }
                        continue;
                    }
                    let raws = match hats[i].takes(ev) {
                        true => hats[i].feed(ev),
                        false => keymap.to_raw(ev).into_iter().collect(),
                    };
                    for raw in raws {
                        if raw == RawEvent::Down(Btn::Power) {
                            let to = !sleep.asleep;
                            sleep.set(to, platform);
                            continue;
                        }
                        actions.extend(gestures.feed(raw, now));
                    }
                }
            }
            if let Some(at) = unsaved {
                if now.saturating_sub(at) >= SAVE_AFTER_MS {
                    save_state(root, state);
                    unsaved = None;
                }
            }
            if sleep.asleep || quitting.is_some() {
                if let Some(s) = shared {
                    update_hud(&mut hud, &mut hud_drawn, &mut overlay, s, now);
                }
                continue;
            }
            for action in actions {
                if let (Some(s), false) = (shared, menu.is_open()) {
                    let headphones = platform.headphones();
                    if let Some(shown) = levels.act(state, headphones, action, now) {
                        if shown.kind == HudKind::Brightness {
                            sleep.backlight = state.brightness + 1;
                            platform.set_backlight(sleep.backlight);
                        }
                        s.set_volume(output_volume(state, headphones));
                        s.set_gain(crate::pico8_hud::screen_gain(state));
                        hud.set_headphones(headphones);
                        hud.show(shown.kind, shown.value, shown.muted, now);
                        hud_drawn = None;
                        unsaved = Some(now);
                        continue;
                    }
                }
                match menu.act(action) {
                    Step::Nothing => {}
                    Step::Open => {
                        stop(sleep.pid);
                        sleep.held = true;
                        saved = Saved::settled();
                        if overlay.is_none() {
                            match Overlay::open() {
                                Ok(o) => overlay = Some(o),
                                Err(e) => eprintln!("slot: pico8: menu: {e}"),
                            }
                        }
                        let (Some(o), Some(s)) = (overlay.as_mut(), saved.as_ref()) else {
                            eprintln!("slot: pico8: menu: no screen, carrying on");
                            menu = Menu::default();
                            sleep.held = false;
                            unsafe { libc::kill(sleep.pid, libc::SIGCONT) };
                            continue;
                        };
                        eprintln!("slot: pico8: menu");
                        for f in &files {
                            grab(f, true);
                        }
                        gestures.set_context(Context::Menu);
                        o.set_grade(state.blue_light, state.dim);
                        o.set_backdrop(s);
                        o.seat(root, state);
                        o.set_status(platform.battery(), &clock_text(state));
                        if let Some(row) = menu.row() {
                            o.show(row, &hidden);
                        }
                    }
                    Step::Moved => {
                        if let (Some(o), Some(row)) = (overlay.as_mut(), menu.row()) {
                            o.draw(row, &hidden);
                        }
                    }
                    Step::Resume => {
                        eprintln!("slot: pico8: menu: resume");
                        if let Some(o) = overlay.as_mut() {
                            o.hide();
                            o.settle();
                        }
                        if let Some(s) = saved.take() {
                            s.put_back();
                        }
                        if let Some(o) = overlay.as_mut() {
                            o.restore();
                        }
                        for f in &files {
                            grab(f, false);
                        }
                        gestures.set_context(Context::Game);
                        sleep.held = false;
                        unsafe { libc::kill(sleep.pid, libc::SIGCONT) };
                    }
                    Step::Quit => {
                        eprintln!("slot: pico8: back to slot");
                        // The frame to power off: the one under the menu, or,
                        // leaving straight from the game, the one on screen,
                        // with PICO-8 stopped so it stays put meanwhile.
                        let game = match saved.as_ref() {
                            Some(s) => s.still(),
                            None => {
                                stop(sleep.pid);
                                Still::take()
                            }
                        };
                        for f in &files {
                            grab(f, false);
                        }
                        // No putting PICO-8's buffer back on screen: it holds
                        // an older menu frame by now, and the power off shows
                        // its own frames and leaves the seated cart in every
                        // buffer.
                        match (&game, &seated) {
                            (Some(g), Some(s)) => power_off(g, s),
                            (None, Some(s)) => s.show(),
                            (_, None) => blank_screen(),
                        }
                        unsafe {
                            libc::kill(sleep.pid, libc::SIGTERM);
                            libc::kill(sleep.pid, libc::SIGCONT);
                        }
                        quitting = Some(Instant::now());
                        break;
                    }
                }
            }
            if let Some(s) = shared {
                update_hud(&mut hud, &mut hud_drawn, &mut overlay, s, now);
            }
        }
    }

    /// The time for the menu's corner, in the zone and format slot shows it.
    fn clock_text(state: &slot_store::SlotState) -> String {
        let utc = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        slot_ui::time_text(state.zone.local(utc), state.clock_24h)
    }

    const HUD_FILE: &str = "/dev/shm/slot-pico8-hud";
    /// Where the shim finds `HUD_FILE`.
    const HUD_ENV: &str = "SLOT_PICO8_HUD";
    const SAVE_AFTER_MS: Millis = 1000;

    /// Keeps the shim's strip in step with `hud`: drawn again whenever its
    /// opacity changes, hidden once it has faded.
    fn update_hud(
        hud: &mut Hud,
        drawn: &mut Option<f32>,
        overlay: &mut Option<Overlay>,
        shared: &Shared,
        now: Millis,
    ) {
        let alpha = hud
            .shown_at
            .map_or(0.0, |at| crate::pico8_hud::hud_alpha(at, now));
        if *drawn == Some(alpha) {
            return;
        }
        *drawn = Some(alpha);
        if alpha <= 0.0 {
            shared.hide();
            return;
        }
        if overlay.is_none() {
            match Overlay::open() {
                Ok(o) => *overlay = Some(o),
                Err(e) => {
                    eprintln!("slot: pico8: hud: {e}");
                    return;
                }
            }
        }
        let Some(o) = overlay.as_mut() else {
            return;
        };
        hud.set_icons(o.hud_icons());
        if let Some((rgba, w, h)) = o.hud_strip(hud, now) {
            shared.show(&rgba, w, h);
        }
    }

    fn save_state(root: &Path, state: &slot_store::SlotState) {
        if let Err(e) = slot_store::write_slot_state(root, state) {
            eprintln!("slot: pico8: could not save volume and brightness: {e}");
        }
    }

    /// Paints all of fb0 black, every buffer. Leaving PICO-8 goes to black
    /// at once, instead of holding its last frame or the menu for the
    /// seconds it takes PICO-8 to quit and slot to start, and slot fades in
    /// from black. Called again once PICO-8 is gone, in case it drew while
    /// quitting.
    fn blank_screen() {
        crate::pico8_menu::blank();
    }

    /// Stops PICO-8 and waits, briefly, until it has: its last frame must be
    /// on the screen before the framebuffer is copied.
    fn stop(pid: libc::pid_t) {
        unsafe { libc::kill(pid, libc::SIGSTOP) };
        let stat = format!("/proc/{pid}/stat");
        for _ in 0..40 {
            let stopped = std::fs::read_to_string(&stat)
                .ok()
                .and_then(|s| {
                    s.rsplit(')')
                        .next()
                        .map(|r| r.trim_start().starts_with('T'))
                })
                .unwrap_or(true);
            if stopped {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
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
        /// The backlight step to wake to, slot's brightness plus one.
        backlight: u8,
        asleep: bool,
        /// The menu has PICO-8 stopped: waking leaves it stopped.
        held: bool,
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
                    platform.set_backlight(self.backlight);
                    if !self.held {
                        unsafe { libc::kill(self.pid, libc::SIGCONT) };
                    }
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
