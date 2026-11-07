//! A launcher entry and icon for the `AppImage`, which has no installer to put them in place. The
//! .deb and AUR packages bring their own, so this only runs inside an `AppImage`.

#[cfg(target_os = "linux")]
const ICON: &str = include_str!("../assets/icons/efa-e.svg");

/// Writes `~/.local/share/applications/efa.desktop` and the icon when they are missing or point
/// at an `AppImage` that moved. Cheap when nothing changed, so it runs on every start.
#[cfg(target_os = "linux")]
pub fn install() {
    use std::path::Path;

    let Some(image) = std::env::var_os("APPIMAGE") else {
        return;
    };
    let Some(data) = crate::persist::data_dir()
        .as_deref()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
    else {
        return;
    };
    let apps = data.join("applications");
    let entry = entry(&Path::new(&image).display().to_string());
    let changed = write_if_changed(&apps.join("efa.desktop"), &entry);
    write_if_changed(&data.join("icons/hicolor/scalable/apps/efa.svg"), ICON);
    if changed {
        let _ = std::process::Command::new("update-desktop-database")
            .arg(&apps)
            .output();
    }
}

/// The installers and app bundle take care of this elsewhere.
#[cfg(not(target_os = "linux"))]
pub fn install() {}

#[cfg(target_os = "linux")]
fn entry(exec: &str) -> String {
    // Exec quoting per the desktop entry spec: the path in double quotes, with ", `, $ and \
    // escaped.
    let mut quoted = String::with_capacity(exec.len() + 2);
    quoted.push('"');
    for c in exec.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    quoted.push('"');
    format!(
        "[Desktop Entry]\nType=Application\nName=efa\nGenericName=Fitting tool\n\
         Comment=EVE Online fitting tool\nExec={quoted} %f\nIcon=efa\nTerminal=false\n\
         Categories=Utility;Game;\nStartupWMClass=efa\n"
    )
}

/// Writes the file when its content differs; returns whether it did.
#[cfg(target_os = "linux")]
fn write_if_changed(path: &std::path::Path, content: &str) -> bool {
    if std::fs::read_to_string(path).is_ok_and(|current| current == content) {
        return false;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(path, content).is_ok()
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn exec_is_quoted() {
        let entry = entry("/home/a b/efa$1.AppImage");
        assert!(entry.contains("Exec=\"/home/a b/efa\\$1.AppImage\" %f\n"));
        assert!(entry.contains("Icon=efa\n"));
    }
}
