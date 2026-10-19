//! Screens and the navigation between them: top-level tabs (Home, Consoles), each with a stack of
//! pushed sub-screens.

use super::gfx::Gfx;
use super::input::{Action, Input, Pointer};
use super::{Rect, hit};
use crate::ctx::Ctx;
use crate::session::sdl::Session;

/// A full-screen view. The app draws the background, top bar and hint bar around it.
pub trait Screen {
    /// Shown as the "‹ title" back affordance on screens pushed on top of this one.
    fn title(&self) -> &str;
    fn handle(&mut self, input: Input, ctx: &mut Ctx) -> Transition;
    /// Draws the content between the top bar and the hint bar.
    fn draw(&mut self, gfx: &mut Gfx<'_>, ctx: &Ctx);
    fn hints(&self) -> Vec<Hint>;
    /// Called when the screen stops being shown without being popped: its tab is switched away
    /// from, or Game37 quits. A place to save state that is otherwise saved lazily.
    fn leave(&mut self, _ctx: &Ctx) {}
    /// A modal is drawn over the screen below it and gets all input: tabs, the back affordance
    /// and the mouse Back button don't act while it is on top. Only hint clicks still turn into
    /// their actions.
    fn is_modal(&self) -> bool {
        false
    }
    /// Whether the background, top bar and hint bar are drawn around it. A running Game has none.
    fn chrome(&self) -> bool {
        true
    }
    /// The embedded game session it runs, if it is a running Game.
    fn session(&mut self) -> Option<&mut Session> {
        None
    }
    /// Called once a frame for the top screen, outside input handling: the seam asynchronous
    /// work (a Core download, say) uses to progress and transition without waiting for input.
    fn tick(&mut self, _ctx: &mut Ctx) -> Transition {
        Transition::None
    }
    /// Whether the screen has asynchronous work pending that screenshot mode should keep
    /// ticking through before drawing the next step, e.g. a Core still downloading.
    fn busy(&self) -> bool {
        false
    }
}

pub enum Transition {
    None,
    Push(Box<dyn Screen>),
    Pop,
    Quit,
}

/// One hint bar entry: the actions' glyphs and what they do.
#[derive(Debug, Clone, PartialEq)]
pub struct Hint {
    pub actions: &'static [Action],
    pub text: &'static str,
}

impl Hint {
    pub const fn new(actions: &'static [Action], text: &'static str) -> Hint {
        Hint { actions, text }
    }
}

/// Where the last frame drew the clickable chrome, for mouse input.
#[derive(Debug, Default, Clone)]
pub struct Hits {
    /// Tab labels and the tab index each switches to.
    pub tabs: Vec<(Rect, usize)>,
    /// The "‹ title" back affordance of a pushed screen.
    pub back: Option<Rect>,
    /// Hint bar entries and the action a click on each performs.
    pub hints: Vec<(Rect, Action)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Quit,
}

struct Tab {
    label: &'static str,
    stack: Vec<Box<dyn Screen>>,
}

pub struct Nav {
    tabs: Vec<Tab>,
    active: usize,
}

impl Nav {
    /// Top-level tabs, the first one active.
    pub fn with_tabs(tabs: Vec<(&'static str, Box<dyn Screen>)>) -> Nav {
        assert!(!tabs.is_empty());
        let tabs = tabs
            .into_iter()
            .map(|(label, root)| Tab {
                label,
                stack: vec![root],
            })
            .collect();
        Nav { tabs, active: 0 }
    }

    /// A single screen without tabs.
    pub fn single(screen: Box<dyn Screen>) -> Nav {
        Nav::with_tabs(vec![("", screen)])
    }

    /// Tab labels, or `None` when there is only one root and no tabs are shown.
    pub fn tab_labels(&self) -> Option<Vec<&'static str>> {
        (self.tabs.len() > 1).then(|| self.tabs.iter().map(|t| t.label).collect())
    }

    pub fn active_tab(&self) -> usize {
        self.active
    }

    /// Number of screens on the active tab's stack, its root included.
    pub fn depth(&self) -> usize {
        self.tabs[self.active].stack.len()
    }

    pub fn top(&mut self) -> &mut dyn Screen {
        self.tabs[self.active]
            .stack
            .last_mut()
            .expect("a tab's stack is never empty")
            .as_mut()
    }

    /// Index of the topmost screen that isn't a modal on the active tab's stack.
    fn base_index(&self) -> usize {
        let stack = &self.tabs[self.active].stack;
        stack.iter().rposition(|s| !s.is_modal()).unwrap_or(0)
    }

    /// The topmost screen that isn't a modal: the one drawn full-screen.
    pub fn base(&mut self) -> &mut dyn Screen {
        let i = self.base_index();
        self.tabs[self.active].stack[i].as_mut()
    }

    /// The modal on top, if any.
    pub fn modal(&mut self) -> Option<&mut dyn Screen> {
        let top = self.top();
        if top.is_modal() { Some(top) } else { None }
    }

    /// Title of the screen Back returns to, when the active screen (below any modal) was pushed.
    pub fn back_title(&self) -> Option<&str> {
        let stack = &self.tabs[self.active].stack;
        let base = self.base_index();
        (base > 0).then(|| stack[base - 1].title())
    }

    pub fn handle(&mut self, input: Input, hits: &Hits, ctx: &mut Ctx) -> Flow {
        let tabs = self.tabs.len();
        let modal = self.top().is_modal();
        match input {
            Input::Pointer(Pointer::Click { x, y }) if hit(&hits.hints, x, y).is_some() => {
                let action = hit(&hits.hints, x, y).unwrap();
                return self.handle(Input::Action(action), &Hits::default(), ctx);
            }
            _ if modal => return self.apply(input, ctx),
            Input::Action(Action::PrevTab) if tabs > 1 => {
                self.switch_to((self.active + tabs - 1) % tabs, ctx)
            }
            Input::Action(Action::NextTab) if tabs > 1 => self.switch_to((self.active + 1) % tabs, ctx),
            Input::Pointer(Pointer::Back) if self.depth() > 1 => self.pop(),
            Input::Pointer(Pointer::Click { x, y }) if hit(&hits.tabs, x, y).is_some() => {
                self.switch_to(hit(&hits.tabs, x, y).unwrap().min(tabs - 1), ctx);
            }
            Input::Pointer(Pointer::Click { x, y }) if hits.back.is_some_and(|r| r.contains(x, y)) => {
                self.pop()
            }
            _ => return self.apply(input, ctx),
        }
        Flow::Continue
    }

    /// Hands `input` to the top screen and carries out its transition.
    fn apply(&mut self, input: Input, ctx: &mut Ctx) -> Flow {
        match self.top().handle(input, ctx) {
            Transition::None => {}
            Transition::Push(screen) => self.push(screen),
            Transition::Pop => self.pop(),
            Transition::Quit => return Flow::Quit,
        }
        Flow::Continue
    }

    pub fn push(&mut self, screen: Box<dyn Screen>) {
        self.tabs[self.active].stack.push(screen);
    }

    /// Polls the top screen's pending async work and carries out its transition. Call once a
    /// frame, for every frame, not only when there is input.
    pub fn tick(&mut self, ctx: &mut Ctx) -> Flow {
        match self.top().tick(ctx) {
            Transition::None => {}
            Transition::Push(screen) => self.push(screen),
            Transition::Pop => self.pop(),
            Transition::Quit => return Flow::Quit,
        }
        Flow::Continue
    }

    fn switch_to(&mut self, tab: usize, ctx: &Ctx) {
        if tab != self.active {
            self.top().leave(ctx);
            self.active = tab;
        }
    }

    /// Tells every screen Game37 is quitting.
    pub fn leave_all(&mut self, ctx: &Ctx) {
        for screen in self.tabs.iter_mut().flat_map(|t| t.stack.iter_mut()) {
            screen.leave(ctx);
        }
    }

    /// Pops the active tab's top screen; a root screen stays.
    pub fn pop(&mut self) {
        let stack = &mut self.tabs[self.active].stack;
        if stack.len() > 1 {
            stack.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// A screen that records what it was asked to handle and answers Confirm with `on_confirm`.
    struct Probe {
        title: &'static str,
        log: Rc<RefCell<Vec<(&'static str, Input)>>>,
        on_confirm: fn() -> Transition,
    }

    impl Screen for Probe {
        fn title(&self) -> &str {
            self.title
        }
        fn handle(&mut self, input: Input, _: &mut Ctx) -> Transition {
            self.log.borrow_mut().push((self.title, input));
            match input {
                Input::Action(Action::Confirm) => (self.on_confirm)(),
                Input::Action(Action::Back) => Transition::Pop,
                _ => Transition::None,
            }
        }
        fn draw(&mut self, _: &mut Gfx<'_>, _: &Ctx) {}
        fn hints(&self) -> Vec<Hint> {
            Vec::new()
        }
    }

    type Log = Rc<RefCell<Vec<(&'static str, Input)>>>;

    /// A screen that records when it is left.
    struct Leaver(&'static str, Rc<RefCell<Vec<&'static str>>>);

    impl Screen for Leaver {
        fn title(&self) -> &str {
            self.0
        }
        fn handle(&mut self, _: Input, _: &mut Ctx) -> Transition {
            Transition::None
        }
        fn draw(&mut self, _: &mut Gfx<'_>, _: &Ctx) {}
        fn hints(&self) -> Vec<Hint> {
            Vec::new()
        }
        fn leave(&mut self, _: &Ctx) {
            self.1.borrow_mut().push(self.0);
        }
    }

    #[test]
    fn switching_tabs_and_quitting_leave_the_screens() {
        let left = Rc::new(RefCell::new(Vec::new()));
        let mut nav = Nav::with_tabs(vec![
            ("Home", Box::new(Leaver("Home", left.clone()))),
            ("Consoles", Box::new(Leaver("Consoles", left.clone()))),
        ]);
        let ctx = &mut Ctx::empty();
        nav.handle(Input::Action(Action::NextTab), &Hits::default(), ctx);
        assert_eq!(*left.borrow(), ["Home"]);
        let hits = Hits {
            tabs: vec![(Rect::new(0.0, 0.0, 10.0, 10.0), 1)],
            ..Hits::default()
        };
        nav.handle(Input::Pointer(Pointer::Click { x: 5.0, y: 5.0 }), &hits, ctx);
        assert_eq!(*left.borrow(), ["Home"], "clicking the active tab leaves nothing");
        nav.leave_all(ctx);
        assert_eq!(*left.borrow(), ["Home", "Home", "Consoles"]);
    }

    fn probe(title: &'static str, log: &Log, on_confirm: fn() -> Transition) -> Box<dyn Screen> {
        Box::new(Probe {
            title,
            log: log.clone(),
            on_confirm,
        })
    }

    fn push_page() -> Transition {
        Transition::Push(Box::new(Probe {
            title: "Page",
            log: Rc::default(),
            on_confirm: || Transition::None,
        }))
    }

    fn tabs(log: &Log) -> Nav {
        Nav::with_tabs(vec![
            ("Home", probe("Home", log, || Transition::None)),
            ("Consoles", probe("Consoles", log, push_page)),
        ])
    }

    fn act(nav: &mut Nav, action: Action) -> Flow {
        nav.handle(Input::Action(action), &Hits::default(), &mut Ctx::empty())
    }

    fn click(nav: &mut Nav, hits: &Hits, x: f32, y: f32) -> Flow {
        nav.handle(Input::Pointer(Pointer::Click { x, y }), hits, &mut Ctx::empty())
    }

    #[test]
    fn shoulder_buttons_switch_tabs_and_wrap() {
        let log = Log::default();
        let mut nav = tabs(&log);
        assert_eq!(nav.active_tab(), 0);
        act(&mut nav, Action::NextTab);
        assert_eq!(nav.active_tab(), 1);
        act(&mut nav, Action::NextTab);
        assert_eq!(nav.active_tab(), 0);
        act(&mut nav, Action::PrevTab);
        assert_eq!(nav.active_tab(), 1);
        assert!(log.borrow().is_empty(), "tab switches don't reach the screens");
    }

    #[test]
    fn clicking_a_tab_label_switches_to_it() {
        let log = Log::default();
        let mut nav = tabs(&log);
        let hits = Hits {
            tabs: vec![
                (Rect::new(0.0, 0.0, 100.0, 50.0), 0),
                (Rect::new(200.0, 0.0, 100.0, 50.0), 1),
            ],
            ..Hits::default()
        };
        click(&mut nav, &hits, 250.0, 20.0);
        assert_eq!(nav.active_tab(), 1);
        click(&mut nav, &hits, 50.0, 20.0);
        assert_eq!(nav.active_tab(), 0);
        click(&mut nav, &hits, 150.0, 20.0);
        assert_eq!(nav.active_tab(), 0);
        assert_eq!(
            log.borrow().len(),
            1,
            "a click outside the chrome reaches the screen"
        );
    }

    #[test]
    fn confirm_pushes_and_back_pops() {
        let log = Log::default();
        let mut nav = tabs(&log);
        act(&mut nav, Action::NextTab);
        act(&mut nav, Action::Confirm);
        assert_eq!(nav.depth(), 2);
        assert_eq!(nav.top().title(), "Page");
        assert_eq!(nav.back_title(), Some("Consoles"));
        act(&mut nav, Action::Back);
        assert_eq!(nav.depth(), 1);
        assert_eq!(nav.top().title(), "Consoles");
        act(&mut nav, Action::Back);
        assert_eq!(nav.depth(), 1, "the root stays");
        assert_eq!(nav.back_title(), None);
    }

    #[test]
    fn each_tab_keeps_its_own_stack() {
        let log = Log::default();
        let mut nav = tabs(&log);
        act(&mut nav, Action::NextTab);
        act(&mut nav, Action::Confirm);
        act(&mut nav, Action::PrevTab);
        assert_eq!(nav.top().title(), "Home");
        act(&mut nav, Action::NextTab);
        assert_eq!(nav.top().title(), "Page");
    }

    #[test]
    fn mouse_back_button_and_back_affordance_pop() {
        let log = Log::default();
        let mut nav = tabs(&log);
        act(&mut nav, Action::NextTab);
        act(&mut nav, Action::Confirm);
        nav.handle(Input::Pointer(Pointer::Back), &Hits::default(), &mut Ctx::empty());
        assert_eq!(nav.depth(), 1);

        act(&mut nav, Action::Confirm);
        let hits = Hits {
            back: Some(Rect::new(0.0, 0.0, 100.0, 50.0)),
            ..Hits::default()
        };
        click(&mut nav, &hits, 10.0, 10.0);
        assert_eq!(nav.depth(), 1);
    }

    #[test]
    fn clicking_a_hint_performs_its_action() {
        let log = Log::default();
        let mut nav = tabs(&log);
        let hits = Hits {
            hints: vec![(Rect::new(0.0, 1000.0, 100.0, 50.0), Action::NextTab)],
            ..Hits::default()
        };
        click(&mut nav, &hits, 10.0, 1010.0);
        assert_eq!(nav.active_tab(), 1);
    }

    #[test]
    fn a_single_screen_has_no_tabs_and_can_quit() {
        let log = Log::default();
        let mut nav = Nav::single(probe("Setup", &log, || Transition::Quit));
        assert_eq!(nav.tab_labels(), None);
        assert_eq!(act(&mut nav, Action::NextTab), Flow::Continue);
        assert_eq!(
            log.borrow().len(),
            1,
            "without tabs, tab actions reach the screen"
        );
        assert_eq!(act(&mut nav, Action::Confirm), Flow::Quit);
    }
}
