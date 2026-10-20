//! libretro Cores: which one plays each Embedded System, and downloading one the first time its
//! System is played (FR-EMB-2, US-23).
//!
//! Cores are never committed (ADR 0004); they are fetched from the libretro build server into
//! `~/.local/share/game37/cores/` on first use and kept there for later launches.

use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::io;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use tempfile::NamedTempFile;

use crate::systems::System;

/// A libretro Core: its display name, the `.so` file the build server publishes, and what it
/// needs to run well (FR-EMB-1, FR-EMB-12, FR-DS-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreInfo {
    pub name: &'static str,
    pub file: &'static str,
    /// `GET_VARIABLE` answers this Core needs ahead of its own defaults, e.g. to turn on hardware
    /// rendering or to lay out Nintendo DS's two screens (FR-DS-1).
    pub options: &'static [(&'static str, &'static str)],
    /// BIOS file names (lowercase), any one of which in `<Games Root>/bios/` is enough
    /// (FR-EMB-12). Empty when the Core needs none.
    pub required_bios: &'static [&'static str],
    /// Whether this Core's system directory is `<data dir>/system` (replaceable data) rather
    /// than `<Games Root>/bios` (BIOS files). Only PPSSPP needs this, for its asset files.
    pub system_in_data_dir: bool,
}

pub static MESEN: CoreInfo = CoreInfo {
    name: "Mesen",
    file: "mesen_libretro.so",
    options: &[],
    required_bios: &[],
    system_in_data_dir: false,
};
pub static BSNES: CoreInfo = CoreInfo {
    name: "bsnes",
    file: "bsnes_libretro.so",
    options: &[],
    required_bios: &[],
    system_in_data_dir: false,
};
pub static SAMEBOY: CoreInfo = CoreInfo {
    name: "SameBoy",
    file: "sameboy_libretro.so",
    options: &[],
    required_bios: &[],
    system_in_data_dir: false,
};
pub static MGBA: CoreInfo = CoreInfo {
    name: "mGBA",
    file: "mgba_libretro.so",
    options: &[],
    required_bios: &[],
    system_in_data_dir: false,
};
pub static MUPEN64PLUS_NEXT: CoreInfo = CoreInfo {
    name: "Mupen64Plus-Next",
    file: "mupen64plus_next_libretro.so",
    options: &[],
    required_bios: &[],
    system_in_data_dir: false,
};
/// Beetle PSX HW only calls `SET_HW_RENDER` when `GET_VARIABLE` answers its own `renderer`
/// option, so this is also needed for OpenGL hardware rendering to have any effect (see
/// `docs/research/m4-cores.md`).
pub static BEETLE_PSX_HW: CoreInfo = CoreInfo {
    name: "Beetle PSX HW",
    file: "mednafen_psx_hw_libretro.so",
    options: &[("beetle_psx_hw_renderer", "hardware_gl")],
    required_bios: &[
        "scph5500.bin",
        "scph5501.bin",
        "scph5502.bin",
        "scph5503.bin",
        "scph7003.bin",
        "scph5552.bin",
    ],
    system_in_data_dir: false,
};
/// Options chosen for FR-DS-1–3: two hybrid layouts (one large screen, one small), the touch
/// pointer and joystick cursor both live, and a visible cursor for aiming it.
pub static MELONDS_DS: CoreInfo = CoreInfo {
    name: "melonDS DS",
    file: "melondsds_libretro.so",
    options: &[
        ("melonds_number_of_screen_layouts", "2"),
        ("melonds_screen_layout1", "hybrid-top"),
        ("melonds_screen_layout2", "hybrid-bottom"),
        ("melonds_hybrid_small_screen", "one"),
        ("melonds_touch_mode", "auto"),
        ("melonds_show_cursor", "always"),
    ],
    required_bios: &[],
    system_in_data_dir: false,
};
pub static PPSSPP: CoreInfo = CoreInfo {
    name: "PPSSPP",
    file: "ppsspp_libretro.so",
    options: &[],
    required_bios: &[],
    system_in_data_dir: true,
};

/// The Core for a System, or `None` for an External System, which never uses a Core, or for an
/// Embedded System with no Core yet.
pub fn for_system(system: &System) -> Option<&'static CoreInfo> {
    match system.id {
        "nes" => Some(&MESEN),
        "snes" => Some(&BSNES),
        "gb" | "gbc" => Some(&SAMEBOY),
        "gba" => Some(&MGBA),
        "n64" => Some(&MUPEN64PLUS_NEXT),
        "ps1" => Some(&BEETLE_PSX_HW),
        "nds" => Some(&MELONDS_DS),
        // PSP stays off until PPSSPP runs on EGL: its glewInit() fails there and it crashes.
        _ => None,
    }
}

/// Where a Core is, or would be, stored: `<data_dir>/cores/<file>`.
pub fn installed_path(data_dir: &Path, core: &CoreInfo) -> PathBuf {
    data_dir.join("cores").join(core.file)
}

/// A Core's own system directory (ADR 0005): `<data dir>/system` for PPSSPP's replaceable
/// assets, `<Games Root>/bios` for every other Core's BIOS files (FR-LIB-9).
pub fn system_dir(core: &CoreInfo, data_dir: &Path, games_root: &Path) -> PathBuf {
    if core.system_in_data_dir {
        data_dir.join("system")
    } else {
        games_root.join("bios")
    }
}

/// Where PPSSPP's assets from `assets_url` unpack to: `<system dir>/PPSSPP`.
pub fn assets_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("system").join("PPSSPP")
}

/// A Core is installed once its library is there and, for PPSSPP, its assets are too.
pub fn is_installed(data_dir: &Path, core: &CoreInfo) -> bool {
    installed_path(data_dir, core).is_file() && (!core.system_in_data_dir || assets_dir(data_dir).is_dir())
}

/// Whether any of `core`'s `required_bios` names is in `<games_root>/bios/`, matched
/// case-insensitively. True when the Core needs none.
pub fn has_required_bios(games_root: &Path, core: &CoreInfo) -> bool {
    if core.required_bios.is_empty() {
        return true;
    }
    let Ok(entries) = fs::read_dir(games_root.join("bios")) else {
        return false;
    };
    let names: HashSet<String> = entries
        .filter_map(|e| Some(e.ok()?.file_name().to_string_lossy().to_lowercase()))
        .collect();
    core.required_bios.iter().any(|name| names.contains(*name))
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

/// A cap on a Core's zip size (FR-EMB-2 is for small libretro Cores, not arbitrary downloads).
const MAX_DOWNLOAD_BYTES: u64 = 100 * 1024 * 1024;

/// A cap on a Core's unzipped size, so a zip bomb can't fill memory or the disk. The real Cores
/// are under 10 MB.
const MAX_CORE_BYTES: u64 = 64 * 1024 * 1024;

/// The size caps a download enforces. A parameter so tests can exercise them with tiny inputs.
#[derive(Debug, Clone, Copy)]
struct Limits {
    /// The zip as downloaded.
    download: u64,
    /// The Core unzipped from it, declared or actual.
    core: u64,
}

const LIMITS: Limits = Limits {
    download: MAX_DOWNLOAD_BYTES,
    core: MAX_CORE_BYTES,
};

/// Where PPSSPP's assets zip comes from (M4 decision): contains the `PPSSPP/` folder, about
/// 9 MB, unpacked into `<data dir>/system/`.
fn assets_url() -> &'static str {
    "https://buildbot.libretro.com/assets/system/PPSSPP.zip"
}

/// A cap on the assets zip as downloaded.
const MAX_ASSETS_DOWNLOAD_BYTES: u64 = 100 * 1024 * 1024;

/// A cap on the assets unpacked, so a zip bomb can't fill memory or the disk. The real assets
/// are a few MB.
const MAX_ASSETS_BYTES: u64 = 64 * 1024 * 1024;

/// A Core download running on a background thread. `poll` never blocks; call it every frame.
/// Dropping a running `Download` cancels it and returns at once: its thread stops at its next step,
/// however long the network keeps it waiting, and leaves no partial files behind.
pub struct Download {
    state: Arc<Mutex<DownloadState>>,
    cancel: Arc<AtomicBool>,
}

impl Download {
    /// Starts downloading `core` into `data_dir` on a background thread.
    pub fn start(core: &'static CoreInfo, data_dir: PathBuf) -> Download {
        Download::spawn_limited(core, data_dir, Arc::new(ureq_fetch), LIMITS)
    }

    #[cfg(test)]
    fn spawn(core: &'static CoreInfo, data_dir: PathBuf, fetch: Fetcher) -> Download {
        Download::spawn_limited(core, data_dir, fetch, LIMITS)
    }

    fn spawn_limited(core: &'static CoreInfo, data_dir: PathBuf, fetch: Fetcher, limits: Limits) -> Download {
        let state = Arc::new(Mutex::new(DownloadState::Running {
            received: 0,
            total: None,
        }));
        let cancel = Arc::new(AtomicBool::new(false));
        let state_in_thread = Arc::clone(&state);
        let cancel_in_thread = Arc::clone(&cancel);
        // Detached: `Drop` never joins it.
        thread::spawn(move || {
            let result = panic::catch_unwind(AssertUnwindSafe(|| {
                run(
                    core,
                    &data_dir,
                    fetch.as_ref(),
                    limits,
                    &state_in_thread,
                    &cancel_in_thread,
                )
            }));
            let final_state = match result {
                Ok(Ok(path)) => DownloadState::Done(path),
                Ok(Err(message)) => DownloadState::Failed(message),
                // A bug, not a network problem: still never leave the screen waiting forever.
                Err(_) => DownloadState::Failed(format!("Couldn't download the {} Core.", core.name)),
            };
            *lock(&state_in_thread) = final_state;
        });
        Download { state, cancel }
    }

    /// Never blocks: reports the last known state.
    pub fn poll(&mut self) -> DownloadState {
        lock(&self.state).clone()
    }

    /// Starts downloading PPSSPP's assets zip into `<data_dir>/system/PPSSPP` on a background
    /// thread. Only needed for PPSSPP (FR-EMB-1); every other Core needs no assets.
    pub fn start_assets(data_dir: PathBuf) -> Download {
        Download::spawn_assets_limited(
            data_dir,
            Arc::new(ureq_fetch),
            MAX_ASSETS_DOWNLOAD_BYTES,
            MAX_ASSETS_BYTES,
        )
    }

    #[cfg(test)]
    fn spawn_assets(data_dir: PathBuf, fetch: Fetcher) -> Download {
        Download::spawn_assets_limited(data_dir, fetch, MAX_ASSETS_DOWNLOAD_BYTES, MAX_ASSETS_BYTES)
    }

    fn spawn_assets_limited(
        data_dir: PathBuf,
        fetch: Fetcher,
        download_cap: u64,
        assets_cap: u64,
    ) -> Download {
        let state = Arc::new(Mutex::new(DownloadState::Running {
            received: 0,
            total: None,
        }));
        let cancel = Arc::new(AtomicBool::new(false));
        let state_in_thread = Arc::clone(&state);
        let cancel_in_thread = Arc::clone(&cancel);
        thread::spawn(move || {
            let result = panic::catch_unwind(AssertUnwindSafe(|| {
                run_assets(
                    &data_dir,
                    fetch.as_ref(),
                    download_cap,
                    assets_cap,
                    &state_in_thread,
                    &cancel_in_thread,
                )
            }));
            let final_state = match result {
                Ok(Ok(path)) => DownloadState::Done(path),
                Ok(Err(message)) => DownloadState::Failed(message),
                Err(_) => DownloadState::Failed("Couldn't download the PPSSPP assets.".to_owned()),
            };
            *lock(&state_in_thread) = final_state;
        });
        Download { state, cancel }
    }
}

/// The state is only ever replaced whole, so a poisoned lock still holds a meaningful value.
fn lock(state: &Mutex<DownloadState>) -> MutexGuard<'_, DownloadState> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Drop for Download {
    /// Never joins: the thread may be blocked in DNS, connect or a read for as long as its timeouts
    /// allow, and the UI must not freeze meanwhile. It sees `cancel` at its next step and removes its
    /// own temp files.
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// The ELF magic a valid Linux shared library starts with.
const ELF_MAGIC: &[u8] = b"\x7fELF";

/// Read and extraction buffer size.
const CHUNK: usize = 64 * 1024;

/// The error a cancelled download ends with. Nobody sees it: the `Download` is already dropped.
const CANCELLED: &str = "cancelled";

fn check_cancel(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        Err(CANCELLED.to_string())
    } else {
        Ok(())
    }
}

fn run(
    core: &CoreInfo,
    data_dir: &Path,
    fetch: &(dyn Fn(&str) -> FetchResult + Send + Sync),
    limits: Limits,
    state: &Mutex<DownloadState>,
    cancel: &AtomicBool,
) -> Result<PathBuf, String> {
    let cores_dir = data_dir.join("cores");
    fs::create_dir_all(&cores_dir).map_err(|e| save_failed(core, &e))?;
    check_cancel(cancel)?;

    let (mut reader, total) = fetch(&url(core)).map_err(|e| match e {
        FetchError::Connect => connect_failed(core),
        FetchError::Status(status) => bad_status(core, status),
    })?;
    check_cancel(cancel)?;

    // Stream to a temp file of this download's own, reporting progress as we go. Its name is
    // unique, so a retry of the same Core while this thread is still winding down never shares
    // it, and dropping it removes it on every path out of here.
    let mut zip_file = temp_file(&cores_dir, core, ".zip.part").map_err(|e| save_failed(core, &e))?;
    let mut buf = vec![0u8; CHUNK];
    let mut received: u64 = 0;
    loop {
        check_cancel(cancel)?;
        let n = reader.read(&mut buf).map_err(|_| incomplete(core))?;
        if n == 0 {
            break;
        }
        received += n as u64;
        if received > limits.download {
            return Err(incomplete(core));
        }
        zip_file.write_all(&buf[..n]).map_err(|e| save_failed(core, &e))?;
        *lock(state) = DownloadState::Running { received, total };
    }
    drop(reader);
    if let Some(total) = total
        && received < total
    {
        return Err(incomplete(core));
    }

    // Open the zip and take the one entry we trust: `by_name` matches the name exactly.
    let mut archive = zip::ZipArchive::new(zip_file.as_file()).map_err(|_| incomplete(core))?;
    let entry = archive.by_name(core.file).map_err(|_| invalid_library(core))?;
    let declared = entry.size();
    let content = read_core(core, entry, declared, limits.core, cancel)?;
    drop(archive);
    drop(zip_file);

    // Verify, make executable, and install with an atomic rename.
    if !content.starts_with(ELF_MAGIC) {
        return Err(invalid_library(core));
    }
    let mut library = temp_file(&cores_dir, core, ".tmp").map_err(|e| save_failed(core, &e))?;
    library.write_all(&content).map_err(|e| save_failed(core, &e))?;
    library
        .as_file()
        .set_permissions(fs::Permissions::from_mode(0o755))
        .map_err(|e| save_failed(core, &e))?;
    check_cancel(cancel)?;
    let final_path = installed_path(data_dir, core);
    library
        .persist(&final_path)
        .map_err(|e| save_failed(core, &e.error))?;
    Ok(final_path)
}

/// Reads a Core out of its zip `entry`, a chunk at a time so a cancel is seen promptly. An entry
/// over `limit`, by its `declared` size or by what it actually unpacks to, isn't a Core: it is
/// rejected without reading more than `limit + 1` bytes.
fn read_core(
    core: &CoreInfo,
    entry: impl Read,
    declared: u64,
    limit: u64,
    cancel: &AtomicBool,
) -> Result<Vec<u8>, String> {
    if declared > limit {
        return Err(invalid_library(core));
    }
    let mut capped = entry.take(limit + 1);
    let mut content = Vec::new();
    let mut buf = vec![0u8; CHUNK];
    loop {
        check_cancel(cancel)?;
        let n = capped.read(&mut buf).map_err(|_| incomplete(core))?;
        if n == 0 {
            break;
        }
        content.extend_from_slice(&buf[..n]);
    }
    if content.len() as u64 > limit {
        return Err(invalid_library(core));
    }
    Ok(content)
}

/// A uniquely named hidden temp file in `cores/`, removed when dropped unless persisted.
fn temp_file(cores_dir: &Path, core: &CoreInfo, suffix: &str) -> io::Result<NamedTempFile> {
    tempfile::Builder::new()
        .prefix(&format!(".{}.", core.file))
        .suffix(suffix)
        .tempfile_in(cores_dir)
}

/// Downloads PPSSPP's assets zip and unpacks the entries under `PPSSPP/` into
/// `<data_dir>/system/PPSSPP`, the same safe way as a Core: a unique temp file and a temp
/// extraction directory, cleaned up on every path out, with no entry escaping its folder via
/// `..` or an absolute path, and both the download and the unpacked size capped.
fn run_assets(
    data_dir: &Path,
    fetch: &(dyn Fn(&str) -> FetchResult + Send + Sync),
    download_cap: u64,
    assets_cap: u64,
    state: &Mutex<DownloadState>,
    cancel: &AtomicBool,
) -> Result<PathBuf, String> {
    let system_dir = data_dir.join("system");
    fs::create_dir_all(&system_dir).map_err(|e| assets_save_failed(&e))?;
    check_cancel(cancel)?;

    let (mut reader, total) = fetch(assets_url()).map_err(|e| match e {
        FetchError::Connect => assets_connect_failed(),
        FetchError::Status(status) => assets_bad_status(status),
    })?;
    check_cancel(cancel)?;

    let mut zip_file = tempfile::Builder::new()
        .prefix(".ppsspp-assets.")
        .suffix(".zip.part")
        .tempfile_in(&system_dir)
        .map_err(|e| assets_save_failed(&e))?;
    let mut buf = vec![0u8; CHUNK];
    let mut received: u64 = 0;
    loop {
        check_cancel(cancel)?;
        let n = reader.read(&mut buf).map_err(|_| assets_incomplete())?;
        if n == 0 {
            break;
        }
        received += n as u64;
        if received > download_cap {
            return Err(assets_incomplete());
        }
        zip_file
            .write_all(&buf[..n])
            .map_err(|e| assets_save_failed(&e))?;
        *lock(state) = DownloadState::Running { received, total };
    }
    drop(reader);
    if let Some(total) = total
        && received < total
    {
        return Err(assets_incomplete());
    }

    let mut archive = zip::ZipArchive::new(zip_file.as_file()).map_err(|_| assets_incomplete())?;
    let extract_dir = tempfile::Builder::new()
        .prefix(".ppsspp-assets-extract.")
        .tempdir_in(&system_dir)
        .map_err(|e| assets_save_failed(&e))?;
    let mut extracted: u64 = 0;
    for i in 0..archive.len() {
        check_cancel(cancel)?;
        let mut entry = archive.by_index(i).map_err(|_| assets_invalid())?;
        // Only entries under `PPSSPP/`, and never one that escapes the extraction dir.
        let Ok(name) = entry.name() else { continue };
        let Some(rel) = safe_asset_path(&name) else {
            continue;
        };
        let dest = extract_dir.path().join(&rel);
        if entry.is_dir() {
            fs::create_dir_all(&dest).map_err(|e| assets_save_failed(&e))?;
            continue;
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| assets_save_failed(&e))?;
        }
        extracted += entry.size();
        if extracted > assets_cap {
            return Err(assets_invalid());
        }
        let mut out = fs::File::create(&dest).map_err(|e| assets_save_failed(&e))?;
        let mut capped = (&mut entry).take(assets_cap + 1);
        let mut written: u64 = 0;
        loop {
            check_cancel(cancel)?;
            let n = capped.read(&mut buf).map_err(|_| assets_incomplete())?;
            if n == 0 {
                break;
            }
            written += n as u64;
            out.write_all(&buf[..n]).map_err(|e| assets_save_failed(&e))?;
        }
        if written > assets_cap {
            return Err(assets_invalid());
        }
    }
    drop(archive);
    drop(zip_file);
    check_cancel(cancel)?;

    let final_dir = assets_dir(data_dir);
    if final_dir.exists() {
        fs::remove_dir_all(&final_dir).map_err(|e| assets_save_failed(&e))?;
    }
    fs::rename(extract_dir.path(), &final_dir).map_err(|e| assets_save_failed(&e))?;
    // Already moved to its final place: disarm the temp dir's own cleanup.
    let _ = extract_dir.keep();
    Ok(final_dir)
}

/// The path under the extraction directory for a zip entry named `name`, or `None` when it is
/// outside `PPSSPP/` or would escape the extraction directory (a `..` component or an absolute
/// path).
fn safe_asset_path(name: &str) -> Option<PathBuf> {
    let rel = name.strip_prefix("PPSSPP/")?;
    if rel.is_empty() {
        return None;
    }
    let mut out = PathBuf::new();
    for component in Path::new(rel).components() {
        match component {
            std::path::Component::Normal(part) => out.push(part),
            _ => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

fn assets_connect_failed() -> String {
    "Couldn't download the PPSSPP assets. Check your internet connection.".to_owned()
}

fn assets_bad_status(status: u16) -> String {
    format!("The libretro build server couldn't provide the PPSSPP assets (HTTP {status}).")
}

fn assets_incomplete() -> String {
    "The PPSSPP assets download was incomplete.".to_owned()
}

fn assets_invalid() -> String {
    "The downloaded PPSSPP assets aren't valid.".to_owned()
}

fn assets_save_failed(error: &io::Error) -> String {
    format!("Couldn't save the PPSSPP assets: {error}.")
}

fn connect_failed(core: &CoreInfo) -> String {
    format!(
        "Couldn't download the {} Core. Check your internet connection.",
        core.name
    )
}

fn bad_status(core: &CoreInfo, status: u16) -> String {
    format!(
        "The libretro build server couldn't provide the {} Core (HTTP {status}).",
        core.name
    )
}

fn incomplete(core: &CoreInfo) -> String {
    format!("The {} Core download was incomplete.", core.name)
}

fn invalid_library(core: &CoreInfo) -> String {
    format!("The downloaded {} Core isn't a valid Linux library.", core.name)
}

fn save_failed(core: &CoreInfo, error: &io::Error) -> String {
    format!("Couldn't save the {} Core: {error}.", core.name)
}

/// The production fetcher: `ureq` over rustls.
///
/// Each step up to the response headers has its own 30 s timeout. The body gets 10 minutes in
/// total, as ureq 3 has no idle timeout between reads: enough for the largest Core on a slow but
/// working connection. The user can cancel at any point regardless.
///
/// Redirects are not followed: the build server answers its zip URLs directly, and NFR-5 allows
/// no other host. A redirect shows up as its HTTP status.
fn ureq_fetch(url: &str) -> FetchResult {
    let step = Some(Duration::from_secs(30));
    let config = ureq::Agent::config_builder()
        .timeout_resolve(step)
        .timeout_connect(step)
        .timeout_send_request(step)
        .timeout_recv_response(step)
        .timeout_recv_body(Some(Duration::from_secs(10 * 60)))
        .max_redirects(0)
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

    /// An [`AssetsDownload`]-style `Download` (PPSSPP's assets) that completes with `bytes` as
    /// the fetched zip's body.
    pub fn spawn_assets(data_dir: PathBuf, bytes: Vec<u8>, total: Option<u64>) -> Download {
        let fetch: Fetcher = Arc::new(move |_url: &str| {
            Ok((
                Box::new(Cursor::new(bytes.clone())) as Box<dyn Read + Send>,
                total,
            ))
        });
        Download::spawn_assets(data_dir, fetch)
    }

    /// An assets `Download` that fails to connect as soon as it is polled.
    pub fn spawn_assets_failing(data_dir: PathBuf) -> Download {
        Download::spawn_assets(data_dir, Arc::new(|_url: &str| Err(FetchError::Connect)))
    }

    /// A zip holding exactly `entries` (`(name, content)`), for assets tests to add entries
    /// outside `PPSSPP/` or that try to escape it.
    pub fn assets_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        let mut writer = zip::ZipWriter::new(Cursor::new(&mut buf));
        let options =
            zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, content) in entries {
            writer.start_file(*name, options).unwrap();
            writer.write_all(content).unwrap();
        }
        writer.finish().unwrap();
        buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::{SYSTEMS, by_id};
    use std::io::Cursor;
    use std::sync::atomic::AtomicU64;
    use std::sync::{Barrier, mpsc};
    use std::time::Instant;

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

    fn core_content() -> Vec<u8> {
        let mut content = ELF_MAGIC.to_vec();
        content.extend_from_slice(b"rest of a fake core");
        content
    }

    fn core_zip() -> Vec<u8> {
        zip_with(SAMEBOY.file, &core_content())
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

    fn wait_for_failure(download: &mut Download) -> String {
        wait_for(|| match download.poll() {
            DownloadState::Failed(m) => Some(m),
            DownloadState::Done(_) => panic!("expected a failure"),
            DownloadState::Running { .. } => None,
        })
    }

    fn wait_for_done(download: &mut Download) -> PathBuf {
        wait_for(|| match download.poll() {
            DownloadState::Done(path) => Some(path),
            DownloadState::Failed(e) => panic!("download failed: {e}"),
            DownloadState::Running { .. } => None,
        })
    }

    /// Everything in `cores/` except an installed SameBoy: any temp file a download left behind.
    fn leftovers(data_dir: &Path) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(data_dir.join("cores")) else {
            return Vec::new();
        };
        entries
            .map(|e| e.unwrap().path())
            .filter(|p| p.file_name().unwrap() != SAMEBOY.file)
            .collect()
    }

    /// `n` zero bytes, counting how many have been read so far into `consumed`.
    struct Zeros {
        remaining: u64,
        consumed: Arc<AtomicU64>,
    }
    impl Read for Zeros {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = (buf.len() as u64).min(self.remaining) as usize;
            buf[..n].fill(0);
            self.remaining -= n as u64;
            self.consumed.fetch_add(n as u64, Ordering::Relaxed);
            Ok(n)
        }
    }

    #[test]
    fn table_has_a_core_for_each_embedded_system_of_this_milestone() {
        assert_eq!(for_system(by_id("nes").unwrap()), Some(&MESEN));
        assert_eq!(for_system(by_id("snes").unwrap()), Some(&BSNES));
        assert_eq!(for_system(by_id("gb").unwrap()), Some(&SAMEBOY));
        assert_eq!(for_system(by_id("gbc").unwrap()), Some(&SAMEBOY));
        assert_eq!(for_system(by_id("gba").unwrap()), Some(&MGBA));
        assert_eq!(for_system(by_id("n64").unwrap()), Some(&MUPEN64PLUS_NEXT));
        assert_eq!(for_system(by_id("ps1").unwrap()), Some(&BEETLE_PSX_HW));
        assert_eq!(for_system(by_id("nds").unwrap()), Some(&MELONDS_DS));
        // PSP stays routed to no Core until the parallel GL-host job re-enables it.
        assert_eq!(for_system(by_id("psp").unwrap()), None);
        for system in SYSTEMS
            .iter()
            .filter(|s| s.kind == crate::systems::Kind::External)
        {
            assert_eq!(for_system(system), None, "{}", system.id);
        }
    }

    #[test]
    fn beetle_psx_hw_options_and_required_bios_match_the_research_doc() {
        assert_eq!(
            BEETLE_PSX_HW.options,
            &[("beetle_psx_hw_renderer", "hardware_gl")]
        );
        assert_eq!(
            BEETLE_PSX_HW.required_bios,
            &[
                "scph5500.bin",
                "scph5501.bin",
                "scph5502.bin",
                "scph5503.bin",
                "scph7003.bin",
                "scph5552.bin",
            ]
        );
    }

    #[test]
    fn melonds_ds_options_match_fr_ds_1_to_3() {
        assert_eq!(
            MELONDS_DS.options,
            &[
                ("melonds_number_of_screen_layouts", "2"),
                ("melonds_screen_layout1", "hybrid-top"),
                ("melonds_screen_layout2", "hybrid-bottom"),
                ("melonds_hybrid_small_screen", "one"),
                ("melonds_touch_mode", "auto"),
                ("melonds_show_cursor", "always"),
            ]
        );
        assert!(
            MELONDS_DS.required_bios.is_empty(),
            "DS's BIOS is optional (FreeBIOS)"
        );
    }

    #[test]
    fn only_ppsspp_uses_the_data_dirs_system_directory() {
        for core in [
            &MESEN,
            &BSNES,
            &SAMEBOY,
            &MGBA,
            &MUPEN64PLUS_NEXT,
            &BEETLE_PSX_HW,
            &MELONDS_DS,
        ] {
            assert!(!core.system_in_data_dir, "{}", core.name);
        }
        assert!(PPSSPP.system_in_data_dir);
    }

    #[test]
    fn ppsspps_system_dir_is_under_the_data_dir_everyone_elses_is_the_bios_folder() {
        let data_dir = Path::new("/data");
        let root = Path::new("/Games");
        assert_eq!(system_dir(&PPSSPP, data_dir, root), data_dir.join("system"));
        assert_eq!(system_dir(&BEETLE_PSX_HW, data_dir, root), root.join("bios"));
    }

    #[test]
    fn has_required_bios_is_true_with_no_requirement_or_any_one_match_case_insensitively() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert!(has_required_bios(root, &MELONDS_DS), "no requirement");
        assert!(!has_required_bios(root, &BEETLE_PSX_HW), "nothing there yet");

        fs::create_dir_all(root.join("bios")).unwrap();
        fs::write(root.join("bios/SCPH5502.BIN"), b"").unwrap();
        assert!(
            has_required_bios(root, &BEETLE_PSX_HW),
            "any one, any case, is enough"
        );

        fs::write(root.join("bios/unrelated.bin"), b"").unwrap();
        assert!(has_required_bios(root, &BEETLE_PSX_HW));
    }

    #[test]
    fn ppsspp_is_installed_only_once_its_library_and_its_assets_are_both_there() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();
        assert!(!is_installed(&data_dir, &PPSSPP));

        touch(&installed_path(&data_dir, &PPSSPP));
        assert!(!is_installed(&data_dir, &PPSSPP), "library only");

        fs::create_dir_all(assets_dir(&data_dir)).unwrap();
        assert!(is_installed(&data_dir, &PPSSPP));
    }

    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"").unwrap();
    }

    #[test]
    fn url_points_at_the_nightly_zip() {
        assert_eq!(
            url(&MESEN),
            "https://buildbot.libretro.com/nightly/linux/x86_64/latest/mesen_libretro.so.zip"
        );
    }

    fn wait_for_assets<F: FnMut() -> Option<T>, T>(mut f: F) -> T {
        for _ in 0..2000 {
            if let Some(value) = f() {
                return value;
            }
            thread::sleep(Duration::from_millis(5));
        }
        panic!("timed out waiting for condition");
    }

    #[test]
    fn assets_install_unpacks_only_entries_under_ppsspp() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();
        let zip = testing::assets_zip(&[
            ("PPSSPP/flash0/font/a.pgf", b"font data"),
            ("PPSSPP/README.txt", b"readme"),
            ("not-ppsspp/other.txt", b"should be ignored"),
        ]);
        let mut download = testing::spawn_assets(data_dir.clone(), zip, None);

        let path = wait_for_assets(|| match download.poll() {
            DownloadState::Done(path) => Some(path),
            DownloadState::Failed(e) => panic!("assets install failed: {e}"),
            DownloadState::Running { .. } => None,
        });

        assert_eq!(path, assets_dir(&data_dir));
        assert_eq!(fs::read(path.join("flash0/font/a.pgf")).unwrap(), b"font data");
        assert_eq!(fs::read(path.join("README.txt")).unwrap(), b"readme");
        assert!(!path.join("other.txt").exists());
        assert!(!data_dir.join("not-ppsspp").exists());
    }

    #[test]
    fn assets_install_refuses_a_path_traversal_entry() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();
        let zip = testing::assets_zip(&[
            ("PPSSPP/good.txt", b"fine"),
            ("PPSSPP/../../evil.txt", b"escape attempt"),
        ]);
        let mut download = testing::spawn_assets(data_dir.clone(), zip, None);

        let path = wait_for_assets(|| match download.poll() {
            DownloadState::Done(path) => Some(path),
            DownloadState::Failed(e) => panic!("assets install failed: {e}"),
            DownloadState::Running { .. } => None,
        });

        assert_eq!(fs::read(path.join("good.txt")).unwrap(), b"fine");
        assert!(!dir.path().join("evil.txt").exists());
        assert!(!data_dir.join("evil.txt").exists());
    }

    #[test]
    fn a_failed_assets_download_leaves_no_temp_files_behind() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();
        let mut download = testing::spawn_assets_failing(data_dir.clone());

        let message = wait_for_assets(|| match download.poll() {
            DownloadState::Failed(m) => Some(m),
            DownloadState::Done(_) => panic!("expected a failure"),
            DownloadState::Running { .. } => None,
        });

        assert_eq!(
            message,
            "Couldn't download the PPSSPP assets. Check your internet connection."
        );
        assert!(!assets_dir(&data_dir).exists());
        let leftover = fs::read_dir(data_dir.join("system"))
            .map(|entries| entries.count())
            .unwrap_or(0);
        assert_eq!(leftover, 0, "no temp files left in system/");
    }

    #[test]
    fn successful_install_is_executable_with_no_temp_files_left() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();
        let mut download = Download::spawn(&SAMEBOY, data_dir.clone(), fixed_reader_fetcher(core_zip()));

        let path = wait_for_done(&mut download);

        assert_eq!(path, installed_path(&data_dir, &SAMEBOY));
        assert!(is_installed(&data_dir, &SAMEBOY));
        assert_eq!(fs::read(&path).unwrap(), core_content());
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
        assert_eq!(leftovers(&data_dir), Vec::<PathBuf>::new());
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
        wait_for_done(&mut download);
    }

    #[test]
    fn missing_entry_is_the_invalid_library_error() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();
        let zip = zip_with("other_libretro.so", ELF_MAGIC);
        let mut download = Download::spawn(&SAMEBOY, data_dir.clone(), fixed_reader_fetcher(zip));

        assert_eq!(wait_for_failure(&mut download), invalid_library(&SAMEBOY));
        assert!(!is_installed(&data_dir, &SAMEBOY));
    }

    #[test]
    fn a_path_traversal_entry_is_the_invalid_library_error() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();
        let zip = zip_with(&format!("../{}", SAMEBOY.file), ELF_MAGIC);
        let mut download = Download::spawn(&SAMEBOY, data_dir.clone(), fixed_reader_fetcher(zip));

        assert_eq!(wait_for_failure(&mut download), invalid_library(&SAMEBOY));
        assert!(!is_installed(&data_dir, &SAMEBOY));
    }

    #[test]
    fn a_non_elf_entry_is_the_invalid_library_error() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();
        let zip = zip_with(SAMEBOY.file, b"not an elf file");
        let mut download = Download::spawn(&SAMEBOY, data_dir.clone(), fixed_reader_fetcher(zip));

        assert_eq!(wait_for_failure(&mut download), invalid_library(&SAMEBOY));
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

        assert_eq!(wait_for_failure(&mut download), incomplete(&SAMEBOY));
        assert!(!is_installed(&data_dir, &SAMEBOY));
        assert_eq!(leftovers(&data_dir), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_fetch_error_is_the_connection_message() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();
        let mut download = Download::spawn(&SAMEBOY, data_dir.clone(), failing_fetcher());

        assert_eq!(wait_for_failure(&mut download), connect_failed(&SAMEBOY));
        assert!(!is_installed(&data_dir, &SAMEBOY));
    }

    #[test]
    fn an_http_error_says_the_build_server_couldnt_provide_the_core() {
        let dir = tempfile::tempdir().unwrap();
        let fetch: Fetcher = Arc::new(|_url: &str| Err(FetchError::Status(404)));
        let mut download = Download::spawn(&SAMEBOY, dir.path().to_owned(), fetch);

        assert_eq!(
            wait_for_failure(&mut download),
            "The libretro build server couldn't provide the SameBoy Core (HTTP 404)."
        );
    }

    #[test]
    fn a_local_disk_error_says_the_core_couldnt_be_saved() {
        let dir = tempfile::tempdir().unwrap();
        // A file where the data directory should be: `cores/` can't be created under it.
        let data_dir = dir.path().join("not-a-dir");
        fs::write(&data_dir, b"").unwrap();
        let mut download = Download::spawn(&SAMEBOY, data_dir, fixed_reader_fetcher(core_zip()));

        let message = wait_for_failure(&mut download);
        assert!(
            message.starts_with("Couldn't save the SameBoy Core: "),
            "{message}"
        );
    }

    #[test]
    fn a_download_over_the_cap_stops_reading_and_fails() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();
        let limits = Limits {
            download: 100 * 1024,
            core: MAX_CORE_BYTES,
        };
        let consumed = Arc::new(AtomicU64::new(0));
        let consumed_in_fetch = Arc::clone(&consumed);
        let fetch: Fetcher = Arc::new(move |_url: &str| {
            let zeros = Zeros {
                remaining: 10 * 1024 * 1024,
                consumed: Arc::clone(&consumed_in_fetch),
            };
            Ok((Box::new(zeros) as Box<dyn Read + Send>, None))
        });
        let mut download = Download::spawn_limited(&SAMEBOY, data_dir.clone(), fetch, limits);

        assert_eq!(wait_for_failure(&mut download), incomplete(&SAMEBOY));
        let consumed = consumed.load(Ordering::Relaxed);
        assert!(
            consumed <= limits.download + CHUNK as u64,
            "read {consumed} bytes past a {} byte cap",
            limits.download
        );
        assert!(!is_installed(&data_dir, &SAMEBOY));
        assert_eq!(leftovers(&data_dir), Vec::<PathBuf>::new());
    }

    #[test]
    fn the_real_caps_are_100_mb_downloaded_and_64_mb_unzipped() {
        assert_eq!(LIMITS.download, 100 * 1024 * 1024);
        assert_eq!(LIMITS.core, 64 * 1024 * 1024);
    }

    #[test]
    fn an_entry_declared_over_the_core_cap_is_the_invalid_library_error() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();
        let limits = Limits {
            download: MAX_DOWNLOAD_BYTES,
            core: 1024,
        };
        // A valid-looking library, just bigger than the cap.
        let mut content = ELF_MAGIC.to_vec();
        content.resize(2048, 0);
        let zip = zip_with(SAMEBOY.file, &content);
        let mut download =
            Download::spawn_limited(&SAMEBOY, data_dir.clone(), fixed_reader_fetcher(zip), limits);

        assert_eq!(wait_for_failure(&mut download), invalid_library(&SAMEBOY));
        assert!(!is_installed(&data_dir, &SAMEBOY));
        assert_eq!(leftovers(&data_dir), Vec::<PathBuf>::new());
    }

    #[test]
    fn an_entry_unpacking_past_its_declared_size_stops_at_the_cap() {
        // A zip bomb lying about its size: declares 0 bytes, unpacks to far more than the cap.
        let consumed = Arc::new(AtomicU64::new(0));
        let bomb = Zeros {
            remaining: 10 * 1024 * 1024,
            consumed: Arc::clone(&consumed),
        };
        let result = read_core(&SAMEBOY, bomb, 0, 1024, &AtomicBool::new(false));

        // Compare lengths, not contents: a failure shouldn't print megabytes of zeros.
        assert_eq!(
            result.map(|content| content.len()),
            Err(invalid_library(&SAMEBOY))
        );
        assert_eq!(consumed.load(Ordering::Relaxed), 1025);
    }

    #[test]
    fn a_core_at_the_cap_is_read_whole() {
        let content = vec![7u8; 1024];
        let result = read_core(
            &SAMEBOY,
            Cursor::new(content.clone()),
            1024,
            1024,
            &AtomicBool::new(false),
        );
        assert_eq!(result, Ok(content));
    }

    #[test]
    fn extraction_stops_when_cancelled() {
        let result = read_core(
            &SAMEBOY,
            Cursor::new(vec![0u8; 16]),
            16,
            1024,
            &AtomicBool::new(true),
        );
        assert_eq!(result, Err(CANCELLED.to_string()));
    }

    #[test]
    fn a_panicking_worker_is_a_failure_not_running_forever() {
        let dir = tempfile::tempdir().unwrap();
        let fetch: Fetcher = Arc::new(|_url: &str| panic!("a bug in the worker"));
        let mut download = Download::spawn(&SAMEBOY, dir.path().to_owned(), fetch);

        assert_eq!(
            wait_for_failure(&mut download),
            "Couldn't download the SameBoy Core."
        );
    }

    /// Serves one chunk of zeros, but only once released; until then it blocks, like a read on a
    /// stalled connection. Tells `entered` when the worker is blocked in it.
    struct BlockedReader {
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
        served: bool,
    }
    impl Read for BlockedReader {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.served {
                return Ok(0);
            }
            let _ = self.entered.send(());
            let _ = self.release.recv();
            self.served = true;
            let n = buf.len().min(16);
            buf[..n].fill(0);
            Ok(n)
        }
    }

    #[test]
    fn dropping_while_blocked_returns_at_once_and_the_worker_cleans_up_after_itself() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let reader = Mutex::new(Some(BlockedReader {
            entered: entered_tx,
            release: release_rx,
            served: false,
        }));
        let fetch: Fetcher = Arc::new(move |_url: &str| {
            let reader = reader
                .lock()
                .unwrap()
                .take()
                .expect("fetcher called more than once");
            Ok((Box::new(reader) as Box<dyn Read + Send>, None))
        });
        // The worker thread owns the fetcher until it exits.
        let worker_alive = Arc::downgrade(&fetch);

        let download = Download::spawn(&SAMEBOY, data_dir.clone(), fetch);
        entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();

        // Release the reader well after the drop should have returned, so a drop that waits for the
        // worker shows up as slow instead of hanging the test.
        let releaser = thread::spawn(move || {
            thread::sleep(Duration::from_millis(500));
            release_tx.send(()).unwrap();
        });
        let started = Instant::now();
        drop(download);
        let elapsed = started.elapsed();
        assert!(elapsed < Duration::from_millis(100), "drop took {elapsed:?}");

        // A retry while the old worker is still blocked installs fine, and the old worker's
        // cleanup doesn't touch it.
        let mut retry = Download::spawn(&SAMEBOY, data_dir.clone(), fixed_reader_fetcher(core_zip()));
        wait_for_done(&mut retry);

        releaser.join().unwrap();
        wait_for(|| worker_alive.upgrade().is_none().then_some(()));
        assert_eq!(leftovers(&data_dir), Vec::<PathBuf>::new());
        assert_eq!(
            fs::read(installed_path(&data_dir, &SAMEBOY)).unwrap(),
            core_content()
        );
    }

    #[test]
    fn two_concurrent_downloads_of_the_same_core_both_install_it_whole() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();
        let zip = core_zip();
        // Both workers are past the fetch before either reads a byte, so their files overlap.
        let barrier = Arc::new(Barrier::new(2));
        let fetch: Fetcher = Arc::new(move |_url: &str| {
            barrier.wait();
            Ok((Box::new(Cursor::new(zip.clone())) as Box<dyn Read + Send>, None))
        });

        let mut first = Download::spawn(&SAMEBOY, data_dir.clone(), Arc::clone(&fetch));
        let mut second = Download::spawn(&SAMEBOY, data_dir.clone(), fetch);
        let first_path = wait_for_done(&mut first);
        let second_path = wait_for_done(&mut second);

        assert_eq!(first_path, second_path);
        assert_eq!(fs::read(&first_path).unwrap(), core_content());
        assert_eq!(leftovers(&data_dir), Vec::<PathBuf>::new());
    }

    /// The only real network access in this suite. Run once with `cargo test -- --ignored cores`.
    #[test]
    #[ignore]
    fn real_download_of_sameboy() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();
        let mut download = Download::start(&SAMEBOY, data_dir.clone());

        let path = wait_for_done(&mut download);

        assert!(is_installed(&data_dir, &SAMEBOY));
        let bytes = fs::read(&path).unwrap();
        assert!(bytes.starts_with(ELF_MAGIC));
        assert_eq!(leftovers(&data_dir), Vec::<PathBuf>::new());
    }
}
