//! Frame scheduling, audio rate control and where the game picture goes (NFR-4, FR-EMB-11).

use std::time::{Duration, Instant};

use crate::ui::{H, Rect, W};

/// How close the display's refresh rate must be to the Core's fps to run one Core frame per
/// displayed frame.
const LOCKSTEP_TOLERANCE: f64 = 0.01;
/// The most Core frames one displayed frame runs; the backlog beyond is dropped (no fast-forward).
pub const MAX_FRAMES_PER_DISPLAY: u32 = 3;
/// How much of each displayed frame's time goes into the running average of their length.
const INTERVAL_SMOOTHING: f64 = 0.05;
/// Lockstep holds while displayed frames average at least this share of the display's refresh
/// interval. Shorter means presents aren't waiting for the display (vsync isn't working), and one
/// Core frame per displayed frame would run the Game too fast.
const LOCKSTEP_MIN_INTERVAL: f64 = 0.9;

/// Decides how many Core frames each displayed frame runs.
#[derive(Debug, Clone, PartialEq)]
pub struct Scheduler {
    fps: f64,
    /// The display's refresh rate when it is close enough to the Core's fps to run in lockstep.
    lockstep: Option<f64>,
    /// The running average length of a displayed frame, in seconds: lockstep only while it shows
    /// the display really paces them.
    interval: f64,
    due: f64,
}

impl Scheduler {
    /// `refresh` is the display's refresh rate, `None` when unknown or not synced to (no vsync).
    pub fn new(fps: f64, refresh: Option<f64>) -> Scheduler {
        let lockstep = refresh.filter(|r| *r > 0.0 && ((r - fps) / fps).abs() <= LOCKSTEP_TOLERANCE);
        Scheduler {
            fps,
            lockstep,
            interval: lockstep.map_or(0.0, |r| 1.0 / r),
            due: 0.0,
        }
    }

    /// Core frames to run for a displayed frame that came `dt` seconds after the last one. In
    /// lockstep that is one, as long as the displayed frames keep to the display's rate; when
    /// they come faster (vsync isn't holding them back), the clock decides instead.
    pub fn frames(&mut self, dt: f64) -> u32 {
        let dt = dt.max(0.0);
        if self.lockstep.is_some() {
            self.interval += (dt - self.interval) * INTERVAL_SMOOTHING;
            if self.locked() {
                return 1;
            }
        }
        self.due += dt * self.fps;
        let whole = self.due.floor();
        self.due -= whole;
        (whole as u32).min(MAX_FRAMES_PER_DISPLAY)
    }

    /// Forgets time owed, e.g. after the window was hidden.
    pub fn reset(&mut self) {
        self.due = 0.0;
    }

    /// The audio speed that matches the Core's output to real time: in lockstep the Core runs at
    /// the display's rate, not its own, so its audio comes that much faster or slower.
    pub fn base_ratio(&self) -> f64 {
        match self.lockstep {
            Some(r) if self.locked() => r / self.fps,
            _ => 1.0,
        }
    }

    /// Whether displayed frames are paced by the display, so lockstep holds.
    fn locked(&self) -> bool {
        self.lockstep
            .is_some_and(|r| self.interval >= LOCKSTEP_MIN_INTERVAL / r)
    }
}

/// How much audio the stream aims to hold, in seconds.
pub const AUDIO_TARGET: f64 = 0.050;
/// More than this queued (after a stall) is cleared rather than played late.
pub const AUDIO_MAX: f64 = 0.200;
/// The most dynamic rate control speeds audio up or slows it down.
const MAX_DEVIATION: f64 = 0.005;

/// The audio stream's frequency ratio for `queued` seconds of audio: `base` (see
/// [`Scheduler::base_ratio`]) nudged by at most ±0.5 %, faster when the queue is above target so
/// it drains, slower when below so it fills.
pub fn audio_ratio(base: f64, queued: f64) -> f64 {
    let error = ((queued - AUDIO_TARGET) / AUDIO_TARGET).clamp(-1.0, 1.0);
    base * (1.0 + MAX_DEVIATION * error)
}

/// The largest rect of `aspect` (width / height) centred in the logical screen; the rest is
/// black bars.
pub fn fit(aspect: f32) -> Rect {
    let aspect = if aspect > 0.0 { aspect } else { W / H };
    if aspect > W / H {
        let h = W / aspect;
        Rect::new(0.0, (H - h) / 2.0, W, h)
    } else {
        let w = H * aspect;
        Rect::new((W - w) / 2.0, 0.0, w, H)
    }
}

/// Playtime counted while the Game runs: not while the Pause Menu is open, nor while the window
/// is hidden.
#[derive(Debug, Clone)]
pub struct PlayTimer {
    counted: Duration,
    /// When the current stretch of play started; `None` while paused or hidden.
    since: Option<Instant>,
    paused: bool,
    hidden: bool,
}

impl PlayTimer {
    pub fn start(now: Instant) -> PlayTimer {
        PlayTimer {
            counted: Duration::ZERO,
            since: Some(now),
            paused: false,
            hidden: false,
        }
    }

    /// The Pause Menu opened or closed at `now`.
    pub fn set_paused(&mut self, paused: bool, now: Instant) {
        self.paused = paused;
        self.sync(now);
    }

    /// The window was hidden or shown at `now`.
    pub fn set_shown(&mut self, shown: bool, now: Instant) {
        self.hidden = !shown;
        self.sync(now);
    }

    /// Stops or restarts counting at `now`.
    fn sync(&mut self, now: Instant) {
        match (self.paused || self.hidden, self.since) {
            (true, Some(since)) => {
                self.counted += now.saturating_duration_since(since);
                self.since = None;
            }
            (false, None) => self.since = Some(now),
            _ => {}
        }
    }

    /// The Playtime counted up to `now`.
    pub fn played(&self, now: Instant) -> Duration {
        self.counted
            + self
                .since
                .map_or(Duration::ZERO, |s| now.saturating_duration_since(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playtime_stops_while_paused() {
        let t0 = Instant::now();
        let at = |s| t0 + Duration::from_secs(s);
        let mut timer = PlayTimer::start(t0);
        timer.set_paused(true, at(10));
        timer.set_paused(true, at(20));
        assert_eq!(timer.played(at(100)), Duration::from_secs(10));
        timer.set_paused(false, at(100));
        timer.set_paused(false, at(105));
        assert_eq!(timer.played(at(130)), Duration::from_secs(40));
    }

    #[test]
    fn playtime_stops_while_hidden_and_while_hidden_and_paused() {
        let t0 = Instant::now();
        let at = |s| t0 + Duration::from_secs(s);
        let mut timer = PlayTimer::start(t0);
        timer.set_shown(false, at(10));
        assert_eq!(timer.played(at(50)), Duration::from_secs(10));
        timer.set_shown(true, at(50));
        timer.set_paused(true, at(60));
        timer.set_shown(false, at(70));
        timer.set_paused(false, at(80));
        assert_eq!(timer.played(at(90)), Duration::from_secs(20), "still hidden");
        timer.set_shown(true, at(90));
        assert_eq!(timer.played(at(95)), Duration::from_secs(25));
    }

    const NES: f64 = 60.0988;

    fn run(s: &mut Scheduler, refresh: f64, displayed: usize) -> Vec<u32> {
        (0..displayed).map(|_| s.frames(1.0 / refresh)).collect()
    }

    #[test]
    fn at_60_hz_each_displayed_frame_runs_one_core_frame() {
        let mut s = Scheduler::new(NES, Some(60.0));
        assert!(run(&mut s, 60.0, 600).iter().all(|&n| n == 1));
        assert!((s.base_ratio() - 60.0 / NES).abs() < 1e-12);
    }

    #[test]
    fn at_59_7_hz_it_is_still_lockstep_and_audio_is_slowed_to_match() {
        let mut s = Scheduler::new(NES, Some(59.7));
        assert!(run(&mut s, 59.7, 600).iter().all(|&n| n == 1));
        assert!(s.base_ratio() < 0.995, "beyond the ±0.5 % rate control alone");
    }

    #[test]
    fn at_144_hz_core_frames_follow_the_clock() {
        let mut s = Scheduler::new(NES, Some(144.0));
        let frames = run(&mut s, 144.0, 1440);
        assert!(frames.iter().all(|&n| n <= 1));
        let total: u32 = frames.iter().sum();
        assert!((600..=601).contains(&total), "{total} Core frames in 10 s");
        assert_eq!(s.base_ratio(), 1.0);
    }

    /// Presents that don't wait for the display (vsync off) would run the Game at their rate: at
    /// 3400 a second, that's 57 times too fast. The clock takes over instead.
    #[test]
    fn lockstep_without_vsync_holding_presents_back_follows_the_clock() {
        let mut s = Scheduler::new(NES, Some(60.0));
        let frames = run(&mut s, 3400.0, 34_000);
        let total: u32 = frames.iter().sum();
        assert!((595..=605).contains(&total), "{total} Core frames in 10 s");
        assert_eq!(s.base_ratio(), 1.0, "not in lockstep");
        assert!(
            run(&mut s, 60.0, 600).iter().skip(120).all(|&n| n == 1),
            "back in lockstep"
        );
    }

    #[test]
    fn a_stall_runs_at_most_three_frames_and_drops_the_rest() {
        let mut s = Scheduler::new(NES, None);
        assert_eq!(s.frames(1.0), 3);
        assert_eq!(s.frames(0.0), 0, "the backlog is gone");
        assert_eq!(s.frames(1.0 / NES), 1);
    }

    #[test]
    fn the_audio_ratio_drains_a_full_queue_and_fills_an_empty_one_within_half_a_percent() {
        assert_eq!(audio_ratio(1.0, AUDIO_TARGET), 1.0);
        assert!(audio_ratio(1.0, 0.075) > 1.0);
        assert!(audio_ratio(1.0, 0.025) < 1.0);
        assert!((audio_ratio(1.0, 0.150) - 1.005).abs() < 1e-12, "clamped");
        assert!((audio_ratio(1.0, 0.0) - 0.995).abs() < 1e-12, "clamped");
        assert!((audio_ratio(0.99, 0.150) - 0.99 * 1.005).abs() < 1e-12);
    }

    #[test]
    fn the_picture_fits_at_its_aspect_with_bars() {
        assert_eq!(fit(4.0 / 3.0), Rect::new(240.0, 0.0, 1440.0, 1080.0));
        assert_eq!(fit(10.0 / 9.0), Rect::new(360.0, 0.0, 1200.0, 1080.0));
        assert_eq!(fit(16.0 / 9.0), Rect::new(0.0, 0.0, 1920.0, 1080.0));
        assert_eq!(fit(3.2), Rect::new(0.0, 240.0, 1920.0, 600.0));
    }
}
