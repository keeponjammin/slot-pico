use std::collections::HashMap;
use std::path::Path;

use crate::Platform;

pub const SELECTED_CORE_FILE: &str = "Config/selected_core.txt";

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Core {
    #[default]
    Mgba,
    Gpsp,
    Gambatte,
    /// fake-08, the libretro PICO-8 core that plays PICO-8 carts inside slot.
    Fake08,
    /// The owner's own PICO-8, which slot hands the device to instead of loading a core.
    Pico8,
    #[cfg(feature = "nds")]
    Dsperate,
}

impl Core {
    #[cfg(not(feature = "nds"))]
    pub const ALL: [Core; 5] = [
        Core::Mgba,
        Core::Gpsp,
        Core::Gambatte,
        Core::Fake08,
        Core::Pico8,
    ];
    #[cfg(feature = "nds")]
    pub const ALL: [Core; 6] = [
        Core::Mgba,
        Core::Gpsp,
        Core::Gambatte,
        Core::Fake08,
        Core::Pico8,
        Core::Dsperate,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Core::Mgba => "mgba",
            Core::Gpsp => "gpsp",
            Core::Gambatte => "gambatte",
            Core::Fake08 => "fake08",
            Core::Pico8 => "pico8",
            #[cfg(feature = "nds")]
            Core::Dsperate => "dsperate",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn text(self) -> &'static str {
        match self {
            Core::Mgba => "mGBA",
            Core::Gpsp => "gpSP",
            Core::Gambatte => "Gambatte",
            Core::Fake08 => "fake-08",
            Core::Pico8 => "PICO-8",
            #[cfg(feature = "nds")]
            Core::Dsperate => "DSperate",
        }
    }

    pub fn parse(s: &str) -> Option<Core> {
        match s.trim().to_ascii_lowercase().as_str() {
            "mgba" => Some(Core::Mgba),
            "gpsp" => Some(Core::Gpsp),
            "gambatte" => Some(Core::Gambatte),
            "fake08" => Some(Core::Fake08),
            "pico8" => Some(Core::Pico8),
            #[cfg(feature = "nds")]
            "dsperate" => Some(Core::Dsperate),
            _ => None,
        }
    }

    pub fn runs(self, platform: Platform) -> bool {
        match self {
            #[cfg(feature = "nds")]
            _ if platform == Platform::Nds => self == Core::Dsperate,
            Core::Mgba => platform != Platform::Pico8,
            Core::Gpsp => platform.hardware() == Platform::Gba,
            Core::Gambatte => matches!(platform, Platform::Gb | Platform::Gbc),
            Core::Fake08 | Core::Pico8 => platform == Platform::Pico8,
            #[cfg(feature = "nds")]
            Core::Dsperate => false,
        }
    }

    /// Whether slot hands the device to another program to play a cart, instead of loading
    /// this core itself.
    pub fn runs_outside(self) -> bool {
        self == Core::Pico8
    }

    pub fn pair(platform: Platform) -> [Core; 2] {
        match platform {
            Platform::Gba | Platform::Gbav => [Core::Mgba, Core::Gpsp],
            Platform::Gb | Platform::Gbc => [Core::Mgba, Core::Gambatte],
            #[cfg(feature = "nds")]
            Platform::Nds => [Core::Dsperate, Core::Dsperate],
            Platform::Pico8 => [Core::Fake08, Core::Pico8],
        }
    }
}

pub fn read_selected_cores(root: &Path) -> HashMap<String, Core> {
    crate::ini::read(root, SELECTED_CORE_FILE)
        .into_iter()
        .filter_map(|(stem, name)| Core::parse(&name).map(|core| (stem, core)))
        .collect()
}

pub fn core_for_platform(root: &Path, stem: &str, platform: Platform, gb_core: Core) -> Core {
    let mut lines = crate::ini::read(root, SELECTED_CORE_FILE);
    lines
        .remove(&core_key(platform, stem))
        .or_else(|| lines.remove(stem))
        .as_deref()
        .and_then(Core::parse)
        .filter(|core| core.runs(platform))
        .unwrap_or_else(|| match platform {
            Platform::Gb | Platform::Gbc if gb_core.runs(platform) => gb_core,
            #[cfg(feature = "nds")]
            Platform::Nds => Core::Dsperate,
            Platform::Pico8 => Core::Fake08,
            _ => Core::Mgba,
        })
}

pub fn write_selected_core(
    root: &Path,
    platform: Platform,
    stem: &str,
    core: Core,
) -> std::io::Result<()> {
    let path = root.join(SELECTED_CORE_FILE);
    let existing = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    let bare = crate::ini::set(&existing, stem, None)?;
    let out = crate::ini::set(&bare, &core_key(platform, stem), Some(core.as_str()))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    crate::atomic::atomic_write(&path, out.as_bytes())
}

fn core_key(platform: Platform, stem: &str) -> String {
    format!("{}/{stem}", platform.dir_name())
}
