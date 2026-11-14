//! What a run's preparation reads once from the bound record, the lens and the selection lists
//! (0xfa3768..0xfa3bb8), and the lens steps it may drive between (0xfa3bbc..0xfa3ed8).

use xm1_saclac_interface::RecordHandle;
use xm1_saclac_profile_records::stock::record::{group_00, group_01, group_04, group_13};
use xm1_saclac_profile_records::stock::shared::{group_15, group_18, group_23};
use xm1_saclac_profile_records::{MeasurementSelection, SharedHeaderByte};

use crate::adjust::motion::drive::{DriveKind, DriveMode, DriveSpeed};
use crate::adjust::motion::ports::{ExposureTimes, GridArea, LensCalibration, LensTables, LimitQuery, MeasurementReadings};
use crate::adjust::motion::queries;
use crate::exposure::levels;
use crate::exposure::ports::{Lens, ProfileAccess, Properties};
use crate::exposure::settings;
use crate::stock::motion::{self as stock};

/// The selection lists' readings the preparation takes, in the selection's own capture mode.
pub trait SelectionLists {
    fn subjects(&mut self) -> u8;
    fn single_subject(&mut self) -> u8;
    /// The second area list.
    fn areas(&mut self) -> AreaList;
    /// Ten area positions before the reference offset is taken off.
    fn area_positions(&mut self) -> [i16; AREA_POSITIONS];
    /// The lens reference the run's tables are read at.
    fn reference(&mut self) -> u32;
    /// The words at 0x1f08 and 0x1f04.
    fn area_words(&mut self) -> [u32; 2];
}

pub const AREA_POSITIONS: usize = 10;
pub const LISTED_AREAS: usize = 10;

/// One listed area; the coordinate words scale with the capture mode's frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ListedArea {
    pub x: u32,
    pub y: u32,
    pub height: u32,
    pub width: u32,
    pub centre_x: u32,
    pub centre_y: u32,
    pub at_18: u32,
    pub at_1c: u32,
    pub at_20: u16,
    pub at_22: u16,
    /// The bytes from 0x24 to the end of the 0x68-byte entry.
    pub rest: [u8; 0x44],
}

impl Default for ListedArea {
    fn default() -> Self { Self { x: 0, y: 0, height: 0, width: 0, centre_x: 0, centre_y: 0, at_18: 0, at_1c: 0, at_20: 0, at_22: 0, rest: [0; 0x44] } }
}

impl ListedArea {
    pub const fn at_26(&self) -> u8 { self.rest[2] }

    pub const fn at_2c(&self) -> u32 { u32::from_le_bytes([self.rest[8], self.rest[9], self.rest[10], self.rest[11]]) }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AreaList {
    /// At most [`LISTED_AREAS`] are read.
    pub count: u32,
    pub areas: [ListedArea; LISTED_AREAS],
}

impl Default for AreaList {
    fn default() -> Self { Self { count: 0, areas: [ListedArea::default(); LISTED_AREAS] } }
}

impl AreaList {
    pub fn listed(&self) -> &[ListedArea] { &self.areas[..(self.count as usize).min(LISTED_AREAS)] }
}

/// The 36-byte request the measurement hands the preparation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FocusRequest {
    pub at_00: u32,
    pub at_04: u32,
    /// A lens position on side `side`.
    pub position: i16,
    pub at_0a: i16,
    pub at_0c: u32,
    pub at_10: u32,
    pub side: u32,
    /// Four lens positions before the reference offset is taken off.
    pub positions: [i16; 4],
    pub at_20: u32,
}

/// The record's readings, with the lens reference offset taken off its positions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordReadings {
    pub record: RecordHandle,
    pub measurement_selection: MeasurementSelection,
    pub scene: u8,
    /// Header byte 0x46: 0, 1 or 2.
    pub channel: u8,
    pub style: u8,
    pub at_0043: u8,
    /// Header byte 0x47.
    pub class: u8,
    pub policy: u8,
    pub lens: u32,
    pub limit_word: u32,
    /// The step the run's levels are read at.
    pub level_step: u32,
    pub at_0035: u8,
    pub at_0038: u8,
    pub at_20cc: u32,
    pub at_20c8: u32,
    pub at_20d0: u32,
    pub locked: u32,
    pub row_scale: [u16; 2],
    pub selection_value: u32,
    pub at_0164: u32,
    pub at_0168: u32,
    pub request: FocusRequest,
    /// The focus confirmation's eight words, its fifth as a calibration index.
    pub confirmation: [u32; 8],
    pub area: GridArea,
    pub at_0028: u16,
    /// Record levels 0xa8, 0x94, 0x96, 0x92 and 0x9c.
    pub levels: [i16; 5],
    pub at_00a4: i16,
    pub at_00b8: i16,
    pub measured_level: i16,
    pub load_test: bool,
    pub at_005f: u8,
    pub at_005b: u8,
    pub compares_subjects: u8,
    pub area_positions: [i16; AREA_POSITIONS],
    /// Header byte 0x63.
    pub armed: u8,
    /// Header byte 0x61.
    pub auxiliary: u8,
    pub area_words: [u32; 2],
    pub drive_kind: DriveKind,
    pub reference: u32,
    pub at_004e: u8,
    pub at_003c: u8,
    pub priority: u8,
    pub at_0039: u8,
    pub at_003a: u8,
    pub at_003b: u8,
    pub designation: [u32; 7],
    pub at_0194: (u32, u8),
    pub record_3e: u8,
    pub lens_state: u32,
}

/// What the measurement hands the preparation besides the selection lists.
#[derive(Clone, Copy, Debug)]
pub struct PreparationInput {
    pub record: RecordHandle,
    /// The lens reference.
    pub lens: u32,
    /// The load-test setting reads 99 (0x742064).
    pub load_test: bool,
    pub request: FocusRequest,
}

fn words<const N: usize, const B: usize>(bytes: [u8; B]) -> [u32; N] {
    core::array::from_fn(|word| u32::from_le_bytes([bytes[4 * word], bytes[4 * word + 1], bytes[4 * word + 2], bytes[4 * word + 3]]))
}

/// Class 0 rows scaled 1 by 1 or 9 by 16 are scaled 2 by 3 (0xfa60f4).
fn adjusted_row_scale(class: u8, [first, second]: [u16; 2]) -> [u16; 2] {
    match (class, first, second) {
        (0, 1, 1) | (0, 9, 16) => [2, 3],
        _ => [first, second],
    }
}

impl RecordReadings {
    pub fn read(
        ports: &mut (impl ProfileAccess + Properties + Lens + LensTables + LensCalibration + MeasurementReadings),
        lists: &mut impl SelectionLists,
        input: &PreparationInput,
    ) -> Self {
        let record = input.record;
        let lens = input.lens;
        let mut profile = ports.profile();
        let limits: [u8; 64] = profile.read_shared(record, &group_18::AT_00E0);
        let reference_key: u32 = profile.read(record, &group_01::AT_00BC);
        let measurement_selection = profile.measurement_selection(record);
        let scene = profile.header(record, SharedHeaderByte::At31);
        let channel = profile.header(record, SharedHeaderByte::At46);
        let style = profile.read(record, &group_00::AT_003F);
        let at_0043 = profile.read(record, &group_00::AT_0043);
        let class = profile.header(record, SharedHeaderByte::At47);
        let policy = profile.read(record, &group_00::AT_0045);
        let armed = profile.header(record, SharedHeaderByte::At63);
        let auxiliary = profile.header(record, SharedHeaderByte::At61);
        let at_0035 = profile.read(record, &group_00::AT_0035);
        let at_0038 = profile.read(record, &group_00::AT_0038);
        let at_20cc = profile.read(record, &group_13::AT_20CC);
        let locked = profile.read(record, &group_13::AT_20B0_4);
        let at_20c8 = profile.read(record, &group_13::AT_20C8);
        let at_20d0 = profile.read(record, &group_13::AT_20D0);
        let row_scale: u32 = profile.read(record, &group_00::AT_0000);
        let at_0164 = profile.read(record, &group_04::AT_0164);
        let at_0168 = profile.read(record, &group_04::AT_0168);
        let confirmation: [u8; 32] = profile.read_shared(record, &group_15::FOCUS_CONFIRMATION);
        let designation = words(profile.lens_designation(record));
        let at_0028 = profile.read(record, &group_00::AT_0028);
        let record_levels = [&group_01::AT_00A8, &group_01::AT_0094, &group_01::AT_0096, &group_01::AT_0092, &group_01::AT_009C].map(|field| profile.read(record, field));
        let at_00a4: i16 = profile.read(record, &group_01::AT_00A4);
        let at_001c: i16 = profile.read(record, &group_00::AT_001C);
        let at_00b8 = profile.read(record, &group_01::AT_00B8).wrapping_add(profile.read(record, &group_01::AT_00BA));
        let at_005f = profile.read(record, &group_00::AT_005F);
        let at_005b = profile.read(record, &group_00::AT_005B);
        let compares_subjects = profile.read(record, &group_00::AT_005D);
        let at_004e = profile.read(record, &group_00::AT_004E);
        let at_003c = profile.read(record, &group_00::AT_003C);
        let priority = profile.read(record, &group_00::AT_003D);
        let at_0039 = profile.read(record, &group_00::AT_0039);
        let at_003a = profile.read(record, &group_00::AT_003A);
        let at_003b = profile.read(record, &group_00::AT_003B);
        let record_3e = profile.read(record, &group_00::AT_003E);
        let at_0194: [u8; 16] = profile.read_shared(record, &group_23::AT_0194);
        let offset = ports.reference_offset(reference_key);
        let less_offset = |position: i16| position.wrapping_sub(offset);
        let lowest = ports.lowest_step(lens);
        let limit_word = u32::from_le_bytes([limits[0x38], limits[0x39], limits[0x3a], limits[0x3b]]);
        let record_step = u32::from_le_bytes([limits[0x3c], limits[0x3d], limits[0x3e], limits[0x3f]]);
        let mut request = input.request;
        request.positions = request.positions.map(less_offset);
        request.position = ports.side_adjusted(request.position, request.side, 1);
        request.side = 1;
        let mut confirmation: [u32; 8] = words(confirmation);
        confirmation[4] = ports.calibration_index(confirmation[4]);
        if armed == 1 && class == 5 {
            confirmation[0] = 0;
        } else if confirmation[0] == 1 {
            confirmation[5] = 1;
        }
        let at_00a4 = less_offset(at_00a4).wrapping_sub(at_001c).wrapping_add(settings::level_step(ports, 2));
        RecordReadings {
            record,
            measurement_selection,
            scene,
            channel,
            style,
            at_0043,
            class,
            policy,
            lens,
            limit_word: if armed == 0 { limit_word } else { 0 },
            level_step: if armed == 0 && class != 0 { record_step } else { lowest },
            at_0035,
            at_0038,
            at_20cc,
            at_20c8,
            at_20d0,
            locked,
            row_scale: adjusted_row_scale(class, [row_scale as u16, (row_scale >> 16) as u16]),
            selection_value: ports.selection_value(record, lens),
            at_0164,
            at_0168,
            request,
            confirmation,
            area: ports.selected_area(record),
            at_0028,
            levels: record_levels.map(less_offset),
            at_00a4,
            at_00b8,
            measured_level: levels::measured_level(ports, record),
            load_test: input.load_test,
            at_005f,
            at_005b,
            compares_subjects,
            area_positions: lists.area_positions().map(less_offset),
            armed,
            auxiliary,
            area_words: lists.area_words(),
            drive_kind: ports.drive_kind(),
            reference: lists.reference(),
            at_004e,
            at_003c,
            priority,
            at_0039,
            at_003a,
            at_003b,
            designation,
            at_0194: (words::<4, 16>(at_0194)[1], at_0194[0xc]),
            record_3e,
            lens_state: ports.lens_state(),
        }
    }

    /// The record level the configured selection picks for `channel`'s search, plus level step 3
    /// (0xf970a4).
    pub fn configured_level(&self, properties: &mut impl Properties) -> i16 {
        let selection = properties.property(stock::LEVEL_SELECTION);
        let selection = if self.channel == 0 { selection & 0xf } else { (selection & 0xfff) >> 4 };
        let [at_00a8, at_0094, at_0096, at_0092, _] = self.levels;
        let level = match selection {
            2 => at_00a8,
            3 => at_0092,
            1 => at_0096,
            _ => at_0094,
        };
        settings::level_step(properties, 3).wrapping_add(level)
    }
}

/// The lens steps a run drives between, and the furthest it reaches out.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StepLimits {
    pub lowest: u32,
    pub highest: u32,
    pub reach: u32,
}

impl StepLimits {
    /// The configured limits for `readings`' lens (0xfa3bbc..0xfa3ed8).
    pub fn configured(ports: &mut (impl Properties + LensCalibration + LensTables + DriveSpeed + ExposureTimes), readings: &RecordReadings) -> Self {
        let lens = readings.lens;
        let last = ports.steps().wrapping_sub(1);
        let enabled = ports.property(stock::STEP_LIMITS);
        let mut lowest = 0;
        let mut highest = last;
        if enabled & 1 != 0 {
            lowest = ports.lowest_step(lens);
            highest = highest.min(ports.highest_step(lens));
        }
        if enabled & 2 != 0 {
            highest = highest.min(Self::fastest_step(ports, readings));
        }
        if enabled & 4 != 0 {
            let entry = ports.table_entry(lens, stock::STEP_LIMIT_TABLE, 5);
            let floor = ports.property(stock::step_limit_floor(entry));
            let ceiling = ports.property(stock::step_limit_ceiling(entry));
            let ceiling = if ceiling > ports.steps().wrapping_sub(1) { ports.steps().wrapping_sub(1) & 0xff } else { ceiling };
            lowest = lowest.max(floor);
            highest = highest.min(ceiling);
        }
        let capped = ports.property(stock::STEP_CAP);
        if capped & 0xf != 0 && capped & 0x10 != 0 {
            let cap = match ports.property(stock::STEP_CAP_VALUE) {
                0 => ports.steps().wrapping_sub(1),
                cap => cap,
            };
            highest = highest.min(cap);
        }
        let reach = if ports.property(stock::REACH) == 0 { 0 } else { Self::reach(ports, readings) };
        Self { lowest, highest, reach }
    }

    /// The highest step whose configured depth the drive covers within one exposure of the
    /// fixed profile.
    fn fastest_step(ports: &mut (impl Properties + LensCalibration + DriveSpeed + ExposureTimes), readings: &RecordReadings) -> u32 {
        let period = ports.exposure_time(stock::SEARCH_PROFILE, 1000, 0);
        let speed = match ports.top_speed(readings.drive_kind, DriveMode::MEASUREMENT) {
            0 => 1,
            speed => speed,
        };
        let mut step = ports.steps().wrapping_sub(1);
        while step != 0 {
            let depth = settings::tenths(ports, stock::FASTEST_DEPTH) & 0xfffe;
            let positions = i32::from(ports.depth_positions(readings.lens, step, depth));
            if (positions.wrapping_mul(1_000_000) as u32) / speed <= period {
                break;
            }
            step -= 1;
        }
        step
    }

    /// The first step whose high lens limit lies within the configured depth of the lens's
    /// channel position, capped where the configured share of the level step's depth fits.
    fn reach(ports: &mut (impl Properties + LensCalibration + LensTables), readings: &RecordReadings) -> u32 {
        let lens = readings.lens;
        let mut step = 0;
        while step < ports.steps().wrapping_sub(1) {
            let depth = settings::tenths(ports, stock::REACH_DEPTH) & 0xfffe;
            let limit = ports.depth_positions(lens, step, depth);
            let query = LimitQuery { channel: readings.channel, lens, step, reference: readings.reference, with_margins: false, lens_state: readings.lens_state };
            let position = ports.lens_limits(&query).high;
            let offset = queries::step_offset(ports, lens, step, position, 1, readings.reference, 0);
            if i32::from(offset).abs() <= i32::from(limit) {
                break;
            }
            step += 1;
        }
        let share = i32::from(ports.depth_positions(lens, readings.level_step, 100)).wrapping_mul(ports.property(stock::REACH_SHARE) as i32);
        let share = u32::from((share / 10) as u16);
        let mut ceiling = ports.steps().wrapping_sub(1);
        while ceiling != 0 && share < i32::from(ports.depth_positions(lens, ceiling, 100)) as u32 {
            ceiling -= 1;
        }
        step.min(ceiling)
    }
}
