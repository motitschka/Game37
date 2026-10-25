//! Settings → Cores: Update all, and one row per Core. Its rows come in a later job.

use super::Section;

pub struct Cores;

impl Section for Cores {
    fn name(&self) -> &'static str {
        "Cores"
    }
}
