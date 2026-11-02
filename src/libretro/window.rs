//! The address window: room in the address space Game37 keeps for a Core that must place large
//! memory maps within reach of its own image. LRPS2's recompilers address its globals and the
//! PS2's memory with 32-bit offsets, so its 320 MB main map and 305 MB code map must lie within
//! 1.9375 GB of every byte of its image and of each other (`RIP_LIMIT` in its `Memory.c`); it
//! places them when it loads a Game, and where it finds no such room it maps them anywhere and
//! later aborts the process.
//!
//! Cores stay mapped after they unload (`RTLD_NODELETE`), beside whatever they never release
//! (Mupen64Plus-Next keeps a 1 GB reservation) and the holes their own maps leave, and a library
//! goes into the highest free range that fits it. After a few other Cores, that is a hole between
//! their images with no room for LRPS2's maps anywhere near.
//!
//! So such a Core's image is loaded into the top of a window of [`SIZE`] bytes, reserved without
//! access (it commits no memory) the first time: every free range above the window is reserved
//! too for the moment of the `dlopen`, so the highest free range that fits is the window's. The
//! window stays reserved but for the image, so that no other Core's maps take the room, except
//! while the Core loads its Game: [`Lease::open`] lets it go right before `retro_load_game`, and
//! the [`Lease`] takes it back once the Core is gone.

use std::ffi::{c_int, c_void};
use std::ops::Range;
use std::sync::{Mutex, MutexGuard};

/// The window: the image, a main and a code map all fit in it, and anything in it is within
/// LRPS2's reach of everything else there.
const SIZE: usize = 0x7000_0000;
/// The window's top part, let go for the image to load into.
const IMAGE_ROOM: usize = 0x2000_0000;

const PROT_NONE: c_int = 0;
const MAP_PRIVATE: c_int = 0x02;
const MAP_ANONYMOUS: c_int = 0x20;
const MAP_NORESERVE: c_int = 0x4000;
const MAP_FIXED_NOREPLACE: c_int = 0x10_0000;

unsafe extern "C" {
    fn mmap(addr: *mut c_void, len: usize, prot: c_int, flags: c_int, fd: c_int, offset: i64) -> *mut c_void;
    fn munmap(addr: *mut c_void, len: usize) -> c_int;
}

/// Reserves `len` bytes without access, at `at` exactly or, with `None`, wherever the kernel puts
/// it. `None` when the range isn't free or can't be had.
fn reserve(at: Option<usize>, len: usize) -> Option<Range<usize>> {
    let (addr, fixed) = match at {
        Some(at) => (at as *mut c_void, MAP_FIXED_NOREPLACE),
        None => (std::ptr::null_mut(), 0),
    };
    let flags = MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE | fixed;
    // SAFETY: an anonymous mapping that, being `MAP_FIXED_NOREPLACE` or unplaced, replaces
    // nothing already mapped.
    let got = unsafe { mmap(addr, len, PROT_NONE, flags, -1, 0) } as usize;
    // `MAP_FAILED`, or an old kernel that took `MAP_FIXED_NOREPLACE` as a mere hint.
    if got == usize::MAX || at.is_some_and(|at| got != at) {
        if got != usize::MAX {
            unreserve(got..got + len);
        }
        return None;
    }
    Some(got..got + len)
}

/// Lets go of a range [`reserve`] gave.
fn unreserve(range: Range<usize>) {
    // SAFETY: only ever called on ranges this module reserved, which hold nothing.
    unsafe { munmap(range.start as *mut c_void, range.len()) };
}

/// A mapping of the process's address space, from `/proc/self/maps`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Mapping {
    range: Range<usize>,
    /// The main thread's stack, which grows down into the free range below it.
    stack: bool,
}

/// The mappings `maps` (in `/proc/self/maps`'s form) lists, in address order.
fn parse_maps(maps: &str) -> Vec<Mapping> {
    maps.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let (lo, hi) = fields.next()?.split_once('-')?;
            let range = usize::from_str_radix(lo, 16).ok()?..usize::from_str_radix(hi, 16).ok()?;
            Some(Mapping {
                range,
                stack: fields.nth(4) == Some("[stack]"),
            })
        })
        .collect()
}

fn mappings() -> Vec<Mapping> {
    parse_maps(&std::fs::read_to_string("/proc/self/maps").unwrap_or_default())
}

/// The free ranges between `maps` within `within`, below the main thread's stack only: the free
/// range right below it is where it grows.
fn free_ranges(maps: &[Mapping], within: &Range<usize>) -> Vec<Range<usize>> {
    maps.windows(2)
        .take_while(|pair| !pair[1].stack)
        .map(|pair| pair[0].range.end.max(within.start)..pair[1].range.start.min(within.end))
        .filter(|gap| !gap.is_empty())
        .collect()
}

/// The window, and the parts of it held reserved.
#[derive(Debug)]
struct Window {
    range: Range<usize>,
    held: Vec<Range<usize>>,
}

impl Window {
    fn new(size: usize) -> Option<Window> {
        let range = reserve(None, size)?;
        Some(Window {
            held: vec![range.clone()],
            range,
        })
    }

    /// Reserves every free part of the window.
    fn hold_free(&mut self) {
        for gap in free_ranges(&mappings(), &self.range) {
            self.held.extend(reserve(Some(gap.start), gap.len()));
        }
    }

    /// Lets go of what is held of `part`.
    fn release(&mut self, part: &Range<usize>) {
        let mut kept = Vec::new();
        for held in self.held.drain(..) {
            let cut = held.start.max(part.start)..held.end.min(part.end);
            if cut.is_empty() {
                kept.push(held);
                continue;
            }
            unreserve(cut.clone());
            kept.extend(
                [held.start..cut.start, cut.end..held.end]
                    .into_iter()
                    .filter(|r| !r.is_empty()),
            );
        }
        self.held = kept;
    }

    /// Runs `map`, which maps something wherever the kernel puts it, so that it lands in the
    /// window's top `room` bytes: that part is let go, and every free range above the window
    /// reserved while `map` runs. The window is held again afterwards, but for what `map` put in.
    fn map_into_top<T>(&mut self, room: usize, map: impl FnOnce() -> T) -> T {
        self.release(&(self.range.end - room..self.range.end));
        let above = free_ranges(&mappings(), &(self.range.end..usize::MAX));
        let fillers: Vec<_> = above
            .iter()
            .filter_map(|gap| reserve(Some(gap.start), gap.len()))
            .collect();
        let mapped = map();
        fillers.into_iter().for_each(unreserve);
        self.hold_free();
        mapped
    }
}

/// The process's window, made the first time a Core needs it. Only the Core that holds the one
/// [`super::Claim`] leases it.
static WINDOW: Mutex<Option<Window>> = Mutex::new(None);

fn window() -> MutexGuard<'static, Option<Window>> {
    WINDOW.lock().unwrap_or_else(|e| e.into_inner())
}

/// The window, leased to the one Core that needs it: it is held again when the lease ends.
#[derive(Debug)]
pub(super) struct Lease(());

impl Lease {
    /// Leases the window, reserving it the first time. `None` where it can't be reserved.
    pub(super) fn take() -> Option<Lease> {
        let mut window = window();
        if window.is_none() {
            *window = Some(Window::new(SIZE)?);
        }
        Some(Lease(()))
    }

    /// Runs `load`, which loads the Core's library, so that its image lands in the window's top.
    pub(super) fn load<T>(&self, load: impl FnOnce() -> T) -> T {
        window()
            .as_mut()
            .expect("a leased window")
            .map_into_top(IMAGE_ROOM, load)
    }

    /// Whether `addr` is in the window.
    pub(super) fn contains(&self, addr: usize) -> bool {
        window().as_ref().is_some_and(|w| w.range.contains(&addr))
    }

    /// Lets go of the whole window, for the Core to place its maps there.
    pub(super) fn open(&self) {
        if let Some(w) = window().as_mut() {
            let all = w.range.clone();
            w.release(&all);
        }
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        if let Some(w) = window().as_mut() {
            w.hold_free();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAPS: &str = "\
55d0c0000000-55d0c0010000 r--p 00000000 103:03 1 /usr/bin/game37
7f0000000000-7f0000100000 rw-p 00000000 00:00 0
7f0000300000-7f0000400000 r-xp 00000000 103:03 2 /usr/lib/libc.so.6
7f0000400000-7f0000500000 ---p 00000000 00:00 0
7f0000600000-7f0000700000 r--p 00000000 103:03 3 /usr/lib/ld-linux-x86-64.so.2
7ffd00000000-7ffd00021000 rw-p 00000000 00:00 0                          [stack]
ffffffffff600000-ffffffffff601000 --xp 00000000 00:00 0                  [vsyscall]
";

    #[test]
    fn maps_parse_into_ranges_and_find_the_stack() {
        let maps = parse_maps(MAPS);
        assert_eq!(maps.len(), 7);
        assert_eq!(maps[1].range, 0x7f00_0000_0000..0x7f00_0010_0000);
        let stacks: Vec<_> = maps.iter().filter(|m| m.stack).map(|m| m.range.start).collect();
        assert_eq!(stacks, [0x7ffd_0000_0000]);
    }

    #[test]
    fn free_ranges_are_the_gaps_within_and_never_the_one_under_the_stack() {
        let maps = parse_maps(MAPS);
        assert_eq!(
            free_ranges(&maps, &(0x7f00_0000_0000..usize::MAX)),
            [
                0x7f00_0010_0000..0x7f00_0030_0000,
                0x7f00_0050_0000..0x7f00_0060_0000
            ]
        );
        // Clipped to the range asked about.
        assert_eq!(
            free_ranges(&maps, &(0x7f00_0020_0000..0x7f00_0058_0000)),
            [
                0x7f00_0020_0000..0x7f00_0030_0000,
                0x7f00_0050_0000..0x7f00_0058_0000
            ]
        );
        assert_eq!(free_ranges(&maps, &(0x7f00_0030_0000..0x7f00_0040_0000)), []);
    }

    /// Whether the whole of `range` is mapped, in one or more mappings.
    fn all_mapped(range: &Range<usize>) -> bool {
        free_ranges(&mappings(), range).is_empty()
    }

    /// A mapping made wherever the kernel puts it lands in the top of a window, below every
    /// mapping above the window; the window is held again around it, and let go when opened.
    #[test]
    fn a_mapping_made_into_a_window_lands_in_its_top_and_the_rest_is_held() {
        const MIB: usize = 1 << 20;
        let mut window = Window::new(256 * MIB).expect("a window");
        let range = window.range.clone();
        assert!(all_mapped(&range), "the window is reserved");

        let image = window.map_into_top(64 * MIB, || reserve(None, 32 * MIB).expect("a mapping"));
        assert!(
            image.start >= range.end - 64 * MIB && image.end <= range.end,
            "{image:x?} is in the top of {range:x?}"
        );
        assert!(all_mapped(&range), "the window is held around it");

        let all = range.clone();
        window.release(&all);
        assert!(window.held.is_empty(), "opened: nothing held");
        assert!(!all_mapped(&range), "opened: room beside the mapping");
        unreserve(image);
        window.hold_free();
        assert!(all_mapped(&range), "held again, whole");
        window.release(&all);
    }
}
