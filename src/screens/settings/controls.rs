//! Settings → Controls: Menu button style and Controller test. Its rows come in a later job.

use super::Section;

pub struct Controls;

impl Section for Controls {
    fn name(&self) -> &'static str {
        "Controls"
    }
}
