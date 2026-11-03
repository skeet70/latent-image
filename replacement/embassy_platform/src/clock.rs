//! Time over the stock kernel tick and the kernel timer counter (reports/xm1-timer-interrupt-laws.md §1).
//!
//! One time unit is one counter count; `RELOAD` counts make one kernel tick, which advances the
//! kernel clock by `KERNEL_UNITS_PER_TICK` and the global tick cell by one.

pub const RELOAD: u32 = 30_000;
pub const KERNEL_UNITS_PER_TICK: i32 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CounterDirection {
    Up,
    Down,
}

impl CounterDirection {
    /// Position within the current tick, 0..RELOAD. Up counts from about 0, Down from RELOAD to 1.
    pub const fn sub_tick(self, count: u32) -> u32 {
        let position = match self {
            CounterDirection::Up => count,
            CounterDirection::Down => RELOAD.saturating_sub(count),
        };
        if position >= RELOAD {
            RELOAD - 1
        } else {
            position
        }
    }

    /// The direction two counter samples taken within one tick show, if they differ.
    pub const fn measured(first: u32, second: u32) -> Option<CounterDirection> {
        if second > first {
            Some(CounterDirection::Up)
        } else if second < first {
            Some(CounterDirection::Down)
        } else {
            None
        }
    }
}

/// One domain's view of time: the 32-bit tick cell extended to 64 bits and clamped monotonic.
#[derive(Debug)]
pub struct Clock {
    epoch: u32,
    last_tick: u32,
    last_now: u64,
    sub_tick_valid: bool,
}

impl Clock {
    pub const fn new() -> Clock {
        Clock { epoch: 0, last_tick: 0, last_now: 0, sub_tick_valid: true }
    }

    /// Drops the sub-tick term, for a counter whose direction disagrees with the build.
    pub fn use_ticks_only(&mut self) {
        self.sub_tick_valid = false;
    }

    /// `tick` and `count` must come from one tick: read the tick cell, the counter, then the
    /// tick cell again, and retry while the two tick reads differ.
    pub fn now(&mut self, tick: u32, count: u32, direction: CounterDirection) -> Sample {
        if tick < self.last_tick {
            self.epoch = self.epoch.wrapping_add(1);
        }
        self.last_tick = tick;
        let ticks = (self.epoch as u64) << 32 | tick as u64;
        let sub = if self.sub_tick_valid { direction.sub_tick(count) } else { 0 };
        let value = ticks * RELOAD as u64 + sub as u64;
        let regression = self.last_now.saturating_sub(value);
        if value > self.last_now {
            self.last_now = value;
        }
        Sample { now: self.last_now, regression }
    }
}

/// A clamped time and how far below the previous value the raw reading fell (0 when it did not).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub now: u64,
    pub regression: u64,
}

/// Root `+0x28` timeout: -1 without a deadline, otherwise whole ticks rounded up, at least one,
/// in kernel clock units. The wake may still be up to one tick early.
pub fn sleep_timeout(now: u64, deadline: u64) -> i32 {
    if deadline == u64::MAX {
        return -1;
    }
    let remaining = deadline.saturating_sub(now);
    let ticks = remaining.div_ceil(RELOAD as u64).max(1);
    let limit = (i32::MAX / KERNEL_UNITS_PER_TICK) as u64;
    ticks.min(limit) as i32 * KERNEL_UNITS_PER_TICK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_directions_map_to_position_within_the_tick() {
        assert_eq!(CounterDirection::Up.sub_tick(0), 0);
        assert_eq!(CounterDirection::Up.sub_tick(29_999), 29_999);
        assert_eq!(CounterDirection::Up.sub_tick(30_000), 29_999);
        assert_eq!(CounterDirection::Down.sub_tick(30_000), 0);
        assert_eq!(CounterDirection::Down.sub_tick(1), 29_999);
        assert_eq!(CounterDirection::Down.sub_tick(0xffff_ffff), 0);
    }

    #[test]
    fn direction_is_measured_from_two_samples() {
        assert_eq!(CounterDirection::measured(100, 200), Some(CounterDirection::Up));
        assert_eq!(CounterDirection::measured(200, 100), Some(CounterDirection::Down));
        assert_eq!(CounterDirection::measured(5, 5), None);
    }

    #[test]
    fn now_combines_ticks_and_sub_ticks() {
        let mut clock = Clock::new();
        assert_eq!(clock.now(3, 1_000, CounterDirection::Up).now, 91_000);
        assert_eq!(clock.now(3, 29_000, CounterDirection::Down).now, 91_000);
        assert_eq!(clock.now(4, 29_000, CounterDirection::Down).now, 121_000);
    }

    #[test]
    fn now_never_runs_backwards_across_a_late_tick_update() {
        let mut clock = Clock::new();
        assert_eq!(clock.now(7, 29_990, CounterDirection::Up), Sample { now: 239_990, regression: 0 });
        // Counter wrapped before the tick IRQ advanced the cell.
        assert_eq!(clock.now(7, 10, CounterDirection::Up), Sample { now: 239_990, regression: 29_980 });
        assert_eq!(clock.now(8, 20, CounterDirection::Up), Sample { now: 240_020, regression: 0 });
    }

    #[test]
    fn now_extends_the_tick_cell_past_its_wrap() {
        let mut clock = Clock::new();
        clock.now(u32::MAX, 0, CounterDirection::Up);
        assert_eq!(clock.now(0, 0, CounterDirection::Up).now, (1u64 << 32) * RELOAD as u64);
    }

    #[test]
    fn ticks_only_mode_drops_the_counter() {
        let mut clock = Clock::new();
        clock.use_ticks_only();
        assert_eq!(clock.now(2, 12_345, CounterDirection::Up).now, 60_000);
    }

    #[test]
    fn timeouts_round_up_to_whole_ticks() {
        assert_eq!(sleep_timeout(0, u64::MAX), -1);
        assert_eq!(sleep_timeout(100, 50), 10);
        assert_eq!(sleep_timeout(0, 1), 10);
        assert_eq!(sleep_timeout(0, 30_000), 10);
        assert_eq!(sleep_timeout(0, 30_001), 20);
        assert_eq!(sleep_timeout(0, 300_000), 100);
        assert_eq!(sleep_timeout(0, u64::MAX - 1), (i32::MAX / 10) * 10);
    }
}
