//! Program names and icons, from desktop entries and the icon theme.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use egui::{ColorImage, TextureHandle, TextureOptions};

const SIZES: &[&str] = &["48", "64", "32", "128", "256", "24"];
const ICON_PX: u32 = 48;

#[derive(Clone, Default)]
pub struct AppInfo {
    pub name: String,
    pub icon: Option<TextureHandle>,
}

#[derive(Default)]
pub struct AppCache {
    entries: HashMap<String, AppInfo>,
    themes: Option<Vec<String>>,
}

impl AppCache {
    /// Name and icon for a window class, looked up once.
    pub fn get(&mut self, ctx: &egui::Context, class: &str, desktop_file: &str) -> AppInfo {
        let key = format!("{class}\n{desktop_file}");
        if let Some(info) = self.entries.get(&key) {
            return info.clone();
        }
        let themes = self.themes.get_or_insert_with(icon_themes).clone();
        let dirs = data_dirs();
        let entry = find_entry(&dirs, desktop_file).or_else(|| find_entry(&dirs, class));
        let name = entry
            .as_ref()
            .and_then(|text| desktop_value(text, "Name"))
            .unwrap_or_else(|| pretty_class(class));
        let icon_name = entry
            .as_ref()
            .and_then(|text| desktop_value(text, "Icon"))
            .unwrap_or_else(|| class.to_string());
        let icon = find_icon(&dirs, &themes, &icon_name)
            .or_else(|| find_icon(&dirs, &themes, &class.to_lowercase()))
            .and_then(|path| load_image(&path))
            .map(|image| ctx.load_texture(format!("app:{key}"), image, TextureOptions::LINEAR));
        let info = AppInfo { name, icon };
        self.entries.insert(key, info.clone());
        info
    }
}

/// `org.kde.dolphin` → `Dolphin`.
pub fn pretty_class(class: &str) -> String {
    let last = class.rsplit('.').next().unwrap_or(class);
    let mut chars = last.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => class.to_string(),
    }
}

fn data_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut dirs = Vec::new();
    match std::env::var_os("XDG_DATA_HOME") {
        Some(dir) => dirs.push(PathBuf::from(dir)),
        None => dirs.extend(home.as_ref().map(|h| h.join(".local/share"))),
    }
    let system = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    dirs.extend(system.split(':').map(PathBuf::from));
    dirs.extend(home.map(|h| h.join(".local/share/flatpak/exports/share")));
    dirs.push(PathBuf::from("/var/lib/flatpak/exports/share"));
    dirs.dedup();
    dirs
}

/// The configured icon theme first, then Breeze and hicolor as fallbacks.
fn icon_themes() -> Vec<String> {
    let mut themes = Vec::new();
    if let Ok(out) = std::process::Command::new("kreadconfig6")
        .args(["--file", "kdeglobals", "--group", "Icons", "--key", "Theme"])
        .output()
    {
        let theme = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !theme.is_empty() {
            themes.push(theme);
        }
    }
    for fallback in ["breeze", "hicolor"] {
        if !themes.iter().any(|t| t == fallback) {
            themes.push(fallback.to_string());
        }
    }
    themes
}

fn find_entry(dirs: &[PathBuf], name: &str) -> Option<String> {
    if name.is_empty() {
        return None;
    }
    let file = format!("{name}.desktop");
    for dir in dirs {
        if let Ok(text) = std::fs::read_to_string(dir.join("applications").join(&file)) {
            return Some(text);
        }
    }
    // Some programs report a class that differs from their desktop file;
    // `StartupWMClass` maps them.
    let lower = name.to_lowercase();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir.join("applications")) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|ext| ext != "desktop") {
                continue;
            }
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let wm_class = desktop_value(&text, "StartupWMClass")
                .unwrap_or_default()
                .to_lowercase();
            if wm_class == lower || stem == lower || stem.rsplit('.').next() == Some(lower.as_str())
            {
                return Some(text);
            }
        }
    }
    None
}

fn find_icon(dirs: &[PathBuf], themes: &[String], name: &str) -> Option<PathBuf> {
    if name.starts_with('/') {
        return Path::new(name).is_file().then(|| PathBuf::from(name));
    }
    let png = format!("{name}.png");
    let svg = format!("{name}.svg");
    for theme in themes {
        for dir in dirs {
            let base = dir.join("icons").join(theme);
            if !base.is_dir() {
                continue;
            }
            for size in SIZES {
                for candidate in [
                    base.join(format!("{size}x{size}/apps")).join(&png),
                    base.join(format!("apps/{size}")).join(&svg),
                    base.join(format!("{size}x{size}/apps")).join(&svg),
                ] {
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                }
            }
            for candidate in [
                base.join("scalable/apps").join(&svg),
                base.join("apps/scalable").join(&svg),
            ] {
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    dirs.iter()
        .flat_map(|d| [d.join("pixmaps").join(&png), d.join("pixmaps").join(&svg)])
        .find(|p| p.is_file())
}

fn load_image(path: &Path) -> Option<ColorImage> {
    if path.extension().is_some_and(|e| e == "svg") {
        load_svg(&std::fs::read(path).ok()?, ICON_PX)
    } else {
        let (w, h, rgba) = crate::tray::decode_png(&std::fs::read(path).ok()?)?;
        Some(ColorImage::from_rgba_unmultiplied(
            [w as usize, h as usize],
            &rgba,
        ))
    }
}

pub fn load_svg(data: &[u8], px: u32) -> Option<ColorImage> {
    use resvg::{tiny_skia, usvg};
    let tree = usvg::Tree::from_data(data, &usvg::Options::default()).ok()?;
    let size = tree.size();
    let scale = px as f32 / size.width().max(size.height());
    let mut pixmap = tiny_skia::Pixmap::new(px, px)?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Some(ColorImage::from_rgba_premultiplied(
        [px as usize, px as usize],
        pixmap.data(),
    ))
}

/// A key from the `[Desktop Entry]` group.
fn desktop_value(text: &str, key: &str) -> Option<String> {
    let mut in_main = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_main = line == "[Desktop Entry]";
            continue;
        }
        if in_main
            && let Some(value) = line
                .strip_prefix(key)
                .and_then(|r| r.trim_start().strip_prefix('='))
        {
            let value = value.trim();
            return (!value.is_empty()).then(|| value.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_values_come_from_the_main_group() {
        let text =
            "[Desktop Entry]\nName=Foo\nName[de]=Fu\nIcon = foo\n[Desktop Action new]\nIcon=bar\n";
        assert_eq!(desktop_value(text, "Icon").as_deref(), Some("foo"));
        assert_eq!(desktop_value(text, "Name").as_deref(), Some("Foo"));
        assert_eq!(desktop_value(text, "Exec"), None);
    }

    #[test]
    fn classes_become_readable_names() {
        assert_eq!(pretty_class("org.kde.dolphin"), "Dolphin");
        assert_eq!(pretty_class("firefox"), "Firefox");
    }
}
