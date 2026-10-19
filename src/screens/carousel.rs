//! The Carousel (FR-CAR-2 to FR-CAR-7): every System in System Order as a wrapping row of cards,
//! the focused one large in the centre, after `screen_carousel` and `console_card` in
//! `design/generate_mockups.py`.

use std::time::Instant;

use crate::ctx::Ctx;
use crate::library::db::Library;
use crate::systems::{self, SYSTEMS, System};
use crate::ui::gfx::{Align, Gfx, TextStyle};
use crate::ui::input::{Action, Input, Pointer};
use crate::ui::nav::{Hint, Screen, Transition};
use crate::ui::text::Weight;
use crate::ui::theme::{BG, Colour, MUTED, TEXT};
use crate::ui::{H, Rect, W};

use super::Stub;

/// Vertical centre of the cards.
const CARD_Y: f32 = 450.0;

/// Where a card sits and how it looks at a distance from the focused card.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Slot {
    cx: f32,
    size: f32,
    opacity: f32,
}

/// The slots at distances 0, 1, 2 to the right of the focused card, then off screen, where cards
/// slide in from and out to. Left of it they mirror around the screen's centre.
const SLOTS: [Slot; 4] = [
    Slot {
        cx: 960.0,
        size: 500.0,
        opacity: 1.0,
    },
    Slot {
        cx: 1450.0,
        size: 310.0,
        opacity: 0.55,
    },
    Slot {
        cx: 1760.0,
        size: 210.0,
        opacity: 0.3,
    },
    Slot {
        cx: 2070.0,
        size: 150.0,
        opacity: 0.0,
    },
];

/// How far from the focused card cards are still drawn.
const REACH: f32 = (SLOTS.len() - 1) as f32;

/// Opacity a card is multiplied by when its System has no Games (FR-CAR-4).
const DIM: f32 = 0.55;

/// Time constant of the eased slide: the cards cover about 98% of a step in 150 ms.
const SLIDE_TAU_SECS: f32 = 0.038;

/// The System `steps` away from `index` in System Order, wrapping around both ends (FR-CAR-7).
/// A font size for text that scales with a sliding card, snapped to `step` pixels so a slide
/// reuses a handful of font faces and cached textures instead of making new ones every frame.
fn text_size(px: f32, step: f32) -> u16 {
    ((px / step).round() * step).max(step) as u16
}

fn wrap(index: usize, steps: i32) -> usize {
    (index as i32 + steps).rem_euclid(SYSTEMS.len() as i32) as usize
}

/// The slot of a card `offset` cards right of the focused one (left when negative), interpolated
/// between slots while the cards slide.
fn slot(offset: f32) -> Slot {
    let d = offset.abs().min(REACH);
    let (i, t) = (d.floor() as usize, d.fract());
    let (a, b) = (SLOTS[i], SLOTS[(i + 1).min(SLOTS.len() - 1)]);
    let lerp = |x: f32, y: f32| x + (y - x) * t;
    let cx = lerp(a.cx, b.cx);
    Slot {
        cx: if offset < 0.0 { W - cx } else { cx },
        size: lerp(a.size, b.size),
        opacity: lerp(a.opacity, b.opacity),
    }
}

/// The cards to draw when the row is at `position` (a System index, fractional while sliding,
/// possibly outside System Order's range): each System's index and its offset from the centre,
/// farthest first so nearer cards draw over them.
fn visible(position: f32) -> Vec<(usize, f32)> {
    let base = position.floor() as i32;
    let mut cards: Vec<(usize, f32)> = (base - REACH as i32..=base + REACH as i32 + 1)
        .map(|k| (k, k as f32 - position))
        .filter(|&(_, offset)| offset.abs() < REACH)
        .map(|(k, offset)| (wrap(0, k), offset))
        .collect();
    cards.sort_by(|a, b| b.1.abs().total_cmp(&a.1.abs()));
    cards
}

/// A card's opacity at `offset` from the centre, dimmed when its System has no Games (FR-CAR-4).
fn card_opacity(offset: f32, games: usize) -> f32 {
    slot(offset).opacity * if games == 0 { DIM } else { 1.0 }
}

/// The settled square a card fills at a whole-number `offset`.
fn card_rect(offset: i32) -> Rect {
    let s = slot(offset as f32);
    Rect::new(s.cx - s.size / 2.0, CARD_Y - s.size / 2.0, s.size, s.size)
}

/// Offset from the focused card of the card under a point, if any.
fn card_at(x: f32, y: f32) -> Option<i32> {
    let reach = REACH as i32 - 1;
    (-reach..=reach).find(|&offset| card_rect(offset).contains(x, y))
}

/// The focused System's index in System Order, from the System stored as last viewed: NES on the
/// first launch, and when the stored one can't be read (FR-CAR-6).
fn restore(library: &Library) -> usize {
    library
        .last_system()
        .map_err(|e| eprintln!("game37: can't read the last System: {e}"))
        .ok()
        .and_then(systems::index_of)
        .unwrap_or(0)
}

/// "12 games", "1 game" or "No games".
fn count_label(games: usize) -> String {
    match games {
        0 => "No games".to_owned(),
        1 => "1 game".to_owned(),
        n => format!("{n} games"),
    }
}

/// Splits a System name into lines of at most `width` characters at spaces, like Python's
/// `textwrap.wrap` in the mockups.
fn wrap_name(name: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in name.split(' ') {
        match lines.last_mut() {
            Some(line) if line.chars().count() + 1 + word.chars().count() <= width => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_owned()),
        }
    }
    lines
}

pub struct Carousel {
    /// Where the row is heading, as an index that keeps counting past either end, so a wrap slides
    /// the short way. The focused System is this modulo the number of Systems.
    target: i32,
    /// Where the row is drawn, easing towards `target`.
    position: f32,
    last_frame: Option<Instant>,
    /// Game counts in System Order, and the library revision they were read at.
    counts: [usize; SYSTEMS.len()],
    counted: Option<u64>,
}

impl Carousel {
    /// Opens on the System last viewed (FR-CAR-6).
    pub fn new(library: &Library) -> Carousel {
        let focus = restore(library);
        Carousel {
            target: focus as i32,
            position: focus as f32,
            last_frame: None,
            counts: [0; SYSTEMS.len()],
            counted: None,
        }
    }

    fn focus(&self) -> usize {
        wrap(0, self.target)
    }

    fn system(&self) -> &'static System {
        &SYSTEMS[self.focus()]
    }

    /// Moves the focus `steps` Systems right (left when negative) and remembers it (FR-CAR-6).
    fn step(&mut self, steps: i32, ctx: &Ctx) {
        self.target += steps;
        // Keep both in range together so the slide in progress carries on.
        let n = SYSTEMS.len() as i32;
        let shift = self.target.div_euclid(n) * n;
        self.target -= shift;
        self.position -= shift as f32;
        if let Err(e) = ctx.library.set_last_system(self.system().id) {
            eprintln!("game37: can't save the last System: {e}");
        }
    }

    fn open(&self) -> Transition {
        Transition::Push(Box::new(Stub::new(self.system().name)))
    }

    /// Re-reads the Game counts when the library changed.
    fn refresh(&mut self, ctx: &Ctx) {
        if self.counted == Some(ctx.revision) {
            return;
        }
        match ctx.library.system_counts() {
            Ok(counts) => {
                for (count, system) in self.counts.iter_mut().zip(&SYSTEMS) {
                    *count = counts.get(system.id).copied().unwrap_or(0);
                }
                self.counted = Some(ctx.revision);
            }
            Err(e) => eprintln!("game37: can't count the Games: {e}"),
        }
    }

    /// Eases `position` towards `target`, or jumps there when animation is off.
    fn advance(&mut self, animate: bool) {
        let now = Instant::now();
        let dt = self
            .last_frame
            .map_or(0.0, |last| (now - last).as_secs_f32().min(0.1));
        self.last_frame = Some(now);
        let gap = self.target as f32 - self.position;
        if !animate || gap.abs() < 0.002 {
            self.position = self.target as f32;
        } else {
            self.position += gap * (1.0 - (-dt / SLIDE_TAU_SECS).exp());
        }
    }

    fn draw_card(&self, gfx: &mut Gfx<'_>, system: &System, offset: f32, opacity: f32) {
        let Slot { cx, size, .. } = slot(offset);
        let cy = CARD_Y;
        let card = Rect::new(cx - size / 2.0, cy - size / 2.0, size, size);
        let colour = Colour::hex(system.colour);
        gfx.fill_round_rect_gradient(
            card,
            size * 0.08,
            colour.alpha(opacity),
            BG.alpha(opacity),
            (0.7, 1.0),
        );
        let white = Colour::hex(0xFFFFFF);
        let (ox, oy) = (cx + size * 0.2, cy - size * 0.2);
        gfx.fill_circle(ox, oy, size * 0.26, white.alpha(0.07 * opacity));
        gfx.fill_circle(ox, oy, size * 0.14, white.alpha(0.08 * opacity));

        let fs = size * if system.name.len() < 10 { 0.13 } else { 0.095 };
        let name = TextStyle::new(text_size(fs, 4.0), Weight::ExtraBold, TEXT.alpha(opacity));
        let lines = wrap_name(system.name, 12);
        let last = lines.len() as f32 - 1.0;
        for (i, line) in lines.iter().enumerate() {
            let y = cy + size * 0.18 + (i as f32 - last) * fs * 1.1;
            gfx.text(cx, y, line, name, Align::Centre);
        }
        let caption = TextStyle::new(
            text_size(size * 0.045, 2.0),
            Weight::Medium,
            TEXT.alpha(0.75 * opacity),
        );
        gfx.text(cx, cy + size * 0.32, &system.caption(), caption, Align::Centre);
    }
}

impl Screen for Carousel {
    fn title(&self) -> &str {
        "Consoles"
    }

    fn handle(&mut self, input: Input, ctx: &mut Ctx) -> Transition {
        match input {
            Input::Action(Action::Left) => self.step(-1, ctx),
            Input::Action(Action::Right) => self.step(1, ctx),
            Input::Action(Action::Confirm) => return self.open(),
            Input::Pointer(Pointer::Wheel { dy }) if dy > 0.0 => self.step(-1, ctx),
            Input::Pointer(Pointer::Wheel { dy }) if dy < 0.0 => self.step(1, ctx),
            Input::Pointer(Pointer::Click { x, y }) => match card_at(x, y) {
                Some(0) => return self.open(),
                Some(offset) => self.step(offset, ctx),
                None => {}
            },
            _ => {}
        }
        Transition::None
    }

    fn draw(&mut self, gfx: &mut Gfx<'_>, ctx: &Ctx) {
        self.refresh(ctx);
        self.advance(ctx.animate);

        // The System's colour tints the screen in place of the accent glow, blending while the
        // cards slide.
        let from = wrap(0, self.position.floor() as i32);
        let tint = Colour::hex(SYSTEMS[from].colour).mix(
            Colour::hex(SYSTEMS[wrap(from, 1)].colour),
            self.position - self.position.floor(),
        );
        gfx.clear(BG);
        gfx.radial_glow(W / 2.0, 0.42 * H, 0.6 * W, 0.6 * H, tint, 0.55);

        for (index, offset) in visible(self.position) {
            let opacity = card_opacity(offset, self.counts[index]);
            self.draw_card(gfx, &SYSTEMS[index], offset, opacity);
        }
        let frame = card_rect(0).inset(-5.0);
        gfx.stroke_round_rect(frame, 46.0, 4.0, Colour::hex(0xFFFFFF));

        gfx.text(
            W / 2.0,
            790.0,
            &count_label(self.counts[self.focus()]),
            TextStyle::new(26, Weight::SemiBold, MUTED),
            Align::Centre,
        );

        let n = SYSTEMS.len();
        for i in 0..n {
            let x = W / 2.0 + (i as f32 - (n - 1) as f32 / 2.0) * 22.0;
            if i == self.focus() {
                gfx.fill_round_rect(Rect::new(x - 12.0, 846.0, 24.0, 8.0), 4.0, Colour::hex(0xFFFFFF));
            } else {
                gfx.fill_circle(x, 850.0, 4.0, Colour::hex(0xFFFFFF).alpha(0.3));
            }
        }
    }

    fn hints(&self) -> Vec<Hint> {
        vec![
            Hint::new(&[Action::Confirm], "Open"),
            Hint::new(&[Action::PrevTab, Action::NextTab], "Switch view"),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sliding_text_sizes_snap_to_steps() {
        assert_eq!(text_size(65.0, 4.0), 64);
        assert_eq!(text_size(63.1, 4.0), 64);
        assert_eq!(text_size(1.0, 2.0), 2);
        let sizes: std::collections::BTreeSet<_> =
            (150..=500).map(|s| text_size(s as f32 * 0.13, 4.0)).collect();
        assert!(sizes.len() <= 13, "{sizes:?}");
    }
    use crate::library::FoundGame;

    fn at(id: &str) -> usize {
        systems::index_of(id).unwrap()
    }

    fn act(carousel: &mut Carousel, ctx: &mut Ctx, input: Input) -> Transition {
        carousel.handle(input, ctx)
    }

    fn opened(t: Transition) -> Option<String> {
        match t {
            Transition::Push(screen) => Some(screen.title().to_owned()),
            _ => None,
        }
    }

    #[test]
    fn wrap_goes_round_both_ends() {
        assert_eq!(wrap(at("ps4"), 1), at("nes"));
        assert_eq!(wrap(at("nes"), -1), at("ps4"));
        assert_eq!(wrap(at("nes"), 1), at("gb"));
        assert_eq!(wrap(at("nes"), -2), at("ps3"));
        assert_eq!(wrap(at("nes"), SYSTEMS.len() as i32), at("nes"));
    }

    #[test]
    fn right_on_ps4_goes_to_nes_and_left_on_nes_goes_to_ps4() {
        let mut ctx = Ctx::empty();
        ctx.library.set_last_system("ps4").unwrap();
        let mut carousel = Carousel::new(&ctx.library);
        assert_eq!(carousel.system().id, "ps4");
        act(&mut carousel, &mut ctx, Input::Action(Action::Right));
        assert_eq!(carousel.system().id, "nes");
        act(&mut carousel, &mut ctx, Input::Action(Action::Left));
        assert_eq!(carousel.system().id, "ps4");
        for _ in 0..SYSTEMS.len() * 3 {
            act(&mut carousel, &mut ctx, Input::Action(Action::Left));
        }
        assert_eq!(carousel.system().id, "ps4", "whole laps come back to the start");
        assert!((0..SYSTEMS.len() as i32).contains(&carousel.target));
    }

    #[test]
    fn settled_cards_are_the_focus_and_two_neighbours_each_side_wrapping() {
        let mut cards = visible(at("nes") as f32);
        cards.sort_by(|a, b| a.1.total_cmp(&b.1));
        let expected = [
            (at("ps3"), -2.0),
            (at("ps4"), -1.0),
            (at("nes"), 0.0),
            (at("gb"), 1.0),
            (at("snes"), 2.0),
        ];
        assert_eq!(cards, expected);
        assert_eq!(
            visible(at("nes") as f32).last(),
            Some(&(at("nes"), 0.0)),
            "the focus draws last"
        );
    }

    #[test]
    fn sliding_cards_include_the_one_coming_in() {
        let offsets: Vec<f32> = visible(0.5).into_iter().map(|(_, o)| o).collect();
        assert_eq!(offsets.len(), 6);
        assert!(offsets.contains(&-2.5) && offsets.contains(&2.5));
    }

    #[test]
    fn slots_mirror_and_interpolate() {
        assert_eq!(slot(0.0).cx, 960.0);
        assert_eq!(slot(-1.0).cx, 470.0);
        assert_eq!(slot(1.0).cx, 1450.0);
        assert_eq!(slot(-2.0).size, 210.0);
        assert_eq!(slot(0.5).size, 405.0);
        assert_eq!(slot(-0.5).cx, 715.0);
    }

    #[test]
    fn empty_systems_are_dimmed() {
        assert_eq!(card_opacity(0.0, 3), 1.0);
        assert_eq!(card_opacity(0.0, 0), DIM);
        assert_eq!(card_opacity(1.0, 1), 0.55);
        assert!((card_opacity(1.0, 0) - 0.55 * DIM).abs() < 1e-6);
    }

    #[test]
    fn counts_label_the_focused_system() {
        assert_eq!(count_label(0), "No games");
        assert_eq!(count_label(1), "1 game");
        assert_eq!(count_label(12), "12 games");
    }

    #[test]
    fn clicks_map_to_the_card_under_them() {
        assert_eq!(card_at(960.0, 450.0), Some(0));
        assert_eq!(card_at(710.0, 200.0), Some(0));
        assert_eq!(card_at(470.0, 450.0), Some(-1));
        assert_eq!(card_at(1450.0, 590.0), Some(1));
        assert_eq!(card_at(160.0, 450.0), Some(-2));
        assert_eq!(card_at(1760.0, 450.0), Some(2));
        assert_eq!(card_at(1250.0, 450.0), None, "the gap between cards");
        assert_eq!(card_at(960.0, 790.0), None, "below the cards");
    }

    #[test]
    fn clicking_the_centre_opens_and_a_side_card_moves_focus() {
        let mut ctx = Ctx::empty();
        let mut carousel = Carousel::new(&ctx.library);
        let click = |x, y| Input::Pointer(Pointer::Click { x, y });
        assert_eq!(opened(act(&mut carousel, &mut ctx, click(160.0, 450.0))), None);
        assert_eq!(carousel.system().id, "ps3");
        assert_eq!(opened(act(&mut carousel, &mut ctx, click(1450.0, 450.0))), None);
        assert_eq!(carousel.system().id, "ps4");
        assert_eq!(
            opened(act(&mut carousel, &mut ctx, click(960.0, 450.0))).as_deref(),
            Some("PlayStation 4")
        );
        assert_eq!(opened(act(&mut carousel, &mut ctx, click(960.0, 900.0))), None);
    }

    #[test]
    fn confirm_opens_the_focused_system_and_the_wheel_moves() {
        let mut ctx = Ctx::empty();
        let mut carousel = Carousel::new(&ctx.library);
        act(
            &mut carousel,
            &mut ctx,
            Input::Pointer(Pointer::Wheel { dy: -1.0 }),
        );
        assert_eq!(carousel.system().id, "gb");
        act(
            &mut carousel,
            &mut ctx,
            Input::Pointer(Pointer::Wheel { dy: 1.0 }),
        );
        act(
            &mut carousel,
            &mut ctx,
            Input::Pointer(Pointer::Wheel { dy: 1.0 }),
        );
        assert_eq!(carousel.system().id, "ps4");
        let t = act(&mut carousel, &mut ctx, Input::Action(Action::Confirm));
        assert_eq!(opened(t).as_deref(), Some("PlayStation 4"));
    }

    #[test]
    fn opens_on_nes_first_then_on_the_system_last_viewed() {
        let mut ctx = Ctx::empty();
        let mut carousel = Carousel::new(&ctx.library);
        assert_eq!(carousel.system().id, "nes");
        act(&mut carousel, &mut ctx, Input::Action(Action::Left));
        let reopened = Carousel::new(&ctx.library);
        assert_eq!(reopened.system().id, "ps4");
        assert_eq!(reopened.position, at("ps4") as f32, "starts settled");
    }

    #[test]
    fn an_unknown_stored_system_falls_back_to_nes() {
        let ctx = Ctx::empty();
        ctx.library.set_last_system("dreamcast").unwrap();
        assert_eq!(Carousel::new(&ctx.library).system().id, "nes");
    }

    #[test]
    fn counts_reload_when_the_library_changes() {
        let mut ctx = Ctx::empty();
        let mut carousel = Carousel::new(&ctx.library);
        carousel.refresh(&ctx);
        assert_eq!(carousel.counts[at("gc")], 0);
        let found = [FoundGame {
            system: "gc",
            rel_path: "gc/Metroid Prime (USA).iso".to_owned(),
            title: "Metroid Prime".to_owned(),
        }];
        ctx.library.sync_system("gc", &found, 0).unwrap();
        carousel.refresh(&ctx);
        assert_eq!(carousel.counts[at("gc")], 0, "same revision, no reload");
        ctx.revision += 1;
        carousel.refresh(&ctx);
        assert_eq!(carousel.counts[at("gc")], 1);
        assert_eq!(carousel.counts[at("nes")], 0);
    }

    #[test]
    fn the_slide_eases_towards_the_focus_and_settles_without_animation() {
        let mut ctx = Ctx::empty();
        let mut carousel = Carousel::new(&ctx.library);
        carousel.advance(true);
        act(&mut carousel, &mut ctx, Input::Action(Action::Left));
        assert_eq!(carousel.target, at("ps4") as i32);
        assert_eq!(
            carousel.position,
            SYSTEMS.len() as f32,
            "slides the short way from NES"
        );
        carousel.advance(false);
        assert_eq!(carousel.position, at("ps4") as f32);
    }

    #[test]
    fn names_wrap_like_the_mockups() {
        assert_eq!(wrap_name("NES", 12), ["NES"]);
        assert_eq!(wrap_name("Nintendo 3DS", 12), ["Nintendo 3DS"]);
        assert_eq!(wrap_name("Game Boy Advance", 12), ["Game Boy", "Advance"]);
        assert_eq!(wrap_name("PlayStation 2", 12), ["PlayStation", "2"]);
    }
}
