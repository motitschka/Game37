//! Motion: values that follow a target over real time, so focus, scrolling and screens glide
//! instead of jumping. Every step is frame-rate independent (an exponential approach with a time
//! constant), and lands on the target at once when animation is off (`Ctx::animate`: screenshot
//! mode and tests), so those always draw the settled state.

use std::time::Instant;

/// Time constant of a scroll: the gap shrinks to about a third every `SCROLL_TAU` seconds, so a
/// scroll looks done in about a fifth of a second.
pub const SCROLL_TAU: f32 = 0.045;
/// Time constant of the focus ring and a focused cover's growth.
pub const FOCUS_TAU: f32 = 0.035;
/// Time constant of a screen fading in.
pub const FADE_TAU: f32 = 0.05;

/// The gap below which an eased value counts as arrived and snaps to its target.
const SETTLED: f32 = 0.01;
/// The longest frame a step accounts for, so a stall (a Game loading, a window drag) never makes
/// a value overshoot or jump.
const MAX_DT: f32 = 0.1;

/// A value that eases towards its target.
#[derive(Debug, Clone)]
pub struct Eased {
    value: f32,
    target: f32,
    last: Option<Instant>,
}

impl Eased {
    /// A value at rest at `value`.
    pub fn new(value: f32) -> Eased {
        Eased {
            value,
            target: value,
            last: None,
        }
    }

    /// Where the value is heading.
    pub fn target(&self) -> f32 {
        self.target
    }

    /// The value as last stepped.
    pub fn value(&self) -> f32 {
        self.value
    }

    /// Heads for `target` from wherever the value is now.
    pub fn set(&mut self, target: f32) {
        self.target = target;
    }

    /// Moves to `value` at once and rests there.
    pub fn jump(&mut self, value: f32) {
        self.value = value;
        self.target = value;
    }

    /// Whether the value has arrived.
    pub fn settled(&self) -> bool {
        self.value == self.target
    }

    /// Steps the value towards its target by the time since the last step, with time constant
    /// `tau` seconds, and returns it. Without `animate` it lands on the target.
    pub fn step(&mut self, animate: bool, tau: f32) -> f32 {
        let now = Instant::now();
        let dt = self
            .last
            .map_or(0.0, |last| (now - last).as_secs_f32().min(MAX_DT));
        self.last = Some(now);
        self.advance(animate, dt, tau)
    }

    /// [`Eased::step`] by an explicit `dt` seconds.
    pub fn advance(&mut self, animate: bool, dt: f32, tau: f32) -> f32 {
        let gap = self.target - self.value;
        if !animate || gap.abs() < SETTLED {
            self.value = self.target;
        } else {
            self.value += gap * approach(dt, tau);
        }
        self.value
    }
}

/// The share of the remaining gap an exponential approach with time constant `tau` covers in `dt`
/// seconds.
pub fn approach(dt: f32, tau: f32) -> f32 {
    if tau <= 0.0 {
        1.0
    } else {
        1.0 - (-dt.max(0.0) / tau).exp()
    }
}

/// Linear interpolation from `a` to `b` by `t`.
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_animation_a_value_lands_on_its_target() {
        let mut e = Eased::new(0.0);
        e.set(100.0);
        assert_eq!(e.advance(false, 0.0, SCROLL_TAU), 100.0);
        assert!(e.settled());
    }

    #[test]
    fn with_animation_a_value_closes_most_of_the_gap_in_a_few_time_constants() {
        let mut e = Eased::new(0.0);
        e.set(100.0);
        let after_one = e.advance(true, SCROLL_TAU, SCROLL_TAU);
        assert!((after_one - 63.2).abs() < 0.1, "{after_one}");
        for _ in 0..20 {
            e.advance(true, SCROLL_TAU, SCROLL_TAU);
        }
        assert!(e.settled(), "{}", e.value());
    }

    #[test]
    fn the_step_is_frame_rate_independent() {
        let (mut fast, mut slow) = (Eased::new(0.0), Eased::new(0.0));
        fast.set(1.0);
        slow.set(1.0);
        for _ in 0..4 {
            fast.advance(true, 1.0 / 240.0, FOCUS_TAU);
        }
        slow.advance(true, 1.0 / 60.0, FOCUS_TAU);
        assert!((fast.value() - slow.value()).abs() < 1e-5);
    }

    #[test]
    fn jump_moves_at_once() {
        let mut e = Eased::new(0.0);
        e.set(5.0);
        e.jump(2.0);
        assert_eq!((e.value(), e.target()), (2.0, 2.0));
    }

    #[test]
    fn a_zero_time_constant_arrives_in_one_step() {
        assert_eq!(approach(0.016, 0.0), 1.0);
        assert_eq!(lerp(2.0, 4.0, 0.5), 3.0);
    }
}
