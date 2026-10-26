//! The Options Menu for one Game (FR-DET-3, FR-DET-4), after `screen_options` in
//! `design/generate_mockups.py`: a panel on the right over the dimmed screen. With five Favorites
//! already, adding a sixth shows a picker of them in the same panel (FR-HOME-6, US-6). Rename
//! opens a wider panel with a text field and an on-screen keyboard (US-3): the menu stays one
//! modal, so saving closes it like every other action. Loading a Save State shows a picker of
//! the Game's slots in the same panel, each with its screenshot and time (FR-EMB-5), and starts
//! the Game from the one chosen. Delete save data shows what the Game has in the same panel, and
//! deletes the part chosen once a confirmation says so; the menu then shows what is left. Choose
//! artwork opens the artwork picker over the menu, its own modal, which goes back to the menu when
//! done.

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use crate::ctx::{self, Ctx, Play, Start};
use crate::launch;
use crate::library::db::GameRow;
use crate::screens::artwork_picker::ArtworkPicker;
use crate::screens::confirm::Confirm;
use crate::screens::message::Message;
use crate::session::states::{self, Rgb, SLOTS, SaveData, StatePaths};
use crate::session::{saves, sdl};
use crate::systems::{self, Kind};
use crate::ui::cover::{self, ellipsize, playtime_label};
use crate::ui::gfx::{Align, Gfx, TextStyle};
use crate::ui::input::{Action, Input, InputKind, Pointer};
use crate::ui::nav::{Hint, Screen, Transition};
use crate::ui::text::Weight;
use crate::ui::text_entry::{KEYBOARD_H, KEYBOARD_W, Outcome, TextEntry};
use crate::ui::theme::{ACCENT, Colour, DIM, LINE, MUTED, STAR, SURFACE, SURFACE2, TEXT};
use crate::ui::{H, Rect, W, hit};

/// At most this many Favorites exist (FR-HOME-6).
pub const MAX_FAVORITES: usize = 5;

const PANEL_W: f32 = 600.0;
const PANEL_X: f32 = W - PANEL_W;
const ITEM_Y: f32 = 252.0;
const ITEM_PITCH: f32 = 86.0;
const PICK_Y: f32 = 290.0;
const PICK_PITCH: f32 = 112.0;
/// The rename panel, centred above the hint bar.
const RENAME_W: f32 = 1240.0;
const RENAME_H: f32 = 768.0;
const RENAME_X: f32 = (W - RENAME_W) / 2.0;
const RENAME_Y: f32 = 120.0;

/// The longest Display Title typing can make, in characters.
pub const MAX_TITLE_CHARS: usize = 100;

const SLOT_Y: f32 = 290.0;
const SLOT_PITCH: f32 = 176.0;
const SLOT_ROW_H: f32 = 160.0;
/// A slot's screenshot, 4:3 like the Pause Menu's.
const THUMB_W: f32 = 176.0;
const THUMB_H: f32 = 132.0;
const THUMB_KEYS: [&str; SLOTS] = ["options-slot-1", "options-slot-2", "options-slot-3"];
/// The delete view's rows, in order.
const DELETE_ROWS: [SaveData; 4] = [
    SaveData::ResumeState,
    SaveData::SaveStates,
    SaveData::InGameSave,
    SaveData::Everything,
];

/// One action in the menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    /// Add to or remove from Favorites.
    Favorite,
    /// Load one of the Game's Save States, with a slot picker; Embedded Systems only, and only
    /// selectable when a slot is filled.
    LoadSaveState,
    /// Play the Game from power-on, without its Resume State; Embedded Systems only (FR-DET-4).
    StartFromPowerOn,
    /// Delete the Game's Resume State, Save States, In-game Save or all of it, in a view of its
    /// own; Embedded Systems only, and only selectable when the Game has any (FR-DET-4).
    DeleteSaveData,
    /// Choose different Artwork from SteamGridDB, in the artwork picker (FR-ART-2).
    ChooseArtwork,
    /// Rename the Display Title; the file on disk stays as it is (FR-DET-5).
    Rename,
    Hide,
    ShowFile,
}

impl Item {
    fn label(self, favorite: bool) -> &'static str {
        match self {
            Item::Favorite if favorite => "Remove from favorites",
            Item::Favorite => "Add to favorites",
            Item::LoadSaveState => "Load save state",
            Item::StartFromPowerOn => "Start from power-on",
            Item::DeleteSaveData => "Delete save data",
            Item::ChooseArtwork => "Choose artwork",
            Item::Rename => "Rename",
            Item::Hide => "Hide game",
            Item::ShowFile => "Show file in file manager",
        }
    }

    /// Whether choosing it opens a further view, shown with a "›".
    fn opens_view(self) -> bool {
        matches!(
            self,
            Item::LoadSaveState | Item::DeleteSaveData | Item::ChooseArtwork
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    List,
    /// Five Favorites exist: choose which one the Game replaces.
    Replace,
    /// Typing a new Display Title.
    Rename,
    /// Choose the Save State slot to start the Game from.
    Load,
    /// Choose which save data to delete.
    Delete,
}

/// A filled Save State slot as the slot picker shows it.
struct Slot {
    when: String,
    has_thumb: bool,
}

/// A slot as read on a thread: `None` when empty, else its time and screenshot.
type ReadSlot = Option<(Slot, Option<Rgb>)>;

pub struct OptionsMenu {
    game: GameRow,
    /// "<System> · <Playtime> played".
    meta: String,
    items: Vec<Item>,
    view: View,
    item: usize,
    /// Every Favorite, newest first, with whether its file is there, while the replace picker is
    /// open.
    favorites: Vec<(GameRow, bool)>,
    /// Whether each Save State slot is filled, slot 1 first; none for an External System.
    filled: Vec<bool>,
    /// When the Resume State was written, if there is one.
    resume_when: Option<String>,
    /// When the In-game Save was written, if there is one.
    srm_when: Option<String>,
    /// Where the Game's Save States are.
    states: StatePaths,
    /// The Game's In-game Save.
    srm: PathBuf,
    /// What a confirmed delete did, left by the confirmation for the next tick to show.
    deleted: Rc<RefCell<Option<Result<(), String>>>>,
    /// The slots' times and screenshots, slot 1 first, `None` when empty: read on a thread when
    /// the slot picker first opens (NFR-3), and empty until then.
    slots: Vec<Option<Slot>>,
    /// The slots being read.
    reading: Option<Receiver<Vec<ReadSlot>>>,
    /// Slot screenshots to hand to `Gfx` on the first draw: `None` forgets an old one.
    uploads: Vec<(usize, Option<Rgb>)>,
    /// The focused entry of the replace or slot picker.
    pick: usize,
    item_hits: Vec<(Rect, usize)>,
    pick_hits: Vec<(Rect, usize)>,
    /// The new Display Title being typed, while renaming.
    entry: Option<TextEntry>,
    /// The cleaned-up title a rename overrides, while renaming.
    cleaned: String,
    /// What the player last used, as of the last frame, for the rename hints.
    kind: InputKind,
}

impl OptionsMenu {
    /// The menu for `game`, its Save State slots read from under `games_root`.
    pub fn new(game: GameRow, games_root: &Path) -> OptionsMenu {
        let system = systems::by_id(&game.system);
        let short = system.map_or("?", |s| s.short);
        let meta = match game.playtime_secs {
            0 => short.to_owned(),
            secs => format!("{short}  ·  {} played", playtime_label(secs)),
        };
        let embedded = system.is_some_and(|s| s.kind == Kind::Embedded);
        let items = [
            Item::Favorite,
            Item::LoadSaveState,
            Item::StartFromPowerOn,
            Item::DeleteSaveData,
            Item::ChooseArtwork,
            Item::Rename,
            Item::Hide,
            Item::ShowFile,
        ]
        .into_iter()
        .filter(|&i| {
            embedded
                || !matches!(
                    i,
                    Item::LoadSaveState | Item::StartFromPowerOn | Item::DeleteSaveData
                )
        })
        .collect();
        let rel_path = Path::new(&game.rel_path);
        let states = StatePaths::new(games_root, &game.system, rel_path);
        let srm = saves::srm_path(games_root, &game.system, rel_path);
        let (filled, resume_when, srm_when) = if embedded {
            read_save_data(&states, &srm)
        } else {
            (Vec::new(), None, None)
        };
        OptionsMenu {
            game,
            meta,
            items,
            view: View::List,
            item: 0,
            favorites: Vec::new(),
            filled,
            resume_when,
            srm_when,
            states,
            srm,
            deleted: Rc::default(),
            slots: Vec::new(),
            reading: None,
            uploads: Vec::new(),
            pick: 0,
            item_hits: Vec::new(),
            pick_hits: Vec::new(),
            entry: None,
            cleaned: String::new(),
            kind: InputKind::Controller,
        }
    }

    /// Whether `item` can be chosen; the others are greyed and skipped by focus.
    pub fn enabled(&self, item: Item) -> bool {
        match item {
            Item::LoadSaveState => self.filled() > 0,
            Item::DeleteSaveData => self.has(SaveData::Everything),
            Item::Favorite
            | Item::StartFromPowerOn
            | Item::ChooseArtwork
            | Item::Rename
            | Item::Hide
            | Item::ShowFile => true,
        }
    }

    /// Whether the Game has `what` to delete; `Everything` when it has any of it.
    fn has(&self, what: SaveData) -> bool {
        match what {
            SaveData::ResumeState => self.resume_when.is_some(),
            SaveData::SaveStates => self.filled() > 0,
            SaveData::InGameSave => self.srm_when.is_some(),
            SaveData::Everything => DELETE_ROWS[..3].iter().any(|&w| self.has(w)),
        }
    }

    /// The line under a delete view row: when it was written, how many slots are filled, or
    /// "None"; none under Everything.
    fn delete_sub(&self, what: SaveData) -> Option<String> {
        let none = || "None".to_owned();
        match what {
            SaveData::ResumeState => Some(self.resume_when.clone().unwrap_or_else(none)),
            SaveData::SaveStates => Some(match self.filled() {
                0 => none(),
                n => format!("{n} of {SLOTS}"),
            }),
            SaveData::InGameSave => Some(self.srm_when.clone().unwrap_or_else(none)),
            SaveData::Everything => None,
        }
    }

    /// The confirmation before deleting `what`, which leaves what it did for [`Self::after_delete`].
    fn confirm_delete(&self, what: SaveData) -> Confirm {
        let (title, body) = match what {
            SaveData::ResumeState => (
                "Delete the Resume State?".to_owned(),
                "The next launch starts from power-on. Your In-game Save and Save States stay.",
            ),
            SaveData::SaveStates => (
                match self.filled() {
                    1 => "Delete the Save State?".to_owned(),
                    n => format!("Delete {n} Save States?"),
                },
                "They are deleted for good, with their screenshots.",
            ),
            SaveData::InGameSave => (
                "Delete the In-game Save?".to_owned(),
                "Your in-game progress is deleted for good, and so is the Resume State. The next \
                 launch starts a new save file.",
            ),
            SaveData::Everything => (
                "Delete all save data?".to_owned(),
                "The In-game Save, the Resume State and every Save State are deleted for good. The \
                 next launch starts a new save file.",
            ),
        };
        let (states, srm, deleted) = (self.states.clone(), self.srm.clone(), self.deleted.clone());
        Confirm::new(title, body, "Delete", move |ctx| {
            *deleted.borrow_mut() = Some(states::delete(what, &states, &srm));
            // The Details Page's Save State count reloads.
            ctx.revision += 1;
        })
    }

    /// Once a confirmed delete is done: re-reads what the Game has, goes back to the list when
    /// nothing is left to delete, and says so when something couldn't be deleted.
    fn after_delete(&mut self) -> Transition {
        let Some(done) = self.deleted.borrow_mut().take() else {
            return Transition::None;
        };
        (self.filled, self.resume_when, self.srm_when) = read_save_data(&self.states, &self.srm);
        // The slot picker reads its slots afresh, and forgets the old screenshots.
        self.reading = None;
        self.slots.clear();
        self.uploads = (0..SLOTS).map(|i| (i, None)).collect();
        if !self.has(SaveData::Everything) {
            self.back_to_list();
        } else if !self.pickable(self.pick) {
            self.pick = self.first_pickable();
        }
        if !self.enabled(self.focused()) {
            self.move_item(-1);
        }
        match done {
            Ok(()) => Transition::None,
            Err(e) => Transition::Push(Box::new(Message::new("Couldn't delete save data", e))),
        }
    }

    /// How many Save State slots are filled.
    fn filled(&self) -> usize {
        self.filled.iter().filter(|&&f| f).count()
    }

    /// Reads the slots' times and screenshots on a thread, once.
    fn read_slots(&mut self) {
        if !self.slots.is_empty() || self.reading.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let states = self.states.clone();
        std::thread::spawn(move || {
            let slots = sdl::slot_infos(&states).into_iter().map(|info| {
                info.map(|info| {
                    let slot = Slot {
                        when: info.when,
                        has_thumb: info.thumb.is_some(),
                    };
                    (slot, info.thumb)
                })
            });
            let _ = tx.send(slots.collect());
        });
        self.reading = Some(rx);
    }

    /// Takes the slots once read, with their screenshots to upload.
    fn poll_slots(&mut self) {
        let Some(rx) = &self.reading else { return };
        match rx.try_recv() {
            Ok(read) => {
                self.reading = None;
                (self.slots, self.uploads) = read
                    .into_iter()
                    .enumerate()
                    .map(|(i, slot)| match slot {
                        Some((slot, thumb)) => (Some(slot), (i, thumb)),
                        None => (None, (i, None)),
                    })
                    .unzip();
            }
            Err(TryRecvError::Disconnected) => self.reading = None,
            Err(TryRecvError::Empty) => {}
        }
    }

    /// The focused entry of the replace or slot picker.
    pub fn picked(&self) -> usize {
        self.pick
    }

    pub fn items(&self) -> &[Item] {
        &self.items
    }

    pub fn view(&self) -> View {
        self.view
    }

    pub fn focused(&self) -> Item {
        self.items[self.item]
    }

    /// Moves focus one step `dir` (-1 or 1), skipping actions that can't be chosen.
    fn move_item(&mut self, dir: isize) {
        let mut i = self.item as isize + dir;
        while (0..self.items.len() as isize).contains(&i) {
            if self.enabled(self.items[i as usize]) {
                self.item = i as usize;
                return;
            }
            i += dir;
        }
    }

    /// Whether the open picker's `i`th entry can be chosen: any Favorite, a filled slot, save
    /// data the Game has.
    fn pickable(&self, i: usize) -> bool {
        match self.view {
            View::List | View::Rename => false,
            View::Replace => i < self.favorites.len(),
            View::Load => self.filled.get(i).copied().unwrap_or(false),
            View::Delete => DELETE_ROWS.get(i).is_some_and(|&what| self.has(what)),
        }
    }

    /// How many entries the open picker has.
    fn pick_len(&self) -> usize {
        match self.view {
            View::List | View::Rename => 0,
            View::Replace => self.favorites.len(),
            View::Load => self.filled.len(),
            View::Delete => DELETE_ROWS.len(),
        }
    }

    /// The open picker's first entry that can be chosen, else 0.
    fn first_pickable(&self) -> usize {
        (0..self.pick_len()).find(|&i| self.pickable(i)).unwrap_or(0)
    }

    /// Moves picker focus one step `dir` (-1 or 1), skipping entries that can't be chosen.
    fn move_pick(&mut self, dir: isize) {
        let len = self.pick_len();
        let mut i = self.pick as isize + dir;
        while (0..len as isize).contains(&i) {
            if self.pickable(i as usize) {
                self.pick = i as usize;
                return;
            }
            i += dir;
        }
    }

    /// Chooses the open picker's `i`th entry, if it can be chosen.
    fn pick_at(&mut self, i: usize, ctx: &mut Ctx) -> Transition {
        if !self.pickable(i) {
            return Transition::None;
        }
        match self.view {
            View::Replace => self.replace(i, ctx),
            View::Load => self.play(Start::Slot(i + 1), ctx),
            View::Delete => Transition::Push(Box::new(self.confirm_delete(DELETE_ROWS[i]))),
            View::List | View::Rename => Transition::None,
        }
    }

    /// Starts the Game from `start`: a chosen slot's Save State or power-on (FR-DET-4). The menu
    /// closes; a Core download or a missing BIOS message takes its place.
    fn play(&mut self, start: Start, ctx: &mut Ctx) -> Transition {
        let play = Play {
            game: self.game.clone(),
            start,
        };
        match launch::confirm_play(play, ctx) {
            Transition::None => Transition::Pop,
            Transition::Push(screen) => Transition::Replace(screen),
            other => other,
        }
    }

    fn choose(&mut self, i: usize, ctx: &mut Ctx) -> Transition {
        let item = self.items[i];
        if !self.enabled(item) {
            return Transition::None;
        }
        self.item = i;
        let id = self.game.id;
        let done = match item {
            Item::Favorite if self.game.favorite => ctx.library.set_favorite(id, false, ctx::now()),
            // Absent Favorites count too: they come back with their file.
            Item::Favorite => match ctx.library.all_favorites() {
                Ok(favorites) if favorites.len() >= MAX_FAVORITES => {
                    self.favorites = favorites;
                    self.pick = 0;
                    self.view = View::Replace;
                    return Transition::None;
                }
                Ok(_) => ctx.library.set_favorite(id, true, ctx::now()),
                Err(e) => Err(e),
            },
            // A Hidden Game is no Favorite: it would count towards the five unseen.
            Item::Hide => ctx
                .library
                .set_hidden(id, true)
                .and_then(|()| ctx.library.set_favorite(id, false, ctx::now())),
            Item::ShowFile => {
                ctx.show_file_request = Some(ctx.games_root.join(&self.game.rel_path));
                return Transition::Pop;
            }
            Item::Rename => {
                self.cleaned = match ctx.library.cleaned_title(id) {
                    Ok(Some(cleaned)) => cleaned,
                    Ok(None) => self.game.title.clone(),
                    Err(e) => return finish(Err(e), ctx),
                };
                self.entry = Some(TextEntry::new(&self.game.title, MAX_TITLE_CHARS));
                self.view = View::Rename;
                return Transition::None;
            }
            Item::LoadSaveState => {
                self.read_slots();
                self.view = View::Load;
                self.pick = self.first_pickable();
                return Transition::None;
            }
            Item::StartFromPowerOn => return self.play(Start::PowerOn, ctx),
            Item::DeleteSaveData => {
                self.view = View::Delete;
                self.pick = self.first_pickable();
                return Transition::None;
            }
            Item::ChooseArtwork => return Transition::Push(Box::new(ArtworkPicker::new(self.game.clone()))),
        };
        finish(done, ctx)
    }

    /// Replaces the `i`th Favorite with this Game.
    fn replace(&mut self, i: usize, ctx: &mut Ctx) -> Transition {
        let Some((old, _)) = self.favorites.get(i) else {
            return Transition::None;
        };
        match ctx.library.replace_favorite(old.id, self.game.id, ctx::now()) {
            Ok(()) => finish(Ok(()), ctx),
            Err(e) => {
                eprintln!("game37: {e}");
                let body = format!(
                    "{} wasn't added, and your Favorites are as they were.",
                    self.game.title
                );
                Transition::Replace(Box::new(Message::new("Couldn't change Favorites", body)))
            }
        }
    }

    /// Saves the typed Display Title, trimmed. Empty, or the cleaned-up title, undoes the rename.
    fn save_rename(&mut self, ctx: &mut Ctx) -> Transition {
        let typed = self.entry.as_ref().map_or("", |e| e.text()).trim();
        let title = (!typed.is_empty() && typed != self.cleaned).then(|| typed.to_owned());
        finish(ctx.library.rename(self.game.id, title), ctx)
    }

    fn back_to_list(&mut self) {
        self.view = View::List;
        self.favorites.clear();
        self.entry = None;
    }

    fn draw_rename(&mut self, gfx: &mut Gfx<'_>) {
        let panel = Rect::new(RENAME_X, RENAME_Y, RENAME_W, RENAME_H);
        gfx.fill_round_rect(panel, 20.0, SURFACE);
        gfx.stroke_round_rect(panel, 20.0, 1.0, LINE);
        let x = RENAME_X + (RENAME_W - KEYBOARD_W) / 2.0;
        let heading = TextStyle::new(30, Weight::ExtraBold, TEXT);
        gfx.text(x, RENAME_Y + 72.0, "Rename", heading, Align::Left);
        let meta = TextStyle::new(18, Weight::Medium, MUTED);
        gfx.text(x, RENAME_Y + 106.0, &self.meta, meta, Align::Left);
        let Some(entry) = &mut self.entry else {
            return;
        };
        let field = Rect::new(x, RENAME_Y + 136.0, KEYBOARD_W, 88.0);
        entry.draw_field(gfx, field, &self.cleaned);
        let note = TextStyle::new(16, Weight::Medium, DIM);
        let reset = format!("Leave empty to use “{}”. The file keeps its name.", self.cleaned);
        let reset = ellipsize(&reset, KEYBOARD_W - 120.0, |s| gfx.measure(s, note));
        gfx.text(x, RENAME_Y + 256.0, &reset, note, Align::Left);
        let count = format!("{} / {MAX_TITLE_CHARS}", entry.text().chars().count());
        gfx.text(x + KEYBOARD_W, RENAME_Y + 256.0, &count, note, Align::Right);
        let top = RENAME_Y + RENAME_H - 48.0 - KEYBOARD_H;
        entry.draw_keyboard(gfx, x, top);
    }

    fn draw_list(&mut self, gfx: &mut Gfx<'_>) {
        self.item_hits.clear();
        for (i, &item) in self.items.iter().enumerate() {
            let y = ITEM_Y + i as f32 * ITEM_PITCH;
            let r = Rect::new(PANEL_X + 24.0, y, PANEL_W - 48.0, 72.0);
            let on = i == self.item;
            if on {
                gfx.fill_round_rect(r, 14.0, SURFACE2);
                gfx.fill_round_rect(Rect::new(r.x, y + 16.0, 5.0, 40.0), 2.0, ACCENT);
            }
            let enabled = self.enabled(item);
            let (ink, icon_ink) = match item {
                _ if !enabled => (DIM, DIM),
                Item::Favorite if self.game.favorite => (TEXT, STAR),
                _ => (TEXT, MUTED),
            };
            icon(gfx, item, PANEL_X + 62.0, y + 38.0, icon_ink);
            let weight = if on { Weight::SemiBold } else { Weight::Medium };
            let label = item.label(self.game.favorite);
            gfx.text(
                PANEL_X + 100.0,
                y + 45.0,
                label,
                TextStyle::new(22, weight, ink),
                Align::Left,
            );
            let side = if enabled { MUTED } else { DIM };
            if item == Item::LoadSaveState {
                let count = format!("{} of {SLOTS}", self.filled());
                let style = TextStyle::new(18, Weight::Medium, side);
                gfx.text(W - 76.0, y + 45.0, &count, style, Align::Right);
            }
            if item.opens_view() {
                let arrow = TextStyle::new(26, Weight::SemiBold, side);
                gfx.text(W - 48.0, y + 45.0, "›", arrow, Align::Right);
            }
            self.item_hits.push((r, i));
        }
    }

    /// The slot picker, after the Pause Menu's Load view: each slot's screenshot, name and time,
    /// the empty ones faded and never focused.
    fn draw_load(&mut self, gfx: &mut Gfx<'_>) {
        self.pick_hits.clear();
        let heading = TextStyle::new(24, Weight::ExtraBold, TEXT);
        gfx.text(PANEL_X + 48.0, 238.0, "Load save state", heading, Align::Left);
        let sub = TextStyle::new(18, Weight::Medium, MUTED);
        gfx.text(PANEL_X + 48.0, 268.0, "Choose a slot", sub, Align::Left);
        for (i, &filled) in self.filled.iter().enumerate() {
            // A filled slot not read yet shows no time or screenshot for now.
            let slot = self.slots.get(i).and_then(Option::as_ref);
            let y = SLOT_Y + i as f32 * SLOT_PITCH;
            let r = Rect::new(PANEL_X + 24.0, y, PANEL_W - 48.0, SLOT_ROW_H);
            let fade = if filled { 1.0 } else { 0.4 };
            if i == self.pick && filled {
                gfx.fill_round_rect(r, 14.0, SURFACE2);
                let bar_y = y + (SLOT_ROW_H - 40.0) / 2.0;
                gfx.fill_round_rect(Rect::new(r.x, bar_y, 5.0, 40.0), 2.0, ACCENT);
            }
            let thumb = Rect::new(PANEL_X + 48.0, y + 14.0, THUMB_W, THUMB_H);
            match slot {
                Some(s) if s.has_thumb && gfx.draw_image(THUMB_KEYS[i], thumb) => {
                    gfx.stroke_round_rect(thumb, 10.0, 2.0, LINE);
                }
                _ if filled => {
                    gfx.fill_round_rect(thumb, 10.0, SURFACE2);
                    gfx.stroke_round_rect(thumb, 10.0, 2.0, LINE);
                }
                _ => {
                    gfx.fill_round_rect(thumb, 10.0, SURFACE2.alpha(fade));
                    gfx.stroke_round_rect_dashed(thumb, 10.0, 2.0, 10.0, 8.0, LINE.alpha(fade));
                }
            }
            let x = thumb.x + THUMB_W + 24.0;
            let name = TextStyle::new(22, Weight::Bold, TEXT.alpha(fade));
            gfx.text(x, y + 68.0, &format!("Slot {}", i + 1), name, Align::Left);
            let when = match slot {
                Some(s) => s.when.as_str(),
                None if filled => "…",
                None => "Empty",
            };
            let style = TextStyle::new(18, Weight::Medium, MUTED.alpha(fade));
            gfx.text(x, y + 100.0, when, style, Align::Left);
            self.pick_hits.push((r, i));
        }
    }

    /// The delete view: what the Game has of each kind of save data, the kinds it has none of
    /// dimmed and never focused.
    fn draw_delete(&mut self, gfx: &mut Gfx<'_>) {
        self.pick_hits.clear();
        let heading = TextStyle::new(24, Weight::ExtraBold, TEXT);
        gfx.text(PANEL_X + 48.0, 238.0, "Delete save data", heading, Align::Left);
        let sub = TextStyle::new(18, Weight::Medium, MUTED);
        gfx.text(PANEL_X + 48.0, 268.0, "Choose what to delete", sub, Align::Left);
        for (i, &what) in DELETE_ROWS.iter().enumerate() {
            let y = PICK_Y + i as f32 * PICK_PITCH;
            let r = Rect::new(PANEL_X + 24.0, y, PANEL_W - 48.0, 100.0);
            let has = self.has(what);
            if i == self.pick && has {
                gfx.fill_round_rect(r, 14.0, SURFACE2);
                gfx.fill_round_rect(Rect::new(r.x, y + 30.0, 5.0, 40.0), 2.0, ACCENT);
            }
            let (ink, sub_ink) = if has { (TEXT, MUTED) } else { (DIM, DIM) };
            let name = TextStyle::new(22, Weight::Bold, ink);
            let label = delete_label(what);
            match self.delete_sub(what) {
                Some(line) => {
                    gfx.text(PANEL_X + 48.0, y + 46.0, label, name, Align::Left);
                    let style = TextStyle::new(18, Weight::Medium, sub_ink);
                    gfx.text(PANEL_X + 48.0, y + 76.0, &line, style, Align::Left);
                }
                None => {
                    gfx.text(PANEL_X + 48.0, y + 58.0, label, name, Align::Left);
                }
            }
            self.pick_hits.push((r, i));
        }
    }

    fn draw_replace(&mut self, gfx: &mut Gfx<'_>, ctx: &Ctx) {
        self.pick_hits.clear();
        let heading = TextStyle::new(24, Weight::ExtraBold, TEXT);
        gfx.text(PANEL_X + 48.0, 238.0, "Favorites are full", heading, Align::Left);
        let sub = TextStyle::new(18, Weight::Medium, MUTED);
        gfx.text(PANEL_X + 48.0, 268.0, "Choose one to replace", sub, Align::Left);
        for (i, (game, present)) in self.favorites.iter().enumerate() {
            let y = PICK_Y + i as f32 * PICK_PITCH;
            let r = Rect::new(PANEL_X + 24.0, y, PANEL_W - 48.0, 100.0);
            if i == self.pick {
                gfx.fill_round_rect(r, 14.0, SURFACE2);
                gfx.fill_round_rect(Rect::new(r.x, y + 30.0, 5.0, 40.0), 2.0, ACCENT);
            }
            cover::draw(
                gfx,
                Rect::new(PANEL_X + 48.0, y + 8.0, 56.0, 84.0),
                game,
                &ctx.artwork.cover(&ctx.library, game),
                false,
                false,
            );
            let title = TextStyle::new(22, Weight::Bold, TEXT);
            let label = ellipsize(&game.title, PANEL_W - 180.0, |s| gfx.measure(s, title));
            gfx.text(PANEL_X + 128.0, y + 46.0, &label, title, Align::Left);
            let short = systems::by_id(&game.system).map_or("?", |s| s.short);
            let line = if *present {
                short.to_owned()
            } else {
                format!("{short}  ·  File not found")
            };
            gfx.text(PANEL_X + 128.0, y + 76.0, &line, sub, Align::Left);
            self.pick_hits.push((r, i));
        }
    }
}

/// Whether each Save State slot of `states` is filled, and when the Resume State and the In-game
/// Save `srm` were written, if they exist: file metadata only, cheap enough for the UI thread.
fn read_save_data(states: &StatePaths, srm: &Path) -> (Vec<bool>, Option<String>, Option<String>) {
    let written = |path: &Path| {
        let modified = fs::metadata(path).ok().filter(|m| m.is_file())?.modified().ok()?;
        Some(sdl::when_label(modified))
    };
    let filled = (1..=SLOTS).map(|n| states.slot(n).is_file()).collect();
    (filled, written(&states.resume()), written(srm))
}

/// A delete view row's name.
fn delete_label(what: SaveData) -> &'static str {
    match what {
        SaveData::ResumeState => "Resume State",
        SaveData::SaveStates => "Save States",
        SaveData::InGameSave => "In-game Save",
        SaveData::Everything => "Everything",
    }
}

/// Ends an action: the library changed, so screens reload; the menu closes.
fn finish(done: Result<(), crate::library::db::DbError>, ctx: &mut Ctx) -> Transition {
    if let Err(e) = done {
        eprintln!("game37: {e}");
    }
    ctx.revision += 1;
    Transition::Pop
}

/// A 24 px icon centred on (cx, cy), drawn rather than a text glyph (not every font covers them).
fn icon(gfx: &mut Gfx<'_>, item: Item, cx: f32, cy: f32, c: Colour) {
    match item {
        Item::Favorite => super::details::star(gfx, cx, cy, 12.0, c),
        Item::LoadSaveState => {
            gfx.stroke_circle(cx, cy, 9.0, 2.5, c);
            gfx.fill_polygon(
                &[(cx - 13.0, cy - 6.0), (cx - 5.0, cy - 6.0), (cx - 9.0, cy + 1.0)],
                c,
            );
        }
        // To the start: a bar and a triangle pointing at it.
        Item::StartFromPowerOn => {
            gfx.fill_rect(Rect::new(cx - 10.0, cy - 9.0, 3.0, 18.0), c);
            gfx.fill_polygon(&[(cx + 10.0, cy - 9.0), (cx + 10.0, cy + 9.0), (cx - 5.0, cy)], c);
        }
        // A bin: lid, handle and body.
        Item::DeleteSaveData => {
            gfx.line(cx - 11.0, cy - 6.0, cx + 11.0, cy - 6.0, 2.5, c);
            gfx.fill_rect(Rect::new(cx - 4.0, cy - 11.0, 8.0, 3.0), c);
            gfx.stroke_round_rect(Rect::new(cx - 8.0, cy - 3.0, 16.0, 14.0), 2.0, 2.5, c);
        }
        Item::ChooseArtwork => {
            gfx.stroke_round_rect(Rect::new(cx - 10.0, cy - 10.0, 20.0, 20.0), 3.0, 2.5, c);
            gfx.fill_rect(Rect::new(cx - 5.0, cy - 5.0, 10.0, 10.0), c);
        }
        Item::Rename => {
            gfx.line(cx - 7.0, cy + 7.0, cx + 8.0, cy - 8.0, 4.0, c);
            gfx.fill_polygon(
                &[(cx - 11.0, cy + 11.0), (cx - 9.0, cy + 4.0), (cx - 4.0, cy + 9.0)],
                c,
            );
        }
        Item::Hide => {
            gfx.stroke_circle(cx, cy, 10.0, 2.5, c);
            gfx.line(cx - 7.0, cy + 7.0, cx + 7.0, cy - 7.0, 2.5, c);
        }
        Item::ShowFile => {
            gfx.stroke_round_rect(Rect::new(cx - 10.0, cy - 10.0, 20.0, 20.0), 3.0, 2.5, c);
            for dy in [-3.0, 3.0] {
                gfx.fill_rect(Rect::new(cx - 6.0, cy + dy - 1.0, 12.0, 2.0), c);
            }
        }
    }
}

impl Screen for OptionsMenu {
    fn title(&self) -> &str {
        &self.game.title
    }

    fn is_modal(&self) -> bool {
        true
    }

    fn handle(&mut self, input: Input, ctx: &mut Ctx) -> Transition {
        if let Some(entry) = &mut self.entry {
            match entry.handle(input) {
                Outcome::Submit => return self.save_rename(ctx),
                Outcome::Cancel => self.back_to_list(),
                Outcome::None | Outcome::Changed => {}
            }
            return Transition::None;
        }
        let list = self.view == View::List;
        match input {
            Input::Action(Action::Up) if list => self.move_item(-1),
            Input::Action(Action::Down) if list => self.move_item(1),
            Input::Action(Action::Confirm) if list => return self.choose(self.item, ctx),
            Input::Action(Action::Back) | Input::Pointer(Pointer::Back) if list => return Transition::Pop,
            Input::Action(Action::Up) => self.move_pick(-1),
            Input::Action(Action::Down) => self.move_pick(1),
            Input::Action(Action::Confirm) => return self.pick_at(self.pick, ctx),
            Input::Action(Action::Back) | Input::Pointer(Pointer::Back) => self.back_to_list(),
            Input::Pointer(Pointer::Move { x, y }) => {
                if let Some(i) = hit(&self.item_hits, x, y).filter(|&i| list && self.enabled(self.items[i])) {
                    self.item = i;
                } else if let Some(i) = hit(&self.pick_hits, x, y).filter(|&i| self.pickable(i)) {
                    self.pick = i;
                }
            }
            Input::Pointer(Pointer::Click { x, .. }) if x < PANEL_X => {
                return if list { Transition::Pop } else { Transition::None };
            }
            Input::Pointer(Pointer::Click { x, y }) => {
                if let Some(i) = hit(&self.item_hits, x, y).filter(|_| list) {
                    return self.choose(i, ctx);
                } else if let Some(i) = hit(&self.pick_hits, x, y).filter(|_| !list) {
                    return self.pick_at(i, ctx);
                }
            }
            _ => {}
        }
        Transition::None
    }

    fn tick(&mut self, _: &mut Ctx) -> Transition {
        self.poll_slots();
        self.after_delete()
    }

    fn busy(&self) -> bool {
        self.reading.is_some()
    }

    fn draw(&mut self, gfx: &mut Gfx<'_>, ctx: &Ctx) {
        self.kind = ctx.input_kind;
        self.poll_slots();
        for (i, thumb) in self.uploads.drain(..) {
            gfx.set_image(THUMB_KEYS[i], thumb.as_ref());
        }
        gfx.fill_rect(Rect::new(0.0, 0.0, W, H), Colour::hex(0x000000).alpha(0.62));
        if self.view == View::Rename {
            self.item_hits.clear();
            return self.draw_rename(gfx);
        }
        gfx.fill_rect(Rect::new(PANEL_X, 0.0, PANEL_W, H), SURFACE);
        gfx.fill_rect(Rect::new(PANEL_X, 0.0, 1.0, H), LINE);
        cover::draw(
            gfx,
            Rect::new(PANEL_X + 48.0, 64.0, 96.0, 144.0),
            &self.game,
            &ctx.artwork.cover(&ctx.library, &self.game),
            false,
            false,
        );
        let title = TextStyle::new(28, Weight::ExtraBold, TEXT);
        let label = ellipsize(&self.game.title, PANEL_W - 216.0, |s| gfx.measure(s, title));
        gfx.text(PANEL_X + 168.0, 118.0, &label, title, Align::Left);
        let meta = TextStyle::new(18, Weight::Medium, MUTED);
        gfx.text(PANEL_X + 168.0, 152.0, &self.meta, meta, Align::Left);
        match self.view {
            View::List => {
                self.pick_hits.clear();
                self.draw_list(gfx);
            }
            View::Replace => {
                self.item_hits.clear();
                self.draw_replace(gfx, ctx);
            }
            View::Rename => {}
            View::Load => {
                self.item_hits.clear();
                self.draw_load(gfx);
            }
            View::Delete => {
                self.item_hits.clear();
                self.draw_delete(gfx);
            }
        }
    }

    fn hints(&self) -> Vec<Hint> {
        match self.view {
            View::List => vec![
                Hint::new(&[Action::Confirm], "Choose"),
                Hint::new(&[Action::Back], "Close"),
            ],
            View::Replace => vec![
                Hint::new(&[Action::Confirm], "Replace"),
                Hint::new(&[Action::Back], "Back"),
            ],
            View::Rename => TextEntry::hints(self.kind, "Save"),
            View::Load => vec![
                Hint::new(&[Action::Confirm], "Load"),
                Hint::new(&[Action::Back], "Back"),
            ],
            View::Delete => vec![
                Hint::new(&[Action::Confirm], "Delete"),
                Hint::new(&[Action::Back], "Back"),
            ],
        }
    }

    fn text_input(&self) -> bool {
        self.view == View::Rename
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::FoundGame;
    use crate::library::db::{Shelf, SortOrder};
    use crate::ui::input::TextInput;

    /// A Ctx with a temporary Games Root holding `n` SNES Games, "G0" to "G<n-1>".
    fn world(n: usize) -> (tempfile::TempDir, Ctx, Vec<GameRow>) {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = Ctx::empty();
        ctx.games_root = dir.path().join("Games");
        let found: Vec<_> = (0..n)
            .map(|i| FoundGame {
                system: "snes",
                rel_path: format!("snes/G{i}.sfc"),
                title: format!("G{i}"),
            })
            .collect();
        ctx.library.sync_system("snes", &found, 0).unwrap();
        let games = ctx.library.system_games("snes", SortOrder::AZ).unwrap();
        (dir, ctx, games)
    }

    fn act(menu: &mut OptionsMenu, ctx: &mut Ctx, action: Action) -> Transition {
        menu.handle(Input::Action(action), ctx)
    }

    fn fresh(ctx: &Ctx, game: &GameRow) -> GameRow {
        ctx.library.game(game.id).unwrap().unwrap()
    }

    fn favorite_ids(ctx: &Ctx) -> Vec<i64> {
        let mut ids: Vec<_> = ctx
            .library
            .shelf(Shelf::Favorites, None)
            .unwrap()
            .iter()
            .map(|g| g.id)
            .collect();
        ids.sort();
        ids
    }

    #[test]
    fn favorite_toggles_and_the_menu_closes() {
        let (_dir, mut ctx, games) = world(1);
        let mut menu = OptionsMenu::new(games[0].clone(), &ctx.games_root);
        assert_eq!(menu.focused(), Item::Favorite, "focus starts on Favorite");
        assert_eq!(Item::Favorite.label(menu.game.favorite), "Add to favorites");
        let revision = ctx.revision;
        assert!(matches!(
            act(&mut menu, &mut ctx, Action::Confirm),
            Transition::Pop
        ));
        assert!(fresh(&ctx, &games[0]).favorite);
        assert!(ctx.revision > revision, "screens reload");

        let mut menu = OptionsMenu::new(fresh(&ctx, &games[0]), &ctx.games_root);
        assert_eq!(Item::Favorite.label(menu.game.favorite), "Remove from favorites");
        assert!(matches!(
            act(&mut menu, &mut ctx, Action::Confirm),
            Transition::Pop
        ));
        assert!(!fresh(&ctx, &games[0]).favorite);
    }

    /// Five Favorites, G0 to G4, favorited in that order, and a menu for G5.
    fn five_favorites() -> (tempfile::TempDir, Ctx, Vec<GameRow>, OptionsMenu) {
        let (dir, ctx, games) = world(6);
        for (i, game) in games[..5].iter().enumerate() {
            ctx.library.set_favorite(game.id, true, i as i64).unwrap();
        }
        let menu = OptionsMenu::new(games[5].clone(), &ctx.games_root);
        (dir, ctx, games, menu)
    }

    #[test]
    fn favorite_with_five_favorites_opens_a_picker_and_the_one_picked_is_replaced() {
        let (_dir, mut ctx, games, mut menu) = five_favorites();
        assert!(matches!(
            act(&mut menu, &mut ctx, Action::Confirm),
            Transition::None
        ));
        assert_eq!(menu.view(), View::Replace);
        assert_eq!(menu.hints()[0], Hint::new(&[Action::Confirm], "Replace"));
        let shown: Vec<_> = menu.favorites.iter().map(|(g, _)| g.title.as_str()).collect();
        assert_eq!(shown, ["G4", "G3", "G2", "G1", "G0"], "the five, newest first");
        assert!(!fresh(&ctx, &games[5]).favorite, "nothing changes yet");

        act(&mut menu, &mut ctx, Action::Down);
        act(&mut menu, &mut ctx, Action::Down);
        assert!(matches!(
            act(&mut menu, &mut ctx, Action::Confirm),
            Transition::Pop
        ));
        let mut want = vec![games[0].id, games[1].id, games[3].id, games[4].id, games[5].id];
        want.sort();
        assert_eq!(favorite_ids(&ctx), want, "G2 made way for G5");
    }

    /// G0's file is away (a drive unplugged): it still counts towards the five, the picker offers
    /// it, and when the file returns there are still five Favorites.
    #[test]
    fn an_absent_favorite_still_counts_and_the_picker_offers_it() {
        let (_dir, mut ctx, games, mut menu) = five_favorites();
        let found = |present: &[usize]| -> Vec<FoundGame> {
            present
                .iter()
                .map(|i| FoundGame {
                    system: "snes",
                    rel_path: format!("snes/G{i}.sfc"),
                    title: format!("G{i}"),
                })
                .collect()
        };
        ctx.library
            .sync_system("snes", &found(&[1, 2, 3, 4, 5]), 0)
            .unwrap();
        assert_eq!(favorite_ids(&ctx).len(), 4, "Home shows four");
        assert!(matches!(
            act(&mut menu, &mut ctx, Action::Confirm),
            Transition::None
        ));
        assert_eq!(menu.view(), View::Replace, "but Favorites are full");
        let shown: Vec<_> = menu
            .favorites
            .iter()
            .map(|(g, p)| (g.title.as_str(), *p))
            .collect();
        assert_eq!(
            shown,
            [
                ("G4", true),
                ("G3", true),
                ("G2", true),
                ("G1", true),
                ("G0", false)
            ]
        );
        act(&mut menu, &mut ctx, Action::Back);
        act(&mut menu, &mut ctx, Action::Back);
        ctx.library
            .sync_system("snes", &found(&[0, 1, 2, 3, 4, 5]), 0)
            .unwrap();
        assert_eq!(
            favorite_ids(&ctx).len(),
            MAX_FAVORITES,
            "G0 is back, and still five"
        );
        assert!(!fresh(&ctx, &games[5]).favorite);

        // Replacing the absent G0 makes way for G5.
        ctx.library
            .sync_system("snes", &found(&[1, 2, 3, 4, 5]), 0)
            .unwrap();
        let mut menu = OptionsMenu::new(fresh(&ctx, &games[5]), &ctx.games_root);
        act(&mut menu, &mut ctx, Action::Confirm);
        for _ in 0..4 {
            act(&mut menu, &mut ctx, Action::Down);
        }
        assert!(matches!(
            act(&mut menu, &mut ctx, Action::Confirm),
            Transition::Pop
        ));
        ctx.library
            .sync_system("snes", &found(&[0, 1, 2, 3, 4, 5]), 0)
            .unwrap();
        assert_eq!(favorite_ids(&ctx).len(), MAX_FAVORITES);
        assert!(fresh(&ctx, &games[5]).favorite && !fresh(&ctx, &games[0]).favorite);
    }

    #[test]
    fn a_click_on_a_favorite_in_the_picker_replaces_it() {
        let (_dir, mut ctx, games, mut menu) = five_favorites();
        act(&mut menu, &mut ctx, Action::Confirm);
        menu.pick_hits = vec![(Rect::new(PANEL_X, 400.0, 100.0, 100.0), 4)];
        let at = Input::Pointer(Pointer::Click {
            x: PANEL_X + 10.0,
            y: 410.0,
        });
        assert!(matches!(menu.handle(at, &mut ctx), Transition::Pop));
        assert!(!fresh(&ctx, &games[0]).favorite, "G0 is the 5th, oldest");
        assert!(fresh(&ctx, &games[5]).favorite);
        assert_eq!(favorite_ids(&ctx).len(), MAX_FAVORITES);
    }

    #[test]
    fn a_failed_replacement_says_so_and_keeps_the_favorites() {
        let (_dir, mut ctx, games, mut menu) = five_favorites();
        let before = favorite_ids(&ctx);
        ctx.library.fail_favoriting(games[5].id);
        act(&mut menu, &mut ctx, Action::Confirm);
        match act(&mut menu, &mut ctx, Action::Confirm) {
            Transition::Replace(message) => assert_eq!(message.title(), "Couldn't change Favorites"),
            _ => panic!("expected a message in the menu's place"),
        }
        assert_eq!(favorite_ids(&ctx), before, "G4 is still a Favorite");
    }

    #[test]
    fn back_from_the_favorite_picker_changes_nothing() {
        let (_dir, mut ctx, _games, mut menu) = five_favorites();
        let before = favorite_ids(&ctx);
        act(&mut menu, &mut ctx, Action::Confirm);
        act(&mut menu, &mut ctx, Action::Down);
        let revision = ctx.revision;
        let outside = Input::Pointer(Pointer::Click { x: 10.0, y: 10.0 });
        assert!(
            matches!(menu.handle(outside, &mut ctx), Transition::None),
            "a click outside"
        );
        assert_eq!(menu.view(), View::Replace);
        assert!(matches!(act(&mut menu, &mut ctx, Action::Back), Transition::None));
        assert_eq!(menu.view(), View::List, "Back returns to the list");
        assert_eq!(menu.focused(), Item::Favorite);
        assert!(matches!(act(&mut menu, &mut ctx, Action::Back), Transition::Pop));
        assert_eq!(favorite_ids(&ctx), before);
        assert_eq!(ctx.revision, revision);
    }

    #[test]
    fn an_action_that_cant_be_chosen_is_listed_but_not_selectable() {
        let (_dir, mut ctx, games) = world(1);
        let mut menu = OptionsMenu::new(games[0].clone(), &ctx.games_root);
        use Item::*;
        assert_eq!(
            menu.items(),
            [
                Favorite,
                LoadSaveState,
                StartFromPowerOn,
                DeleteSaveData,
                ChooseArtwork,
                Rename,
                Hide,
                ShowFile
            ]
        );
        assert!(!menu.enabled(LoadSaveState), "no Save State to load");
        assert!(!menu.enabled(DeleteSaveData), "no save data to delete");
        act(&mut menu, &mut ctx, Action::Down);
        assert_eq!(menu.focused(), StartFromPowerOn, "focus skips it");
        act(&mut menu, &mut ctx, Action::Down);
        assert_eq!(menu.focused(), ChooseArtwork, "and Delete save data");
        act(&mut menu, &mut ctx, Action::Up);
        act(&mut menu, &mut ctx, Action::Up);
        assert_eq!(menu.focused(), Favorite);
        act(&mut menu, &mut ctx, Action::Up);
        assert_eq!(menu.focused(), Favorite, "and stops at the ends");

        menu.item_hits = vec![(Rect::new(PANEL_X, 300.0, 100.0, 50.0), 1)];
        menu.handle(
            Input::Pointer(Pointer::Move {
                x: PANEL_X + 1.0,
                y: 301.0,
            }),
            &mut ctx,
        );
        assert_eq!(menu.focused(), Favorite, "nor does the pointer focus them");
        let click = Input::Pointer(Pointer::Click {
            x: PANEL_X + 1.0,
            y: 301.0,
        });
        assert!(matches!(menu.handle(click, &mut ctx), Transition::None));
        assert!(!fresh(&ctx, &games[0]).favorite);
    }

    #[test]
    fn choose_artwork_opens_the_artwork_picker_over_the_menu_and_back_returns_to_it() {
        use crate::ui::nav::{Flow, Hits, Nav};
        let (_dir, mut ctx, games) = world(1);
        let mut menu = OptionsMenu::new(games[0].clone(), &ctx.games_root);
        act(&mut menu, &mut ctx, Action::Down);
        act(&mut menu, &mut ctx, Action::Down);
        assert_eq!(menu.focused(), Item::ChooseArtwork);
        assert!(menu.enabled(Item::ChooseArtwork));
        let mut nav = Nav::single(Box::new(OptionsMenu::new(games[0].clone(), &ctx.games_root)));
        nav.push(Box::new(menu));
        let press = |nav: &mut Nav, ctx: &mut Ctx, action| {
            assert_eq!(
                nav.handle(Input::Action(action), &Hits::default(), ctx),
                Flow::Continue
            );
        };
        let revision = ctx.revision;
        press(&mut nav, &mut ctx, Action::Confirm);
        assert_eq!(nav.depth(), 3, "the picker is pushed over the menu");
        assert_eq!(nav.top().title(), "Choose artwork");
        assert!(nav.top().is_modal());
        press(&mut nav, &mut ctx, Action::Back);
        assert_eq!(nav.depth(), 2, "Back returns to the menu");
        assert_eq!(nav.top().title(), "G0");
        assert_eq!(ctx.revision, revision, "nothing changed");
    }

    #[test]
    fn save_data_items_are_listed_for_embedded_systems_only() {
        let mut game = world(1).2.remove(0);
        game.system = "gc".into();
        use Item::*;
        assert_eq!(
            OptionsMenu::new(game, Path::new("/nonexistent")).items(),
            [Favorite, ChooseArtwork, Rename, Hide, ShowFile]
        );
    }

    /// Fills Save State slots `slots` of `game`, each with a tiny screenshot.
    fn fill_slots(ctx: &Ctx, game: &GameRow, slots: &[usize]) {
        let states = StatePaths::new(&ctx.games_root, &game.system, Path::new(&game.rel_path));
        let thumb = Rgb {
            width: 1,
            height: 1,
            data: vec![0, 128, 255],
        };
        for &n in slots {
            let png = crate::session::states::encode_png(&thumb);
            crate::session::states::write_slot(&states.slot(n), b"state", &states.thumb(n), png).unwrap();
        }
    }

    fn click(menu: &mut OptionsMenu, ctx: &mut Ctx, (x, y): (f32, f32)) -> Transition {
        menu.handle(Input::Pointer(Pointer::Click { x, y }), ctx)
    }

    /// Ticks `menu` until its slots are read, as the app does every frame.
    fn wait_for_slots(menu: &mut OptionsMenu, ctx: &mut Ctx) {
        for _ in 0..5000 {
            menu.tick(ctx);
            if !menu.busy() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("the slots were never read");
    }

    /// Opening the menu decodes no screenshot (NFR-3): the slot picker reads them on a thread,
    /// and its input is handled meanwhile.
    #[test]
    fn slot_screenshots_are_read_off_the_ui_thread_once_the_slot_picker_opens() {
        let (_dir, mut ctx, games) = world(1);
        fill_slots(&ctx, &games[0], &[2, 3]);
        let mut menu = OptionsMenu::new(games[0].clone(), &ctx.games_root);
        assert_eq!(menu.filled(), 2, "the count needs no screenshot");
        assert!(menu.slots.is_empty() && menu.uploads.is_empty() && !menu.busy());
        act(&mut menu, &mut ctx, Action::Down);
        act(&mut menu, &mut ctx, Action::Confirm);
        assert_eq!((menu.view(), menu.picked()), (View::Load, 1));
        act(&mut menu, &mut ctx, Action::Down);
        assert_eq!(menu.picked(), 2, "input is handled while reading");
        wait_for_slots(&mut menu, &mut ctx);
        let uploads: Vec<_> = menu.uploads.iter().map(|(i, t)| (*i, t.is_some())).collect();
        assert_eq!(uploads, [(0, false), (1, true), (2, true)]);
    }

    #[test]
    fn load_save_state_cant_be_chosen_without_a_filled_slot() {
        let (_dir, mut ctx, games) = world(1);
        let mut menu = OptionsMenu::new(games[0].clone(), &ctx.games_root);
        assert!(!menu.enabled(Item::LoadSaveState));
        menu.item_hits = vec![(Rect::new(PANEL_X, 338.0, 100.0, 72.0), 1)];
        assert!(matches!(
            click(&mut menu, &mut ctx, (PANEL_X + 1.0, 340.0)),
            Transition::None
        ));
        assert_eq!(menu.view(), View::List, "no slot picker opens");

        fill_slots(&ctx, &games[0], &[3]);
        let menu = OptionsMenu::new(games[0].clone(), &ctx.games_root);
        assert!(menu.enabled(Item::LoadSaveState), "one filled slot is enough");
    }

    #[test]
    fn the_slot_picker_lists_the_filled_slots_and_skips_the_empty_one() {
        let (_dir, mut ctx, games) = world(1);
        fill_slots(&ctx, &games[0], &[1, 3]);
        let mut menu = OptionsMenu::new(games[0].clone(), &ctx.games_root);
        act(&mut menu, &mut ctx, Action::Down);
        assert_eq!(menu.focused(), Item::LoadSaveState);
        assert!(matches!(
            act(&mut menu, &mut ctx, Action::Confirm),
            Transition::None
        ));
        assert_eq!((menu.view(), menu.picked()), (View::Load, 0));
        assert_eq!(menu.hints()[0], Hint::new(&[Action::Confirm], "Load"));
        wait_for_slots(&mut menu, &mut ctx);
        let shown: Vec<_> = menu
            .slots
            .iter()
            .map(|s| s.as_ref().map(|s| s.has_thumb))
            .collect();
        assert_eq!(
            shown,
            [Some(true), None, Some(true)],
            "slots 1 and 3, with screenshots"
        );
        assert!(menu.slots[0].as_ref().unwrap().when.starts_with("Today, "));
        let uploads: Vec<_> = menu.uploads.iter().map(|(i, t)| (*i, t.is_some())).collect();
        assert_eq!(uploads, [(0, true), (1, false), (2, true)]);

        act(&mut menu, &mut ctx, Action::Down);
        assert_eq!(menu.picked(), 2, "the empty slot 2 is skipped");
        act(&mut menu, &mut ctx, Action::Down);
        assert_eq!(menu.picked(), 2, "and focus stops at the end");
        act(&mut menu, &mut ctx, Action::Up);
        assert_eq!(menu.picked(), 0);

        // The empty slot takes neither the pointer's focus nor a click.
        menu.pick_hits = vec![(Rect::new(PANEL_X, 466.0, 100.0, 160.0), 1)];
        let at = (PANEL_X + 1.0, 470.0);
        menu.handle(Input::Pointer(Pointer::Move { x: at.0, y: at.1 }), &mut ctx);
        assert_eq!(menu.picked(), 0);
        assert!(matches!(click(&mut menu, &mut ctx, at), Transition::None));
        assert_eq!(menu.view(), View::Load);
        assert!(ctx.play.is_none(), "nothing to load");
    }

    /// A Ctx whose snes Core is installed, so playing an snes Game asks `app.rs` to start it.
    fn install_snes_core(ctx: &mut Ctx, dir: &Path) {
        ctx.data_dir = dir.join("data");
        let core = crate::cores::for_system(systems::by_id("snes").unwrap()).unwrap();
        let path = crate::cores::installed_path(&ctx.data_dir, core);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"").unwrap();
    }

    #[test]
    fn choosing_slot_2_asks_to_play_the_game_from_slot_2_and_closes_the_menu() {
        let (dir, mut ctx, games) = world(1);
        install_snes_core(&mut ctx, dir.path());
        fill_slots(&ctx, &games[0], &[2]);
        let mut menu = OptionsMenu::new(games[0].clone(), &ctx.games_root);
        act(&mut menu, &mut ctx, Action::Down);
        act(&mut menu, &mut ctx, Action::Confirm);
        assert_eq!(menu.picked(), 1, "focus starts on the first filled slot");
        assert!(matches!(
            act(&mut menu, &mut ctx, Action::Confirm),
            Transition::Pop
        ));
        let want = Play {
            game: games[0].clone(),
            start: Start::Slot(2),
        };
        assert_eq!(ctx.play.take(), Some(want.clone()));

        // A click on the slot does the same.
        let mut menu = OptionsMenu::new(games[0].clone(), &ctx.games_root);
        act(&mut menu, &mut ctx, Action::Down);
        act(&mut menu, &mut ctx, Action::Confirm);
        menu.pick_hits = vec![(Rect::new(PANEL_X, 466.0, 100.0, 160.0), 1)];
        assert!(matches!(
            click(&mut menu, &mut ctx, (PANEL_X + 1.0, 470.0)),
            Transition::Pop
        ));
        assert_eq!(ctx.play.take(), Some(want));
    }

    #[test]
    fn choosing_a_slot_before_the_core_is_downloaded_puts_the_download_in_the_menus_place() {
        let (dir, mut ctx, games) = world(1);
        ctx.data_dir = dir.path().join("data");
        ctx.start_download = Box::new(crate::cores::testing::spawn_failing);
        fill_slots(&ctx, &games[0], &[1]);
        let mut menu = OptionsMenu::new(games[0].clone(), &ctx.games_root);
        act(&mut menu, &mut ctx, Action::Down);
        act(&mut menu, &mut ctx, Action::Confirm);
        assert!(matches!(
            act(&mut menu, &mut ctx, Action::Confirm),
            Transition::Replace(_)
        ));
        assert!(ctx.play.is_none(), "the download plays it once done");
    }

    #[test]
    fn back_from_the_slot_picker_returns_to_the_list_unchanged() {
        let (dir, mut ctx, games) = world(1);
        install_snes_core(&mut ctx, dir.path());
        fill_slots(&ctx, &games[0], &[1, 2]);
        let mut menu = OptionsMenu::new(games[0].clone(), &ctx.games_root);
        act(&mut menu, &mut ctx, Action::Down);
        act(&mut menu, &mut ctx, Action::Confirm);
        act(&mut menu, &mut ctx, Action::Down);
        let revision = ctx.revision;
        let outside = Input::Pointer(Pointer::Click { x: 10.0, y: 10.0 });
        assert!(matches!(menu.handle(outside, &mut ctx), Transition::None));
        assert_eq!(menu.view(), View::Load, "a click outside keeps the picker");
        assert!(matches!(act(&mut menu, &mut ctx, Action::Back), Transition::None));
        assert_eq!(menu.view(), View::List, "Back returns to the list");
        assert_eq!(menu.focused(), Item::LoadSaveState);
        assert_eq!(menu.hints()[0], Hint::new(&[Action::Confirm], "Choose"));
        assert!(matches!(
            menu.handle(Input::Pointer(Pointer::Back), &mut ctx),
            Transition::Pop
        ));
        assert!(ctx.play.is_none() && ctx.launch.is_none());
        assert_eq!(ctx.revision, revision);
    }

    #[test]
    fn start_from_power_on_asks_to_play_the_game_from_power_on_and_closes_the_menu() {
        let (dir, mut ctx, games) = world(1);
        install_snes_core(&mut ctx, dir.path());
        let mut menu = OptionsMenu::new(games[0].clone(), &ctx.games_root);
        assert!(menu.enabled(Item::StartFromPowerOn), "even with no save data");
        act(&mut menu, &mut ctx, Action::Down);
        assert_eq!(menu.focused(), Item::StartFromPowerOn);
        assert!(matches!(
            act(&mut menu, &mut ctx, Action::Confirm),
            Transition::Pop
        ));
        let want = Play {
            game: games[0].clone(),
            start: Start::PowerOn,
        };
        assert_eq!(ctx.play.take(), Some(want));
    }

    /// Writes a dummy Resume State and In-game Save for `game`, as asked.
    fn write_saves(ctx: &Ctx, game: &GameRow, resume: bool, srm: bool) {
        let rel = Path::new(&game.rel_path);
        if resume {
            let states = StatePaths::new(&ctx.games_root, &game.system, rel);
            saves::write(&states.resume(), b"resume").unwrap();
        }
        if srm {
            saves::write(&saves::srm_path(&ctx.games_root, &game.system, rel), b"srm").unwrap();
        }
    }

    /// Opens the delete view from the list.
    fn open_delete(menu: &mut OptionsMenu, ctx: &mut Ctx) {
        while menu.focused() != Item::DeleteSaveData {
            act(menu, ctx, Action::Down);
        }
        assert!(matches!(act(menu, ctx, Action::Confirm), Transition::None));
        assert_eq!(menu.view(), View::Delete);
    }

    fn delete_subs(menu: &OptionsMenu) -> Vec<Option<String>> {
        DELETE_ROWS.iter().map(|&w| menu.delete_sub(w)).collect()
    }

    fn pickable_rows(menu: &OptionsMenu) -> Vec<bool> {
        (0..DELETE_ROWS.len()).map(|i| menu.pickable(i)).collect()
    }

    /// Chooses the focused delete view row and answers its confirmation, Delete when `yes`, else
    /// Cancel. Returns the confirmation's title and the menu's next tick.
    fn answer(menu: &mut OptionsMenu, ctx: &mut Ctx, yes: bool) -> (String, Transition) {
        let Transition::Push(mut confirm) = act(menu, ctx, Action::Confirm) else {
            panic!("expected a confirmation");
        };
        let title = confirm.title().to_owned();
        if yes {
            confirm.handle(Input::Action(Action::Right), ctx);
        }
        let closes = confirm.handle(Input::Action(Action::Confirm), ctx);
        assert!(matches!(closes, Transition::Pop));
        (title, menu.tick(ctx))
    }

    /// Every file under the Games Root.
    fn files(ctx: &Ctx) -> Vec<PathBuf> {
        let mut all: Vec<_> = walkdir::WalkDir::new(&ctx.games_root)
            .into_iter()
            .map(|e| e.unwrap().into_path())
            .filter(|p| p.is_file())
            .collect();
        all.sort();
        all
    }

    #[test]
    fn delete_save_data_is_enabled_by_any_save_data() {
        type Fill = fn(&Ctx, &GameRow);
        let cases: [(&str, Fill); 3] = [
            ("a Resume State", |ctx, g| write_saves(ctx, g, true, false)),
            ("a Save State", |ctx, g| fill_slots(ctx, g, &[2])),
            ("an In-game Save", |ctx, g| write_saves(ctx, g, false, true)),
        ];
        for (what, fill) in cases {
            let (_dir, ctx, games) = world(1);
            let menu = OptionsMenu::new(games[0].clone(), &ctx.games_root);
            assert!(!menu.enabled(Item::DeleteSaveData), "nothing yet");
            fill(&ctx, &games[0]);
            let menu = OptionsMenu::new(games[0].clone(), &ctx.games_root);
            assert!(menu.enabled(Item::DeleteSaveData), "{what}");
        }
    }

    #[test]
    fn the_delete_view_dims_and_skips_what_the_game_has_none_of() {
        let (_dir, mut ctx, games) = world(1);
        write_saves(&ctx, &games[0], true, false);
        fill_slots(&ctx, &games[0], &[1, 3]);
        let mut menu = OptionsMenu::new(games[0].clone(), &ctx.games_root);
        open_delete(&mut menu, &mut ctx);
        assert_eq!(menu.hints()[0], Hint::new(&[Action::Confirm], "Delete"));
        let subs = delete_subs(&menu);
        assert!(subs[0].as_deref().unwrap().starts_with("Today, "), "{subs:?}");
        assert_eq!(subs[1..], [Some("2 of 3".into()), Some("None".into()), None]);
        assert_eq!(pickable_rows(&menu), [true, true, false, true], "no In-game Save");
        assert_eq!(menu.picked(), 0);
        act(&mut menu, &mut ctx, Action::Down);
        act(&mut menu, &mut ctx, Action::Down);
        assert_eq!(menu.picked(), 3, "the In-game Save is skipped");

        // Neither the pointer nor a click takes the dimmed row.
        menu.pick_hits = vec![(Rect::new(PANEL_X, 514.0, 100.0, 100.0), 2)];
        let at = (PANEL_X + 1.0, 520.0);
        menu.handle(Input::Pointer(Pointer::Move { x: at.0, y: at.1 }), &mut ctx);
        assert_eq!(menu.picked(), 3);
        assert!(matches!(click(&mut menu, &mut ctx, at), Transition::None));

        assert!(matches!(act(&mut menu, &mut ctx, Action::Back), Transition::None));
        assert_eq!(menu.view(), View::List, "Back returns to the list");
        assert_eq!(menu.focused(), Item::DeleteSaveData);
    }

    #[test]
    fn cancelling_a_delete_deletes_nothing() {
        let (_dir, mut ctx, games) = world(1);
        write_saves(&ctx, &games[0], true, true);
        fill_slots(&ctx, &games[0], &[1]);
        let before = files(&ctx);
        assert_eq!(before.len(), 4);
        let revision = ctx.revision;
        let mut menu = OptionsMenu::new(games[0].clone(), &ctx.games_root);
        open_delete(&mut menu, &mut ctx);
        for row in 0..DELETE_ROWS.len() {
            menu.pick = row;
            assert!(matches!(answer(&mut menu, &mut ctx, false).1, Transition::None));
            assert_eq!(files(&ctx), before, "row {row}");
            assert_eq!(menu.view(), View::Delete);
        }
        assert_eq!(ctx.revision, revision);
    }

    #[test]
    fn deleting_the_save_states_shows_what_is_left() {
        let (_dir, mut ctx, games) = world(1);
        fill_slots(&ctx, &games[0], &[2]);
        let menu = OptionsMenu::new(games[0].clone(), &ctx.games_root);
        assert_eq!(
            menu.confirm_delete(SaveData::SaveStates).title(),
            "Delete the Save State?"
        );

        write_saves(&ctx, &games[0], true, false);
        fill_slots(&ctx, &games[0], &[1, 3]);
        let mut menu = OptionsMenu::new(games[0].clone(), &ctx.games_root);
        open_delete(&mut menu, &mut ctx);
        act(&mut menu, &mut ctx, Action::Down);
        let revision = ctx.revision;
        let (title, next) = answer(&mut menu, &mut ctx, true);
        assert_eq!(title, "Delete 3 Save States?");
        assert!(matches!(next, Transition::None));
        assert!(ctx.revision > revision, "the Details Page reloads its count");
        let states = StatePaths::new(&ctx.games_root, "snes", Path::new(&games[0].rel_path));
        assert_eq!(files(&ctx), [states.resume()]);

        assert_eq!(menu.view(), View::Delete, "the Resume State is left");
        assert_eq!(delete_subs(&menu)[1].as_deref(), Some("None"));
        assert_eq!(pickable_rows(&menu), [true, false, false, true]);
        assert_eq!(menu.picked(), 0, "focus leaves the emptied row");
        assert!(!menu.enabled(Item::LoadSaveState));
        assert_eq!(menu.uploads.len(), SLOTS, "the slot screenshots are forgotten");
    }

    #[test]
    fn deleting_everything_returns_to_the_list() {
        let (_dir, mut ctx, games) = world(1);
        write_saves(&ctx, &games[0], true, true);
        fill_slots(&ctx, &games[0], &[1, 2, 3]);
        let mut menu = OptionsMenu::new(games[0].clone(), &ctx.games_root);
        open_delete(&mut menu, &mut ctx);
        for _ in 0..3 {
            act(&mut menu, &mut ctx, Action::Down);
        }
        let (title, next) = answer(&mut menu, &mut ctx, true);
        assert_eq!(title, "Delete all save data?");
        assert!(matches!(next, Transition::None));
        assert_eq!(files(&ctx), Vec::<PathBuf>::new());
        assert_eq!(menu.view(), View::List, "nothing is left to delete");
        assert!(!menu.enabled(Item::DeleteSaveData));
        assert_eq!(
            menu.focused(),
            Item::StartFromPowerOn,
            "focus leaves the disabled item"
        );
    }

    #[test]
    fn deleting_the_in_game_save_takes_the_resume_state_and_a_failure_says_so() {
        let (_dir, mut ctx, games) = world(1);
        write_saves(&ctx, &games[0], true, true);
        fill_slots(&ctx, &games[0], &[1]);
        let mut menu = OptionsMenu::new(games[0].clone(), &ctx.games_root);
        open_delete(&mut menu, &mut ctx);
        menu.pick = 2;
        let (title, _) = answer(&mut menu, &mut ctx, true);
        assert_eq!(title, "Delete the In-game Save?");
        assert_eq!(pickable_rows(&menu), [false, true, false, true]);
        assert_eq!(menu.picked(), 1);

        // A folder where the Resume State goes can't be deleted as a file.
        let rel = Path::new(&games[0].rel_path);
        let states = StatePaths::new(&ctx.games_root, "snes", rel);
        std::fs::create_dir(states.resume()).unwrap();
        write_saves(&ctx, &games[0], false, true);
        let mut menu = OptionsMenu::new(games[0].clone(), &ctx.games_root);
        open_delete(&mut menu, &mut ctx);
        menu.pick = 2;
        match answer(&mut menu, &mut ctx, true).1 {
            Transition::Push(message) => assert_eq!(message.title(), "Couldn't delete save data"),
            _ => panic!("expected a message"),
        }
        assert!(
            saves::srm_path(&ctx.games_root, "snes", rel).exists(),
            "kept with the Resume State"
        );
    }

    #[test]
    fn hide_hides_the_game_and_unfavorites_it() {
        let (_dir, mut ctx, games) = world(2);
        ctx.library.set_favorite(games[0].id, true, 0).unwrap();
        let mut menu = OptionsMenu::new(fresh(&ctx, &games[0]), &ctx.games_root);
        for _ in 0..4 {
            act(&mut menu, &mut ctx, Action::Down);
        }
        assert_eq!(menu.focused(), Item::Hide);
        let revision = ctx.revision;
        assert!(matches!(
            act(&mut menu, &mut ctx, Action::Confirm),
            Transition::Pop
        ));
        assert!(ctx.revision > revision);
        assert_eq!(ctx.library.game(games[0].id).unwrap(), None);
        let left = ctx.library.system_games("snes", SortOrder::AZ).unwrap();
        assert_eq!(left.iter().map(|g| g.id).collect::<Vec<_>>(), [games[1].id]);
        assert_eq!(ctx.library.shelf_count(Shelf::Favorites).unwrap(), 0);
    }

    #[test]
    fn show_file_asks_for_the_games_file_and_a_folder_game_for_its_folder() {
        let (_dir, mut ctx, games) = world(1);
        let mut menu = OptionsMenu::new(games[0].clone(), &ctx.games_root);
        for _ in 0..5 {
            act(&mut menu, &mut ctx, Action::Down);
        }
        assert_eq!(menu.focused(), Item::ShowFile);
        let revision = ctx.revision;
        assert!(matches!(
            act(&mut menu, &mut ctx, Action::Confirm),
            Transition::Pop
        ));
        assert_eq!(
            ctx.show_file_request.take(),
            Some(ctx.games_root.join("snes/G0.sfc"))
        );
        assert_eq!(ctx.revision, revision, "nothing to reload");

        let folder = ctx.games_root.join("ps3/Demon's Souls");
        std::fs::create_dir_all(folder.join("PS3_GAME")).unwrap();
        ctx.scan_all(0).unwrap();
        let game = ctx.library.system_games("ps3", SortOrder::AZ).unwrap().remove(0);
        let mut menu = OptionsMenu::new(game, &ctx.games_root);
        let at = (PANEL_X + 10.0, 600.0);
        menu.item_hits = vec![(Rect::new(PANEL_X, 590.0, 100.0, 50.0), menu.items().len() - 1)];
        menu.handle(Input::Pointer(Pointer::Click { x: at.0, y: at.1 }), &mut ctx);
        assert_eq!(ctx.show_file_request, Some(folder));
    }

    #[test]
    fn back_or_a_click_outside_the_panel_closes_the_menu_and_changes_nothing() {
        let (_dir, mut ctx, games) = world(1);
        let mut menu = OptionsMenu::new(games[0].clone(), &ctx.games_root);
        let revision = ctx.revision;
        assert!(matches!(act(&mut menu, &mut ctx, Action::Back), Transition::Pop));
        let outside = Input::Pointer(Pointer::Click {
            x: PANEL_X - 1.0,
            y: 10.0,
        });
        assert!(matches!(menu.handle(outside, &mut ctx), Transition::Pop));
        assert_eq!(ctx.revision, revision);
        assert!(!fresh(&ctx, &games[0]).favorite);
    }

    /// A world of one Game whose file exists on disk with a few bytes, and a menu for it with
    /// the rename view open.
    fn renaming() -> (tempfile::TempDir, Ctx, GameRow, OptionsMenu) {
        let (dir, mut ctx, games) = world(1);
        let snes = ctx.games_root.join("snes");
        std::fs::create_dir_all(&snes).unwrap();
        std::fs::write(snes.join("G0.sfc"), b"not a game").unwrap();
        let mut menu = OptionsMenu::new(games[0].clone(), &ctx.games_root);
        for _ in 0..3 {
            act(&mut menu, &mut ctx, Action::Down);
        }
        assert_eq!(menu.focused(), Item::Rename);
        assert!(matches!(
            act(&mut menu, &mut ctx, Action::Confirm),
            Transition::None
        ));
        assert_eq!(menu.view(), View::Rename);
        assert!(menu.text_input(), "the field takes typed text");
        assert_eq!(
            menu.entry.as_ref().unwrap().text(),
            "G0",
            "it starts as the Display Title"
        );
        (dir, ctx, games[0].clone(), menu)
    }

    fn type_in(menu: &mut OptionsMenu, ctx: &mut Ctx, text: &str) {
        for ch in text.chars() {
            menu.handle(Input::Text(TextInput::Char(ch)), ctx);
        }
    }

    /// The file is where it was, with the same name and bytes, alone in its folder.
    fn file_untouched(ctx: &Ctx) {
        let snes = ctx.games_root.join("snes");
        let names: Vec<_> = std::fs::read_dir(&snes)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names, ["G0.sfc"]);
        assert_eq!(std::fs::read(snes.join("G0.sfc")).unwrap(), b"not a game");
    }

    #[test]
    fn rename_with_the_on_screen_keyboard_changes_the_display_title_not_the_file() {
        let (_dir, mut ctx, game, mut menu) = renaming();
        let revision = ctx.revision;
        use Action::*;
        // Y erases "G0"; Shift, m, k, then X for a space and + to save.
        let steps = [
            Options, Options, Down, Down, Confirm, Right, Right, Right, Right, Right, Right, Right, Confirm,
            Up, Left, Confirm, Sort,
        ];
        for step in steps {
            assert!(
                matches!(act(&mut menu, &mut ctx, step), Transition::None),
                "{step:?}"
            );
        }
        assert_eq!(menu.entry.as_ref().unwrap().text(), "Mk ");
        assert!(
            matches!(act(&mut menu, &mut ctx, Start), Transition::Pop),
            "saving closes the menu"
        );
        assert_eq!(fresh(&ctx, &game).title, "Mk", "trimmed");
        assert!(ctx.revision > revision, "screens reload");
        file_untouched(&ctx);
    }

    #[test]
    fn rename_by_typing_and_saving_with_enter() {
        let (_dir, mut ctx, game, mut menu) = renaming();
        menu.handle(Input::Text(TextInput::Backspace), &mut ctx);
        menu.handle(Input::Text(TextInput::Backspace), &mut ctx);
        type_in(&mut menu, &mut ctx, "  Pokémon Sapphire ");
        assert!(matches!(act(&mut menu, &mut ctx, Action::Start), Transition::Pop));
        assert_eq!(fresh(&ctx, &game).title, "Pokémon Sapphire");
        file_untouched(&ctx);
    }

    #[test]
    fn an_empty_name_or_the_cleaned_up_title_undoes_the_rename() {
        for typed in ["", "   ", " G0 "] {
            let (_dir, mut ctx, game, _) = renaming();
            ctx.library.rename(game.id, Some("Mine".into())).unwrap();
            let mut menu = OptionsMenu::new(fresh(&ctx, &game), &ctx.games_root);
            for _ in 0..3 {
                act(&mut menu, &mut ctx, Action::Down);
            }
            act(&mut menu, &mut ctx, Action::Confirm);
            for _ in 0.."Mine".len() {
                act(&mut menu, &mut ctx, Action::Options);
            }
            type_in(&mut menu, &mut ctx, typed);
            assert!(matches!(act(&mut menu, &mut ctx, Action::Start), Transition::Pop));
            assert_eq!(fresh(&ctx, &game).title, "G0", "{typed:?}");
            // Undone, not stored as "G0": a new cleaned-up title shows.
            let found = [FoundGame {
                system: "snes",
                rel_path: "snes/G0.sfc".into(),
                title: "G0 Deluxe".into(),
            }];
            ctx.library.sync_system("snes", &found, 0).unwrap();
            assert_eq!(fresh(&ctx, &game).title, "G0 Deluxe", "{typed:?}");
        }
    }

    #[test]
    fn cancelling_a_rename_changes_nothing() {
        let (_dir, mut ctx, game, mut menu) = renaming();
        let revision = ctx.revision;
        type_in(&mut menu, &mut ctx, "xyz");
        act(&mut menu, &mut ctx, Action::Confirm);
        assert!(matches!(act(&mut menu, &mut ctx, Action::Back), Transition::None));
        assert_eq!(menu.view(), View::List, "B goes back to the list");
        assert!(!menu.text_input());
        assert_eq!(menu.focused(), Item::Rename);
        assert!(matches!(
            act(&mut menu, &mut ctx, Action::Confirm),
            Transition::None
        ));
        assert_eq!(
            menu.entry.as_ref().unwrap().text(),
            "G0",
            "a new rename starts over"
        );
        assert!(matches!(
            menu.handle(Input::Pointer(Pointer::Back), &mut ctx),
            Transition::None
        ));
        assert!(matches!(act(&mut menu, &mut ctx, Action::Back), Transition::Pop));
        assert_eq!(fresh(&ctx, &game).title, "G0");
        assert_eq!(ctx.revision, revision);
        file_untouched(&ctx);
    }

    #[test]
    fn typed_qefs_and_space_reach_the_field_and_switch_no_tab() {
        use crate::ui::controls::{ControlEvent, Controls};
        use crate::ui::nav::{Flow, Hits, Nav};
        use sdl3::keyboard::Keycode;
        let (_dir, mut ctx, game, menu) = renaming();
        let mut nav = Nav::with_tabs(vec![
            ("Home", Box::new(OptionsMenu::new(game.clone(), &ctx.games_root))),
            (
                "Consoles",
                Box::new(OptionsMenu::new(game.clone(), &ctx.games_root)),
            ),
        ]);
        nav.push(Box::new(menu));
        let mut controls = Controls::new(InputKind::Keyboard);
        controls.text = nav.top().text_input();
        let mut inputs = Vec::new();
        for key in [Keycode::Q, Keycode::E, Keycode::F, Keycode::S, Keycode::Space] {
            let down = ControlEvent::KeyDown {
                key,
                repeat: false,
                ctrl: false,
            };
            controls.handle(0, down, &mut inputs);
            controls.handle(1, ControlEvent::KeyUp { key }, &mut inputs);
        }
        controls.typed("qefs ", &mut inputs);
        controls.handle(
            2,
            ControlEvent::KeyDown {
                key: Keycode::Return,
                repeat: false,
                ctrl: false,
            },
            &mut inputs,
        );
        for input in inputs {
            assert_eq!(nav.handle(input, &Hits::default(), &mut ctx), Flow::Continue);
            assert_eq!(nav.active_tab(), 0, "{input:?} switched tabs");
        }
        assert_eq!(nav.depth(), 1, "Enter saved and closed the menu");
        assert_eq!(
            fresh(&ctx, &game).title,
            "G0qefs",
            "no Options, Sort or Confirm changed the text"
        );
    }
}
