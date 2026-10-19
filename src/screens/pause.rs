//! The Pause Menu over a running Game (FR-EMB-3, FR-EMB-4, FR-EMB-5), after `screen_pause` in
//! `design/generate_mockups.py`. It only decides what the player asked for; the session beneath
//! it carries out saving, loading and quitting (see `session::sdl`).

use crate::ctx::Ctx;
use crate::session::states::{Rgb, SLOTS};
use crate::ui::cover::fit_lines;
use crate::ui::gfx::{Align, Gfx, TextStyle};
use crate::ui::input::{Action, Input, Pointer};
use crate::ui::nav::{Hint, Screen, Transition};
use crate::ui::text::Weight;
use crate::ui::theme::{ACCENT, Colour, DIM, LINE, MUTED, SURFACE, SURFACE2, TEXT, WARN};
use crate::ui::{H, M, Rect, W, hit};

/// What the player asked the session to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// Save to slot 1 to 3.
    Save(usize),
    Load(usize),
    Quit,
}

/// A full slot: when it was saved and its screenshot, if that could be read.
#[derive(Debug, Clone, PartialEq)]
pub struct SlotInfo {
    pub when: String,
    pub thumb: Option<Rgb>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    List,
    Save,
    Load,
}

const ITEMS: [&str; 4] = ["Resume", "Save state", "Load state", "Quit to library"];
const THUMB_KEYS: [&str; SLOTS] = ["pause-slot-1", "pause-slot-2", "pause-slot-3"];
const CARD_W: f32 = 360.0;
const CARD_H: f32 = 270.0;
const CARD_X: f32 = 680.0;
const CARD_Y: f32 = 366.0;
const CARD_PITCH: f32 = 392.0;
/// How dark the frozen Game is behind the menu: darker than the mockup's `dim(sv, 0.6)`, so the
/// slot labels stay legible over a bright Game.
const BACKDROP: f32 = 0.72;

struct Slot {
    /// `None` when empty.
    when: Option<String>,
    has_thumb: bool,
    error: Option<&'static str>,
}

pub struct PauseMenu {
    title: String,
    /// "<System> · <Playtime> played".
    meta: String,
    view: View,
    item: usize,
    /// The focused slot in a slot view; `None` in the load view when every slot is empty.
    slot: Option<usize>,
    slots: Vec<Slot>,
    /// Screenshots to hand to `Gfx` on the next draw: `None` forgets a slot's old one.
    uploads: Vec<(usize, Option<Rgb>)>,
    request: Option<Request>,
    item_hits: Vec<(Rect, usize)>,
    slot_hits: Vec<(Rect, usize)>,
}

impl PauseMenu {
    pub fn new(title: &str, meta: String, slots: Vec<Option<SlotInfo>>) -> PauseMenu {
        let mut menu = PauseMenu {
            title: title.to_owned(),
            meta,
            view: View::List,
            item: 0,
            slot: None,
            slots: Vec::new(),
            uploads: Vec::new(),
            request: None,
            item_hits: Vec::new(),
            slot_hits: Vec::new(),
        };
        for (i, info) in slots.into_iter().enumerate() {
            menu.slots.push(Slot {
                when: None,
                has_thumb: false,
                error: None,
            });
            menu.set_slot(i, info);
        }
        menu
    }

    fn set_slot(&mut self, i: usize, info: Option<SlotInfo>) {
        let (when, thumb) = info.map_or((None, None), |s| (Some(s.when), s.thumb));
        self.slots[i] = Slot {
            when,
            has_thumb: thumb.is_some(),
            error: None,
        };
        self.uploads.push((i, thumb));
    }

    pub fn view(&self) -> View {
        self.view
    }

    pub fn focused_slot(&self) -> Option<usize> {
        self.slot
    }

    /// The request the player made, for the session to carry out.
    pub fn take_request(&mut self) -> Option<Request> {
        self.request.take()
    }

    /// Slot `n` (1 to 3) was saved: show its new screenshot and time, or why it failed.
    pub fn saved(&mut self, n: usize, info: Result<SlotInfo, ()>) {
        match info {
            Ok(info) => self.set_slot(n - 1, Some(info)),
            Err(()) => self.slots[n - 1].error = Some("Couldn't save to this slot"),
        }
    }

    /// Slot `n` (1 to 3) couldn't be loaded.
    pub fn load_failed(&mut self, n: usize) {
        self.slots[n - 1].error = Some("Couldn't load this state");
    }

    fn selectable(&self, i: usize) -> bool {
        self.view == View::Save || self.slots[i].when.is_some()
    }

    fn open(&mut self, item: usize) -> Transition {
        self.item = item;
        match item {
            0 => return Transition::Pop,
            1 => {
                self.view = View::Save;
                self.slot = Some(0);
            }
            2 => {
                self.view = View::Load;
                self.slot = (0..SLOTS).find(|&i| self.selectable(i));
            }
            _ => self.request = Some(Request::Quit),
        }
        Transition::None
    }

    fn back_to_list(&mut self) {
        self.view = View::List;
        self.slot = None;
        for slot in &mut self.slots {
            slot.error = None;
        }
    }

    fn activate_slot(&mut self, i: usize) {
        if !self.selectable(i) {
            return;
        }
        self.slot = Some(i);
        self.request = Some(match self.view {
            View::Load => Request::Load(i + 1),
            _ => Request::Save(i + 1),
        });
    }

    /// Moves slot focus one step `dir` (-1 or 1), skipping slots that can't be chosen.
    fn move_slot(&mut self, dir: isize) {
        let Some(from) = self.slot else { return };
        let mut i = from as isize + dir;
        while (0..SLOTS as isize).contains(&i) {
            if self.selectable(i as usize) {
                self.slot = Some(i as usize);
                return;
            }
            i += dir;
        }
    }
}

impl Screen for PauseMenu {
    fn title(&self) -> &str {
        &self.title
    }

    fn is_modal(&self) -> bool {
        true
    }

    fn handle(&mut self, input: Input, _: &mut Ctx) -> Transition {
        let list = self.view == View::List;
        match input {
            Input::Action(Action::Up) if list => self.item = self.item.saturating_sub(1),
            Input::Action(Action::Down) if list => self.item = (self.item + 1).min(ITEMS.len() - 1),
            Input::Action(Action::Confirm) if list => return self.open(self.item),
            Input::Action(Action::Back) | Input::Pointer(Pointer::Back) if list => return Transition::Pop,
            Input::Action(Action::Left) if !list => self.move_slot(-1),
            Input::Action(Action::Right) if !list => self.move_slot(1),
            Input::Action(Action::Confirm) => {
                if let Some(i) = self.slot {
                    self.activate_slot(i);
                }
            }
            Input::Action(Action::Back) | Input::Pointer(Pointer::Back) => self.back_to_list(),
            Input::Pointer(Pointer::Move { x, y }) => {
                if let Some(i) = hit(&self.item_hits, x, y).filter(|_| list) {
                    self.item = i;
                } else if let Some(i) = hit(&self.slot_hits, x, y).filter(|&i| !list && self.selectable(i)) {
                    self.slot = Some(i);
                }
            }
            Input::Pointer(Pointer::Click { x, y }) => {
                if let Some(i) = hit(&self.item_hits, x, y) {
                    self.back_to_list();
                    return self.open(i);
                } else if let Some(i) = hit(&self.slot_hits, x, y).filter(|_| !list) {
                    self.activate_slot(i);
                }
            }
            _ => {}
        }
        Transition::None
    }

    fn draw(&mut self, gfx: &mut Gfx<'_>, _: &Ctx) {
        for (i, thumb) in self.uploads.drain(..) {
            gfx.set_image(THUMB_KEYS[i], thumb.as_ref());
        }
        gfx.fill_rect(Rect::new(0.0, 0.0, W, H), Colour::hex(0x000000).alpha(BACKDROP));
        gfx.fill_rect(Rect::new(0.0, 0.0, 600.0, H), SURFACE.alpha(0.94));
        gfx.fill_rect(Rect::new(600.0, 0.0, 1.0, H), LINE);
        let mut x = M;
        for c in "PAUSED".chars() {
            let style = TextStyle::new(15, Weight::Bold, ACCENT);
            x += gfx.text(x, 170.0, &c.to_string(), style, Align::Left) + 3.0;
        }
        let big = TextStyle::new(40, Weight::ExtraBold, TEXT);
        let title = fit_lines(&self.title, 600.0 - 2.0 * M + 48.0, 1, |s| gfx.measure(s, big));
        gfx.text(M, 228.0, title.first().map_or("", |s| s), big, Align::Left);
        let meta = TextStyle::new(19, Weight::Medium, MUTED);
        gfx.text(M, 266.0, &self.meta, meta, Align::Left);
        self.item_hits.clear();
        for (i, label) in ITEMS.iter().enumerate() {
            let y = 320.0 + i as f32 * 84.0;
            let r = Rect::new(M - 24.0, y, 456.0, 70.0);
            let on = i == self.item;
            if on {
                gfx.fill_round_rect(r, 14.0, SURFACE2);
                gfx.fill_round_rect(Rect::new(M - 24.0, y + 15.0, 5.0, 40.0), 2.0, ACCENT);
            }
            let weight = if on { Weight::Bold } else { Weight::Medium };
            gfx.text(M, y + 44.0, label, TextStyle::new(24, weight, TEXT), Align::Left);
            if i == 1 || i == 2 {
                let arrow = TextStyle::new(26, Weight::SemiBold, MUTED);
                gfx.text(M + 408.0, y + 44.0, "›", arrow, Align::Right);
            }
            self.item_hits.push((r, i));
        }
        let footer = TextStyle::new(16, Weight::Medium, DIM);
        gfx.text(
            M,
            960.0,
            "Opens with Guide, Select + Start, or Esc",
            footer,
            Align::Left,
        );
        self.slot_hits.clear();
        if self.view == View::List {
            return;
        }
        let heading = if self.view == View::Save {
            "Save to slot"
        } else {
            "Load from slot"
        };
        let heading_style = TextStyle::new(30, Weight::ExtraBold, TEXT);
        gfx.text(CARD_X, 330.0, heading, heading_style, Align::Left);
        for (i, key) in THUMB_KEYS.into_iter().enumerate() {
            let r = Rect::new(CARD_X + i as f32 * CARD_PITCH, CARD_Y, CARD_W, CARD_H);
            let fade = if self.selectable(i) { 1.0 } else { 0.4 };
            let slot = &self.slots[i];
            if self.slot == Some(i) {
                gfx.stroke_round_rect(r.inset(-6.0), 16.0, 4.0, TEXT);
            }
            if slot.when.is_some() && slot.has_thumb && gfx.draw_image(key, r) {
                gfx.stroke_round_rect(r, 12.0, 2.0, LINE);
            } else {
                gfx.fill_round_rect(r, 12.0, SURFACE.alpha(fade));
                gfx.stroke_round_rect_dashed(r, 12.0, 2.0, 10.0, 8.0, LINE.alpha(fade));
                if slot.when.is_none() {
                    let style = TextStyle::new(22, Weight::SemiBold, MUTED.alpha(fade));
                    gfx.text(
                        r.x + CARD_W / 2.0,
                        r.y + 146.0,
                        "+  Empty slot",
                        style,
                        Align::Centre,
                    );
                }
            }
            let name = TextStyle::new(22, Weight::Bold, TEXT.alpha(fade));
            gfx.text(r.x, r.y + 312.0, &format!("Slot {}", i + 1), name, Align::Left);
            let (line, colour) = match (slot.error, &slot.when) {
                (Some(error), _) => (error, WARN),
                (None, Some(when)) => (when.as_str(), MUTED),
                (None, None) => ("Empty", MUTED.alpha(fade)),
            };
            let style = TextStyle::new(18, Weight::Medium, colour);
            gfx.text(r.x, r.y + 340.0, line, style, Align::Left);
            self.slot_hits.push((r, i));
        }
    }

    fn hints(&self) -> Vec<Hint> {
        let (confirm, back) = match self.view {
            View::List => ("Select", "Resume"),
            View::Save => ("Save here", "Back"),
            View::Load => ("Load", "Back"),
        };
        vec![
            Hint::new(&[Action::Confirm], confirm),
            Hint::new(&[Action::Back], back),
        ]
    }

    fn pause_menu(&mut self) -> Option<&mut PauseMenu> {
        Some(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn act(menu: &mut PauseMenu, action: Action) -> Transition {
        menu.handle(Input::Action(action), &mut Ctx::empty())
    }

    fn full(when: &str) -> Option<SlotInfo> {
        Some(SlotInfo {
            when: when.to_owned(),
            thumb: None,
        })
    }

    #[test]
    fn save_state_opens_the_save_view_and_back_returns_to_the_list() {
        let mut menu = PauseMenu::new(
            "Chrono Trigger",
            "SNES · 0h 01m played".into(),
            vec![None, None, None],
        );
        assert!(matches!(act(&mut menu, Action::Down), Transition::None));
        act(&mut menu, Action::Confirm);
        assert_eq!((menu.view(), menu.focused_slot()), (View::Save, Some(0)));
        act(&mut menu, Action::Right);
        act(&mut menu, Action::Right);
        act(&mut menu, Action::Right);
        assert_eq!(
            menu.focused_slot(),
            Some(2),
            "empty slots can be saved to; focus stops at the end"
        );
        act(&mut menu, Action::Confirm);
        assert_eq!(menu.take_request(), Some(Request::Save(3)));
        assert_eq!(menu.take_request(), None);
        menu.saved(3, Ok(full("Today, 21:14").unwrap()));
        assert_eq!(menu.view(), View::Save, "stays in the save view");
        assert!(matches!(act(&mut menu, Action::Back), Transition::None));
        assert_eq!(menu.view(), View::List);
        assert!(
            matches!(act(&mut menu, Action::Back), Transition::Pop),
            "Back resumes"
        );
    }

    #[test]
    fn the_load_view_skips_empty_slots() {
        let mut menu = PauseMenu::new("T", String::new(), vec![None, full("a"), None]);
        act(&mut menu, Action::Down);
        act(&mut menu, Action::Down);
        act(&mut menu, Action::Confirm);
        assert_eq!((menu.view(), menu.focused_slot()), (View::Load, Some(1)));
        act(&mut menu, Action::Left);
        act(&mut menu, Action::Right);
        assert_eq!(menu.focused_slot(), Some(1));
        act(&mut menu, Action::Confirm);
        assert_eq!(menu.take_request(), Some(Request::Load(2)));

        let mut empty = PauseMenu::new("T", String::new(), vec![None, None, None]);
        empty.open(2);
        assert_eq!(empty.focused_slot(), None);
        act(&mut empty, Action::Confirm);
        assert_eq!(empty.take_request(), None, "nothing to load");
    }

    #[test]
    fn resume_pops_and_quit_asks_the_session() {
        let mut menu = PauseMenu::new("T", String::new(), vec![None, None, None]);
        assert!(matches!(act(&mut menu, Action::Confirm), Transition::Pop));
        for _ in 0..5 {
            act(&mut menu, Action::Down);
        }
        act(&mut menu, Action::Confirm);
        assert_eq!(menu.take_request(), Some(Request::Quit));
    }
}
