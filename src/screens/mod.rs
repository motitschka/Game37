//! The screens, and which ones Game37 starts on (FR-CFG-1).

mod carousel;
pub mod download_core;
pub mod game_grid;
pub mod home;
pub mod message;
mod no_config;
pub mod pause;
pub mod playing;
mod stub;

use std::path::Path;

use crate::config::{Config, ConfigError};
use crate::ctx::Ctx;
use crate::ui::nav::Nav;

pub use carousel::Carousel;
pub use game_grid::GameGrid;
pub use home::Home;
pub use no_config::NoConfig;
pub use stub::Stub;

/// Home and Consoles when a Config exists; otherwise the placeholder for First-run Setup, which
/// isn't built yet.
pub fn start(config: Result<Option<Config>, ConfigError>, path: &Path, ctx: &Ctx) -> Nav {
    match config {
        Ok(Some(_config)) => Nav::with_tabs(vec![
            ("Home", Box::new(Home::new())),
            ("Consoles", Box::new(Carousel::new(&ctx.library))),
        ]),
        Ok(None) => Nav::single(Box::new(NoConfig::missing(path))),
        Err(e) => Nav::single(Box::new(NoConfig::unreadable(&e))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    }

    #[test]
    fn no_config_starts_on_the_setup_placeholder() {
        let mut nav = start(Ok(None), Path::new("/c.toml"), &Ctx::empty());
        assert_eq!(nav.tab_labels(), None);
        assert_eq!(nav.top().title(), "First-run Setup");
    }
}
