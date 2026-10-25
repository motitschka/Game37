//! Settings → Display: Startup window mode. Its rows come in a later job.

use super::Section;

pub struct Display;

impl Section for Display {
    fn name(&self) -> &'static str {
        "Display"
    }
}
