pub const UI_FONT: &str = if cfg!(target_os = "windows") {
    "Segoe UI"
} else if cfg!(target_os = "macos") {
    ".SystemUIFont"
} else {
    "DejaVu Sans"
};

pub const MONO_FONT: &str = if cfg!(target_os = "windows") {
    "Consolas"
} else if cfg!(target_os = "macos") {
    "Menlo"
} else {
    "DejaVu Sans Mono"
};

pub const MISSING_LINK: u32 = 0xf09a91;

pub fn highlight_color(color: opalglass::markdown::HighlightColor) -> u32 {
    use opalglass::markdown::HighlightColor::*;
    match color {
        Default | Yellow => 0x665326,
        Red => 0x70313d,
        Orange => 0x704524,
        Green => 0x285a44,
        Blue => 0x2d4b73,
        Purple => 0x594175,
    }
}

pub fn missing_link_underline() -> gpui::UnderlineStyle {
    gpui::UnderlineStyle {
        thickness: gpui::px(1.),
        color: Some(gpui::rgb(MISSING_LINK).into()),
        wavy: true,
    }
}

#[path = "theme/typography.rs"]
pub(crate) mod typography;
