//! Screens and the navigation between them: top-level tabs (Home, Consoles), each with a stack of
//! pushed sub-screens.

use super::gfx::Gfx;
use super::input::{Action, Input, Pointer};
use super::{Rect, hit};
use crate::ctx::Ctx;

/// A full-screen view. The app draws the background, top bar and hint bar around it.
pub trait Screen {
    /// Shown as the "‹ title" back affordance on screens pushed on top of this one.
    fn title(&self) -> &str;
    fn handle(&mut self, input: Input, ctx: &mut Ctx) -> Transition;
    /// Draws the content between the top bar and the hint bar.
    fn draw(&mut self, gfx: &mut Gfx<'_>, ctx: &Ctx);
    fn hints(&self) -> Vec<Hint>;
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

    /// Title of the screen Back returns to, when the active screen was pushed.
    pub fn back_title(&self) -> Option<&str> {
        let stack = &self.tabs[self.active].stack;
        (stack.len() > 1).then(|| stack[stack.len() - 2].title())
    }

    pub fn handle(&mut self, input: Input, hits: &Hits, ctx: &mut Ctx) -> Flow {
        let tabs = self.tabs.len();
        match input {
            Input::Action(Action::PrevTab) if tabs > 1 => self.active = (self.active + tabs - 1) % tabs,
            Input::Action(Action::NextTab) if tabs > 1 => self.active = (self.active + 1) % tabs,
            Input::Pointer(Pointer::Back) if self.depth() > 1 => self.pop(),
            Input::Pointer(Pointer::Click { x, y }) if hit(&hits.tabs, x, y).is_some() => {
                self.active = hit(&hits.tabs, x, y).unwrap().min(tabs - 1);
            }
            Input::Pointer(Pointer::Click { x, y }) if hits.back.is_some_and(|r| r.contains(x, y)) => {
                self.pop()
            }
            Input::Pointer(Pointer::Click { x, y }) if hit(&hits.hints, x, y).is_some() => {
                let action = hit(&hits.hints, x, y).unwrap();
                return self.handle(Input::Action(action), &Hits::default(), ctx);
            }
            _ => match self.top().handle(input, ctx) {
                Transition::None => {}
                Transition::Push(screen) => self.tabs[self.active].stack.push(screen),
                Transition::Pop => self.pop(),
                Transition::Quit => return Flow::Quit,
            },
        }
        Flow::Continue
    }

    /// Pops the active tab's top screen; a root screen stays.
    fn pop(&mut self) {
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
