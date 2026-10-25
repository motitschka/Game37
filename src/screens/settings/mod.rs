//! Settings (FR-SET-1), after `settings_page` and `settings_row` in `design/generate_mockups.py`:
//! the sections down the left, the focused section's rows on the right. Up and Down move within a
//! column; Right or Confirm goes from a section into its rows, Left or Back goes back, and Back on
//! the sections leaves Settings. Each section lives in its own file as a [`Section`], so this file
//! only knows the focus, the layout and the mouse.

mod about;
mod artwork;
mod bios;
mod controls;
mod cores;
mod display;
mod hidden;
mod library;

use crate::ctx::Ctx;
use crate::ui::cover::ellipsize;
use crate::ui::gfx::{Align, Gfx, TextStyle};
use crate::ui::input::{Action, Input, Pointer};
use crate::ui::nav::{Entry, Hint, Screen, Transition};
use crate::ui::text::Weight;
use crate::ui::theme::{ACCENT, DANGER, LINE, MUTED, SURFACE, SURFACE2, TEXT, WARN};
use crate::ui::{M, Rect, W, hit};

/// The first section and the first row.
const TOP: f32 = 240.0;
const SECTION_PITCH: f32 = 76.0;
const SECTION_W: f32 = 360.0;
const SECTION_H: f32 = 64.0;
/// Left edge of the rows column.
const RX: f32 = M + 448.0;
const RW: f32 = W - M - RX;
const ROW_H: f32 = 76.0;
/// A row with a sub-line.
const ROW_SUB_H: f32 = 92.0;
const ROW_GAP: f32 = 12.0;
/// Extra space above a danger row, which stands apart from the rest.
const DANGER_GAP: f32 = 24.0;

/// One row of a section, as it is now.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub label: &'static str,
    /// Right-aligned, e.g. a path or a count.
    pub value: Option<String>,
    /// Whether the value is a warning, drawn in `WARN`.
    pub warn: bool,
    /// A muted line under the label.
    pub sub: Option<&'static str>,
    /// Whether it opens a sub-list, shown with a "›".
    pub opens: bool,
    /// A destructive action: drawn in `DANGER`, set apart below the others.
    pub danger: bool,
    /// What Confirm does, for the hint bar.
    pub verb: &'static str,
}

impl Row {
    pub fn new(label: &'static str) -> Row {
        Row {
            label,
            value: None,
            warn: false,
            sub: None,
            opens: false,
            danger: false,
            verb: "Open",
        }
    }

    pub fn value(self, value: impl Into<String>) -> Row {
        Row {
            value: Some(value.into()),
            ..self
        }
    }

    pub fn warn(self) -> Row {
        Row { warn: true, ..self }
    }

    pub fn sub(self, sub: &'static str) -> Row {
        Row {
            sub: Some(sub),
            ..self
        }
    }

    pub fn opens(self) -> Row {
        Row { opens: true, ..self }
    }

    pub fn danger(self) -> Row {
        Row { danger: true, ..self }
    }

    pub fn verb(self, verb: &'static str) -> Row {
        Row { verb, ..self }
    }
}

/// One section of Settings. A section with no rows (About) only shows text and can't be entered.
pub trait Section {
    fn name(&self) -> &'static str;
    /// Its rows as they are now. Called every frame and before every input, so it should be cheap.
    fn rows(&mut self, _ctx: &Ctx) -> Vec<Row> {
        Vec::new()
    }
    /// Confirm, or a click on the focused row, on row `i`.
    fn confirm(&mut self, _i: usize, _ctx: &mut Ctx) -> Transition {
        Transition::None
    }
    /// Input while row `i` has the focus, before Settings' own handling: a choice row takes Left
    /// and Right, say. `None` leaves the input to Settings.
    fn handle(&mut self, _i: usize, _input: Input, _ctx: &mut Ctx) -> Option<Transition> {
        None
    }
    /// What a section without rows shows in the rows column.
    fn draw_body(&mut self, _gfx: &mut Gfx<'_>, _ctx: &Ctx) {}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Sections,
    Rows,
}

pub struct Settings {
    sections: Vec<Box<dyn Section>>,
    section: usize,
    row: usize,
    focus: Focus,
    /// The focused section's rows as last read, for the hints and the mouse.
    rows: Vec<Row>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings::new()
    }
}

impl Settings {
    pub fn new() -> Settings {
        Settings::with_sections(vec![
            Box::new(library::Library::default()),
            Box::new(artwork::Artwork),
            Box::new(bios::Bios),
            Box::new(cores::Cores),
            Box::new(controls::Controls),
            Box::new(display::Display),
            Box::new(about::About),
        ])
    }

    fn with_sections(sections: Vec<Box<dyn Section>>) -> Settings {
        Settings {
            sections,
            section: 0,
            row: 0,
            focus: Focus::Sections,
            rows: Vec::new(),
        }
    }

    pub fn focus(&self) -> Focus {
        self.focus
    }

    /// The focused section's name.
    pub fn section(&self) -> &'static str {
        self.sections[self.section].name()
    }

    /// The focused row's index within its section.
    pub fn row(&self) -> usize {
        self.row
    }

    /// Re-reads the focused section's rows and keeps the focus on one that exists.
    fn refresh(&mut self, ctx: &Ctx) {
        self.rows = self.sections[self.section].rows(ctx);
        self.row = self.row.min(self.rows.len().saturating_sub(1));
        if self.rows.is_empty() {
            self.focus = Focus::Sections;
        }
    }

    fn focus_section(&mut self, i: usize, ctx: &Ctx) {
        if i != self.section {
            self.section = i;
            self.row = 0;
        }
        self.focus = Focus::Sections;
        self.refresh(ctx);
    }

    /// Moves the focus into the focused section's first row, if it has rows.
    fn enter(&mut self) {
        if !self.rows.is_empty() {
            self.focus = Focus::Rows;
            self.row = 0;
        }
    }

    fn move_row(&mut self, dir: isize) {
        let last = self.rows.len().saturating_sub(1) as isize;
        self.row = (self.row as isize + dir).clamp(0, last) as usize;
    }

    fn click(&mut self, x: f32, y: f32, ctx: &mut Ctx) -> Transition {
        let sections: Vec<_> = (0..self.sections.len()).map(|i| (section_rect(i), i)).collect();
        if let Some(i) = hit(&sections, x, y) {
            if i == self.section && self.focus == Focus::Sections {
                self.enter();
            } else {
                self.focus_section(i, ctx);
            }
        } else if let Some(i) = hit(&row_rects(&self.rows), x, y) {
            if i == self.row && self.focus == Focus::Rows {
                return self.sections[self.section].confirm(i, ctx);
            }
            self.focus = Focus::Rows;
            self.row = i;
        }
        Transition::None
    }

    fn draw_sections(&self, gfx: &mut Gfx<'_>) {
        for (i, section) in self.sections.iter().enumerate() {
            let r = section_rect(i);
            let on = i == self.section;
            let ink = match (on, self.focus) {
                (true, Focus::Sections) => {
                    gfx.fill_round_rect(r, 14.0, SURFACE2);
                    gfx.fill_round_rect(Rect::new(r.x, r.y + 12.0, 5.0, 40.0), 2.0, ACCENT);
                    TextStyle::new(24, Weight::Bold, TEXT)
                }
                (true, Focus::Rows) => {
                    gfx.fill_round_rect(r, 14.0, SURFACE);
                    TextStyle::new(24, Weight::Bold, ACCENT)
                }
                (false, _) => TextStyle::new(24, Weight::Medium, MUTED),
            };
            gfx.text(M, r.y + 41.0, section.name(), ink, Align::Left);
        }
    }

    fn draw_rows(&self, gfx: &mut Gfx<'_>) {
        for (row, (r, i)) in self.rows.iter().zip(row_rects(&self.rows)) {
            let focused = self.focus == Focus::Rows && i == self.row;
            if focused {
                gfx.fill_round_rect(r, 14.0, SURFACE2);
                gfx.fill_round_rect(Rect::new(r.x, r.y + r.h / 2.0 - 20.0, 5.0, 40.0), 2.0, ACCENT);
            } else {
                gfx.fill_round_rect(r, 14.0, SURFACE.alpha(0.6));
            }
            let base = r.y + r.h / 2.0 + if row.sub.is_some() { -6.0 } else { 8.0 };
            let weight = if focused { Weight::Bold } else { Weight::SemiBold };
            let ink = if row.danger { DANGER } else { TEXT };
            let label = TextStyle::new(22, weight, ink);
            let label_w = gfx.text(RX + 32.0, base, row.label, label, Align::Left);
            if let Some(sub) = row.sub {
                let style = TextStyle::new(16, Weight::Medium, MUTED);
                gfx.text(RX + 32.0, base + 30.0, sub, style, Align::Left);
            }
            let mut right = RX + RW - 32.0;
            if row.opens {
                let arrow = TextStyle::new(26, Weight::SemiBold, MUTED);
                gfx.text(right, r.y + r.h / 2.0 + 9.0, "›", arrow, Align::Right);
                right -= 36.0;
            }
            if let Some(value) = &row.value {
                let style = TextStyle::new(18, Weight::Medium, if row.warn { WARN } else { MUTED });
                let room = right - (RX + 32.0 + label_w + 48.0);
                let value = ellipsize(value, room, |s| gfx.measure(s, style));
                gfx.text(right, r.y + r.h / 2.0 + 7.0, &value, style, Align::Right);
            }
        }
    }
}

/// Where section `i` is drawn.
fn section_rect(i: usize) -> Rect {
    Rect::new(M - 24.0, TOP + i as f32 * SECTION_PITCH, SECTION_W, SECTION_H)
}

/// Where each row is drawn, with its index.
fn row_rects(rows: &[Row]) -> Vec<(Rect, usize)> {
    let mut y = TOP;
    let mut rects = Vec::with_capacity(rows.len());
    for (i, row) in rows.iter().enumerate() {
        if row.danger && i > 0 {
            y += DANGER_GAP;
        }
        let h = if row.sub.is_some() { ROW_SUB_H } else { ROW_H };
        rects.push((Rect::new(RX, y, RW, h), i));
        y += h + ROW_GAP;
    }
    rects
}

impl Screen for Settings {
    fn title(&self) -> &str {
        "Settings"
    }

    fn handle(&mut self, input: Input, ctx: &mut Ctx) -> Transition {
        self.refresh(ctx);
        if self.focus == Focus::Rows
            && let Some(transition) = self.sections[self.section].handle(self.row, input, ctx)
        {
            return transition;
        }
        match (self.focus, input) {
            (Focus::Sections, Input::Action(Action::Up)) => {
                self.focus_section(self.section.saturating_sub(1), ctx)
            }
            (Focus::Sections, Input::Action(Action::Down)) => {
                self.focus_section((self.section + 1).min(self.sections.len() - 1), ctx)
            }
            (Focus::Sections, Input::Action(Action::Right | Action::Confirm)) => self.enter(),
            (Focus::Sections, Input::Action(Action::Back)) => return Transition::Pop,
            (Focus::Rows, Input::Action(Action::Up)) => self.move_row(-1),
            (Focus::Rows, Input::Action(Action::Down)) => self.move_row(1),
            (Focus::Rows, Input::Action(Action::Left | Action::Back)) => self.focus = Focus::Sections,
            (Focus::Rows, Input::Action(Action::Confirm)) => {
                return self.sections[self.section].confirm(self.row, ctx);
            }
            (_, Input::Pointer(Pointer::Click { x, y })) => return self.click(x, y, ctx),
            _ => {}
        }
        Transition::None
    }

    fn draw(&mut self, gfx: &mut Gfx<'_>, ctx: &Ctx) {
        self.refresh(ctx);
        gfx.text(
            M,
            196.0,
            "Settings",
            TextStyle::new(48, Weight::ExtraBold, TEXT),
            Align::Left,
        );
        self.draw_sections(gfx);
        gfx.fill_rect(Rect::new(M + 400.0, TOP, 1.0, 740.0), LINE);
        if self.rows.is_empty() {
            self.sections[self.section].draw_body(gfx, ctx);
        } else {
            self.draw_rows(gfx);
        }
    }

    fn hints(&self) -> Vec<Hint> {
        let verb = match self.focus {
            Focus::Sections => (!self.rows.is_empty()).then_some("Open"),
            Focus::Rows => self.rows.get(self.row).map(|r| r.verb),
        };
        let back = Hint::new(&[Action::Back], "Back");
        match verb {
            Some(verb) => vec![Hint::new(&[Action::Confirm], verb), back],
            None => vec![back],
        }
    }

    fn entry(&self) -> Option<Entry> {
        Some(Entry::Settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// A section with `n` rows that logs what reaches it.
    struct Fake {
        name: &'static str,
        n: usize,
        log: Rc<RefCell<Vec<String>>>,
    }

    impl Section for Fake {
        fn name(&self) -> &'static str {
            self.name
        }

        fn rows(&mut self, _: &Ctx) -> Vec<Row> {
            (0..self.n).map(|_| Row::new("Row").verb("Do")).collect()
        }

        fn confirm(&mut self, i: usize, _: &mut Ctx) -> Transition {
            self.log.borrow_mut().push(format!("confirm {i}"));
            Transition::None
        }

        fn handle(&mut self, i: usize, input: Input, _: &mut Ctx) -> Option<Transition> {
            let Input::Action(Action::Right) = input else {
                return None;
            };
            self.log.borrow_mut().push(format!("right {i}"));
            Some(Transition::None)
        }
    }

    /// Settings with a section of three rows, an empty one and one of two rows.
    fn settings() -> (Settings, Rc<RefCell<Vec<String>>>) {
        let log = Rc::new(RefCell::new(Vec::new()));
        let fake = |name, n| -> Box<dyn Section> {
            Box::new(Fake {
                name,
                n,
                log: log.clone(),
            })
        };
        let s = Settings::with_sections(vec![fake("A", 3), fake("Empty", 0), fake("B", 2)]);
        (s, log)
    }

    fn act(s: &mut Settings, ctx: &mut Ctx, action: Action) -> Transition {
        s.handle(Input::Action(action), ctx)
    }

    fn click(s: &mut Settings, ctx: &mut Ctx, r: Rect) -> Transition {
        let (x, y) = (r.x + 10.0, r.y + 10.0);
        s.handle(Input::Pointer(Pointer::Click { x, y }), ctx)
    }

    fn at(s: &Settings) -> (Focus, &'static str, usize) {
        (s.focus(), s.section(), s.row())
    }

    #[test]
    fn the_sections_are_in_order_and_the_screen_is_the_settings_entry() {
        let s = Settings::new();
        let names: Vec<_> = s.sections.iter().map(|s| s.name()).collect();
        assert_eq!(
            names,
            [
                "Library", "Artwork", "BIOS", "Cores", "Controls", "Display", "About"
            ]
        );
        assert_eq!((s.title(), s.entry()), ("Settings", Some(Entry::Settings)));
    }

    #[test]
    fn up_and_down_move_through_the_sections_and_stop_at_the_ends() {
        let (mut s, _) = settings();
        let ctx = &mut Ctx::empty();
        act(&mut s, ctx, Action::Up);
        assert_eq!(at(&s), (Focus::Sections, "A", 0));
        act(&mut s, ctx, Action::Down);
        assert_eq!(s.section(), "Empty");
        act(&mut s, ctx, Action::Down);
        act(&mut s, ctx, Action::Down);
        assert_eq!(s.section(), "B");
    }

    #[test]
    fn right_or_confirm_enters_a_section_and_left_or_back_returns_to_it() {
        for enter in [Action::Right, Action::Confirm] {
            for leave in [Action::Left, Action::Back] {
                let (mut s, log) = settings();
                let ctx = &mut Ctx::empty();
                act(&mut s, ctx, enter);
                assert_eq!(at(&s), (Focus::Rows, "A", 0));
                act(&mut s, ctx, Action::Down);
                act(&mut s, ctx, Action::Down);
                act(&mut s, ctx, Action::Down);
                assert_eq!(s.row(), 2, "stops at the last row");
                act(&mut s, ctx, Action::Up);
                assert_eq!(s.row(), 1);
                assert!(matches!(act(&mut s, ctx, leave), Transition::None));
                assert_eq!(at(&s), (Focus::Sections, "A", 1), "the section stays focused");
                assert!(log.borrow().is_empty(), "nothing acted");
            }
        }
    }

    #[test]
    fn back_on_the_sections_leaves_settings() {
        let (mut s, _) = settings();
        let ctx = &mut Ctx::empty();
        act(&mut s, ctx, Action::Down);
        assert!(matches!(act(&mut s, ctx, Action::Back), Transition::Pop));
    }

    #[test]
    fn a_section_without_rows_cant_be_entered() {
        let (mut s, _) = settings();
        let ctx = &mut Ctx::empty();
        act(&mut s, ctx, Action::Down);
        for action in [Action::Right, Action::Confirm] {
            act(&mut s, ctx, action);
            assert_eq!(at(&s), (Focus::Sections, "Empty", 0));
        }
        assert_eq!(s.hints(), [Hint::new(&[Action::Back], "Back")]);
        let mut real = Settings::new();
        for _ in 0..6 {
            act(&mut real, ctx, Action::Down);
        }
        act(&mut real, ctx, Action::Right);
        assert_eq!(at(&real), (Focus::Sections, "About", 0));
    }

    #[test]
    fn confirm_on_a_row_goes_to_its_section_and_so_does_input_the_section_takes() {
        let (mut s, log) = settings();
        let ctx = &mut Ctx::empty();
        act(&mut s, ctx, Action::Right);
        act(&mut s, ctx, Action::Down);
        act(&mut s, ctx, Action::Confirm);
        act(&mut s, ctx, Action::Right);
        assert_eq!(*log.borrow(), ["confirm 1", "right 1"]);
        assert_eq!(at(&s), (Focus::Rows, "A", 1));
    }

    #[test]
    fn the_hints_name_what_confirm_does_where_the_focus_is() {
        let (mut s, _) = settings();
        let ctx = &mut Ctx::empty();
        act(&mut s, ctx, Action::Up);
        let back = Hint::new(&[Action::Back], "Back");
        assert_eq!(s.hints(), [Hint::new(&[Action::Confirm], "Open"), back.clone()]);
        act(&mut s, ctx, Action::Right);
        assert_eq!(s.hints(), [Hint::new(&[Action::Confirm], "Do"), back]);
    }

    #[test]
    fn a_click_focuses_a_section_or_a_row_and_a_click_on_the_focused_row_acts() {
        let (mut s, log) = settings();
        let ctx = &mut Ctx::empty();
        act(&mut s, ctx, Action::Up);
        click(&mut s, ctx, section_rect(2));
        assert_eq!(at(&s), (Focus::Sections, "B", 0));
        click(&mut s, ctx, section_rect(2));
        assert_eq!(
            at(&s),
            (Focus::Rows, "B", 0),
            "a click on the focused section enters it"
        );
        click(&mut s, ctx, section_rect(0));
        assert_eq!(at(&s), (Focus::Sections, "A", 0));
        let rows = row_rects(&s.rows);
        click(&mut s, ctx, rows[2].0);
        assert_eq!(at(&s), (Focus::Rows, "A", 2));
        assert!(log.borrow().is_empty());
        click(&mut s, ctx, rows[2].0);
        assert_eq!(*log.borrow(), ["confirm 2"]);
    }

    #[test]
    fn a_danger_row_stands_apart_and_a_row_with_a_sub_line_is_taller() {
        let rows = [Row::new("a"), Row::new("b"), Row::new("c").sub("s").danger()];
        let rects: Vec<_> = row_rects(&rows).into_iter().map(|(r, _)| (r.y, r.h)).collect();
        assert_eq!(rects, [(240.0, 76.0), (328.0, 76.0), (440.0, 92.0)]);
    }
}
