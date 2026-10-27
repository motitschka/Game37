//! Choosing a Standalone Emulator's `.exe` (FR-WIN-4) in Game37's own file browser rather than the
//! Windows dialog, after `screen_exe_browser` in `design/generate_mockups.py`. It works with a
//! controller alone and shows only folders and `.exe` files: places down the left, the current
//! folder's subfolders then `.exe` files on the right, the emulator's usual file highlighted.
//!
//! Up and Down move in a column, Left and Right between the two. Confirm opens a place or a folder
//! and chooses an `.exe`, which hands its path to the callback and closes the browser. Back goes up
//! a folder; from the folder of the place last opened it goes to the places instead, and from the
//! places it cancels, as X does anywhere. Folders are read, and the places discovered, each on a
//! thread of their own (NFR-3), so even `C:\Windows\System32` or a disconnected network drive never
//! holds up a frame; the places column shows "Loading…" until they arrive.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use crate::ctx::Ctx;
use crate::screens::setup::folders::trim_start;
use crate::ui::cover::ellipsize;
use crate::ui::gfx::{Align, Gfx, TextStyle};
use crate::ui::input::{Action, Input, Pointer};
use crate::ui::nav::{Hint, Screen, Transition};
use crate::ui::scroll;
use crate::ui::text::Weight;
use crate::ui::theme::{ACCENT, Colour, LINE, MUTED, SURFACE, SURFACE2, TEXT};
use crate::ui::{M, Rect, W, hit};

/// Top and bottom of the two panels.
const TOP: f32 = 320.0;
const BOTTOM: f32 = 970.0;
/// Inset of the first row from a panel's top, and of the last visible one from its bottom.
const PAD: f32 = 20.0;
const PLACES_W: f32 = 400.0;
const PLACE_H: f32 = 48.0;
const PLACE_PITCH: f32 = 56.0;
/// Left edge and width of the list panel.
const LX: f32 = M + 440.0;
const LW: f32 = W - M - LX;
const ROW_H: f32 = 56.0;
const ROW_PITCH: f32 = 64.0;
/// Height of the rows' view inside a panel.
const VIEW: f32 = BOTTOM - TOP - 2.0 * PAD;
const ICON: f32 = 26.0;
const CRUMB_SEP: &str = "  ›  ";

/// What happens with the chosen `.exe`.
type OnChoose = Box<dyn FnOnce(&mut Ctx, PathBuf)>;
/// Reads one folder's listing, on the reading thread.
type Read = Arc<dyn Fn(&Path) -> io::Result<Vec<Item>> + Send + Sync>;
/// Finds this computer's places, on the discovery thread.
type Discover = Arc<dyn Fn() -> Vec<Place> + Send + Sync>;

/// One entry of the places column: a folder to start from.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub label: String,
    pub path: PathBuf,
}

impl Place {
    pub fn new(label: impl Into<String>, path: impl Into<PathBuf>) -> Place {
        Place {
            label: label.into(),
            path: path.into(),
        }
    }
}

/// This computer's places, each only if it exists and once: on Windows the drives, Downloads,
/// Desktop, Program Files and winget's packages folder; elsewhere Home, Downloads, Desktop and `/`.
pub fn places() -> Vec<Place> {
    let mut places: Vec<Place> = Vec::new();
    for place in candidate_places() {
        if place.path.is_dir() && !places.iter().any(|p| p.path == place.path) {
            places.push(place);
        }
    }
    places
}

#[cfg(not(windows))]
fn candidate_places() -> Vec<Place> {
    let dirs = [
        ("Home", dirs::home_dir()),
        ("Downloads", dirs::download_dir()),
        ("Desktop", dirs::desktop_dir()),
    ];
    let mut places: Vec<Place> = dirs
        .into_iter()
        .filter_map(|(l, p)| Some(Place::new(l, p?)))
        .collect();
    places.push(Place::new("/", "/"));
    places
}

/// Every drive letter (`places` keeps those that exist), then the folders, Program Files (x86) only
/// where it differs from Program Files.
#[cfg(windows)]
fn candidate_places() -> Vec<Place> {
    let mut places: Vec<Place> = ('A'..='Z')
        .map(|letter| Place::new(format!("{letter}:"), format!("{letter}:\\")))
        .collect();
    let dirs = [
        ("Downloads", dirs::download_dir()),
        ("Desktop", dirs::desktop_dir()),
    ];
    places.extend(dirs.into_iter().filter_map(|(l, p)| Some(Place::new(l, p?))));
    let env = |name: &str| std::env::var_os(name).map(PathBuf::from);
    places.extend(env("ProgramFiles").map(|p| Place::new("Program Files", p)));
    places.extend(env("ProgramFiles(x86)").map(|p| Place::new("Program Files (x86)", p)));
    places.extend(
        env("LOCALAPPDATA").map(|p| Place::new("WinGet Packages", p.join(r"Microsoft\WinGet\Packages"))),
    );
    places
}

/// One entry of a folder's listing: a subfolder or an `.exe` file.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// The file name as shown.
    pub name: String,
    pub path: PathBuf,
    pub folder: bool,
}

/// What `dir` holds that the browser shows: its subfolders, then its `.exe` files (the extension
/// in any case), each sorted case-insensitively, hidden ones left out. A link counts as what it
/// points to; one that points nowhere, or an entry that can't be read, is left out.
pub fn read_folder(dir: &Path) -> io::Result<Vec<Item>> {
    let mut items = Vec::new();
    for entry in fs::read_dir(dir)? {
        let Ok(entry) = entry else {
            continue;
        };
        let name = entry.file_name();
        if is_hidden(&entry, &name) {
            continue;
        }
        let path = entry.path();
        let folder = match entry.file_type() {
            Ok(kind) if kind.is_symlink() => match fs::metadata(&path) {
                Ok(target) => target.is_dir(),
                Err(_) => continue,
            },
            Ok(kind) => kind.is_dir(),
            Err(_) => continue,
        };
        if folder || is_exe(&name) {
            let name = name.to_string_lossy().into_owned();
            items.push(Item { name, path, folder });
        }
    }
    items.sort_by_cached_key(|item| (!item.folder, item.name.to_lowercase(), item.name.clone()));
    Ok(items)
}

fn is_exe(name: &OsStr) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
}

/// A dot name is hidden.
#[cfg(not(windows))]
fn is_hidden(_: &fs::DirEntry, name: &OsStr) -> bool {
    name.as_encoded_bytes().starts_with(b".")
}

/// An entry with the hidden attribute is hidden. `DirEntry::metadata` comes with the listing on
/// Windows, so this reads nothing more.
#[cfg(windows)]
fn is_hidden(entry: &fs::DirEntry, _: &OsStr) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    entry
        .metadata()
        .is_ok_and(|m| m.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0)
}

/// The place `folder` is in that is nearest to it, for the places column's mark and for Back.
fn place_of(places: &[Place], folder: &Path) -> Option<usize> {
    (0..places.len())
        .filter(|&i| folder.starts_with(&places[i].path))
        .max_by_key(|&i| places[i].path.components().count())
}

/// `folder` as "C:  ›  Program Files  ›  PCSX2": from the outermost place it is in, named by that
/// place's label, else the whole path.
fn breadcrumb(places: &[Place], folder: &Path) -> String {
    let outer = places
        .iter()
        .filter(|p| folder.starts_with(&p.path))
        .min_by_key(|p| p.path.components().count());
    let mut parts = Vec::new();
    let rest = match outer {
        Some(place) => {
            parts.push(place.label.clone());
            folder.strip_prefix(&place.path).unwrap_or(folder)
        }
        None => folder,
    };
    for component in rest.components() {
        match component {
            Component::Prefix(prefix) => parts.push(prefix.as_os_str().to_string_lossy().into_owned()),
            // The root after a drive (`C:\`) adds nothing to "C:".
            Component::RootDir if parts.is_empty() => parts.push("/".to_owned()),
            Component::Normal(name) => parts.push(name.to_string_lossy().into_owned()),
            _ => {}
        }
    }
    parts.join(CRUMB_SEP)
}

/// The current folder's listing, as far as it has arrived.
enum Listing {
    Loading,
    Failed,
    Ready(Vec<Item>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Column {
    Places,
    List,
}

pub struct ExeBrowser {
    title: String,
    usually: String,
    expected: String,
    in_setup: bool,
    on_choose: Option<OnChoose>,
    /// The folder asked to start in, if any; else the first place's.
    start: Option<PathBuf>,
    places: Vec<Place>,
    /// Whether `places` has arrived: the places column shows "Loading…" until it has.
    places_ready: bool,
    /// The discovery under way, if it has started; it starts on the next tick.
    places_pending: Option<Receiver<Vec<Place>>>,
    discover: Discover,
    /// True until `places` has arrived, when `start` was `None`: `open` needs the places to pick a
    /// folder to start in.
    awaiting_places: bool,
    /// The place last opened, or the one the start folder is in: Back from its own folder goes to
    /// the places column rather than further up.
    place: Option<usize>,
    column: Column,
    /// The focused place, while the places column has the focus.
    place_focus: usize,
    places_scroll: f32,
    folder: PathBuf,
    listing: Listing,
    /// The read of `folder` under way, if it has started; it starts on the next tick.
    pending: Option<Receiver<io::Result<Vec<Item>>>>,
    read: Read,
    /// The subfolder to focus once the listing arrives: the one Back came up from.
    select: Option<PathBuf>,
    /// The focused entry of the listing.
    focus: usize,
    scroll: f32,
    place_hits: Vec<(Rect, usize)>,
    item_hits: Vec<(Rect, usize)>,
}

impl ExeBrowser {
    /// Browses for `emulator`'s `.exe`, usually named `expected`, from `start` or else the first of
    /// this computer's places. `in_setup` is whether First-run Setup opens it, whose top bar shows
    /// no clock. Choosing an `.exe` calls `on_choose` with its path; cancelling doesn't.
    pub fn new(
        emulator: &str,
        expected: &str,
        start: Option<PathBuf>,
        in_setup: bool,
        on_choose: impl FnOnce(&mut Ctx, PathBuf) + 'static,
    ) -> ExeBrowser {
        let awaiting_places = start.is_none();
        let mut browser = ExeBrowser {
            title: format!("Choose {emulator}'s program"),
            usually: format!("Usually {expected}"),
            expected: expected.to_owned(),
            in_setup,
            on_choose: Some(Box::new(on_choose)),
            start: start.clone(),
            places: Vec::new(),
            places_ready: false,
            places_pending: None,
            discover: Arc::new(places),
            awaiting_places,
            place: None,
            column: Column::List,
            place_focus: 0,
            places_scroll: 0.0,
            folder: PathBuf::new(),
            listing: Listing::Loading,
            pending: None,
            read: Arc::new(read_folder),
            select: None,
            focus: 0,
            scroll: 0.0,
            place_hits: Vec::new(),
            item_hits: Vec::new(),
        };
        if let Some(start) = start {
            browser.open(start);
        }
        browser
    }

    /// Shows `places` instead of this computer's, synchronously: the seam tests and screenshot
    /// mode use.
    pub fn with_places(mut self, places: Vec<Place>) -> ExeBrowser {
        self.set_places(places);
        self
    }

    /// `places` has arrived (synchronously from `with_places`, or off the discovery thread):
    /// opens the start folder if `open` was waiting on it, and marks the place it's in.
    fn set_places(&mut self, places: Vec<Place>) {
        self.places = places;
        self.places_ready = true;
        self.places_pending = None;
        if self.awaiting_places {
            let first = self.places.first().map(|p| p.path.clone());
            self.open(self.start.clone().or(first).unwrap_or_default());
            self.awaiting_places = false;
        }
        self.place = place_of(&self.places, &self.folder);
        self.place_focus = self.place.unwrap_or(0);
    }

    /// Shows `folder`, its listing loading from the next tick on.
    fn open(&mut self, folder: PathBuf) {
        self.folder = folder;
        self.listing = Listing::Loading;
        // A read still under way for the folder before is dropped with its channel.
        self.pending = None;
        self.select = None;
        self.focus = 0;
        self.scroll = 0.0;
    }

    fn items(&self) -> &[Item] {
        match &self.listing {
            Listing::Ready(items) => items,
            _ => &[],
        }
    }

    fn focused_item(&self) -> Option<&Item> {
        self.items().get(self.focus)
    }

    /// Whether `item` is the file the emulator is usually started from, which is highlighted.
    fn is_expected(&self, item: &Item) -> bool {
        !item.folder && item.name.eq_ignore_ascii_case(&self.expected)
    }

    /// What the list shows instead of entries, if anything.
    fn list_message(&self) -> Option<&'static str> {
        match &self.listing {
            Listing::Loading => Some("Loading…"),
            Listing::Failed => Some("Can't open this folder"),
            Listing::Ready(items) if items.is_empty() => Some("No folders or .exe files here"),
            Listing::Ready(_) => None,
        }
    }

    /// Starts discovering the places if it hasn't started, and takes them once they arrive.
    fn poll_places(&mut self) {
        if !self.places_ready && self.places_pending.is_none() {
            let (send, receive) = mpsc::channel();
            let discover = self.discover.clone();
            thread::spawn(move || {
                // The browser may have moved on and dropped the receiver: nobody needs this then.
                let _ = send.send(discover());
            });
            self.places_pending = Some(receive);
        }
        let Some(pending) = &self.places_pending else {
            return;
        };
        let places = match pending.try_recv() {
            Ok(places) => places,
            Err(TryRecvError::Empty) => return,
            // The discovery thread is gone without an answer: show no places rather than hang.
            Err(TryRecvError::Disconnected) => Vec::new(),
        };
        self.set_places(places);
    }

    /// Starts reading the folder if it hasn't started, and takes the listing once it has arrived.
    fn poll(&mut self) {
        self.poll_places();
        if self.awaiting_places {
            // `open` needs the places first; `set_places` will call it once they arrive.
            return;
        }
        if matches!(self.listing, Listing::Loading) && self.pending.is_none() {
            let (send, receive) = mpsc::channel();
            let (read, dir) = (self.read.clone(), self.folder.clone());
            thread::spawn(move || {
                // The browser may have moved on and dropped the receiver: nobody needs this then.
                let _ = send.send(read(&dir));
            });
            self.pending = Some(receive);
        }
        let Some(pending) = &self.pending else {
            return;
        };
        let read = match pending.try_recv() {
            Ok(read) => read,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err(io::Error::other("the reading thread stopped")),
        };
        self.pending = None;
        self.arrive(read);
    }

    /// Shows a listing that has arrived, focused on the folder Back came up from, else on the
    /// usual file, else on the first entry.
    fn arrive(&mut self, read: io::Result<Vec<Item>>) {
        let items = match read {
            Ok(items) => items,
            Err(e) => {
                eprintln!("game37: can't read {}: {e}", self.folder.display());
                self.listing = Listing::Failed;
                return;
            }
        };
        let select = self.select.take();
        self.focus = select
            .and_then(|s| items.iter().position(|i| i.path == s))
            .or_else(|| items.iter().position(|i| self.is_expected(i)))
            .unwrap_or(0);
        self.listing = Listing::Ready(items);
        self.scroll = 0.0;
        self.follow();
    }

    /// Scrolls the list until the focused entry shows.
    fn follow(&mut self) {
        let start = self.focus as f32 * ROW_PITCH;
        let content = self.items().len() as f32 * ROW_PITCH - (ROW_PITCH - ROW_H);
        self.scroll = scroll::clamp(
            scroll::keep_visible(self.scroll, start, ROW_H, VIEW),
            content,
            VIEW,
        );
    }

    fn step(&mut self, dir: isize) {
        let move_in =
            |at: usize, len: usize| (at as isize + dir).clamp(0, len.saturating_sub(1) as isize) as usize;
        match self.column {
            Column::Places => {
                self.place_focus = move_in(self.place_focus, self.places.len());
                let start = self.place_focus as f32 * PLACE_PITCH;
                let content = self.places.len() as f32 * PLACE_PITCH - (PLACE_PITCH - PLACE_H);
                self.places_scroll = scroll::clamp(
                    scroll::keep_visible(self.places_scroll, start, PLACE_H, VIEW),
                    content,
                    VIEW,
                );
            }
            Column::List => {
                self.focus = move_in(self.focus, self.items().len());
                self.follow();
            }
        }
    }

    fn focus_places(&mut self) {
        if self.column == Column::List {
            self.column = Column::Places;
            self.place_focus = self.place.unwrap_or(self.place_focus);
        }
    }

    fn confirm(&mut self, ctx: &mut Ctx) -> Transition {
        match self.column {
            Column::Places => {
                if let Some(place) = self.places.get(self.place_focus) {
                    self.open(place.path.clone());
                    self.place = Some(self.place_focus);
                    self.column = Column::List;
                }
            }
            Column::List => match self.focused_item() {
                Some(item) if item.folder => self.open(item.path.clone()),
                Some(item) => {
                    let path = item.path.clone();
                    if let Some(on_choose) = self.on_choose.take() {
                        on_choose(ctx, path);
                    }
                    return Transition::Pop;
                }
                None => {}
            },
        }
        Transition::None
    }

    /// Up a folder, except from the place's own folder (or the top of the drive), which moves to
    /// the places; from the places, cancels.
    fn back(&mut self) -> Transition {
        if self.column == Column::Places {
            return Transition::Pop;
        }
        let at_place = self.place.is_some_and(|i| self.places[i].path == self.folder);
        match self.folder.parent() {
            Some(parent) if !at_place => {
                let child = self.folder.clone();
                self.open(parent.to_path_buf());
                self.select = Some(child);
            }
            _ => self.focus_places(),
        }
        Transition::None
    }

    /// Focuses what is under the pointer; whether anything was.
    fn hover(&mut self, x: f32, y: f32) -> bool {
        if let Some(i) = hit(&self.place_hits, x, y) {
            self.column = Column::Places;
            self.place_focus = i;
        } else if let Some(i) = hit(&self.item_hits, x, y) {
            self.column = Column::List;
            self.focus = i;
        } else {
            return false;
        }
        true
    }

    fn draw_places(&mut self, gfx: &mut Gfx<'_>) {
        let panel = Rect::new(M, TOP, PLACES_W, BOTTOM - TOP);
        draw_panel(gfx, panel);
        self.place_hits.clear();
        if !self.places_ready {
            let style = TextStyle::new(20, Weight::Medium, MUTED);
            gfx.text(
                M + 40.0,
                TOP + PAD + PLACE_H / 2.0 + 7.0,
                "Loading…",
                style,
                Align::Left,
            );
            return;
        }
        gfx.clip(Some(panel.inset(8.0)));
        for (i, place) in self.places.iter().enumerate() {
            let r = Rect::new(
                M + 12.0,
                TOP + PAD + i as f32 * PLACE_PITCH - self.places_scroll,
                376.0,
                PLACE_H,
            );
            if r.y + r.h < TOP || r.y > BOTTOM {
                continue;
            }
            let on = match self.column {
                Column::Places => i == self.place_focus,
                Column::List => Some(i) == self.place,
            };
            let style = if on {
                gfx.fill_round_rect(r, 12.0, SURFACE2);
                gfx.fill_round_rect(Rect::new(r.x, r.y + 8.0, 5.0, 32.0), 2.0, ACCENT);
                TextStyle::new(20, Weight::Bold, TEXT)
            } else if Some(i) == self.place {
                // The place the folder is in, while the focus is on another place.
                TextStyle::new(20, Weight::Bold, ACCENT)
            } else {
                TextStyle::new(20, Weight::Medium, MUTED)
            };
            let x = M + 40.0;
            let label = ellipsize(&place.label, r.x + r.w - 16.0 - x, |s| gfx.measure(s, style));
            gfx.text(x, r.y + 31.0, &label, style, Align::Left);
            self.place_hits.push((r, i));
        }
        gfx.clip(None);
    }

    fn draw_list(&mut self, gfx: &mut Gfx<'_>) {
        let panel = Rect::new(LX, TOP, LW, BOTTOM - TOP);
        draw_panel(gfx, panel);
        self.item_hits.clear();
        if let Some(message) = self.list_message() {
            let style = TextStyle::new(20, Weight::Medium, MUTED);
            gfx.text(
                LX + 40.0,
                TOP + PAD + ROW_H / 2.0 + 7.0,
                message,
                style,
                Align::Left,
            );
            return;
        }
        let content = self.items().len() as f32 * ROW_PITCH - (ROW_PITCH - ROW_H);
        self.scroll = scroll::clamp(self.scroll, content, VIEW);
        // Only the rows in view, so a folder of thousands draws as fast as a short one.
        let first = (self.scroll / ROW_PITCH).floor() as usize;
        let last = (((self.scroll + VIEW) / ROW_PITCH).ceil() as usize + 1).min(self.items().len());
        gfx.clip(Some(panel.inset(8.0)));
        for i in first..last {
            let item = &self.items()[i];
            let r = Rect::new(
                LX + 12.0,
                TOP + PAD + i as f32 * ROW_PITCH - self.scroll,
                LW - 24.0,
                ROW_H,
            );
            let focused = self.column == Column::List && i == self.focus;
            if focused {
                gfx.fill_round_rect(r, 12.0, SURFACE2);
                gfx.fill_round_rect(Rect::new(r.x, r.y + r.h / 2.0 - 20.0, 5.0, 40.0), 2.0, ACCENT);
            }
            if item.folder {
                folder_icon(gfx, LX + 40.0, r.y + 15.0, MUTED);
                let chevron = TextStyle::new(22, Weight::SemiBold, MUTED);
                gfx.text(LX + LW - 32.0, r.y + r.h / 2.0 + 7.0, "›", chevron, Align::Right);
            } else {
                exe_icon(gfx, LX + 40.0, r.y + 13.0, ACCENT);
            }
            let expected = self.is_expected(item);
            let weight = if expected || focused {
                Weight::Bold
            } else {
                Weight::Medium
            };
            let style = TextStyle::new(20, weight, if expected { ACCENT } else { TEXT });
            let x = LX + 86.0;
            let name = ellipsize(&item.name, LX + LW - 64.0 - x, |s| gfx.measure(s, style));
            gfx.text(x, r.y + r.h / 2.0 + 7.0, &name, style, Align::Left);
            self.item_hits.push((r, i));
        }
        gfx.clip(None);
    }
}

fn draw_panel(gfx: &mut Gfx<'_>, r: Rect) {
    gfx.fill_round_rect(r, 14.0, SURFACE);
    gfx.stroke_round_rect(r, 14.0, 1.0, LINE);
}

/// A folder glyph with its top-left at (x, y): a tab over a body, as `folder_icon` in the mockups.
fn folder_icon(gfx: &mut Gfx<'_>, x: f32, y: f32, ink: Colour) {
    let s = ICON;
    let tab = [
        (x, y + s * 0.18),
        (x + s * 0.38, y + s * 0.18),
        (x + s * 0.48, y + s * 0.32),
        (x, y + s * 0.32),
    ];
    gfx.fill_polygon(&tab, ink);
    gfx.fill_rect(Rect::new(x, y + s * 0.32, s, s * 0.68), ink);
}

/// A document glyph with a folded corner for an `.exe`, its top-left at (x, y), as `exe_icon` in
/// the mockups.
fn exe_icon(gfx: &mut Gfx<'_>, x: f32, y: f32, ink: Colour) {
    let (s, c) = (ICON, ICON * 0.22);
    let outline = [
        (x, y),
        (x + s - c, y),
        (x + s, y + c),
        (x + s, y + s),
        (x, y + s),
        (x, y),
    ];
    for pair in outline.windows(2) {
        gfx.line(pair[0].0, pair[0].1, pair[1].0, pair[1].1, 2.0, ink);
    }
    gfx.line(x + s - c, y, x + s - c, y + c, 2.0, ink);
    gfx.line(x + s - c, y + c, x + s, y + c, 2.0, ink);
}

impl Screen for ExeBrowser {
    fn title(&self) -> &str {
        &self.title
    }

    fn handle(&mut self, input: Input, ctx: &mut Ctx) -> Transition {
        match input {
            Input::Action(Action::Up) => self.step(-1),
            Input::Action(Action::Down) => self.step(1),
            Input::Action(Action::Left) => self.focus_places(),
            Input::Action(Action::Right) => self.column = Column::List,
            Input::Action(Action::Confirm) => return self.confirm(ctx),
            Input::Action(Action::Back) => return self.back(),
            // X on a Nintendo-style controller, as in the mockup.
            Input::Action(Action::Sort) => return Transition::Pop,
            Input::Pointer(Pointer::Move { x, y }) => {
                self.hover(x, y);
            }
            Input::Pointer(Pointer::Click { x, y }) if self.hover(x, y) => return self.confirm(ctx),
            Input::Pointer(Pointer::Wheel { dy }) => {
                let content = self.items().len() as f32 * ROW_PITCH - (ROW_PITCH - ROW_H);
                self.scroll = scroll::clamp(self.scroll - dy * ROW_PITCH, content, VIEW);
            }
            _ => {}
        }
        Transition::None
    }

    fn tick(&mut self, _: &mut Ctx) -> Transition {
        self.poll();
        Transition::None
    }

    fn busy(&self) -> bool {
        !self.places_ready || matches!(self.listing, Listing::Loading)
    }

    fn draw(&mut self, gfx: &mut Gfx<'_>, _: &Ctx) {
        gfx.text(
            M,
            196.0,
            &self.title,
            TextStyle::new(44, Weight::ExtraBold, TEXT),
            Align::Left,
        );
        let muted = TextStyle::new(20, Weight::Medium, MUTED);
        gfx.text(M, 232.0, &self.usually, muted, Align::Left);
        // A long path keeps the folder it ends in.
        let crumb = trim_start(&breadcrumb(&self.places, &self.folder), W - 2.0 * M, |s| {
            gfx.measure(s, muted)
        });
        gfx.text(M, 280.0, &crumb, muted, Align::Left);
        self.draw_places(gfx);
        self.draw_list(gfx);
    }

    fn hints(&self) -> Vec<Hint> {
        let mut hints = Vec::new();
        match self.column {
            Column::Places => {
                if !self.places.is_empty() {
                    hints.push(Hint::new(&[Action::Confirm], "Open"));
                }
                hints.push(Hint::new(&[Action::Back], "Cancel"));
            }
            Column::List => {
                if let Some(item) = self.focused_item() {
                    let verb = if item.folder { "Open" } else { "Choose" };
                    hints.push(Hint::new(&[Action::Confirm], verb));
                }
                hints.push(Hint::new(&[Action::Back], "Up"));
                hints.push(Hint::new(&[Action::Sort], "Cancel"));
            }
        }
        hints
    }

    /// First-run Setup's top bar has no clock: Setup shows its progress there.
    fn clock(&self) -> bool {
        !self.in_setup
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    fn names(items: &[Item]) -> Vec<&str> {
        items.iter().map(|i| i.name.as_str()).collect()
    }

    /// `folders` and empty `files` made under a fresh temp dir.
    fn tree(folders: &[&str], files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for f in folders {
            fs::create_dir_all(dir.path().join(f)).unwrap();
        }
        for f in files {
            fs::write(dir.path().join(f), b"").unwrap();
        }
        dir
    }

    type Chosen = Rc<RefCell<Vec<PathBuf>>>;

    /// A browser for PCSX2 at `start`, with one place, "Root", at `root`.
    fn browser(root: &Path, start: &Path) -> (ExeBrowser, Chosen) {
        let chosen: Chosen = Rc::default();
        let log = chosen.clone();
        let b = ExeBrowser::new(
            "PCSX2",
            "pcsx2-qt.exe",
            Some(start.to_owned()),
            false,
            move |_, p| log.borrow_mut().push(p),
        )
        .with_places(vec![Place::new("Root", root)]);
        (b, chosen)
    }

    /// Ticks until the listing has arrived.
    fn settle(b: &mut ExeBrowser, ctx: &mut Ctx) {
        let deadline = Instant::now() + Duration::from_secs(5);
        b.tick(ctx);
        while b.busy() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(1));
            b.tick(ctx);
        }
        assert!(!b.busy(), "the listing never arrived");
    }

    fn act(b: &mut ExeBrowser, ctx: &mut Ctx, action: Action) -> Transition {
        b.handle(Input::Action(action), ctx)
    }

    #[test]
    fn only_folders_and_exe_files_show_whatever_the_case_hidden_ones_left_out() {
        let dir = tree(
            &["bin", ".cache", "tools.d"],
            &[
                "a.EXE",
                "b.exe",
                "c.Exe",
                "readme.txt",
                ".secret.exe",
                "exe",
                "setup.exe.bak",
            ],
        );
        // Windows hides by the hidden attribute, not by a leading dot.
        #[cfg(windows)]
        for name in [".cache", ".secret.exe"] {
            let status = std::process::Command::new("attrib")
                .arg("+h")
                .arg(dir.path().join(name))
                .status()
                .unwrap();
            assert!(status.success(), "attrib +h {name}");
        }
        let items = read_folder(dir.path()).unwrap();
        assert_eq!(names(&items), ["bin", "tools.d", "a.EXE", "b.exe", "c.Exe"]);
        assert!(items[0].folder && !items[2].folder);
        assert_eq!(items[2].path, dir.path().join("a.EXE"));
    }

    #[test]
    fn folders_come_first_then_files_each_sorted_case_insensitively() {
        let dir = tree(&["beta", "Alpha", "gamma"], &["Zed.exe", "apple.exe", "Mid.exe"]);
        let items = read_folder(dir.path()).unwrap();
        assert_eq!(
            names(&items),
            ["Alpha", "beta", "gamma", "apple.exe", "Mid.exe", "Zed.exe"]
        );
    }

    #[test]
    fn confirm_opens_a_folder_and_chooses_an_exe_once() {
        let ctx = &mut Ctx::empty();
        let dir = tree(&["PCSX2/bin"], &["PCSX2/bin/tool.exe"]);
        let (mut b, chosen) = browser(dir.path(), dir.path());
        settle(&mut b, ctx);
        assert_eq!(b.hints()[0], Hint::new(&[Action::Confirm], "Open"));
        assert!(matches!(act(&mut b, ctx, Action::Confirm), Transition::None));
        settle(&mut b, ctx);
        assert_eq!(b.folder, dir.path().join("PCSX2"));
        act(&mut b, ctx, Action::Confirm);
        settle(&mut b, ctx);
        assert_eq!(b.hints()[0], Hint::new(&[Action::Confirm], "Choose"));
        assert!(chosen.borrow().is_empty());
        assert!(matches!(act(&mut b, ctx, Action::Confirm), Transition::Pop));
        assert_eq!(*chosen.borrow(), [dir.path().join("PCSX2/bin/tool.exe")]);
    }

    #[test]
    fn a_click_opens_a_folder_or_chooses_an_exe() {
        let ctx = &mut Ctx::empty();
        let dir = tree(&["sub"], &["run.exe"]);
        let (mut b, chosen) = browser(dir.path(), dir.path());
        settle(&mut b, ctx);
        b.item_hits = vec![
            (Rect::new(0.0, 0.0, 50.0, 50.0), 0),
            (Rect::new(0.0, 60.0, 50.0, 50.0), 1),
        ];
        let click = |y| Input::Pointer(Pointer::Click { x: 10.0, y });
        assert!(matches!(b.handle(click(70.0), ctx), Transition::Pop));
        assert_eq!(*chosen.borrow(), [dir.path().join("run.exe")]);
        let (mut b, _) = browser(dir.path(), dir.path());
        settle(&mut b, ctx);
        b.item_hits = vec![(Rect::new(0.0, 0.0, 50.0, 50.0), 0)];
        b.handle(click(10.0), ctx);
        assert_eq!(b.folder, dir.path().join("sub"));
    }

    #[test]
    fn back_goes_up_then_from_the_places_folder_to_the_places_then_cancels() {
        let ctx = &mut Ctx::empty();
        let dir = tree(&["a", "b/c"], &[]);
        let (mut b, chosen) = browser(dir.path(), &dir.path().join("b/c"));
        settle(&mut b, ctx);
        assert_eq!(b.hints()[0], Hint::new(&[Action::Back], "Up"), "c is empty");
        act(&mut b, ctx, Action::Back);
        settle(&mut b, ctx);
        assert_eq!(b.folder, dir.path().join("b"));
        act(&mut b, ctx, Action::Back);
        settle(&mut b, ctx);
        assert_eq!(b.folder, dir.path());
        assert_eq!(
            b.focused_item().unwrap().name,
            "b",
            "focused on the folder it came up from"
        );
        assert!(matches!(act(&mut b, ctx, Action::Back), Transition::None));
        assert_eq!((b.folder.as_path(), b.column), (dir.path(), Column::Places));
        assert_eq!(b.hints()[1], Hint::new(&[Action::Back], "Cancel"));
        assert!(matches!(act(&mut b, ctx, Action::Back), Transition::Pop));
        assert!(chosen.borrow().is_empty());
    }

    #[test]
    fn x_cancels_without_choosing() {
        let ctx = &mut Ctx::empty();
        let dir = tree(&[], &["pcsx2-qt.exe"]);
        let (mut b, chosen) = browser(dir.path(), dir.path());
        settle(&mut b, ctx);
        assert!(matches!(act(&mut b, ctx, Action::Sort), Transition::Pop));
        assert!(chosen.borrow().is_empty());
    }

    #[test]
    fn left_and_right_move_between_the_places_and_the_list() {
        let ctx = &mut Ctx::empty();
        let one = tree(&["x"], &[]);
        let two = tree(&[], &["y.exe"]);
        let (b, _) = browser(one.path(), one.path());
        let mut b = b.with_places(vec![Place::new("One", one.path()), Place::new("Two", two.path())]);
        settle(&mut b, ctx);
        act(&mut b, ctx, Action::Left);
        assert_eq!((b.column, b.place_focus), (Column::Places, 0));
        act(&mut b, ctx, Action::Down);
        act(&mut b, ctx, Action::Right);
        assert_eq!(b.column, Column::List);
        assert_eq!(b.folder, one.path(), "Right only moves the focus");
        act(&mut b, ctx, Action::Left);
        act(&mut b, ctx, Action::Down);
        act(&mut b, ctx, Action::Confirm);
        settle(&mut b, ctx);
        assert_eq!((b.column, b.place), (Column::List, Some(1)));
        assert_eq!(names(b.items()), ["y.exe"]);
    }

    #[test]
    fn the_usual_file_is_highlighted_and_focused_in_any_case() {
        let ctx = &mut Ctx::empty();
        let dir = tree(&["bin"], &["PCSX2-Qt.EXE", "updater.exe"]);
        let (mut b, _) = browser(dir.path(), dir.path());
        settle(&mut b, ctx);
        let marked: Vec<bool> = b.items().iter().map(|i| b.is_expected(i)).collect();
        assert_eq!(marked, [false, true, false]);
        assert_eq!(b.focus, 1);
    }

    #[test]
    fn an_unreadable_folder_says_so_and_back_still_goes_up() {
        let ctx = &mut Ctx::empty();
        let dir = tree(&[], &["a.exe"]);
        let (mut b, _) = browser(dir.path(), &dir.path().join("gone"));
        settle(&mut b, ctx);
        assert_eq!(b.list_message(), Some("Can't open this folder"));
        act(&mut b, ctx, Action::Back);
        settle(&mut b, ctx);
        assert_eq!(b.list_message(), None);
        assert_eq!(names(b.items()), ["a.exe"]);
    }

    #[test]
    fn it_shows_loading_until_the_read_on_its_own_thread_arrives() {
        let ctx = &mut Ctx::empty();
        let dir = tree(&[], &["a.exe"]);
        let (open, gate) = mpsc::channel::<()>();
        let gate = Mutex::new(gate);
        // A read on the UI thread would wait out the gate here, then fail.
        let read: Read = Arc::new(move |dir| {
            let opened = gate.lock().unwrap().recv_timeout(Duration::from_secs(5));
            opened.map_err(io::Error::other)?;
            read_folder(dir)
        });
        let (mut b, _) = browser(dir.path(), dir.path());
        b.read = read;
        b.tick(ctx);
        b.tick(ctx);
        assert_eq!(b.list_message(), Some("Loading…"));
        assert!(b.busy());
        open.send(()).unwrap();
        settle(&mut b, ctx);
        assert_eq!(names(b.items()), ["a.exe"]);
    }

    #[test]
    fn slow_place_discovery_runs_off_the_ui_thread_and_shows_loading_meanwhile() {
        let ctx = &mut Ctx::empty();
        let dir = tree(&[], &["a.exe"]);
        let (open, gate) = mpsc::channel::<()>();
        let gate = Mutex::new(gate);
        // Discovery on the UI thread (e.g. a disconnected network drive) would wait out the gate
        // here, stalling `new` and every tick until it timed out.
        let discover: Discover = Arc::new(move || {
            gate.lock().unwrap().recv_timeout(Duration::from_secs(5)).unwrap();
            Vec::new()
        });
        let started = Instant::now();
        let mut b = ExeBrowser::new(
            "PCSX2",
            "pcsx2-qt.exe",
            Some(dir.path().to_owned()),
            false,
            |_, _| {},
        );
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "new() waited on place discovery"
        );
        b.discover = discover;
        let ticked = Instant::now();
        b.tick(ctx);
        assert!(
            ticked.elapsed() < Duration::from_millis(500),
            "tick() waited on place discovery"
        );
        b.tick(ctx);
        assert!(!b.places_ready, "the gate is still shut");
        assert!(b.busy());
        // The folder listing, read independently, still arrives while the places are loading.
        let listed = Instant::now() + Duration::from_secs(5);
        while matches!(b.listing, Listing::Loading) && Instant::now() < listed {
            thread::sleep(Duration::from_millis(1));
            b.tick(ctx);
        }
        assert_eq!(names(b.items()), ["a.exe"]);
        open.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !b.places_ready && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(1));
            b.tick(ctx);
        }
        assert!(b.places_ready, "the places never arrived");
        assert!(!b.busy());
    }

    #[test]
    fn the_breadcrumb_starts_at_the_outermost_place() {
        let places = [
            Place::new("C:", "/fake/C:"),
            Place::new("Program Files", "/fake/C:/Program Files"),
        ];
        let folder = Path::new("/fake/C:/Program Files/PCSX2");
        assert_eq!(breadcrumb(&places, folder), "C:  ›  Program Files  ›  PCSX2");
        assert_eq!(place_of(&places, folder), Some(1));
        assert_eq!(breadcrumb(&[], Path::new("/a/b")), "/  ›  a  ›  b");
    }
}
