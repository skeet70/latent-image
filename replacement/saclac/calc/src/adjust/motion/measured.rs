//! The run's levels against those calc measured for it (calc 0x3c03, 0xf9cdfc): when the
//! configured measured level lies above the record's by more than the configured margin, the
//! run's frames are exposed again at the measured level.

use xm1_saclac_profile_records::stock::record::group_01;

use crate::adjust::motion::exposure::{ExposureInputs, ExposurePorts, FrameExposure};
use crate::adjust::motion::prepare::{corrected, profile_mode, trace_exposure};
use crate::adjust::motion::state::MotionState;
use crate::adjust::timing::CaptureTiming;
use crate::exposure::ports::Lens;
use crate::exposure::settings;
use crate::scene_levels::moved_curve_point;
use crate::stock::motion::{self as stock, ExposureSummary, LevelCheckTrace};

/// Everything the level check reaches.
pub trait MeasuredPorts: ExposurePorts + CaptureTiming {}

impl<T: ExposurePorts + CaptureTiming> MeasuredPorts for T {}

/// The record levels calc measured for the run.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MeasuredLevels {
    /// In the order of the readings' record levels.
    pub levels: [i16; 5],
    /// The level measured at `curve_point` of the level curves.
    pub curve_level: i16,
    pub curve_point: u16,
}

/// What the run made of the measured levels.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LevelCheck {
    /// The record's level at the measured curve point.
    pub curve_level: i16,
    /// The run's frames were exposed again at the measured level.
    pub reexposed: bool,
}

/// The level pair the check compares.
#[derive(Clone, Copy)]
enum CheckedLevel {
    Record(usize),
    Curve,
}

impl CheckedLevel {
    /// Selections 1 to 5 pick record levels 0x96, 0xa8 and 0x92, the curve level and level 0x9c;
    /// any other picks level 0x94, or 0x9c for a class 2 record at selection 0.
    const fn selected(selection: u32, class: u8) -> Self {
        match selection {
            0 if class == 2 => Self::Record(4),
            1 => Self::Record(2),
            2 => Self::Record(0),
            3 => Self::Record(3),
            4 => Self::Curve,
            5 => Self::Record(4),
            _ => Self::Record(1),
        }
    }
}

fn summary(ports: &mut impl Lens, exposure: &FrameExposure, lens: u32) -> ExposureSummary {
    ExposureSummary {
        at_06: exposure.at_06,
        lens_step: exposure.lens_step,
        step_level: ports.level(exposure.lens_step, lens),
        exposure_level: exposure.exposure_level,
        first_level: exposure.first_level,
        mode: profile_mode(exposure.profile),
        change: exposure.change,
        step: exposure.step,
    }
}

impl MotionState {
    /// Also traces both sets of levels and the two prepared exposures.
    pub fn check_measured_levels(&mut self, ports: &mut impl MeasuredPorts, measured: &MeasuredLevels) {
        let readings = self.readings;
        let reference_key: u32 = ports.profile().read(readings.record, &group_01::AT_00BC);
        let reference = ports.reference_offset(reference_key);
        let curve_level = moved_curve_point(ports, u32::from(measured.curve_point), readings.at_00b8, reference).wrapping_sub(reference);
        let step_two = settings::level_step(ports, 2);
        let configured = ports.property(stock::MEASURED_LEVEL_SELECTION);
        let selection = if readings.channel == 0 { configured & 0xf } else { (configured & 0xfff) >> 4 };
        let (level, record_level) = match CheckedLevel::selected(selection, readings.class) {
            CheckedLevel::Record(index) => (measured.levels[index], readings.levels[index]),
            CheckedLevel::Curve => (measured.curve_level, curve_level),
        };
        let excess = level.wrapping_sub(step_two).wrapping_sub(record_level);
        let reexposed = i32::from(excess) > ports.property(stock::MEASURED_LEVEL_MARGIN) as i32;
        ports.write_u32(stock::reexposure_flag(self.section), u32::from(reexposed));
        if reexposed {
            let inputs = ExposureInputs { readings: &readings, limits: self.limits, level_ceiling: self.search.level_ceiling, results: self.results };
            let frames = inputs.frame(ports, &mut self.program, level, 0, readings.level_step);
            let frames = corrected(ports, frames, &self.exposures.first, readings.lens);
            trace_exposure(ports, self.section, &frames, 0, readings.lens);
            ports.write_u32(stock::reexposure_word(self.section), 0);
            self.exposures.second = frames;
        }
        self.level_check = LevelCheck { curve_level, reexposed };
        let raised = |level: i16| i32::from(level) + i32::from(step_two);
        let [at_00a8, at_0094, at_0096, at_0092, _] = readings.levels;
        let [measured_00a8, measured_0094, measured_0096, measured_0092, _] = measured.levels;
        let trace = LevelCheckTrace {
            record_levels: [at_00a8, at_0094, at_0096, at_0092, curve_level].map(raised),
            measured_levels: [measured_00a8, measured_0094, measured_0096, measured_0092, measured.curve_level],
            second: summary(ports, &self.exposures.prepared_second, readings.lens),
            first: summary(ports, &self.exposures.first, readings.lens),
            selection,
            curve_point: measured.curve_point,
        };
        stock::trace_level_check(self.section, &trace, ports);
    }
}
