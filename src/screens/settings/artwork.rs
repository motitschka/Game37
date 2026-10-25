//! Settings → Artwork: the SteamGridDB API key. Its rows come in a later job.

use super::Section;

pub struct Artwork;

impl Section for Artwork {
    fn name(&self) -> &'static str {
        "Artwork"
    }
}
