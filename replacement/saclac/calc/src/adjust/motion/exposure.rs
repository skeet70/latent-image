//! How a measurement run exposes its frames at a level (0xf971a0): an 18-step program of capture
//! modes, gains and lens steps built from the configuration, and the step a level falls in.

use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_profile_records::stock::record::group_00;
use xm1_saclac_profile_records::stock::shared::group_27;

use crate::adjust::motion::ports::{ExposureTimes, LensCalibration, ProfileGains};
use crate::adjust::motion::queries;
use crate::adjust::motion::readings::{RecordReadings, StepLimits};
use crate::adjust::timing::LevelScale;
use crate::exposure::ports::{CaptureProfiles, Lens, ProfileAccess, Properties, Property, StockMemory};
use crate::exposure::settings;
use crate::stock::motion::{self as stock};

pub const PROGRAM_STEPS: usize = 18;

/// One step of the exposure program.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProgramStep {
    /// The highest level the step covers.
    pub top: i16,
    /// The lowest level, the previous step's top.
    pub bottom: i16,
    pub low_step_level: i16,
    pub high_step_level: i16,
    /// The configured first range: its top, and the next step's top as its base.
    pub first_top: i16,
    pub first_base: i16,
    /// The next step's floor; for the last step the configured ceiling.
    pub ceiling: i16,
    pub floor: i16,
    /// The configured second range, its top capped at the run's level ceiling.
    pub second_top: i16,
    pub second_base: i16,
    pub offset: i16,
    pub low_step: u32,
    pub high_step: u32,
    pub gain: u32,
    pub mode: u8,
    pub profile: u32,
}

pub type Program = [ProgramStep; PROGRAM_STEPS];

/// Which of the three configured programs a run follows.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ProgramKind {
    /// No gain.
    #[default]
    Plain,
    /// Gain steps 12 down to 4.
    Even,
    /// Gain steps 13 down to 4.
    Odd,
}

impl ProgramKind {
    pub const fn index(self) -> usize {
        match self {
            Self::Plain => 0,
            Self::Even => 1,
            Self::Odd => 2,
        }
    }
}

/// The exposure of one frame, as the run's timing records keep it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FrameExposure {
    /// The program step the level fell in; 0 for the fixed exposure.
    pub step: u8,
    /// `level` less level step 2.
    pub base_level: i16,
    pub level: i16,
    pub at_06: i16,
    /// The change the caller asked for plus the program's second range.
    pub change: i16,
    /// The level the exposure time is taken at.
    pub exposure_level: i16,
    pub at_0c: i16,
    pub profile_offset: i16,
    pub lens_step: u32,
    pub profile: u32,
    /// Ten times the linear factor of a negative change, else 10.
    pub change_scale: u16,
    pub gain_scale: u16,
    /// Microseconds.
    pub time: u32,
    /// How many of the program's steps below this one add to their ranges.
    pub ranges_below: u8,
    pub program: ProgramKind,
    pub gain: u32,
    pub frames: u8,
    pub mode: u8,
    pub first_level: i16,
    pub at_30: i16,
    pub at_32: i16,
    pub step_offset: i16,
}

pub trait ExposurePorts: Properties + ProfileAccess + Lens + CaptureProfiles + LensCalibration + ExposureTimes + ProfileGains + LevelScale + StockMemory {}

impl<T: Properties + ProfileAccess + Lens + CaptureProfiles + LensCalibration + ExposureTimes + ProfileGains + LevelScale + StockMemory> ExposurePorts for T {}

/// What a frame's exposure is computed from besides its level.
pub struct ExposureInputs<'a> {
    pub readings: &'a RecordReadings,
    pub limits: StepLimits,
    /// The highest level the second ranges reach.
    pub level_ceiling: i16,
    /// The contrast results the peak check reads.
    pub results: SharedAddress,
}

fn tenths(properties: &mut impl Properties, id: Property) -> i16 { settings::tenths(properties, id) as i16 }

/// Ten times the linear factor of `-value` when it is negative, else 10 (0xf97100).
fn negative_scale(scale: &mut impl LevelScale, value: i16) -> u16 {
    if value >= 0 { 10 } else { (scale.linear(value.wrapping_neg()) / 10) as u16 }
}

/// Ten times the linear factor of `value` when it is not negative, else 10 (0xf97138).
fn positive_scale(scale: &mut impl LevelScale, value: i16) -> u16 {
    if value < 0 { 10 } else { (scale.linear(value) / 10) as u16 }
}

fn floored(properties: &mut impl Properties, level: i16) -> i16 { level.max(tenths(properties, stock::level_floor(0))) }

impl ExposureInputs<'_> {
    fn step_level(&self, ports: &mut impl Lens, step: u32) -> i16 { ports.level(step, self.readings.lens) }

    fn peaked(&self, ports: &mut impl ExposurePorts) -> bool { queries::level_peaked(ports, self.results) }

    /// The exposure of a frame at `level` changed by `change`, with `program` rebuilt for it
    /// around lens step `step`; the fixed exposure leaves `program` as it was.
    pub fn frame(&self, ports: &mut impl ExposurePorts, program: &mut Program, level: i16, change: i16, step: u32) -> FrameExposure {
        if ports.property(stock::FIXED_EXPOSURE) == 1 { self.fixed(ports, level) } else { self.programmed(ports, program, level, change, step) }
    }

    fn fixed(&self, ports: &mut impl ExposurePorts, level: i16) -> FrameExposure {
        let step_two = settings::level_step(ports, 2);
        let exposure_level = settings::signed_word(ports, stock::FIXED_LEVEL);
        let gain_step = match ports.property(stock::FIXED_GAIN_STEP) {
            index @ 0..=8 => index as u8 + 1,
            9 => 0,
            _ => 2,
        };
        let at_0c = settings::level_step(ports, gain_step);
        let configured_step = ports.property(stock::FIXED_STEP);
        let lens_step = if configured_step < ports.steps() { configured_step } else { 0 };
        let step_offset = at_0c.wrapping_sub(step_two);
        let lens_level = self.step_level(ports, lens_step);
        let at_06 = exposure_level.wrapping_sub(step_offset).wrapping_add(lens_level).wrapping_sub(ports.level_offset(stock::SEARCH_PROFILE));
        let change = at_06.wrapping_sub(level);
        let shortest = ports.shortest_time(stock::SEARCH_PROFILE);
        let time = ports.exposure_time(stock::SEARCH_PROFILE, exposure_level, 0).max(shortest);
        FrameExposure {
            base_level: level.wrapping_sub(step_two),
            level,
            at_06,
            change,
            exposure_level,
            at_0c,
            lens_step,
            profile: stock::SEARCH_PROFILE,
            change_scale: negative_scale(ports, change),
            gain_scale: positive_scale(ports, step_offset),
            time,
            frames: 4,
            step_offset,
            ..FrameExposure::default()
        }
    }

    /// Which configured program the record's window and `level` choose (0xf97354..0xf974a4).
    fn program_kind(&self, ports: &mut impl ExposurePorts, level: i16) -> ProgramKind {
        match ports.property(stock::PROGRAM) {
            1 => return ProgramKind::Plain,
            2 => return ProgramKind::Even,
            3 => return ProgramKind::Odd,
            _ => {}
        }
        let level_gated = ports.property(stock::PROGRAM_LEVEL_GATE) != 0;
        let limit = tenths(ports, stock::PROGRAM_LEVEL_LIMIT);
        let record = self.readings.record;
        let mut profile = ports.profile();
        let status = profile.read_shared(record, &group_27::WINDOW_STATUS).0;
        let windowed = if profile.read_shared(record, &group_27::AT_0738) == 4 { ProgramKind::Odd } else { ProgramKind::Even };
        let variant = profile.read(record, &group_00::AT_0066);
        let within_limit = if level_gated && level <= limit { windowed } else { ProgramKind::Plain };
        if variant != 0 && self.readings.locked == 3 && level <= limit {
            return windowed;
        }
        match status {
            1 | 2 => within_limit,
            3 => ProgramKind::Even,
            4 => ProgramKind::Odd,
            _ if self.peaked(ports) => within_limit,
            _ => ProgramKind::Plain,
        }
    }

    fn programmed(&self, ports: &mut impl ExposurePorts, program: &mut Program, level: i16, change: i16, step: u32) -> FrameExposure {
        let step_two = settings::level_step(ports, 2);
        let kind = self.program_kind(ports, level);
        self.build(ports, program, kind, step);
        let changed = level.wrapping_add(change);
        let (index, lens_step) = self.locate(ports, program, changed);
        let entry = program[index];
        let increases = stock::PROGRAM_INCREASES[kind.index()];
        let (first_level, second) = if increases[index] {
            let headroom = entry.top.wrapping_add(entry.first_base).wrapping_sub(changed);
            if headroom <= entry.first_top {
                (headroom, entry.second_base)
            } else {
                (entry.first_top, headroom.wrapping_add(entry.second_base.wrapping_sub(entry.first_top)).min(entry.second_top))
            }
        } else {
            (entry.first_base, entry.second_base)
        };
        let change = change.wrapping_add(second);
        let ranges_below = if index > 14 { 0 } else { increases[..index].iter().filter(|&&increases| increases).count() as u8 };
        let profile_offset = ports.level_offset(entry.profile);
        let lens_level = self.step_level(ports, lens_step);
        let exposure_level = second.wrapping_add(first_level).wrapping_add(changed).wrapping_add(entry.offset).wrapping_add(profile_offset).wrapping_sub(lens_level);
        let exposure_level = match index {
            0 => exposure_level.max(entry.floor),
            17 => exposure_level.min(entry.ceiling),
            _ => exposure_level,
        };
        let at_06 = lens_level.wrapping_sub(entry.offset.wrapping_add(profile_offset).wrapping_add(first_level)).wrapping_add(exposure_level);
        let time = ports.exposure_time(entry.profile, exposure_level, entry.gain);
        let mut exposure = FrameExposure {
            step: index as u8,
            base_level: level.wrapping_sub(step_two),
            level,
            at_06,
            change,
            exposure_level,
            at_0c: first_level.wrapping_add(entry.offset).wrapping_add(step_two),
            profile_offset,
            lens_step,
            profile: entry.profile,
            change_scale: negative_scale(ports, change),
            time,
            ranges_below,
            program: kind,
            gain: entry.gain,
            mode: entry.mode,
            first_level,
            step_offset: entry.offset,
            ..FrameExposure::default()
        };
        exposure.frames = self.frames(ports, &exposure);
        let frames_level = ports.log2_difference(u32::from(exposure.frames).wrapping_mul(100) >> 2, 100);
        exposure.gain_scale = positive_scale(ports, first_level.wrapping_add(frames_level));
        exposure
    }

    /// How many frames the run averages at this exposure, 4 to 63 (0xf98168..0xf982c0).
    fn frames(&self, ports: &mut impl ExposurePorts, exposure: &FrameExposure) -> u8 {
        let by_excess = ports.property(stock::FRAMES_BY_EXCESS) == 1;
        let excess = exposure.at_06.wrapping_sub(exposure.level);
        let measure = if by_excess { excess } else { exposure.change };
        if measure <= 0 || (!by_excess && exposure.step == 0) || self.peaked(ports) {
            return 4;
        }
        let configured = ports.property(stock::FRAMES_FACTOR).wrapping_mul(200);
        let averaged = (configured.checked_div(ports.linear(exposure.first_level)).unwrap_or(0) as u8 as u32 + 1) >> 1;
        let frames = averaged.min(ports.property(stock::FRAMES_LIMIT)).min(63) as u8;
        if frames <= 4 {
            return 4;
        }
        if measure > 0x9e8 {
            return frames;
        }
        let factor = ((ports.linear(measure) << 3) / 100 + 1) >> 1 & 0x7fff;
        factor.min(u32::from(frames)) as u8
    }

    /// The program step `level` falls in and the lens step it exposes at (0xf97d88..0xf97f48).
    fn locate(&self, ports: &mut impl Lens, program: &Program, level: i16) -> (usize, u32) {
        let last = PROGRAM_STEPS - 1;
        if program[last].bottom <= level {
            return (last, program[last].low_step);
        }
        let index = (1..last).rev().find(|&index| program[index].bottom <= level).unwrap_or(0);
        let entry = &program[index];
        let lens_step = match index {
            0 | 13 => self.descending_step(ports, entry, level),
            16 => self.ascending_step(ports, entry, level),
            _ => entry.low_step,
        };
        (index, lens_step)
    }

    /// From the high step up to the low step, the first at which the step's top exceeds `level`.
    fn ascending_step(&self, ports: &mut impl Lens, entry: &ProgramStep, level: i16) -> u32 {
        let (low, high) = (entry.low_step as i16, entry.high_step as i16);
        if high >= low {
            return entry.high_step;
        }
        let mut step = high;
        loop {
            let top = entry.top.wrapping_sub(entry.low_step_level).wrapping_add(self.step_level(ports, i32::from(step) as u32));
            if level < top {
                return i32::from(step) as u32;
            }
            step = step.wrapping_add(1);
            if step >= low {
                return i32::from(step) as u32;
            }
        }
    }

    /// From the low step down to the high step, the first whose bottom `level` reaches.
    fn descending_step(&self, ports: &mut impl Lens, entry: &ProgramStep, level: i16) -> u32 {
        let (low, high) = (entry.low_step as i16, entry.high_step as i16);
        if low <= high {
            return entry.low_step;
        }
        let mut step = low;
        loop {
            let bottom = entry.bottom.wrapping_sub(entry.high_step_level).wrapping_add(self.step_level(ports, i32::from(step) as u32));
            if level >= bottom {
                return i32::from(step) as u32;
            }
            step = step.wrapping_sub(1);
            if step <= high {
                return i32::from(step) as u32;
            }
        }
    }

    /// Fills `program` for `kind` with lens steps between the run's limits around `step`
    /// (0xf974a8..0xf97d84).
    fn build(&self, ports: &mut impl ExposurePorts, program: &mut Program, kind: ProgramKind, step: u32) {
        let increases = stock::PROGRAM_INCREASES[kind.index()];
        let mut used = 0;
        program[PROGRAM_STEPS - 1].first_top = 0;
        program[PROGRAM_STEPS - 1].first_base = 0;
        for index in (0..PROGRAM_STEPS - 1).rev() {
            let base = program[index + 1].first_top;
            program[index].first_base = base;
            program[index].first_top = base;
            if increases[index] {
                program[index].first_top = base.wrapping_add(tenths(ports, stock::first_range(kind.index(), used)));
                used += 1;
            }
        }
        let mut used = 0;
        program[PROGRAM_STEPS - 1].second_top = 0;
        program[PROGRAM_STEPS - 1].second_base = 0;
        for index in (0..PROGRAM_STEPS - 1).rev() {
            let base = program[index + 1].second_top;
            program[index].second_base = base;
            program[index].second_top = base;
            if increases[index] {
                program[index].second_top = base.wrapping_add(tenths(ports, stock::second_range(kind.index(), used)));
                used += 1;
            }
            program[index].second_top = program[index].second_top.min(self.level_ceiling);
        }
        let StepLimits { lowest, highest, reach } = self.limits;
        let reach = reach.max(lowest).min(highest);
        let step = step.max(lowest).min(highest).max(reach);
        for (index, entry) in program.iter_mut().enumerate() {
            let (low, high) = match index {
                0 if ports.property(stock::REACH) == 1 => (reach, lowest),
                0..=12 => (reach, reach),
                13 => (step, reach),
                14 | 15 => (step, step),
                16 => (highest, step),
                _ => (highest, highest),
            };
            entry.low_step = low;
            entry.high_step = high;
            entry.low_step_level = self.step_level(ports, low);
            entry.high_step_level = self.step_level(ports, high);
        }
        for (index, entry) in program.iter_mut().enumerate() {
            let mode = ports.property(stock::program_mode(kind.index(), index)).min(3) as u8;
            entry.mode = mode;
            entry.profile = stock::MODE_PROFILES[usize::from(mode)];
            entry.offset = i16::from(ports.property(stock::program_offset(kind.index(), index)) as u8 as i8).wrapping_mul(10);
        }
        let refines = stock::PROGRAM_REFINES[kind.index()];
        for index in (0..PROGRAM_STEPS).rev() {
            let next = program.get(index + 1).copied().unwrap_or_default();
            let mut entry = program[index];
            if kind == ProgramKind::Plain {
                entry.gain = 0;
                let floor = if refines[index] { tenths(ports, stock::level_floor(index)) } else { next.floor };
                entry.floor = floored(ports, floor);
            } else {
                entry.gain = if refines[index] { stock::PROGRAM_GAINS[kind.index()][index] } else { next.gain };
                let floor = match index {
                    2..=15 => ports.gain_level(entry.profile, entry.gain),
                    _ => tenths(ports, stock::level_floor(index)),
                };
                entry.floor = floored(ports, floor);
                entry.gain = self.fastest_gain(ports, &entry, kind);
            }
            entry.ceiling = if index == PROGRAM_STEPS - 1 { tenths(ports, stock::LEVEL_CEILING) } else { next.floor };
            program[index] = entry;
        }
        for index in 0..PROGRAM_STEPS {
            let entry = program[index];
            let offset = ports.level_offset(entry.profile);
            program[index].bottom = if index == 0 {
                entry.high_step_level.wrapping_sub(entry.first_top.wrapping_add(entry.second_top)).wrapping_add(entry.floor.wrapping_sub(offset).wrapping_sub(entry.offset))
            } else {
                program[index - 1].top
            };
            program[index].top = entry
                .low_step_level
                .wrapping_sub(entry.second_base.wrapping_add(entry.first_base))
                .wrapping_add(entry.ceiling.wrapping_sub(offset).wrapping_sub(entry.offset));
        }
    }

    /// The highest gain whose exposure lasts at least as long as both the profile's exposure at
    /// the longest level and the step's floor allow; 4 when none does (0xf97a7c..0xf97cc0).
    fn fastest_gain(&self, ports: &mut impl ExposurePorts, entry: &ProgramStep, kind: ProgramKind) -> u32 {
        let floor_time = 100_000_000u32.checked_div(ports.linear(entry.floor)).unwrap_or(0);
        let longest = ports.exposure_time(entry.profile, stock::LONGEST_LEVEL, 0);
        let gains: &[u32] = if kind == ProgramKind::Even { &[12, 10, 8, 6] } else { &[13, 11, 9, 7, 5] };
        gains
            .iter()
            .copied()
            .find(|&gain| {
                let time = ports.exposure_time(entry.profile, 0, gain);
                longest <= time && floor_time <= time
            })
            .unwrap_or(4)
    }
}
