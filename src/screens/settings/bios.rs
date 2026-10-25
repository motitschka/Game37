//! Settings → BIOS: the BIOS status, one row per System. Its rows come in a later job.

use super::Section;

pub struct Bios;

impl Section for Bios {
    fn name(&self) -> &'static str {
        "BIOS"
    }
}
