//! The contrast scan's lens drive, and where along it each focus area's sample falls. Times are
//! microseconds of scan time unless named otherwise.

use crate::adjust::configuration::Configuration;
use crate::adjust::drive_step::search_mode;
use crate::adjust::motion::drive::{DriveKind, DriveLimits, DriveProfile};
use crate::adjust::motion::sampling::{SampleTargets, PRIMARY_AREAS, SECONDARY_AREAS};
use crate::adjust::timing::{CaptureTiming, LevelScale};
use crate::exposure::ports::Properties;

/// When a group of focus areas is exposed within a frame, in pipeline lines.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AreaTiming {
    /// How long each area is exposed.
    pub exposure: u32,
    /// Where the first area's exposure starts.
    pub start: u32,
    /// Between one area's exposure and the next one's.
    pub gap: u16,
    /// Secondary areas: the target slots filled, those past `count` with the first area's target.
    pub rows: u8,
    /// The areas exposed one after another.
    pub count: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SampleAreas {
    /// Up to seven areas.
    Primary(AreaTiming),
    /// Up to ten areas.
    Secondary(AreaTiming),
}

impl SampleAreas {
    /// The measurement's layout word 1 samples the secondary areas (0xfa9e78, 0xfa9f9c, 0xfaa4a4).
    pub const fn from_layout(layout: u32, primary: AreaTiming, secondary: AreaTiming) -> Self {
        if layout == 1 { Self::Secondary(secondary) } else { Self::Primary(primary) }
    }
}

impl Default for SampleAreas {
    fn default() -> Self { Self::Primary(AreaTiming::default()) }
}

/// The frame timing a scan samples by.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SampleTiming {
    /// The exposure level, in hundredths of a doubling.
    pub level: i16,
    /// The capture pipeline source.
    pub source: u32,
    pub areas: SampleAreas,
}

/// Where a scan drive runs and when it starts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScanLeg {
    pub start: i16,
    pub end: i16,
    pub reverse: bool,
    pub start_time: i32,
}

/// Each sampled area's exposure start relative to its frame's, and how long it lasts.
struct Schedule {
    span: i32,
    offsets: [i32; SECONDARY_AREAS],
    sampled: usize,
}

impl Schedule {
    fn offsets(&self) -> &[i32] { &self.offsets[..self.sampled] }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ScanDrive {
    pub profile: DriveProfile,
    /// The scan time at which the drive starts.
    pub start_time: i32,
    timing: SampleTiming,
}

impl ScanDrive {
    pub const fn timing(&self) -> &SampleTiming { &self.timing }

    pub fn set_timing(&mut self, timing: SampleTiming) { self.timing = timing; }

    fn schedule(&self, ports: &mut (impl CaptureTiming + LevelScale)) -> Schedule {
        let span = match ports.linear(self.timing.level) {
            0 => 0,
            linear => (100_000_000 / linear) as i32,
        };
        let source = self.timing.source;
        let lag = u32::from(ports.reference_line(source)).wrapping_sub(ports.readout_start(source));
        let period = ports.line_period(source).unwrap_or(0);
        let (areas, slots) = match self.timing.areas {
            SampleAreas::Primary(areas) => (areas, PRIMARY_AREAS),
            SampleAreas::Secondary(areas) => (areas, SECONDARY_AREAS),
        };
        let sampled = usize::from(areas.count).min(slots);
        let first = areas.start.wrapping_add(areas.exposure >> 1).wrapping_add(lag);
        let step = u32::from(areas.gap).wrapping_add(areas.exposure);
        let mut offsets = [0; SECONDARY_AREAS];
        for (index, offset) in (0u32..).zip(&mut offsets[..sampled]) {
            let line = first.wrapping_add(step.wrapping_mul(index));
            *offset = line.wrapping_mul(period).wrapping_sub(span as u32) as i32;
        }
        Schedule { span, offsets, sampled }
    }

    /// Takes `level` as the exposure level and returns the lens travel at `speed` from a frame's
    /// start to halfway through its latest-exposed area (0xfaa578).
    pub fn exposure_travel(&mut self, ports: &mut (impl CaptureTiming + LevelScale), level: i16, speed: u32) -> u16 {
        self.timing.level = level;
        let schedule = self.schedule(ports);
        let Some(&latest) = schedule.offsets().iter().max() else {
            return 0;
        };
        let travelled = |time: i32| i32::from((i64::from(speed) * i64::from(time) / 1_000_000) as i16);
        let midpoint = (travelled(latest) + travelled(latest.wrapping_add(schedule.span))) / 2;
        midpoint.max(0) as u16
    }

    /// Plans the drive along `leg` at up to `speed` (0xfa9c4c).
    pub fn plan(&mut self, limits: &mut impl DriveLimits, configuration: &Configuration, leg: ScanLeg, speed: u32, kind: DriveKind) {
        self.profile.start = leg.start;
        self.profile.end = leg.end;
        self.profile.reverse = leg.reverse;
        self.start_time = leg.start_time;
        self.profile.plan_for(speed, kind, search_mode(configuration), limits);
    }

    /// The scan time `frames` frame intervals in, less the drive's start time.
    fn drive_time(&self, frames: u32, interval: u32) -> i32 { frames.wrapping_mul(interval).wrapping_sub(self.start_time as u32) as i32 }

    /// The lens position `frames` frame intervals into the scan (0xfa9ff4).
    pub fn position_at_frame(&self, frames: u32, interval: u32) -> i16 { self.profile.position_at(self.drive_time(frames, interval)) }

    /// Each sampled area's lens target for a frame taken at `position`, `frames` frame intervals
    /// into the scan: the position moved by the drive's midpoint over the area's exposure
    /// (0xfa9e34, 0xfaa018).
    pub fn sample_targets(&self, ports: &mut (impl CaptureTiming + LevelScale + Properties), position: i16, frames: u32, interval: u32) -> SampleTargets {
        let schedule = self.schedule(ports);
        let distance = (i32::from(position) - i32::from(self.profile.start)).abs();
        let time = if 0 < distance && distance < i32::from(self.profile.distance()) {
            self.profile.time_to(position) as i32
        } else {
            self.drive_time(frames, interval)
        };
        let mut targets = SampleTargets::default();
        let slots = match self.timing.areas {
            SampleAreas::Primary(_) => &mut targets.primary[..],
            SampleAreas::Secondary(_) => &mut targets.secondary[..],
        };
        for (slot, &offset) in slots.iter_mut().zip(schedule.offsets()) {
            *slot = self.profile.midpoint_correction(ports, position, time, offset, schedule.span).wrapping_add(position);
        }
        if let SampleAreas::Secondary(areas) = self.timing.areas {
            let first = targets.secondary[0];
            for slot in targets.secondary.iter_mut().take(usize::from(areas.rows)).skip(schedule.sampled) {
                *slot = first;
            }
        }
        targets
    }
}
