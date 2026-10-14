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
