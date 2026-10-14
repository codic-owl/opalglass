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

pub fn missing_link_underline() -> gpui::UnderlineStyle {
    gpui::UnderlineStyle {
        thickness: gpui::px(1.),
        color: Some(gpui::rgb(MISSING_LINK).into()),
        wavy: true,
    }
}
