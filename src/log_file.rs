//! The log file on Windows. The release `game37.exe` has no console (`windows_subsystem` in `main.rs`),
//! so what Game37 writes to stderr goes to `logs\game37.log` in its data folder instead, with the
//! previous run's kept as `game37.old.log`. The tester sends this file.

use std::ffi::c_void;
use std::fs::{self, File};
use std::io;
use std::os::windows::io::IntoRawHandle;
use std::path::Path;

const STD_ERROR_HANDLE: u32 = -12i32 as u32;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn SetStdHandle(std_handle: u32, handle: *mut c_void) -> i32;
}

unsafe extern "C" {
    fn _open_osfhandle(handle: isize, flags: i32) -> i32;
    fn _dup2(from: i32, to: i32) -> i32;
}

/// Points this process's stderr at a new `game37.log` in `logs_dir`: Rust's stderr and SDL's log through
/// the Windows stderr handle, the C runtime's (where Cores log) through its file descriptor 2.
pub fn redirect_stderr(logs_dir: &Path) -> io::Result<()> {
    let handle = open(logs_dir)?.into_raw_handle();
    // SAFETY: `handle` is an open file handle that is never closed, so it stays valid for the whole
    // process; the C runtime's descriptor made from it is never closed either.
    unsafe {
        if SetStdHandle(STD_ERROR_HANDLE, handle) == 0 {
            return Err(io::Error::last_os_error());
        }
        let fd = _open_osfhandle(handle as isize, 0);
        if fd < 0 || _dup2(fd, 2) != 0 {
            return Err(io::Error::other("can't point the C runtime's stderr at the log"));
        }
    }
    Ok(())
}

/// Creates `logs_dir` if needed, renames its `game37.log` to `game37.old.log` (replacing the one
/// before) and returns a new, empty `game37.log`.
fn open(logs_dir: &Path) -> io::Result<File> {
    fs::create_dir_all(logs_dir)?;
    let log = logs_dir.join("game37.log");
    match fs::rename(&log, logs_dir.join("game37.old.log")) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    File::create(log)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    #[test]
    fn keeps_the_previous_run_as_the_old_log() {
        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path().join("game37").join("logs");
        let read = |name: &str| fs::read_to_string(logs.join(name)).unwrap();

        write!(open(&logs).unwrap(), "first run").unwrap();
        assert_eq!(read("game37.log"), "first run");
        assert!(!logs.join("game37.old.log").exists());

        write!(open(&logs).unwrap(), "second run").unwrap();
        assert_eq!(read("game37.log"), "second run");
        assert_eq!(read("game37.old.log"), "first run");

        open(&logs).unwrap();
        assert_eq!(read("game37.log"), "");
        assert_eq!(read("game37.old.log"), "second run");
    }
}
