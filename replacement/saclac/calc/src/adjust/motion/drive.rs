//! The focus lens drive between two positions: a ramp from rest to its top speed, a second ramp
//! up to the cruise speed, the cruise, and both ramps mirrored. Positions are lens steps, times
//! microseconds, speeds steps per second and accelerations steps per second squared.

use xm1_saclac_interface::calc::SharedAddress;

use crate::exposure::ports::StockMemory;
use crate::stock::motion as trace;

/// The lens's drive kind the limits are read for (0x76f360 at preparation).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DriveKind(pub u32);

/// The drive limits image 4 keeps in its configuration; each entry is called for direction 0
/// and drive step 8.
pub trait DriveLimits {
    /// The highest speed the first ramp may reach (0x748090).
    fn first_ramp_speed(&mut self, kind: DriveKind) -> u32;
    /// 0x748104.
    fn first_ramp_acceleration(&mut self, kind: DriveKind) -> u32;
    /// 0xffff where the configuration sets none (0x7481c4).
    fn second_ramp_acceleration(&mut self, kind: DriveKind) -> u32;
    /// A requested speed below it is driven as one cruise (0x747b24).
    fn slowest_ramped_speed(&mut self) -> u32;
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Ramp {
    /// The speed the ramp ends at.
    pub speed: u32,
    pub acceleration: u32,
    pub distance: u16,
    pub duration: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DriveProfile {
    pub start: i16,
    pub end: i16,
    pub reverse: bool,
    pub first: Ramp,
    /// Ends at the cruise speed.
    pub second: Ramp,
    pub cruise_distance: u16,
    pub cruise_duration: u32,
}

/// What one planning settled on, as the dump frame records it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Planning {
    pub first_acceleration: u32,
    pub second_acceleration: u32,
    /// The requested speed, lowered to what the second ramp reaches when it runs.
    pub requested_speed: u32,
    pub first_speed: u32,
    pub distance: u16,
    pub first_distance: u16,
    pub first_duration: u32,
    pub second_distance: u16,
    pub second_duration: u32,
    pub cruise_distance: u16,
    pub cruise_duration: u32,
}

impl Planning {
    /// Records the planning after the drive's start time, as 0xf969a8 does.
    pub fn trace(&self, start_time: i32, section: SharedAddress, memory: &mut impl StockMemory) {
        let words = [
            start_time as u32,
            self.first_acceleration,
            self.second_acceleration,
            self.requested_speed,
            self.first_speed,
            u32::from(self.distance),
            u32::from(self.first_distance),
            self.first_duration,
            u32::from(self.second_distance),
            self.second_duration,
            u32::from(self.cruise_distance),
            self.cruise_duration,
        ];
        for (index, word) in (0..).zip(words) {
            memory.write_u32(trace::drive_planning(section, index), word);
        }
    }
}

/// The library's 64-bit quotient: a zero divisor gives all ones, or zero for a zero dividend
/// (0x89bed0).
pub(crate) fn quotient(dividend: u64, divisor: u64) -> u64 {
    match dividend.checked_div(divisor) {
        Some(value) => value,
        None if dividend == 0 => 0,
        None => u64::MAX,
    }
}

/// The correctly rounded square root, as the library's `sqrtf` returns it (0xee9c50).
pub(crate) fn square_root(value: f32) -> f32 {
    if value.is_nan() || value < 0.0 {
        return f32::NAN;
    }
    if value == 0.0 || value.is_infinite() {
        return value;
    }
    let bits = value.to_bits();
    let (mut significand, mut exponent) = match bits >> 23 {
        0 => {
            let shift = (bits & 0x007f_ffff).leading_zeros() - 8;
            (u64::from(bits << shift), 1 - shift as i32)
        }
        biased => (u64::from(bits & 0x007f_ffff | 0x0080_0000), biased as i32),
    };
    // value = significand * 2^(exponent - 150); make the power even, then widen.
    exponent -= 150;
    if exponent & 1 != 0 {
        significand <<= 1;
        exponent -= 1;
    }
    significand <<= 38;
    exponent -= 38;
    let (root, remainder) = integer_root(significand);
    let width = 64 - root.leading_zeros();
    let dropped = width - 24;
    let mut kept = root >> dropped;
    let rest = root & ((1 << dropped) - 1);
    let half = 1 << (dropped - 1);
    if rest > half || (rest == half && (remainder != 0 || kept & 1 == 1)) {
        kept += 1;
    }
    let mut power = exponent / 2 + dropped as i32;
    if kept == 1 << 24 {
        kept >>= 1;
        power += 1;
    }
    f32::from_bits(((power + 23 + 127) as u32) << 23 | (kept as u32 & 0x007f_ffff))
}

fn integer_root(value: u64) -> (u64, u64) {
    let mut root = 0;
    let mut remainder = value;
    let mut bit = 1u64 << 62;
    while bit > value {
        bit >>= 2;
    }
    while bit != 0 {
        if remainder >= root + bit {
            remainder -= root + bit;
            root = (root >> 1) + bit;
        } else {
            root >>= 1;
        }
        bit >>= 2;
    }
    (root, remainder)
}

/// Distance covered from rest in `time` under `acceleration`, a·t²/2 with the time in
/// microseconds; above 0xffffff the acceleration is taken in thousands (0xf95f4c).
fn accelerated(acceleration: u32, time: i32) -> u16 {
    let time = i64::from(time) as u64;
    let square = time.wrapping_mul(time) / 10_000;
    let distance = if acceleration <= 0x00ff_ffff {
        square.wrapping_mul(u64::from(acceleration)) / 200_000_000
    } else {
        square.wrapping_mul(u64::from(acceleration) / 1000) / 200_000
    };
    distance as u16
}

fn at_speed(speed: u32, time: i32) -> u16 { ((i64::from(time) as u64).wrapping_mul(u64::from(speed)) / 1_000_000) as u16 }

/// The time at which a ramp from `speed` under `acceleration` has covered `distance`, after
/// `base`.
fn speeding(base: u32, speed: u32, distance: u16, acceleration: u32) -> u32 {
    let speed = speed as f32;
    let rate = acceleration as f32;
    let root = square_root(speed * speed + (rate + rate) * f32::from(distance));
    (base as f32 + (root - speed) * 1_000_000.0 / rate) as u32
}

/// The same for a ramp down from `speed`.
fn slowing(base: u32, speed: u32, distance: u16, acceleration: u32) -> u32 {
    let speed = speed as f32;
    let rate = acceleration as f32;
    let root = square_root(speed * speed - (rate + rate) * f32::from(distance));
    (base as f32 + (speed - root) * 1_000_000.0 / rate) as u32
}

/// One ramp fitted into `extent`, the distance it and its mirror may share (0xf96828).
#[derive(Clone, Copy, Debug, Default)]
struct RampFit {
    distance: u16,
    speed: u32,
    duration: u32,
}

fn fit_ramp(extent: u16, initial: u32, acceleration: u32, limit: u32) -> RampFit {
    if acceleration == 0 {
        return RampFit { distance: 0, speed: limit, duration: 0 };
    }
    let squares = (u64::from(limit) * u64::from(limit)).wrapping_sub(u64::from(initial) * u64::from(initial));
    let full = quotient(squares, u64::from(acceleration.wrapping_mul(2))) as u16;
    if i32::from(extent) >= i32::from(full.wrapping_mul(2) as i16) {
        let duration = if full == 0 {
            0
        } else {
            let gain = u64::from(limit).wrapping_sub(u64::from(initial));
            quotient(gain.wrapping_mul(1_000_000), u64::from(acceleration)) as u32
        };
        return RampFit { distance: full, speed: limit, duration };
    }
    let half = extent >> 1;
    let start = initial as f32;
    let rate = acceleration as f32;
    let root = square_root(start * start + (rate + rate) * f32::from(half));
    let duration = ((root - start) * 1_000_000.0 / rate) as u32;
    let gained = (u64::from(duration) * u64::from(acceleration) / 1_000_000) as u32;
    RampFit { distance: half, speed: initial.wrapping_add(gained), duration }
}

impl DriveProfile {
    pub fn distance(&self) -> u16 { (i32::from(self.end) - i32::from(self.start)).unsigned_abs() as u16 }

    /// Plans the drive's phases for up to `speed`, keeping its ends and direction (0xf969a8).
    pub fn plan(&mut self, speed: u32, kind: DriveKind, limits: &mut impl DriveLimits) -> Planning {
        let distance = self.distance();
        let first_limit = limits.first_ramp_speed(kind);
        let first_acceleration = limits.first_ramp_acceleration(kind);
        let second_acceleration = limits.second_ramp_acceleration(kind);
        let mut requested = speed;
        let mut cruise_speed = speed;
        let mut first = RampFit { speed: first_limit.min(requested), ..RampFit::default() };
        let mut second = RampFit::default();
        if limits.slowest_ramped_speed() <= requested && (first_acceleration | second_acceleration) != 0 {
            if first.speed != 0 {
                first = fit_ramp(distance, 0, first_acceleration, first.speed);
                cruise_speed = first.speed;
            }
            if i32::from(distance) > 2 * i32::from(first.distance) && requested > first.speed {
                second = fit_ramp(distance.wrapping_sub(first.distance.wrapping_mul(2)), first.speed, second_acceleration, requested);
                requested = second.speed;
                cruise_speed = requested;
            }
        }
        let ramps = u32::from(first.distance) + u32::from(second.distance);
        let (cruise_distance, cruise_duration) = if i32::from(distance) > (2 * ramps) as i32 && cruise_speed != 0 {
            let remaining = distance.wrapping_sub((2 * ramps) as u16);
            (remaining, quotient(u64::from(remaining) * 1_000_000, u64::from(cruise_speed)) as u32)
        } else {
            (0, 0)
        };
        self.first = Ramp { speed: first.speed, acceleration: first_acceleration, distance: first.distance, duration: first.duration };
        self.second = Ramp { speed: cruise_speed, acceleration: second_acceleration, distance: second.distance, duration: second.duration };
        self.cruise_distance = cruise_distance;
        self.cruise_duration = cruise_duration;
        Planning {
            first_acceleration,
            second_acceleration,
            requested_speed: requested,
            first_speed: first.speed,
            distance,
            first_distance: first.distance,
            first_duration: first.duration,
            second_distance: second.distance,
            second_duration: second.duration,
            cruise_distance,
            cruise_duration,
        }
    }

    /// The lens position `time` after the drive starts; the start before it (0xf96004).
    pub fn position_at(&self, time: i32) -> i16 {
        let travelled = if time <= 0 { 0 } else { self.travelled(time) };
        let offset = if self.reverse { travelled.wrapping_neg() } else { travelled };
        offset.wrapping_add(self.start as u16) as i16
    }

    fn travelled(&self, time: i32) -> u16 {
        let (first, second) = (&self.first, &self.second);
        let first_end = first.duration as i32;
        let second_end = first_end.wrapping_add(second.duration as i32);
        let cruise_end = second_end.wrapping_add(self.cruise_duration as i32);
        let slowing_end = cruise_end.wrapping_add(second.duration as i32);
        let stop = slowing_end.wrapping_add(first_end);
        if time <= first_end {
            accelerated(first.acceleration, time)
        } else if time <= second_end {
            let elapsed = time.wrapping_sub(first_end);
            first.distance.wrapping_add(at_speed(first.speed, elapsed)).wrapping_add(accelerated(second.acceleration, elapsed))
        } else if time <= cruise_end {
            let elapsed = time.wrapping_sub(second_end);
            second.distance.wrapping_add(first.distance).wrapping_add(at_speed(second.speed, elapsed))
        } else if time <= slowing_end {
            let elapsed = time.wrapping_sub(cruise_end);
            let reached = second.distance.wrapping_add(first.distance).wrapping_add(self.cruise_distance);
            reached.wrapping_add(at_speed(second.speed, elapsed)).wrapping_sub(accelerated(second.acceleration, elapsed))
        } else if time <= stop {
            let elapsed = time.wrapping_sub(slowing_end);
            let reached = self.cruise_distance.wrapping_add(first.distance).wrapping_add(second.distance.wrapping_mul(2));
            reached.wrapping_add(at_speed(first.speed, elapsed)).wrapping_sub(accelerated(first.acceleration, elapsed))
        } else {
            self.distance()
        }
    }

    /// The time after the drive starts at which it reaches `position`; zero where the phase
    /// holding it has no acceleration or speed (0xf96bec).
    pub fn time_to(&self, position: i16) -> u32 {
        let (first, second) = (&self.first, &self.second);
        let distance = (i32::from(position) - i32::from(self.start)).unsigned_abs() as u16;
        let second_start = first.distance;
        let cruise_start = second_start.wrapping_add(second.distance);
        let slowing_start = cruise_start.wrapping_add(self.cruise_distance);
        let last_start = slowing_start.wrapping_add(second.distance);
        let stop = last_start.wrapping_add(first.distance);
        if distance == 0 {
            0
        } else if distance <= second_start {
            if first.acceleration == 0 {
                return 0;
            }
            let travelled = f32::from(distance);
            (square_root((travelled + travelled) / first.acceleration as f32) * 1_000_000.0) as u32
        } else if distance <= cruise_start {
            if second.acceleration == 0 {
                return 0;
            }
            speeding(first.duration, first.speed, distance.wrapping_sub(second_start), second.acceleration)
        } else if distance <= slowing_start {
            if second.speed == 0 {
                return 0;
            }
            let cruising = quotient(u64::from(distance.wrapping_sub(cruise_start)) * 1_000_000, u64::from(second.speed)) as u32;
            second.duration.wrapping_add(first.duration).wrapping_add(cruising)
        } else if distance <= last_start {
            if second.acceleration == 0 {
                return 0;
            }
            let base = second.duration.wrapping_add(first.duration).wrapping_add(self.cruise_duration);
            slowing(base, second.speed, distance.wrapping_sub(slowing_start), second.acceleration)
        } else if distance <= stop {
            if first.acceleration == 0 {
                return 0;
            }
            let base = self.cruise_duration.wrapping_add(first.duration).wrapping_add(second.duration.wrapping_mul(2));
            slowing(base, first.speed, distance.wrapping_sub(last_start), first.acceleration)
        } else {
            self.cruise_duration.wrapping_add(second.duration.wrapping_add(first.duration).wrapping_mul(2))
        }
    }
}
