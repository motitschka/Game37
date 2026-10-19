//! The library: Games found under the Games Root, and what Game37 records about them.

/// One Game found on disk by a scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundGame {
    /// The System's id, which is also its folder name.
    pub system: &'static str,
    /// Path relative to the Games Root with `/` separators, e.g. `snes/Super Mario World (USA).sfc`.
    /// A Game's identity: the same path is the same Game.
    pub rel_path: String,
    /// The cleaned-up Display Title, before any rename by the player.
    pub title: String,
}
