//! The contrast scan's lens drive, and where along it each focus area's sample falls. Times are
//! microseconds of scan time unless named otherwise.

use xm1_saclac_interface::RecordHandle;

use crate::adjust::configuration::Configuration;
use crate::adjust::drive_step::{scan_step, search_mode, Monitor, ScanStep, ScanStepQuery};
use crate::adjust::lens::{LensConfiguration, LensControl, LensSteps, ReferenceScale};
use crate::adjust::motion::drive::{DriveKind, DriveLimits, DriveProfile};
use crate::adjust::motion::sampling::{SampleTargets, PRIMARY_AREAS, SECONDARY_AREAS};
use crate::adjust::timing::{CaptureTiming, LevelScale};
use crate::exposure::ports::{ProfileAccess, Properties};
use crate::exposure::settings;
use crate::stock::adjust as stock;

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

/// What the search took in for the frame a scan is planned from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScanFrame {
    /// The lens's calibration index.
    pub index: u32,
    pub level: i16,
    pub resource: u32,
    /// The reference scale entry the frame interval is read from.
    pub scale_group: u16,
    pub scale_entry: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScanQuery {
    pub frame: ScanFrame,
    pub monitor: Option<Monitor>,
    pub contrast: u32,
    pub drive_kind: DriveKind,
    /// Drives past the second limit, else past the first.
    pub toward_second: bool,
    /// The first and second limits, as the area pair leaves them.
    pub limits: [i16; 2],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScanPlan {
    pub step: ScanStep,
    /// How far the lens moves before a frame's latest area is half exposed.
    pub travel: u16,
    /// The limits widened by the travel.
    pub limits: [i16; 2],
    /// The drive's ends: the lens's position and the travel past the far limit, clamped into the
    /// lens's position limits.
    pub window: [i16; 2],
    /// The drive runs toward the first limit.
    pub reverse: bool,
}

/// Where a frame's samples put the lens.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CorrectedPosition {
    /// The first secondary area's target, or the centre primary area's.
    pub target: i16,
    /// Where the drive has the lens.
    pub position: i16,
}

/// Everything planning a scan reaches.
pub trait ScanPorts: Properties + ProfileAccess + LensSteps + ReferenceScale + LensControl + LensConfiguration + CaptureTiming + LevelScale + DriveLimits {}

impl<T: Properties + ProfileAccess + LensSteps + ReferenceScale + LensControl + LensConfiguration + CaptureTiming + LevelScale + DriveLimits> ScanPorts for T {}

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
    profile: DriveProfile,
    /// The scan time at which the drive starts.
    start_time: i32,
    /// Microseconds per frame.
    interval: u32,
    timing: SampleTiming,
}

impl ScanDrive {
    pub fn set_timing(&mut self, timing: SampleTiming) { self.timing = timing; }

    /// Plans the scan's step and its drive for the frame the search took in (0xf3ec50 after the
    /// area pair: 0xfa9bc0, 0xfaa990, 0xfaa578, 0xfaa670, 0xfa9bf0, 0xfaa6a0, 0xfa9c4c).
    pub fn plan_scan(&mut self, ports: &mut impl ScanPorts, configuration: &Configuration, record: RecordHandle, query: ScanQuery) -> ScanPlan {
        let frame = query.frame;
        self.interval = 100_000_000 / ports.reference_scale(frame.scale_group, frame.scale_entry).max(1);
        let step_query = ScanStepQuery { monitor: query.monitor, index: frame.index, resource: frame.resource, interval: self.interval, contrast: query.contrast, drive_kind: query.drive_kind };
        let step = scan_step(ports, configuration, record, step_query);
        let travel = self.exposure_travel(ports, frame.level, step.step.rate);
        let reach = travel as i16;
        let [first, second] = query.limits;
        let limits = [first.wrapping_sub(reach), second.wrapping_add(reach)];
        let position = ports.position() as i16;
        let reverse = !query.toward_second;
        let far = if reverse { limits[0].wrapping_sub(reach) } else { limits[1].wrapping_add(reach) };
        let mut window = [position, far];
        clamp_window(ports, frame.resource, &mut window);
        let delay = u32::from(settings::word(ports, stock::SCAN_DRIVE_DELAY)).wrapping_add(ports.property(stock::SCAN_DRIVE_DELAY_TRIM));
        self.profile.start = window[0];
        self.profile.end = window[1];
        self.profile.reverse = reverse;
        self.start_time = delay.wrapping_mul(100) as i32;
        self.profile.plan_for(step.step.rate, query.drive_kind, search_mode(configuration), ports);
        ScanPlan { step, travel, limits, window, reverse }
    }

    /// The lens position `frames` frame intervals into the scan, and the target its samples put
    /// the lens at (0xf3edcc: 0xfa9ff4, 0xfa9e34, 0xfaa018).
    pub fn corrected_position(&self, ports: &mut (impl CaptureTiming + LevelScale + Properties), frames: u16) -> CorrectedPosition {
        let time = u32::from(frames).wrapping_mul(self.interval).wrapping_sub(self.start_time as u32) as i32;
        let position = self.profile.position_at(time);
        let targets = self.sample_targets(ports, position, time);
        let target = match self.timing.areas {
            SampleAreas::Primary(_) => targets.primary[PRIMARY_AREAS / 2],
            SampleAreas::Secondary(_) => targets.secondary[0],
        };
        CorrectedPosition { target, position }
    }

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
    fn exposure_travel(&mut self, ports: &mut (impl CaptureTiming + LevelScale), level: i16, speed: u32) -> u16 {
        self.timing.level = level;
        let schedule = self.schedule(ports);
        let Some(&latest) = schedule.offsets().iter().max() else {
            return 0;
        };
        let travelled = |time: i32| i32::from((i64::from(speed) * i64::from(time) / 1_000_000) as i16);
        let midpoint = (travelled(latest) + travelled(latest.wrapping_add(schedule.span))) / 2;
        midpoint.max(0) as u16
    }

    /// Each sampled area's lens target for a frame taken at `position`, `time` into the drive:
    /// the position moved by the drive's midpoint over the area's exposure (0xfaa018).
    fn sample_targets(&self, ports: &mut (impl CaptureTiming + LevelScale + Properties), position: i16, time: i32) -> SampleTargets {
        let schedule = self.schedule(ports);
        let distance = (i32::from(position) - i32::from(self.profile.start)).abs();
        let time = if 0 < distance && distance < i32::from(self.profile.distance()) { self.profile.time_to(position) as i32 } else { time };
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

/// Clamps the window into the lens's position limits for `key`, the limit on its starting side
/// pulled in by the lens's travel margin (0xfaa6a0).
fn clamp_window(ports: &mut (impl LensConfiguration + LensControl), key: u32, window: &mut [i16; 2]) {
    let margin = ports.travel_margin() as i16;
    let [mut lower, mut upper] = ports.position_limits(key);
    if window[1] < window[0] {
        upper = upper.wrapping_sub(margin);
    } else {
        lower = lower.wrapping_add(margin);
    }
    for end in window.iter_mut() {
        *end = (*end).min(upper).max(lower);
    }
}
