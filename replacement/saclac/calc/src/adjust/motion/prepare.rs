//! A measurement run's preparation (0xfa364c): its readings, search setup, frame exposures and
//! statistics, and the dump-frame section it clears and traces.

use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_profile_records::stock::record::group_01;
use xm1_saclac_profile_records::stock::shared::group_27;

use crate::adjust::metering::MeteringTables;
use crate::adjust::motion::drive::{quotient, DriveSpeed};
use crate::adjust::motion::exposure::{ExposureInputs, ExposurePorts, FrameExposure, Program, ProgramStep, PROGRAM_STEPS};
use crate::adjust::motion::plan::Plan;
use crate::adjust::motion::ports::{BoundsQuery, ChannelRates, Clock, LensBounds, MeasurementReadings, SearchBounds};
use crate::adjust::motion::preparation::{searches_in_full, CaptureFrames, LensStatus, PreviousFocus};
use crate::adjust::motion::queries;
use crate::adjust::motion::readings::{AreaList, PreparationInput, RecordReadings, SelectionLists, StepLimits};
use crate::adjust::motion::search::{ListReadings, SearchMode, SearchPorts, SearchSetup};
use crate::adjust::motion::state::{Drive, FrameExposures, MotionState, SampleSchedule};
use crate::adjust::motion::statistics::Statistics;
use crate::adjust::peak::Logarithms;
use crate::adjust::timing::CaptureTiming;
use crate::exposure::ports::{Lens, ProfileAccess, Properties};
use crate::exposure::settings;
use crate::stock::motion::{self as stock, AreaTrace, PreparationTrace};

/// Everything the preparation reaches.
pub trait PreparationPorts:
    ExposurePorts
    + SearchPorts
    + MeasurementReadings
    + DriveSpeed
    + LensStatus
    + CaptureFrames
    + ChannelRates
    + Logarithms
    + CaptureTiming
    + MeteringTables
    + Clock
{
}

impl<
        T: ExposurePorts
            + SearchPorts
            + MeasurementReadings
            + DriveSpeed
            + LensStatus
            + CaptureFrames
            + ChannelRates
            + Logarithms
            + CaptureTiming
            + MeteringTables
            + Clock,
    > PreparationPorts for T
{
}

/// What the preparation hands the pipeline besides the motion state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Preparation {
    pub searches_in_full: bool,
    pub frame_delay: u8,
}

impl Preparation {
    /// The output record's first byte.
    pub const fn flags(self) -> u8 { 2 | self.searches_in_full as u8 }

    /// The buffers at the output record's +0x14 to +0x28, in that order.
    pub const fn image_buffers(self) -> [SharedAddress; 6] {
        let [first, second, third, fourth, fifth, sixth] = stock::IMAGE_BUFFERS;
        [second, first, third, fourth, fifth, sixth]
    }
}

/// The record level a search mode's frames are exposed at (0xfa3f40..0xfa4010).
fn frame_level(properties: &mut impl Properties, readings: &RecordReadings, mode: SearchMode) -> i16 {
    let selection = properties.property(stock::FRAME_LEVEL_SELECTION);
    let [at_00a8, at_0094, at_0096, at_0092, at_009c] = readings.levels;
    let choice = match mode {
        SearchMode::Area => match (selection >> 4) & 3 {
            0 => 4,
            choice => choice,
        },
        SearchMode::Treated => 5,
        SearchMode::Channel => (selection >> 2) & 3,
        SearchMode::Plain => selection & 3,
    };
    let level = match choice {
        1 => at_0096,
        2 => at_00a8,
        3 => at_0092,
        4 => at_009c,
        5 => readings.area_positions[0],
        _ => at_0094,
    };
    level.wrapping_add(settings::level_step(properties, 2))
}

/// The first frame's change: the channel's drive period squared, scaled by the configured
/// factors, as a level above 2.5 octaves, capped at the configured limit (0xfa4048..0xfa4158).
/// A scaled period below one leaves stock reading a stale stack word (0xfa40f8); it gives no
/// change here.
fn first_change(ports: &mut (impl Properties + ChannelRates + Logarithms), readings: &RecordReadings) -> i16 {
    if readings.channel == 2 {
        return 0;
    }
    let rate = ports.channel_rate(readings.lens, readings.channel);
    let Some(period) = 1_000_000u32.checked_div(rate) else { return 0 };
    let squared = quotient(u64::from(period) * u64::from(period), 1000);
    if squared == 0 {
        return 0;
    }
    let [first, second] = stock::CHANGE_FACTORS.map(|id| u32::from(settings::word(ports, id)));
    let factor = first.wrapping_mul(second) / 100;
    let scaled = quotient(squared.wrapping_mul(u64::from(factor)), 1000);
    if scaled == 0 {
        return 0;
    }
    let Some(log) = ports.centi_log2(quotient(scaled, 100)) else { return 0 };
    let mut change = (log as u16).wrapping_sub(250);
    if readings.scene == stock::DELAYED_SCENE {
        change = change.wrapping_sub(ports.property(stock::CHANGE_SCENE_OFFSET) as u16);
    }
    let limit = (ports.property(stock::CHANGE_LIMIT) as i16).wrapping_mul(10);
    (change as i16).min(limit).max(0)
}

/// The second frame's change toward the stored second-channel exposure, in the configured
/// percent and capped at the configured limit (0xfa417c..0xfa427c).
fn second_change(ports: &mut (impl Properties + ProfileAccess + Lens), readings: &RecordReadings, level: i16) -> i16 {
    if !queries::second_channel_available(ports, readings.record) {
        return 0;
    }
    let record = readings.record;
    let profile = ports.profile();
    let step: u32 = profile.read_shared(record, &group_27::AT_0708);
    let base: i16 = profile.read_shared(record, &group_27::AT_070C);
    let first: i16 = profile.read_shared(record, &group_27::AT_070E);
    let second: i16 = profile.read_shared(record, &group_27::AT_0712);
    let third: i16 = profile.read_shared(record, &group_27::AT_0714);
    let level_index: u32 = profile.read_shared(record, &group_27::AT_071C);
    let step_level = ports.level(step, readings.lens);
    let index_level = settings::level_step(ports, u8::try_from(level_index).unwrap_or(u8::MAX));
    let chosen = level.wrapping_sub(settings::level_step(ports, 2));
    let change = step_level.wrapping_add(base).wrapping_sub(index_level).wrapping_sub(third).wrapping_sub(second).wrapping_sub(first).wrapping_sub(chosen);
    if change <= 0 {
        return 0;
    }
    let scaled = (i32::from(settings::level(ports, stock::SECOND_CHANGE_SCALE)) * i32::from(change) / 100) as i16;
    let limit = settings::word(ports, stock::SECOND_CHANGE_LIMIT);
    if i32::from(scaled) > i32::from(limit) { limit as i16 } else { scaled }
}

/// The index of `profile` among the program's capture-mode profiles, else 255 (0xf95d14).
fn profile_mode(profile: u32) -> u32 { stock::MODE_PROFILES.iter().position(|&mode| mode == profile).map_or(255, |index| index as u32) }

/// The second exposure as the section records it (0xfa42b4..0xfa43c4).
fn trace_exposure(ports: &mut impl PreparationPorts, section: SharedAddress, exposure: &FrameExposure, requested: i16, lens: u32) {
    let signed = |value: i16| i32::from(value) as u32;
    let step_level = ports.level(exposure.lens_step, lens);
    let period = ports.line_period(exposure.profile).unwrap_or(0);
    let words = [
        signed(exposure.at_06),
        exposure.lens_step,
        signed(step_level),
        signed(exposure.exposure_level),
        signed(exposure.first_level),
        profile_mode(exposure.profile),
        signed(exposure.change),
        signed(requested),
        signed(exposure.level),
        u32::from(exposure.step),
        signed(exposure.change_scale as i16),
        signed(exposure.gain_scale as i16),
        exposure.program.index() as u32,
        period,
        exposure.time,
    ];
    for (word, value) in (0..).zip(words) {
        ports.write_u32(stock::exposure_trace(section, word), value);
    }
}

/// Moves the first exposure to the second's lens step, raising its level by any fall in the
/// step's level up to the configured ceiling, and retimes it (0xfa43c8..0xfa446c).
fn corrected(ports: &mut impl PreparationPorts, mut first: FrameExposure, second: &FrameExposure, lens: u32) -> FrameExposure {
    let fall = ports.level(first.lens_step, lens).wrapping_sub(ports.level(second.lens_step, lens));
    first.lens_step = second.lens_step;
    first.exposure_level = first.exposure_level.wrapping_add(fall.max(0));
    let ceiling = settings::tenths(ports, stock::LEVEL_CEILING) as i16;
    if first.exposure_level > ceiling {
        first.exposure_level = ceiling;
    }
    first.time = ports.exposure_time(first.profile, first.exposure_level, first.gain);
    first
}

/// The run's search bounds: channel 1's high bound for a wide run (0xf9863c with 0xf98378).
pub(crate) fn search_bounds(ports: &mut (impl LensBounds + Properties), readings: &RecordReadings, wide: bool, step: u32) -> SearchBounds {
    let query = |channel, lens_state| BoundsQuery { lens: readings.lens, scene: readings.scene, channel, class: readings.class, step, reference: readings.reference, held: readings.auxiliary, lens_state };
    let mut bounds = ports.search_bounds(&query(readings.channel, readings.lens_state));
    if readings.scene == stock::MASKED_SCENE && matches!(readings.channel, 1 | 2) {
        bounds.low = ports.search_bounds(&query(readings.channel - 1, 0)).low;
    }
    if wide {
        bounds.high = ports.search_bounds(&query(1, 0)).high;
    }
    bounds
}

fn area_trace(readings: &RecordReadings, areas: &AreaList, step_two: i16) -> [AreaTrace; 10] {
    core::array::from_fn(|index| {
        let area = &areas.areas[index];
        AreaTrace {
            position: i32::from(readings.area_positions[index]) + i32::from(step_two),
            centre_x: area.centre_x,
            centre_y: area.centre_y,
            at_22: area.at_22,
            x: area.x,
            y: area.y,
            width: area.width,
            height: area.height,
            at_20: area.at_20,
            at_18: area.at_18,
            at_1c: area.at_1c,
        }
    })
}

impl MotionState {
    /// Prepares a run for the record and lens in `input`, tracing into the client's dump frame
    /// `frame`.
    pub fn prepare(ports: &mut impl PreparationPorts, lists: &mut impl SelectionLists, input: &PreparationInput, frame: SharedAddress) -> (Self, Preparation) {
        let prepared_at = ports.now();
        let readings = RecordReadings::read(ports, lists, input);
        let limits = StepLimits::configured(ports, &readings);
        let section = stock::section(frame);
        let results = stock::results(frame);
        let listed = lists.areas();
        let areas = listed.in_mode(ports, stock::SEARCH_PROFILE);
        let list_readings = ListReadings { subjects: lists.subjects(), single_subject: lists.single_subject() };
        let search = SearchSetup::decide(ports, &readings, list_readings, &areas, results);
        let level = frame_level(ports, &readings, search.mode);
        let inputs = ExposureInputs { readings: &readings, limits, level_ceiling: search.level_ceiling, results };
        let mut program: Program = [ProgramStep::default(); PROGRAM_STEPS];
        let first_change = first_change(ports, &readings);
        let first = inputs.frame(ports, &mut program, level, first_change, readings.level_step);
        let second_change = second_change(ports, &readings, level);
        let second = inputs.frame(ports, &mut program, level, second_change, readings.level_step);
        trace_exposure(ports, section, &second, second_change, readings.lens);
        let first = corrected(ports, first, &second, readings.lens);
        let (areas, treated_settings) = if search.mode == SearchMode::Treated {
            (listed.in_mode(ports, second.profile), stock::TREATED_PLAN_SETTINGS.map(|id| ports.property(id) as u8))
        } else {
            (AreaList::default(), [0; 4])
        };
        let previous = PreviousFocus::decide(ports, &readings, search.mode == SearchMode::Treated);
        let preparation = Preparation { searches_in_full: searches_in_full(ports, &readings, previous), frame_delay: search.frame_delay };
        let bounds = search_bounds(ports, &readings, search.channel_class == crate::adjust::motion::search::ChannelClass::Wide, second.lens_step);
        let statistics = Statistics::set_up(ports, &readings, &first, section);
        stock::clear_section(section, ports);
        let step_two = settings::level_step(ports, 2);
        let record_level: i16 = ports.profile().read(readings.record, &group_01::AT_0092);
        let trace = PreparationTrace {
            confirmation: [readings.confirmation[1] as i32, i32::from(readings.confirmation[2] as i16), readings.confirmation[7] as i32],
            record_level: i32::from(record_level) + i32::from(step_two),
            snapshot_position: search.snapshot_position,
            reference: readings.reference,
            drive_kind: readings.drive_kind.0,
            configured_level: readings.configured_level(ports),
            area_count: areas.count,
            areas: area_trace(&readings, &areas, step_two),
        };
        stock::trace_preparation(section, &trace, ports);
        let state = Self {
            section,
            results,
            prepared_at,
            readings,
            limits,
            search,
            areas,
            exposures: FrameExposures { first, second },
            program,
            previous,
            bounds,
            statistics,
            treated_settings,
            plan: Plan::default(),
            sampling: SampleSchedule::default(),
            drive: Drive::default(),
        };
        (state, preparation)
    }
}
