//! libretro Cores: which one plays each Embedded System, and downloading one the first time its
//! System is played (FR-EMB-2, US-23).
//!
//! Cores are never committed (ADR 0004); they are fetched from the libretro build server into
//! `~/.local/share/game37/cores/` on first use and kept there for later launches.

use std::fmt;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;

use crate::systems::System;

/// A libretro Core: its display name and the `.so` file the build server publishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreInfo {
    pub name: &'static str,
    pub file: &'static str,
}

pub static MESEN: CoreInfo = CoreInfo {
    name: "Mesen",
    file: "mesen_libretro.so",
};
pub static BSNES: CoreInfo = CoreInfo {
    name: "bsnes",
    file: "bsnes_libretro.so",
};
pub static SAMEBOY: CoreInfo = CoreInfo {
    name: "SameBoy",
    file: "sameboy_libretro.so",
};
pub static MGBA: CoreInfo = CoreInfo {
    name: "mGBA",
    file: "mgba_libretro.so",
};

/// The Core for a System, or `None` for a System without one yet (N64, DS, PS1, PSP) or an
/// External System, which never uses a Core.
pub fn for_system(system: &System) -> Option<&'static CoreInfo> {
    match system.id {
        "nes" => Some(&MESEN),
        "snes" => Some(&BSNES),
        "gb" | "gbc" => Some(&SAMEBOY),
        "gba" => Some(&MGBA),
        _ => None,
    }
}

/// Where a Core is, or would be, stored: `<data_dir>/cores/<file>`.
pub fn installed_path(data_dir: &Path, core: &CoreInfo) -> PathBuf {
    data_dir.join("cores").join(core.file)
}

pub fn is_installed(data_dir: &Path, core: &CoreInfo) -> bool {
    installed_path(data_dir, core).is_file()
}

/// The libretro build server's URL for a Core's nightly zip.
pub fn url(core: &CoreInfo) -> String {
    format!(
        "https://buildbot.libretro.com/nightly/linux/x86_64/latest/{}.zip",
        core.file
    )
}

/// How far a [`Download`] has got.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadState {
    Running { received: u64, total: Option<u64> },
    Done(PathBuf),
    Failed(String),
}

/// Why fetching the Core's zip failed, before any bytes of its body were read.
enum FetchError {
    /// Couldn't connect at all: DNS, refused, timed out.
    Connect,
    /// Connected, but the server didn't answer 200.
    Status(u16),
}

type FetchResult = Result<(Box<dyn Read + Send>, Option<u64>), FetchError>;

/// Performs the HTTP GET for a Core's zip. Injectable so tests run offline; production uses
/// [`ureq_fetch`].
type Fetcher = Arc<dyn Fn(&str) -> FetchResult + Send + Sync>;

/// A Core download running on a background thread. `poll` never blocks; call it every frame.
/// Dropping a running `Download` stops it promptly and leaves no partial files behind.
pub struct Download {
    state: Arc<Mutex<DownloadState>>,
    cancel: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Download {
    /// Starts downloading `core` into `data_dir` on a background thread.
    pub fn start(core: &'static CoreInfo, data_dir: PathBuf) -> Download {
        Download::spawn(core, data_dir, Arc::new(ureq_fetch))
    }

    fn spawn(core: &'static CoreInfo, data_dir: PathBuf, fetch: Fetcher) -> Download {
        let state = Arc::new(Mutex::new(DownloadState::Running {
            received: 0,
            total: None,
        }));
        let cancel = Arc::new(AtomicBool::new(false));
        let state_in_thread = Arc::clone(&state);
        let cancel_in_thread = Arc::clone(&cancel);
        let handle = thread::spawn(move || {
            let result = run(
                core,
                &data_dir,
                fetch.as_ref(),
                &state_in_thread,
                &cancel_in_thread,
            );
            let final_state = match result {
                Ok(path) => DownloadState::Done(path),
                Err(message) => DownloadState::Failed(message),
            };
            *state_in_thread.lock().unwrap() = final_state;
        });
        Download {
            state,
            cancel,
            handle: Some(handle),
        }
    }

    /// Never blocks: reports the last known state.
    pub fn poll(&mut self) -> DownloadState {
        self.state.lock().unwrap().clone()
    }
}

impl Drop for Download {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// A cap on a Core's zip size (FR-EMB-2 is for small libretro Cores, not arbitrary downloads).
const MAX_DOWNLOAD_BYTES: u64 = 100 * 1024 * 1024;

/// The ELF magic a valid Linux shared library starts with.
const ELF_MAGIC: &[u8] = b"\x7fELF";

fn run(
    core: &CoreInfo,
    data_dir: &Path,
    fetch: &(dyn Fn(&str) -> FetchResult + Send + Sync),
    state: &Arc<Mutex<DownloadState>>,
    cancel: &Arc<AtomicBool>,
) -> Result<PathBuf, String> {
    let cores_dir = data_dir.join("cores");
    let part_path = cores_dir.join(format!(".{}.zip.part", core.file));
    let tmp_path = cores_dir.join(format!(".{}.tmp", core.file));

    let result = download_and_install(core, &cores_dir, &part_path, &tmp_path, fetch, state, cancel);
    // Every path, success or failure, leaves no temp files behind.
    let _ = fs::remove_file(&part_path);
    let _ = fs::remove_file(&tmp_path);
    result
}

fn download_and_install(
    core: &CoreInfo,
    cores_dir: &Path,
    part_path: &Path,
    tmp_path: &Path,
    fetch: &(dyn Fn(&str) -> FetchResult + Send + Sync),
    state: &Arc<Mutex<DownloadState>>,
    cancel: &Arc<AtomicBool>,
) -> Result<PathBuf, String> {
    fs::create_dir_all(cores_dir).map_err(|_| incomplete(core))?;

    let (mut reader, total) = fetch(&url(core)).map_err(|e| match e {
        FetchError::Connect => connect_failed(core),
        FetchError::Status(status) => bad_status(core, status),
    })?;

    // Step 2: stream to the `.part` file, reporting progress as we go.
    let mut file = fs::File::create(part_path).map_err(|_| incomplete(core))?;
    let mut buf = [0u8; 64 * 1024];
    let mut received: u64 = 0;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".to_string());
        }
        let n = reader.read(&mut buf).map_err(|_| incomplete(core))?;
        if n == 0 {
            break;
        }
        received += n as u64;
        if received > MAX_DOWNLOAD_BYTES {
            return Err(incomplete(core));
        }
        file.write_all(&buf[..n]).map_err(|_| incomplete(core))?;
        *state.lock().unwrap() = DownloadState::Running { received, total };
    }
    drop(file);
    if let Some(total) = total
        && received < total
    {
        return Err(incomplete(core));
    }

    // Step 3: open the zip and find the one entry we trust.
    let zip_file = fs::File::open(part_path).map_err(|_| incomplete(core))?;
    let mut archive = zip::ZipArchive::new(zip_file).map_err(|_| incomplete(core))?;
    let mut entry = archive.by_name(core.file).map_err(|_| invalid_library(core))?;
    if !matches!(entry.name(), Ok(name) if name == core.file) {
        return Err(invalid_library(core));
    }
    let mut content = Vec::new();
    entry.read_to_end(&mut content).map_err(|_| incomplete(core))?;
    drop(entry);
    drop(archive);

    // Step 4: extract, verify, make executable, and install.
    if !content.starts_with(ELF_MAGIC) {
        return Err(invalid_library(core));
    }
    fs::write(tmp_path, &content).map_err(|_| incomplete(core))?;
    fs::set_permissions(tmp_path, fs::Permissions::from_mode(0o755)).map_err(|_| incomplete(core))?;
    let final_path = cores_dir.join(core.file);
    fs::rename(tmp_path, &final_path).map_err(|_| incomplete(core))?;
    Ok(final_path)
}

fn connect_failed(core: &CoreInfo) -> String {
    format!(
        "Couldn't download the {} Core. Check your internet connection.",
        core.name
    )
}

fn bad_status(core: &CoreInfo, status: u16) -> String {
    format!(
        "Couldn't download the {} Core. Check your internet connection. (The server returned HTTP {status}.)",
        core.name
    )
}

fn incomplete(core: &CoreInfo) -> String {
    format!("The {} Core download was incomplete.", core.name)
}

fn invalid_library(core: &CoreInfo) -> String {
    format!("The downloaded {} Core isn't a valid Linux library.", core.name)
}

/// The production fetcher: `ureq` over rustls, with a 30 s connect/read timeout.
fn ureq_fetch(url: &str) -> FetchResult {
    let config = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .timeout_recv_body(Some(Duration::from_secs(30)))
        .http_status_as_error(false)
        .build();
    let agent: ureq::Agent = config.into();
    let response = agent.get(url).call().map_err(|_| FetchError::Connect)?;
    let status = response.status().as_u16();
    if status != 200 {
        return Err(FetchError::Status(status));
    }
    let total = response.body().content_length();
    let reader = response.into_body().into_reader();
    Ok((Box::new(reader), total))
}

impl fmt::Debug for Download {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Download").finish_non_exhaustive()
    }
}

/// Offline [`Download`]s for other modules' tests (the download screen's, in particular): never
/// touches the network.
#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use std::io::Cursor;

    /// A `Download` that completes with `bytes` as the fetched zip's body.
    pub fn spawn(core: &'static CoreInfo, data_dir: PathBuf, bytes: Vec<u8>, total: Option<u64>) -> Download {
        let fetch: Fetcher = Arc::new(move |_url: &str| {
            Ok((
                Box::new(Cursor::new(bytes.clone())) as Box<dyn Read + Send>,
                total,
            ))
        });
        Download::spawn(core, data_dir, fetch)
    }

    /// A `Download` that fails to connect as soon as it is polled.
    pub fn spawn_failing(core: &'static CoreInfo, data_dir: PathBuf) -> Download {
        Download::spawn(core, data_dir, Arc::new(|_url: &str| Err(FetchError::Connect)))
    }

    /// A valid zip for `core`: its file, holding a tiny fake ELF library.
    pub fn core_zip(core: &CoreInfo) -> Vec<u8> {
        let mut content = ELF_MAGIC.to_vec();
        content.extend_from_slice(b"fake core");
        let mut buf = Vec::new();
        let mut writer = zip::ZipWriter::new(Cursor::new(&mut buf));
        let options =
            zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        writer.start_file(core.file, options).unwrap();
        writer.write_all(&content).unwrap();
        writer.finish().unwrap();
        buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::{SYSTEMS, by_id};
    use std::io;
    use std::io::Cursor;
    use std::sync::mpsc;

    fn zip_with(entry_name: &str, content: &[u8]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(Cursor::new(&mut buf));
            let options =
                zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            writer.start_file(entry_name, options).unwrap();
            writer.write_all(content).unwrap();
            writer.finish().unwrap();
        }
        buf
    }

    fn core_zip() -> Vec<u8> {
        let mut content = ELF_MAGIC.to_vec();
        content.extend_from_slice(b"rest of a fake core");
        zip_with(SAMEBOY.file, &content)
    }

    fn fixed_reader_fetcher(bytes: Vec<u8>) -> Fetcher {
        let total = bytes.len() as u64;
        Arc::new(move |_url: &str| {
            Ok((
                Box::new(Cursor::new(bytes.clone())) as Box<dyn Read + Send>,
                Some(total),
            ))
        })
    }

    fn truncated_total_fetcher(bytes: Vec<u8>, claimed_total: u64) -> Fetcher {
        Arc::new(move |_url: &str| {
            Ok((
                Box::new(Cursor::new(bytes.clone())) as Box<dyn Read + Send>,
                Some(claimed_total),
            ))
        })
    }

    fn failing_fetcher() -> Fetcher {
        Arc::new(|_url: &str| Err(FetchError::Connect))
    }

    fn wait_for<F: FnMut() -> Option<T>, T>(mut f: F) -> T {
        for _ in 0..2000 {
            if let Some(value) = f() {
                return value;
            }
            thread::sleep(Duration::from_millis(5));
        }
        panic!("timed out waiting for condition");
    }

    #[test]
    fn table_has_a_core_for_each_embedded_system_of_this_milestone() {
        assert_eq!(for_system(by_id("nes").unwrap()), Some(&MESEN));
        assert_eq!(for_system(by_id("snes").unwrap()), Some(&BSNES));
        assert_eq!(for_system(by_id("gb").unwrap()), Some(&SAMEBOY));
        assert_eq!(for_system(by_id("gbc").unwrap()), Some(&SAMEBOY));
        assert_eq!(for_system(by_id("gba").unwrap()), Some(&MGBA));
        for id in ["n64", "nds", "ps1", "psp"] {
            assert_eq!(for_system(by_id(id).unwrap()), None, "{id}");
        }
        for system in SYSTEMS
            .iter()
            .filter(|s| s.kind == crate::systems::Kind::External)
        {
            assert_eq!(for_system(system), None, "{}", system.id);
        }
    }

    #[test]
    fn url_points_at_the_nightly_zip() {
        assert_eq!(
            url(&MESEN),
            "https://buildbot.libretro.com/nightly/linux/x86_64/latest/mesen_libretro.so.zip"
        );
    }

    #[test]
    fn successful_install_is_executable_with_no_temp_files_left() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();
        let mut download = Download::spawn(&SAMEBOY, data_dir.clone(), fixed_reader_fetcher(core_zip()));

        let path = wait_for(|| match download.poll() {
            DownloadState::Done(path) => Some(path),
            DownloadState::Failed(e) => panic!("download failed: {e}"),
            DownloadState::Running { .. } => None,
        });

        assert_eq!(path, installed_path(&data_dir, &SAMEBOY));
        assert!(is_installed(&data_dir, &SAMEBOY));
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);

        let cores_dir = data_dir.join("cores");
        assert!(!cores_dir.join(format!(".{}.zip.part", SAMEBOY.file)).exists());
        assert!(!cores_dir.join(format!(".{}.tmp", SAMEBOY.file)).exists());
    }

    #[test]
    fn progress_is_reported_with_total_from_the_length() {
        let dir = tempfile::tempdir().unwrap();
        let zip = core_zip();
        let total = zip.len() as u64;
        let half = zip.len() / 2;
        let (first, second) = (zip[..half].to_vec(), zip[half..].to_vec());

        struct StepReader(mpsc::Receiver<Vec<u8>>);
        impl Read for StepReader {
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                match self.0.recv() {
                    Ok(chunk) => {
                        let n = chunk.len().min(buf.len());
                        buf[..n].copy_from_slice(&chunk[..n]);
                        Ok(n)
                    }
                    Err(_) => Ok(0),
                }
            }
        }

        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        // `Fetcher` is `Fn`, not `FnOnce`, but `Download::spawn` only ever calls it once, so a
        // `Mutex` just to make the one-shot `rx` reusable across calls is enough here.
        let rx = Mutex::new(Some(rx));
        let fetch: Fetcher = Arc::new(move |_url: &str| {
            let rx = rx.lock().unwrap().take().expect("fetcher called more than once");
            Ok((Box::new(StepReader(rx)) as Box<dyn Read + Send>, Some(total)))
        });

        let mut download = Download::spawn(&SAMEBOY, dir.path().to_owned(), fetch);
        tx.send(first.clone()).unwrap();

        let received = wait_for(|| match download.poll() {
            DownloadState::Running { received, total: t } if received > 0 => {
                assert_eq!(t, Some(total));
                Some(received)
            }
            DownloadState::Failed(e) => panic!("download failed: {e}"),
            _ => None,
        });
        assert_eq!(received, first.len() as u64);

        tx.send(second).unwrap();
        drop(tx);
        wait_for(|| match download.poll() {
            DownloadState::Done(_) => Some(()),
            DownloadState::Failed(e) => panic!("download failed: {e}"),
            _ => None,
        });
    }

    #[test]
    fn missing_entry_is_the_invalid_library_error() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();
        let zip = zip_with("other_libretro.so", ELF_MAGIC);
        let mut download = Download::spawn(&SAMEBOY, data_dir.clone(), fixed_reader_fetcher(zip));

        let message = wait_for(|| match download.poll() {
            DownloadState::Failed(m) => Some(m),
            DownloadState::Done(_) => panic!("expected a failure"),
            _ => None,
        });
        assert_eq!(message, invalid_library(&SAMEBOY));
        assert!(!is_installed(&data_dir, &SAMEBOY));
    }

    #[test]
    fn a_path_traversal_entry_is_the_invalid_library_error() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();
        let zip = zip_with(&format!("../{}", SAMEBOY.file), ELF_MAGIC);
        let mut download = Download::spawn(&SAMEBOY, data_dir.clone(), fixed_reader_fetcher(zip));

        let message = wait_for(|| match download.poll() {
            DownloadState::Failed(m) => Some(m),
            DownloadState::Done(_) => panic!("expected a failure"),
            _ => None,
        });
        assert_eq!(message, invalid_library(&SAMEBOY));
        assert!(!is_installed(&data_dir, &SAMEBOY));
    }

    #[test]
    fn a_non_elf_entry_is_the_invalid_library_error() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();
        let zip = zip_with(SAMEBOY.file, b"not an elf file");
        let mut download = Download::spawn(&SAMEBOY, data_dir.clone(), fixed_reader_fetcher(zip));

        let message = wait_for(|| match download.poll() {
            DownloadState::Failed(m) => Some(m),
            DownloadState::Done(_) => panic!("expected a failure"),
            _ => None,
        });
        assert_eq!(message, invalid_library(&SAMEBOY));
        assert!(!is_installed(&data_dir, &SAMEBOY));
    }

    #[test]
    fn a_truncated_stream_is_the_incomplete_error() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();
        let zip = core_zip();
        let claimed_total = zip.len() as u64 + 1000;
        let mut download = Download::spawn(
            &SAMEBOY,
            data_dir.clone(),
            truncated_total_fetcher(zip, claimed_total),
        );

        let message = wait_for(|| match download.poll() {
            DownloadState::Failed(m) => Some(m),
            DownloadState::Done(_) => panic!("expected a failure"),
            _ => None,
        });
        assert_eq!(message, incomplete(&SAMEBOY));
        assert!(!is_installed(&data_dir, &SAMEBOY));
    }

    #[test]
    fn a_fetch_error_is_the_connection_message() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();
        let mut download = Download::spawn(&SAMEBOY, data_dir.clone(), failing_fetcher());

        let message = wait_for(|| match download.poll() {
            DownloadState::Failed(m) => Some(m),
            DownloadState::Done(_) => panic!("expected a failure"),
            _ => None,
        });
        assert_eq!(message, connect_failed(&SAMEBOY));
        assert!(!is_installed(&data_dir, &SAMEBOY));
    }

    #[test]
    fn dropping_mid_download_leaves_no_partial_files_and_stops_the_thread() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();

        struct SlowReader {
            remaining: usize,
        }
        impl Read for SlowReader {
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                if self.remaining == 0 {
                    return Ok(0);
                }
                thread::sleep(Duration::from_millis(2));
                let n = buf.len().min(self.remaining).min(1024);
                for b in &mut buf[..n] {
                    *b = 0;
                }
                self.remaining -= n;
                Ok(n)
            }
        }
        let fetch: Fetcher = Arc::new(|_url: &str| {
            Ok((
                Box::new(SlowReader {
                    remaining: 10 * 1024 * 1024,
                }) as Box<dyn Read + Send>,
                None,
            ))
        });

        let mut download = Download::spawn(&SAMEBOY, data_dir.clone(), fetch);
        wait_for(|| match download.poll() {
            DownloadState::Running { received, .. } if received > 0 => Some(()),
            _ => None,
        });

        drop(download);

        let cores_dir = data_dir.join("cores");
        assert!(!cores_dir.join(format!(".{}.zip.part", SAMEBOY.file)).exists());
        assert!(!cores_dir.join(format!(".{}.tmp", SAMEBOY.file)).exists());
        assert!(!is_installed(&data_dir, &SAMEBOY));
    }

    /// G2: the only real network access in this suite. Run once with `cargo test -- --ignored
    /// cores`.
    #[test]
    #[ignore]
    fn real_download_of_sameboy() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();
        let mut download = Download::start(&SAMEBOY, data_dir.clone());

        let path = wait_for(|| match download.poll() {
            DownloadState::Done(path) => Some(path),
            DownloadState::Failed(e) => panic!("download failed: {e}"),
            DownloadState::Running { .. } => None,
        });

        assert!(is_installed(&data_dir, &SAMEBOY));
        let bytes = fs::read(&path).unwrap();
        assert!(bytes.starts_with(ELF_MAGIC));
    }
}
