//! Display Titles derived from file and folder names (FR-LIB-6).

/// The Display Title for a Game, from its file name without the extension or its folder name.
///
/// Removes region, revision and dump tags (`(...)` and `[...]` groups) but keeps disc numbers,
/// turns `_` into spaces and moves a trailing article to the front:
/// `Legend of Zelda, The - A Link to the Past (USA) [!]` becomes
/// `The Legend of Zelda - A Link to the Past`.
pub fn display_title(name: &str) -> String {
    let stripped = strip_groups(name).replace('_', " ");
    let mut title = stripped.split_whitespace().collect::<Vec<_>>().join(" ");
    if let Some(rest) = title.strip_suffix(" -") {
        title = rest.trim_end().to_string();
    }
    let title = move_article(&title);
    if title.is_empty() {
        name.trim().to_string()
    } else {
        title
    }
}

/// Removes every `(...)` and `[...]` group except `(Disc N)` / `(Disk N)`, which becomes `(Disc N)`.
/// An unclosed bracket and everything after it is kept as it is.
fn strip_groups(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut rest = name;
    while let Some(start) = rest.find(['(', '[']) {
        let close = if rest[start..].starts_with('(') { ')' } else { ']' };
        let Some(len) = rest[start..].find(close) else {
            break;
        };
        out.push_str(&rest[..start]);
        let inner = &rest[start + 1..start + len];
        match disc_number(inner) {
            Some(n) if close == ')' => {
                out.push_str(" (Disc ");
                out.push_str(n);
                out.push_str(") ");
            }
            // A space keeps the words on either side apart, as in `Game(USA)Extra`.
            _ => out.push(' '),
        }
        rest = &rest[start + len + 1..];
    }
    out.push_str(rest);
    out
}

/// The number in a `Disc N` or `Disk N` group.
fn disc_number(inner: &str) -> Option<&str> {
    let mut words = inner.split_whitespace();
    let (word, number, None) = (words.next()?, words.next()?, words.next()) else {
        return None;
    };
    let is_disc = word.eq_ignore_ascii_case("disc") || word.eq_ignore_ascii_case("disk");
    (is_disc && number.bytes().all(|b| b.is_ascii_digit())).then_some(number)
}

/// Moves a trailing `, The` / `, A` / `, An` to the front, looking first at the part before the
/// first ` - ` and then at the whole title.
fn move_article(title: &str) -> String {
    let (head, tail) = match title.find(" - ") {
        Some(i) => title.split_at(i),
        None => (title, ""),
    };
    if let Some(moved) = article_first(head) {
        return moved + tail;
    }
    article_first(title).unwrap_or_else(|| title.to_string())
}

fn article_first(part: &str) -> Option<String> {
    ["The", "An", "A"].iter().find_map(|article| {
        let base = part.strip_suffix(article)?.strip_suffix(", ")?;
        (!base.is_empty()).then(|| format!("{article} {base}"))
    })
}

#[cfg(test)]
mod tests {
    use super::display_title;

    #[test]
    fn removes_region_and_dump_tags() {
        assert_eq!(display_title("Super Mario World (USA) [!]"), "Super Mario World");
    }

    #[test]
    fn keeps_disc_number_and_drops_region() {
        assert_eq!(
            display_title("Final Fantasy VII (Europe) (Disc 1)"),
            "Final Fantasy VII (Disc 1)"
        );
    }

    #[test]
    fn normalises_disk_to_disc() {
        assert_eq!(
            display_title("Metal Gear Solid (Disk 2) (USA)"),
            "Metal Gear Solid (Disc 2)"
        );
        assert_eq!(display_title("Riven (disc 3)"), "Riven (Disc 3)");
    }

    #[test]
    fn disc_group_without_a_number_is_removed() {
        assert_eq!(display_title("Game (Disc A)"), "Game");
        assert_eq!(display_title("Game [Disc 1]"), "Game");
    }

    #[test]
    fn moves_article_before_subtitle_to_front() {
        assert_eq!(
            display_title("Legend of Zelda, The - A Link to the Past (USA)"),
            "The Legend of Zelda - A Link to the Past"
        );
    }

    #[test]
    fn moves_trailing_article_of_whole_title() {
        assert_eq!(display_title("Simpsons, The (Europe)"), "The Simpsons");
        assert_eq!(display_title("Boy and His Blob, A"), "A Boy and His Blob");
        assert_eq!(display_title("American Tail, An"), "An American Tail");
        assert_eq!(display_title("Escape - Island, The"), "The Escape - Island");
    }

    #[test]
    fn article_must_follow_a_comma() {
        assert_eq!(display_title("Into The"), "Into The");
        assert_eq!(display_title("Banana"), "Banana");
    }

    #[test]
    fn underscores_become_spaces() {
        assert_eq!(display_title("Pokemon_Emerald"), "Pokemon Emerald");
    }

    #[test]
    fn removes_revision_and_language_groups() {
        assert_eq!(
            display_title("Mario Kart 64 (Europe) (Rev 1) (En,Fr,De)"),
            "Mario Kart 64"
        );
    }

    #[test]
    fn collapses_whitespace_and_strips_trailing_dash() {
        assert_eq!(
            display_title("  Sonic   the  Hedgehog - (USA) "),
            "Sonic the Hedgehog"
        );
        assert_eq!(display_title("Game(USA)Extra"), "Game Extra");
    }

    #[test]
    fn plain_names_are_unchanged() {
        assert_eq!(display_title("BLUS12345"), "BLUS12345");
    }

    #[test]
    fn falls_back_to_input_when_nothing_is_left() {
        assert_eq!(display_title("(USA)"), "(USA)");
        assert_eq!(display_title(" [!] "), "[!]");
    }

    #[test]
    fn unclosed_bracket_is_kept() {
        assert_eq!(display_title("Game (USA"), "Game (USA");
    }
}
