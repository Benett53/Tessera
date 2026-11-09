//! Start the service at login, through `~/.config/autostart/tessera.desktop`.
//!
//! KDE's "Autostart" settings page lists and toggles the same file. Switching
//! off writes `Hidden=true` rather than deleting it, as that page does.

use std::path::PathBuf;

const FILE_NAME: &str = "tessera.desktop";

fn entry_path() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("autostart")
        .join(FILE_NAME)
}

fn current_exe() -> Result<String, String> {
    std::env::current_exe()
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(|error| error.to_string())
}

/// Quote an `Exec` argument as the desktop entry specification wants it.
fn quote(argument: &str) -> String {
    if argument
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-+".contains(c))
    {
        argument.to_string()
    } else {
        let escaped = argument
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('`', "\\`")
            .replace('$', "\\$");
        format!("\"{escaped}\"")
    }
}

fn entry(executable: &str, enabled: bool) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Tessera\n\
         Comment=Window zones and remembered window positions\n\
         Exec={} --daemon\n\
         Icon=tessera\n\
         Terminal=false\n\
         X-KDE-autostart-phase=1\n\
         X-GNOME-Autostart-enabled={enabled}\n\
         Hidden={}\n",
        quote(executable),
        !enabled
    )
}

fn read_entry() -> Option<String> {
    std::fs::read_to_string(entry_path()).ok()
}

fn value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines()
        .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
        .map(str::trim)
}

/// The program an `Exec` line starts, without arguments or quotes.
fn exec_target(exec: &str) -> String {
    let exec = exec.trim();
    if let Some(rest) = exec.strip_prefix('"') {
        rest.split('"')
            .next()
            .unwrap_or_default()
            .replace("\\\\", "\\")
    } else {
        exec.split_whitespace()
            .next()
            .unwrap_or_default()
            .to_string()
    }
}

pub fn set(enabled: bool) -> Result<(), String> {
    let path = entry_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::write(&path, entry(&current_exe()?, enabled)).map_err(|error| error.to_string())
}

/// Repoint an entry whose program no longer exists at this copy. Only a
/// missing target is repaired, so a test build never takes over the entry of
/// the installed copy.
pub fn repair() -> bool {
    let Some(text) = read_entry() else {
        return false;
    };
    let target = value(&text, "Exec").map(exec_target).unwrap_or_default();
    if !target.is_empty() && std::path::Path::new(&target).exists() {
        return false;
    }
    set(is_enabled()).is_ok()
}

pub fn is_enabled() -> bool {
    read_entry().is_some_and(|text| {
        value(&text, "Hidden") != Some("true")
            && value(&text, "X-GNOME-Autostart-enabled") != Some("false")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_round_trips_through_quoting() {
        for path in ["/home/a/.local/bin/tessera", "/opt/My Apps/tessera"] {
            let text = entry(path, true);
            assert_eq!(exec_target(value(&text, "Exec").unwrap()), path);
            assert!(value(&text, "Exec").unwrap().ends_with(" --daemon"));
        }
    }

    #[test]
    fn disabled_entry_is_hidden() {
        let text = entry("/usr/bin/tessera", false);
        assert_eq!(value(&text, "Hidden"), Some("true"));
    }
}
