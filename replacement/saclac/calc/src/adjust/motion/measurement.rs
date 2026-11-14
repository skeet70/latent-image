//! Measurement policy and typed settings for the image pipeline.

#![forbid(unsafe_code)]

use crate::adjust::lens::LensConfiguration;
use crate::adjust::motion::drive::quotient;
use crate::adjust::motion::layout::window_start_case;
use crate::adjust::motion::ports::{ExposureTimes, LensCalibration, LensTables, LensWords};
use crate::adjust::motion::search::{signed_byte, AreaCell, SearchMode};
use crate::adjust::motion::state::MotionState;
use crate::adjust::timing::LevelScale;
use crate::exposure::ports::{Properties, Property};
use crate::exposure::settings;
use crate::stock::measurement::{self as stock, LimitCheck};

/// Everything the measurement settings reach.
pub trait MeasurementPorts: Properties + LensCalibration + LensTables + LensConfiguration + LensWords + ExposureTimes + LevelScale {}

impl<T: Properties + LensCalibration + LensTables + LensConfiguration + LensWords + ExposureTimes + LevelScale> MeasurementPorts for T {}

pub const MAP_ROWS: usize = 7;
pub const MAP_COLUMNS: usize = 7;

/// Each cell's relation to the measured cell, one bit per relation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AreaMap(pub [[u8; MAP_COLUMNS]; MAP_ROWS]);

impl AreaMap {
    pub const CELL: u8 = 1;
    pub const MEASURED: u8 = 4;
    pub const COLUMN_NEIGHBOUR: u8 = 8;
    pub const ROW_NEIGHBOUR: u8 = 0x10;
    pub const NEIGHBOURHOOD: u8 = 0xa0;
    pub const WIDE_NEIGHBOURHOOD: u8 = 0x40;
    /// A grid of one cell maps its first cell alone.
    pub const SINGLE: u8 = 0x85;

    fn around(area: AreaCell) -> Self {
        let near = |index: u8, at: usize, within: i32| (i32::from(index as i8) - at as i32).abs() <= within;
        Self(core::array::from_fn(|row| {
            core::array::from_fn(|column| {
                let same_row = i32::from(area.row as i8) == row as i32;
                let same_column = i32::from(area.column as i8) == column as i32;
                let relations = [
                    (same_row && same_column, Self::MEASURED),
                    (same_column && near(area.row, row, 1), Self::COLUMN_NEIGHBOUR),
                    (same_row && near(area.column, column, 1), Self::ROW_NEIGHBOUR),
                    (near(area.row, row, 1) && near(area.column, column, 1), Self::NEIGHBOURHOOD),
                    (near(area.row, row, 2) && near(area.column, column, 2), Self::WIDE_NEIGHBOURHOOD),
                ];
                relations.iter().filter(|(holds, _)| *holds).fold(Self::CELL, |bits, (_, bit)| bits | bit)
            })
        }))
    }

    fn single() -> Self {
        let mut map = [[0; MAP_COLUMNS]; MAP_ROWS];
        map[0][0] = Self::SINGLE;
        Self(map)
    }
}

/// The margin is signed and the resulting floor wraps to a level halfword.
fn limit_applies(ports: &mut impl Properties, state: &MotionState, check: LimitCheck) -> bool {
    if ports.property(check.enable) & u32::from(check.mask) == 0 {
        return false;
    }
    let Some(margin) = check.margin else { return true };
    let floor = state.layout.lowest_level.wrapping_sub(signed_byte(ports, margin).wrapping_mul(10));
    state.exposures.second.level <= floor
}

/// A selected limit uses upper word 1; an unselected limit uses 3.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Limit {
    pub selected: bool,
}

impl Limit {
    fn of(ports: &mut impl Properties, state: &MotionState, check: LimitCheck) -> Self { Self { selected: limit_applies(ports, state, check) } }

    pub const fn words(self) -> [u32; 2] { if self.selected { [0, 1] } else { [0, 3] } }
}

/// The search's own limit: a channel search's flag, or the limit of its mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SearchLimit {
    /// Every search plain, or the load test.
    Off,
    Channel(bool),
    Mode(Limit),
}

impl SearchLimit {
    pub const fn words(self) -> [u32; 2] {
        match self {
            Self::Off => [0, 3],
            Self::Channel(limited) => [limited as u32, 3],
            Self::Mode(limit) => limit.words(),
        }
    }

    pub const fn selected(self) -> bool {
        match self {
            Self::Off => false,
            Self::Channel(limited) => limited,
            Self::Mode(limit) => limit.selected,
        }
    }
}

/// Whether the run's level lies above the program's lowest level less each configured margin
/// (0xf9700c, 0xf97058).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LevelChecks {
    pub first: bool,
    pub second: bool,
}

fn above_margin(ports: &mut impl Properties, state: &MotionState, margin: Property) -> bool {
    state.layout.lowest_level.wrapping_sub(signed_byte(ports, margin).wrapping_mul(10)) < state.exposures.second.level
}

fn level_checks(ports: &mut impl Properties, state: &MotionState) -> LevelChecks {
    let first = ports.property(stock::LEVEL_CHECK_BITS) & 2 == 0 || above_margin(ports, state, stock::LEVEL_CHECK_MARGIN);
    let second = ports.property(stock::SECOND_LEVEL_CHECK) != 0 && above_margin(ports, state, stock::SECOND_LEVEL_MARGIN);
    LevelChecks { first, second }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FirstCheck {
    /// 3, 2 or 4 by the configured kind 1, 2 or any other.
    pub kind: u32,
    /// Grid layout uses the configured alternate third level.
    pub alternate_level: bool,
    pub limit: Limit,
    pub above_margin: bool,
}

/// The fourteen channels the configuration names, and whether the set is kept for a long
/// exposure.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ChannelSet {
    pub channels: [bool; 14],
    pub kept: bool,
}

/// The first byte's bits 1 to 8 below the second byte's bits 1 to 7.
fn channel_bits(ports: &mut impl Properties) -> u32 {
    let high = ports.property(stock::CHANNEL_BITS[1]);
    let low = ports.property(stock::CHANNEL_BITS[0]);
    (low >> 1 & 0xff) | (high & 0xfe) << 6
}

fn named_channels(bits: u32) -> [bool; 14] { core::array::from_fn(|index| bits >> index & 1 != 0) }

/// The configured channels; past the seventh, none while the second exposure is shorter than the
/// configured cutoff's.
fn channel_set(ports: &mut impl MeasurementPorts, state: &MotionState) -> ChannelSet {
    let second = &state.exposures.second;
    let short_gain = ports.property(stock::CHANNEL_TIMES[0]);
    let short = ports.exposure_time(second.profile, 0, short_gain);
    let long_gain = ports.property(stock::CHANNEL_TIMES[1]);
    let long = ports.exposure_time(second.profile, 0, long_gain);
    let named = named_channels(channel_bits(ports));
    let mut channels = [false; 14];
    for (index, channel) in channels.iter_mut().enumerate() {
        if ports.property(stock::CHANNEL_CUTOFF) != 0 && index > 6 && second.time < short {
            break;
        }
        *channel = named[index];
    }
    let kept = ports.property(stock::CHANNEL_KEPT) == 0 || long < second.time;
    ChannelSet { channels, kept }
}

/// The configured gains at four levels, interpolated at the first level plus the frames' level.
fn row_gain(ports: &mut impl MeasurementPorts, state: &MotionState) -> i32 {
    let second = &state.exposures.second;
    let frames = ports.log2_difference(u32::from(second.frames).wrapping_mul(25), 100);
    let level = i32::from(second.first_level.wrapping_add(frames));
    let levels = stock::ROW_GAIN_LEVELS.map(|id| ports.property(id) as i32 * 10);
    let gains = stock::ROW_GAINS.map(|id| ports.property(id) as i32 * 10);
    for high in (1..4).rev() {
        if level >= levels[high] { return gains[high]; }
        if level >= levels[high - 1] {
            let width = levels[high] - levels[high - 1];
            return (level - levels[high - 1]).wrapping_mul(gains[high] - gains[high - 1])
                .checked_div(width).unwrap_or(0).wrapping_add(gains[high - 1]);
        }
    }
    gains[0]
}

/// One statistics row's weight at the run's gain, and its two configured level offsets.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RowWeight {
    pub weight: u32,
    pub offset: i32,
    pub negative_offset: i32,
}

fn row_weights(ports: &mut impl MeasurementPorts, state: &MotionState) -> [[RowWeight; 4]; 3] {
    let gain = row_gain(ports, state) as u32;
    let mut weights = [[RowWeight::default(); 4]; 3];
    for (index, weight) in weights.iter_mut().flatten().enumerate() {
        let configured = u32::from(settings::word(ports, stock::ROW_WEIGHTS[index]));
        *weight = RowWeight {
            weight: configured.wrapping_mul(gain) / 100,
            offset: i32::from(ports.property(stock::ROW_OFFSETS[index]) as u8),
            negative_offset: -i32::from(ports.property(stock::ROW_NEGATIVE_OFFSETS[index]) as u8),
        };
    }
    weights
}

/// 1 at or below the lowest level less the first margin, 3 at or above it less the second, else
/// the share between them in fifths, rounded and offset by one.
fn level_band(ports: &mut impl Properties, state: &MotionState) -> u8 {
    let lowest = state.layout.lowest_level;
    let [first, second] = stock::LEVEL_BAND.map(|id| lowest.wrapping_sub(signed_byte(ports, id).wrapping_mul(10)));
    let span = second.wrapping_sub(first);
    let level = state.exposures.second.level;
    if span == 0 || level <= first {
        return 1;
    }
    if level >= second {
        return 3;
    }
    let share = (i32::from(level) - i32::from(first)).wrapping_mul(20).wrapping_div(i32::from(span));
    ((i32::from(share.wrapping_add(10) as u8) + 5) / 10) as u8
}

/// Listed layouts stop after the first step; only alternate grids admit the third.
fn step_order(ports: &mut impl Properties, kind: u32, id: Property) -> [u32; 3] {
    let bits = ports.property(id);
    let mut order = [4; 3];
    let mut next = 0;
    for (bit, step, needs) in [(1, 0, 0), (2, 1, 1), (4, 2, 3)] {
        if (needs == 1 && kind == 1) || (needs == 3 && kind != 3) {
            break;
        }
        if bits & bit != 0 {
            order[next] = step;
            next += 1;
        }
    }
    order
}

/// A channel search's drive figures.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ChannelDrive {
    pub depth: i16,
    pub positions: [i16; 2],
    pub curve: u16,
    pub value: u16,
    /// Frames per step at the lens's rate, at most 255.
    pub frames: u32,
    pub step_frames: u32,
    pub slope: u8,
    pub travel: u16,
}

fn channel_drive(ports: &mut impl MeasurementPorts, state: &MotionState) -> ChannelDrive {
    let readings = &state.readings;
    let step = state.exposures.second.lens_step;
    let side = u32::from(state.plan.profile.reverse);
    let configured_depth = settings::word(ports, stock::CHANNEL_DEPTH);
    let depth = ports.depth_positions(readings.lens, step, configured_depth);
    let [first, second] = state.layout.channel_positions;
    let positions = [ports.side_adjusted(first, 1, side), ports.side_adjusted(second, 1, side)];
    let curve = ports.channel_curve(readings.lens as i32) as u16;
    let value = ports.channel_value(readings.lens as i32) as u16;
    let rate = ports.position_rate(second, readings.lens, step, readings.reference);
    let curve_word = ports.curve_word(readings.lens);
    let base = u64::from(settings::word(ports, stock::CHANNEL_DRIVE_BASE));
    let factor = u64::from(ports.property(stock::CHANNEL_DRIVE_FACTOR));
    let limit = ports.property(stock::CHANNEL_DRIVE_LIMIT);
    let frames = 100_000u32.checked_div(rate).map_or(0xff, |frames| frames.min(0xff));
    let denominator = (u64::from(frames) * 3).wrapping_mul(u64::from(curve_word)).wrapping_mul(factor).wrapping_add(base * 100_000);
    let step_frames = if denominator == 0 { 0xff } else { (quotient((u64::from(frames) * 100_000).wrapping_mul(base), denominator) as u32).min(0xff) };
    let limit_frames = 10_000u32.checked_div(limit).unwrap_or(0);
    let slope = if frames == limit_frames {
        0x80
    } else {
        let per_frame = 0x80u32.checked_div(limit_frames.wrapping_sub(frames)).unwrap_or(0);
        (limit_frames.wrapping_sub(0xff).wrapping_mul(per_frame) as u8).min(0x80)
    };
    let lens_value = ports.channel_value(readings.lens as i32);
    let mapped = ports.mapped_step_word(step, readings.lens);
    let scale = u64::from(settings::word(ports, stock::CHANNEL_TRAVEL_SCALE));
    let percent = u64::from(ports.property(stock::CHANNEL_TRAVEL_PERCENT) + 100);
    let travel = quotient((scale * u64::from(mapped as u16) * 1000).wrapping_mul(percent), u64::from(lens_value.wrapping_mul(lens_value).wrapping_mul(100)));
    let travel = if travel % 10 != 0 { travel.wrapping_add(10) } else { travel };
    let travel = (travel / 10) as u16;
    ChannelDrive { depth, positions, curve, value, frames, step_frames, slope, travel }
}

/// A treated search's window case and its window byte.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TreatedSearch {
    /// 0 when the run's case is off in the third plan header byte, else 2 when it is on in the
    /// fourth, else 1.
    pub window: u32,
    pub setting: u8,
}

/// Plain-search limits and travel over one frame.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PlainSearch {
    pub first: bool,
    pub second: bool,
    pub travel: u16,
}

fn plain_search(ports: &mut impl MeasurementPorts, state: &MotionState) -> PlainSearch {
    let checks = stock::LIMIT_CHECKS;
    let first = limit_applies(ports, state, checks[5]);
    let second = limit_applies(ports, state, checks[6]);
    let [rate, floor, ceiling] = stock::PLAIN_TRAVEL.map(|id| ports.property(id));
    let scaled = rate.wrapping_mul(150_000).checked_div(u32::from(state.plan.frame_depth)).unwrap_or(0);
    let scaled = if scaled % 10 > 4 { scaled + 10 } else { scaled };
    let cap = (scaled / 10).max(floor.wrapping_mul(100) & 0xffff).min(ceiling.wrapping_mul(100) & 0xffff);
    PlainSearch { first, second, travel: (u32::from(state.plan.frame_travel).wrapping_mul(cap) / 1000) as u16 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlainSettings {
    pub level_reached: bool,
    /// 6 leaves a step out.
    pub steps: [u32; 5],
    pub plain: Option<PlainSearch>,
    pub alternate: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KindSettings {
    Plain(PlainSettings),
    Channel { drive: ChannelDrive, plain: PlainSettings },
    Treated(TreatedSearch),
}

/// A masked-scene run's configured depths at its level step, its first table position and the
/// tracked area's cell.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MaskedSearch {
    pub depths: [i16; 4],
    pub position: i16,
    pub cell: u16,
}

/// The lens bounds and low limit on the plan's side, and the configured depth the drive covers.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DriveBounds {
    pub low: i16,
    pub high: i16,
    pub low_limit: i16,
    pub drive_kind: u32,
    pub depth: Option<i16>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MeasurementSettings {
    pub map: AreaMap,
    /// The measured cell of a primary grid of more than one cell.
    pub measured_cell: Option<u8>,
    pub secondary_areas: u8,
    pub level_band: u8,
    pub search_limit: SearchLimit,
    pub depth: i16,
    pub first_check: Option<FirstCheck>,
    pub level_checks: Option<LevelChecks>,
    pub channels: Option<ChannelSet>,
    pub kind_flag: u8,
    pub selection_limit: bool,
    pub step_orders: [[u32; 3]; 2],
    pub period_long: bool,
    pub row_weights: [[RowWeight; 4]; 3],
    pub kind_code: u32,
    pub last_depth: i16,
    pub level_window: bool,
    pub level_limit: bool,
    pub kind_limits: [Limit; 2],
    pub load_test_plain: bool,
    pub second_level_checks: Option<LevelChecks>,
    pub second_channels: Option<[bool; 14]>,
    pub kind: KindSettings,
    pub masked: Option<MaskedSearch>,
    pub ranged: Option<[u32; 2]>,
    pub bounds: Option<DriveBounds>,
}

/// The tracked area's centre as a cell of the primary grid, rows and columns at most 6, or the
/// centre cell 24 for an untreated run (0xf9c3e0..0xf9c48c).
fn tracked_cell(state: &MotionState) -> u16 {
    let Some(tracking) = state.layout.tracking else { return 24 };
    let grid = &state.layout.grids[0];
    let window = &state.layout.crop.window;
    let along = |gap: u16, origin: u32, window_origin: u32, centre: u32, size: u32| {
        let offset = (u32::from(gap) >> 1).wrapping_sub(origin.wrapping_sub(window_origin)).wrapping_add(centre);
        (offset.checked_div(u32::from(gap).wrapping_add(size)).unwrap_or(0) as i8).min(6)
    };
    let column = along(grid.gap_horizontal, grid.left, window.left, tracking.area.centre_x, grid.cell_width);
    let row = along(grid.gap_vertical, grid.top, window.top, tracking.area.centre_y, grid.cell_height).max(0);
    (i32::from(row) * 7 + i32::from(column.max(0))) as u16
}

impl MotionState {
    /// The measurement settings of the prepared and laid-out run.
    pub fn measurement_settings(&self, ports: &mut impl MeasurementPorts) -> MeasurementSettings {
        let readings = &self.readings;
        let mode = self.search.mode;
        let level = self.exposures.second.level;
        let lens_step = self.exposures.second.lens_step;
        let checks = stock::LIMIT_CHECKS;
        let primary = &self.layout.grids[0];
        let (map, measured_cell) =
            if primary.columns == 1 && primary.rows == 1 { (AreaMap::single(), None) } else { (AreaMap::around(self.search.area), (self.search.area.row < 7 && self.search.area.column < 7).then(|| self.search.area.index())) };
        let secondary = &self.layout.grids[1];
        let secondary_areas = if self.plan.layout.code() == 0 { 0 } else { secondary.columns.wrapping_mul(secondary.rows) };
        let plain_configured = ports.property(stock::PLAIN_SEARCH) & 1 != 0;
        let search_limit = match mode {
            _ if plain_configured || readings.load_test => SearchLimit::Off,
            SearchMode::Channel => SearchLimit::Channel(limit_applies(ports, self, checks[8])),
            SearchMode::Treated => SearchLimit::Mode(Limit::of(ports, self, checks[3])),
            SearchMode::Area => SearchLimit::Mode(Limit::of(ports, self, checks[4])),
            SearchMode::Plain => SearchLimit::Mode(Limit::of(ports, self, checks[0])),
        };
        let level_band = level_band(ports, self);
        let configured_depth = settings::tenths(ports, stock::SEARCH_DEPTH) & 0xfffe;
        let depth = ports.depth_positions(readings.lens, lens_step, configured_depth);
        let first_check = (!readings.load_test && ports.property(stock::FIRST_CHECK) != 0).then(|| {
            let kind = match ports.property(stock::FIRST_CHECK_KIND) {
                1 => 3,
                2 => 2,
                _ => 4,
            };
            let limit = Limit::of(ports, self, checks[7]);
            let above_margin = ports.property(stock::LEVEL_CHECK_BITS) & 1 == 0 || above_margin(ports, self, stock::LEVEL_CHECK_MARGIN);
            FirstCheck { kind, alternate_level: self.plan.layout.code() == 2, limit, above_margin }
        });
        let level_checks_on = !readings.load_test && ports.property(stock::LEVEL_CHECKS) != 0;
        let first_level_checks = (level_checks_on && mode != SearchMode::Channel).then(|| level_checks(ports, self));
        let channels = (!readings.load_test && ports.property(stock::CHANNEL_SETS) & 1 != 0).then(|| channel_set(ports, self));
        let kind_flag = if ports.property(stock::KIND_FLAG) != 0 && !readings.load_test && readings.scene != stock::MASKED_SCENE && mode != SearchMode::Treated {
            crate::stock::plan::kind_row(self.plan.kind)[0]
        } else {
            0
        };
        let (selection, [first_order, second_order]) = if self.plan.kind.0 == 11 { (checks[10], stock::KIND_11_ORDERS) } else { (checks[9], stock::ORDERS) };
        let selection_limit = limit_applies(ports, self, selection);
        let step_orders = [step_order(ports, self.plan.layout.code(), second_order), step_order(ports, self.plan.layout.code(), first_order)];
        let row_weights = row_weights(ports, self);
        let kind_code = match mode {
            _ if readings.load_test => 0,
            SearchMode::Channel => 1,
            SearchMode::Treated => 2,
            SearchMode::Plain | SearchMode::Area => 0,
        };
        let configured_depth = settings::tenths(ports, stock::LAST_DEPTH) & 0xfffe;
        let last_depth = ports.depth_positions(readings.lens, lens_step, configured_depth);
        let level_window = mode != SearchMode::Treated
            && level <= settings::word(ports, stock::LEVEL_WINDOW[0]) as i16
            && level >= settings::word(ports, stock::LEVEL_WINDOW[1]) as i16;
        let level_limit = ports.property(stock::LEVEL_LIMIT[0]) == 1 && level <= settings::tenths(ports, stock::LEVEL_LIMIT[1]) as i16;
        let kind_limits = [checks[1], checks[2]].map(|check| Limit::of(ports, self, check));
        let load_test_plain = readings.load_test && ports.property(stock::LOAD_TEST_PLAIN) != 0;
        let second_level_checks = level_checks_on.then(|| level_checks(ports, self));
        let second_channels = (!readings.load_test && ports.property(stock::CHANNEL_SETS) & 2 != 0).then(|| named_channels(channel_bits(ports)));
        let kind = if kind_code == 2 {
                let case = window_start_case(readings, &self.areas.areas[0]);
                let [rows_setting, columns_setting, first_case, second_case] = self.treated_settings;
                let window = match (case & u32::from(first_case) != 0, case & u32::from(second_case) != 0) {
                    (false, _) => 0,
                    (true, true) => 2,
                    (true, false) => 1,
                };
                let class = self.layout.tracking.map_or(1, |tracking| tracking.class.code());
                KindSettings::Treated(TreatedSearch { window, setting: if class < 2 { rows_setting } else { columns_setting } })
        } else {
                let reach = i32::from(settings::word(ports, stock::PLAIN_LEVEL).wrapping_mul(10) as i16);
                let level_reached = readings.load_test || mode == SearchMode::Area || i32::from(level) >= reach;
                let steps = if mode == SearchMode::Area {
                    [0, 6, 6, 6, 6]
                } else if readings.load_test {
                    match ports.property(stock::LOAD_TEST_AREA) {
                        choice @ 1..=4 => [choice - 1, 6, 6, 6, 6],
                        _ => [0, 1, 2, 3, 6],
                    }
                } else {
                    [0, 1, 2, 3, if self.search.below_reference { 4 } else { 6 }]
                };
                let plain = (mode != SearchMode::Area && !readings.load_test).then(|| plain_search(ports, self));
                let alternate = match self.plan.layout.code() {
                    2 if ports.property(stock::ALTERNATE_KIND) == 1 => 1,
                    2 => 2,
                    _ => 0,
                };
                let plain = PlainSettings { level_reached, steps, plain, alternate };
                if kind_code == 1 { KindSettings::Channel { drive: channel_drive(ports, self), plain } } else { KindSettings::Plain(plain) }
        };
        let masked = (readings.scene == stock::MASKED_SCENE).then(|| {
            let depths = stock::MASKED_DEPTHS.map(|id| { let depth = settings::tenths(ports, id) & 0xfffe; ports.depth_positions(readings.lens, readings.level_step, depth) });
            let position = ports.side_adjusted(self.layout.channel_positions[0], 1, u32::from(self.plan.profile.reverse));
            MaskedSearch { depths, position, cell: tracked_cell(self) }
        });
        let ranged = matches!(self.plan.kind.0, 17 | 18).then(|| {
            let window = if mode == SearchMode::Treated { self.plan.secondary_share } else { self.plan.primary_share };
            let second = &self.exposures.second;
            stock::RANGED_FACTORS.map(|id| {
                let factor = u32::from(settings::word(ports, id));
                let share = quotient(u64::from(factor.wrapping_mul(100) / 100) * u64::from(window), 10_000) as u32;
                u32::from(second.gain_scale).wrapping_mul(u32::from(second.change_scale).wrapping_mul(share) / 10) / 10
            })
        });
        let bounds = (!plain_configured && !readings.load_test).then(|| {
            let side = u32::from(self.plan.profile.reverse);
            let drive_kind = match self.plan.layout.code() {
                1 => 0,
                _ => {
                    let id = if self.plan.kind.0 == 11 { stock::DRIVE_KIND_SETTINGS[0] } else { stock::DRIVE_KIND_SETTINGS[1] };
                    Some(ports.property(id)).filter(|&kind| kind <= 4).unwrap_or(0)
                }
            };
            let [first, second] = stock::DRIVE_DEPTH.map(|id| ports.property(id));
            let required = (first.wrapping_mul(second) / 10) as u16;
            let depth = (required != 0).then(|| ports.depth_positions(readings.lens, lens_step, required));
            DriveBounds {
                low: ports.side_adjusted(self.bounds.low, 1, side),
                high: ports.side_adjusted(self.bounds.high, 1, side),
                low_limit: ports.side_adjusted(self.layout.low_limit, 1, side),
                drive_kind,
                depth,
            }
        });
        MeasurementSettings {
            map,
            measured_cell,
            secondary_areas,
            level_band,
            search_limit,
            depth,
            first_check,
            level_checks: first_level_checks,
            channels,
            kind_flag,
            selection_limit,
            step_orders,
            period_long: self.plan.frame_depth > 150,
            row_weights,
            kind_code,
            last_depth,
            level_window,
            level_limit,
            kind_limits,
            load_test_plain,
            second_level_checks,
            second_channels,
            kind,
            masked,
            ranged,
            bounds,
        }
    }
}

impl MotionState {
    /// Returns the decisions also available to owned measurement consumers.
    pub fn set_up_measurement<P: MeasurementPorts + crate::exposure::ports::StockMemory>(
        &self,
        records: stock::MeasurementRecords,
        ports: &mut P,
    ) -> MeasurementSettings {
        let settings = self.measurement_settings(ports);
        stock::write_measurement(records, self, &settings, ports);
        settings
    }
}
