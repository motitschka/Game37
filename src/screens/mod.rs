//! The screens, and which ones Game37 starts on (FR-CFG-1).

pub mod artwork_picker;
mod carousel;
pub mod confirm;
pub mod controller_test;
pub mod core_queue;
pub mod details;
pub mod download_core;
pub mod exe_browser;
pub mod game_grid;
pub mod home;
pub mod message;
mod no_config;
pub mod options;
pub mod pause;
pub mod playing;
mod search;
mod settings;
mod setup;
mod stub;

use std::path::Path;

use crate::config::{Config, ConfigError};
use crate::ctx::Ctx;
use crate::ui::nav::{Entries, Nav};

pub use carousel::Carousel;
pub use controller_test::ControllerTest;
pub use game_grid::GameGrid;
pub use home::Home;
pub use no_config::NoConfig;
pub use search::Search;
pub use settings::Settings;
pub use setup::Setup;
pub use stub::Stub;

/// Home and Consoles, with Search and Settings, when a Config exists; otherwise First-run Setup,
/// proposing `setup_root` as the Games Root.
pub fn start(config: Result<Option<Config>, ConfigError>, setup_root: &Path, ctx: &Ctx) -> Nav {
    match config {
        Ok(Some(_config)) => Nav::with_tabs(vec![
            ("Home", Box::new(Home::new())),
            ("Consoles", Box::new(Carousel::new(&ctx.library))),
        ])
        .with_entries(entries()),
        Ok(None) => Nav::single(Box::new(Setup::new(setup_root.to_path_buf()))),
        Err(e) => Nav::single(Box::new(NoConfig::unreadable(&e))),
    }
}

/// The Search and Settings screens the top bar opens.
fn entries() -> Entries {
    Entries {
        search: Box::new(|ctx| Box::new(Search::new(ctx))),
        settings: Box::new(|_| Box::new(Settings::new())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::input::{Action, Input};
    use crate::ui::nav::{Entry, Hits};
    use std::path::PathBuf;

    #[test]
    fn a_config_starts_on_home() {
        let mut nav = start(
            Ok(Some(Config {
                games_root: PathBuf::from("/g"),
            })),
            Path::new("/c.toml"),
            &Ctx::empty(),
        );
        assert_eq!(nav.tab_labels(), Some(vec!["Home", "Consoles"]));
        assert_eq!(nav.top().title(), "Home");
        assert!(nav.has_library(), "with Search and Settings");
    }

    #[test]
    fn no_config_starts_on_first_run_setup() {
        let mut nav = start(Ok(None), Path::new("/g"), &Ctx::empty());
        assert_eq!(nav.tab_labels(), None);
        assert_eq!(nav.top().title(), "First-run Setup");
        assert!(!nav.top().clock(), "Setup's progress shows where the clock was");
        assert!(!nav.has_library(), "no Search or Settings");
    }

    #[test]
    fn search_and_settings_open_from_the_top_bar() {
        let ctx = &mut Ctx::empty();
        let config = Config {
            games_root: PathBuf::from("/g"),
        };
        let mut nav = start(Ok(Some(config)), Path::new("/c.toml"), ctx);
        let mut open = |action| {
            nav.handle(Input::Action(action), &Hits::default(), ctx);
            let top = nav.top();
            let seen = (top.title().to_owned(), top.entry(), top.search_pill());
            nav.handle(Input::Action(Action::Back), &Hits::default(), ctx);
            assert_eq!(nav.depth(), 1, "Back leaves {}", seen.0);
            seen
        };
        assert_eq!(
            open(Action::Search),
            ("Search".to_owned(), Some(Entry::Search), false),
            "Search draws its own field instead of the pill"
        );
        assert_eq!(
            open(Action::Settings),
            ("Settings".to_owned(), Some(Entry::Settings), true)
        );
    }
}
