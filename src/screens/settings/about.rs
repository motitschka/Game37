//! Settings → About, after `screen_settings_about` in `design/generate_mockups.py`: the version, the
//! license, the paths in use and the credits. Plain text, no rows.

use std::path::Path;

use super::{RX, Section, TOP};
use crate::ctx::Ctx;
use crate::screens::home::tilde;
use crate::ui::gfx::{Align, Gfx, TextStyle};
use crate::ui::text::Weight;
use crate::ui::theme::{MUTED, TEXT};

pub struct About;

impl Section for About {
    fn name(&self) -> &'static str {
        "About"
    }

    fn draw_body(&mut self, gfx: &mut Gfx<'_>, ctx: &Ctx) {
        let (x, vx) = (RX + 32.0, RX + 32.0 + 240.0);
        let heading = TextStyle::new(22, Weight::Bold, TEXT);
        let muted = TextStyle::new(20, Weight::Medium, MUTED);
        let version = format!("Game37 {}", env!("CARGO_PKG_VERSION"));
        let title = TextStyle::new(28, Weight::ExtraBold, TEXT);
        gfx.text(x, TOP + 40.0, &version, title, Align::Left);
        gfx.text(x, TOP + 80.0, "Licensed under GPL-3.0", muted, Align::Left);
        let mut y = TOP + 168.0;
        gfx.text(x, y, "Paths", heading, Align::Left);
        let home = dirs::home_dir();
        let path = |p: &Path| tilde(p, home.as_deref());
        let paths = [
            ("Config", path(&ctx.config_path)),
            ("Games Root", path(&ctx.games_root)),
            ("Data folder", path(&ctx.data_dir)),
        ];
        let value = TextStyle::new(20, Weight::Medium, TEXT);
        for (i, (label, path)) in paths.iter().enumerate() {
            let line = y + 48.0 + i as f32 * 40.0;
            gfx.text(x, line, label, muted, Align::Left);
            gfx.text(vx, line, path, value, Align::Left);
        }
        y += 48.0 + 3.0 * 40.0 + 48.0;
        gfx.text(x, y, "Credits", heading, Align::Left);
        let credits = ["Cores from the libretro buildbot", "Artwork from SteamGridDB"];
        for (i, line) in credits.iter().enumerate() {
            gfx.text(x, y + 48.0 + i as f32 * 40.0, line, muted, Align::Left);
        }
    }
}
