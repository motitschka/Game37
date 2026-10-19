//! The embedded game session: an Embedded System's Game running on its libretro Core inside
//! Game37's window (FR-EMB-*). The pure parts (input mapping, frame timing, audio rate control,
//! In-game Saves) live here, apart from the SDL parts, so they can be tested without a window.

pub mod input;
pub mod saves;
pub mod sdl;
pub mod states;
pub mod timing;
