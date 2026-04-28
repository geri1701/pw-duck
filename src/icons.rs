use std::path::PathBuf;

use ksni::Icon;

#[cfg(feature = "gui")]
pub const APP_ICON_NAME: &str = "pw-duck";

pub fn tray_icon_pixmap() -> Vec<Icon> {
    vec![
        icon_from_argb(16, include_bytes!("../assets/icons/pixmap/pw-duck-16.argb")),
        icon_from_argb(22, include_bytes!("../assets/icons/pixmap/pw-duck-22.argb")),
        icon_from_argb(24, include_bytes!("../assets/icons/pixmap/pw-duck-24.argb")),
        icon_from_argb(32, include_bytes!("../assets/icons/pixmap/pw-duck-32.argb")),
        icon_from_argb(48, include_bytes!("../assets/icons/pixmap/pw-duck-48.argb")),
        icon_from_argb(64, include_bytes!("../assets/icons/pixmap/pw-duck-64.argb")),
    ]
}

fn icon_from_argb(size: i32, data: &'static [u8]) -> Icon {
    debug_assert_eq!(data.len(), (size * size * 4) as usize);
    Icon {
        width: size,
        height: size,
        data: data.to_vec(),
    }
}

pub fn icon_theme_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("PW_DUCK_ICON_THEME_PATH").map(PathBuf::from) {
        if path.exists() {
            return Some(path);
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(prefix) = exe.parent().and_then(|bin| bin.parent()) {
            let installed = prefix.join("share/icons");
            if installed.exists() {
                return Some(installed);
            }
        }
    }

    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/icons");
    if source.exists() {
        return Some(source);
    }

    None
}
