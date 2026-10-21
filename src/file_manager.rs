//! Shows a Game's file or folder in the file manager (US-16), through the freedesktop
//! `org.freedesktop.FileManager1.ShowItems` D-Bus method, which opens its folder with it selected.
//! Without a file manager on D-Bus, opens the containing folder with `xdg-open`.

use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::process::{Command, Stdio};

/// `path` as a `file://` URI, every byte but unreserved ones and `/` percent-encoded.
pub fn file_uri(path: &Path) -> String {
    let mut uri = String::from("file://");
    for &b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            uri.push(b as char);
        } else {
            uri.push_str(&format!("%{b:02X}"));
        }
    }
    uri
}

/// Runs a command to the end without its output; whether it succeeded.
fn run(cmd: &mut Command) -> bool {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Shows `path` in the file manager on a thread of its own, so the UI never waits, and waits
/// for each child there, so none is left a zombie.
pub fn show(path: &Path) {
    let path = path.to_owned();
    std::thread::spawn(move || {
        let items = format!("['{}']", file_uri(&path));
        let shown = run(Command::new("gdbus").args([
            "call",
            "--session",
            "--dest",
            "org.freedesktop.FileManager1",
            "--object-path",
            "/org/freedesktop/FileManager1",
            "--method",
            "org.freedesktop.FileManager1.ShowItems",
            &items,
            "",
        ]));
        let folder = path.parent().unwrap_or(&path);
        if !shown && !run(Command::new("xdg-open").arg(folder)) {
            eprintln!("game37: can't show {} in the file manager", path.display());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_becomes_a_percent_encoded_file_uri() {
        assert_eq!(
            file_uri(Path::new("/home/m/Games/snes/Super Mario World (USA).sfc")),
            "file:///home/m/Games/snes/Super%20Mario%20World%20%28USA%29.sfc"
        );
        assert_eq!(
            file_uri(Path::new("/g/Link's ['x'] é")),
            "file:///g/Link%27s%20%5B%27x%27%5D%20%C3%A9",
            "quotes can't end the D-Bus string list early"
        );
    }
}
