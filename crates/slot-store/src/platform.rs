use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Platform {
    #[default]
    Gba,
    Gb,
    Gbc,
    Gbav,
    #[cfg(feature = "nds")]
    Nds,
    Pico8,
}

impl Platform {
    #[cfg(not(feature = "nds"))]
    pub const ALL: [Platform; 5] = [
        Platform::Gba,
        Platform::Gbav,
        Platform::Gbc,
        Platform::Gb,
        Platform::Pico8,
    ];
    #[cfg(feature = "nds")]
    pub const ALL: [Platform; 6] = [
        Platform::Nds,
        Platform::Gba,
        Platform::Gbav,
        Platform::Gbc,
        Platform::Gb,
        Platform::Pico8,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Platform::Gba => "Game Boy Advance",
            Platform::Gb => "Game Boy",
            Platform::Gbc => "Game Boy Color",
            Platform::Gbav => "Game Boy Advance Video",
            #[cfg(feature = "nds")]
            Platform::Nds => "Nintendo DS",
            Platform::Pico8 => "PICO-8",
        }
    }

    pub fn dir_name(self) -> &'static str {
        match self {
            Platform::Gba => "GBA",
            Platform::Gb => "GB",
            Platform::Gbc => "GBC",
            Platform::Gbav => "GBAV",
            #[cfg(feature = "nds")]
            Platform::Nds => "NDS",
            Platform::Pico8 => "PICO8",
        }
    }

    pub fn hardware(self) -> Platform {
        match self {
            Platform::Gbav => Platform::Gba,
            p => p,
        }
    }

    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Platform::Gba | Platform::Gbav => &["gba"],
            Platform::Gb | Platform::Gbc => &["gb", "gbc"],
            #[cfg(feature = "nds")]
            Platform::Nds => &["nds"],
            Platform::Pico8 => &["p8", "p8.png"],
        }
    }

    pub fn picture(self) -> (u32, u32) {
        match self {
            Platform::Gba | Platform::Gbav => (240, 160),
            Platform::Gb | Platform::Gbc => (160, 144),
            #[cfg(feature = "nds")]
            Platform::Nds => (256, 192),
            Platform::Pico8 => (128, 128),
        }
    }

    pub fn accepts(self, path: &Path) -> bool {
        self.stem_of(path).is_some()
    }

    /// The name a game file is listed under: its name without the
    /// extension, where PICO-8's `.p8.png` counts as one extension.
    pub fn stem_of(self, path: &Path) -> Option<&str> {
        let name = path.file_name()?.to_str()?;
        self.extensions().iter().find_map(|ext| {
            let cut = name.len().checked_sub(ext.len() + 1)?;
            let (stem, tail) = (name.get(..cut)?, name.get(cut..)?);
            (!stem.is_empty() && tail.starts_with('.') && tail[1..].eq_ignore_ascii_case(ext))
                .then_some(stem)
        })
    }
}
