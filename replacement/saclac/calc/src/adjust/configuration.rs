//! saclac's adjustment configuration: the flags the controllers and the search set for the round,
//! the configuration words other tasks share with saclac, and the subject-motion measurement the
//! round runs on each frame pair with its class and offset histories.

use xm1_saclac_interface::RecordHandle;
use xm1_saclac_profile_records::stock::record::group_00;
use xm1_saclac_profile_records::stock::shared::{group_14, group_26, group_27};
use xm1_saclac_profile_records::ProfileRecords;

use crate::exposure::ports::{ProfileAccess, Properties, Property, StockMemory};
use crate::stock::adjust as stock;

pub const HISTORY: usize = 20;
pub const INTERVALS: usize = 10;
pub const GRID_CELLS: usize = 256;
const GRID_COLUMNS: usize = 16;

/// The profile records with writes, for the records the layer stores.
pub trait ProfileStore: ProfileAccess {
    fn records(&mut self) -> &mut ProfileRecords<Self::Records>;
}

/// The frame interval of a pipeline timing word (0x1005f60).
pub trait FrameInterval {
    fn frame_interval(&mut self, timing: u32) -> Option<u32>;
}

/// The calc library's diagnostic state around the motion classification.
pub trait LibraryDiagnostics {
    /// Copies the library's published kind, speed, area and exposure into the record's motion
    /// record (0xf5246c).
    fn publish_selection(&mut self, record: RecordHandle);
    /// Snapshots the library's diagnostic buffers while the diagnostic setting is on (0xf524e4).
    fn snapshot_diagnostics(&mut self);
    /// Publishes kind 3 with zero speed, area and exposure (0xf52440).
    fn reset_selection(&mut self);
    /// Clears the library's diagnostic buffers (0xf5251c).
    fn reset_diagnostics(&mut self);
}

/// The configuration words other tasks store into the block they share with saclac.
pub mod shared {
    use super::*;

    /// 0xfa7ef4.
    pub fn mode_is_one(memory: &mut impl StockMemory) -> bool { memory.read_u32(stock::SHARED_MODE) == 1 }

    /// 0xfa7f40.
    pub fn word(memory: &mut impl StockMemory) -> u32 { memory.read_u32(stock::SHARED_WORD) }

    /// Set by another task and enabled by the feature setting (0xfa7f80).
    pub fn feature_enabled(ports: &mut (impl StockMemory + Properties)) -> bool {
        ports.read_u32(stock::SHARED_FEATURE) == 1 && ports.property(stock::FEATURE_SETTING) != 0
    }

    /// Cleared by another task; the controllers mark it seen (0xfa7fd8, 0xfa7fb8).
    pub fn seen(memory: &mut impl StockMemory) -> u32 { memory.read_u32(stock::SHARED_SEEN) }

    pub fn store_seen(memory: &mut impl StockMemory, seen: u32) { memory.write_u32(stock::SHARED_SEEN, seen) }

    /// Set by another task; the round clears it (0xfaa790, 0xfaa770).
    pub fn request(memory: &mut impl StockMemory) -> u32 { memory.read_u32(stock::SHARED_REQUEST) }

    pub fn store_request(memory: &mut impl StockMemory, request: u32) { memory.write_u32(stock::SHARED_REQUEST, request) }
}

/// Which frame pairs a motion measurement compares: live view's (0xf3515c) or the half press's
/// (0xf2457c).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MotionSource {
    Live,
    HalfPress,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MotionClass {
    #[default]
    Zero,
    One,
    Two,
    /// Nothing decides the class.
    Undetermined,
}

impl MotionClass {
    /// Codes 1 to 3 name their class, every other code class 0 (0xfa82fc, 0xfa8388).
    pub const fn from_code(code: u32) -> Self {
        match code {
            1 => Self::One,
            2 => Self::Two,
            3 => Self::Undetermined,
            _ => Self::Zero,
        }
    }

    const fn exact(code: u32) -> Option<Self> {
        match code {
            0..=3 => Some(Self::from_code(code)),
            _ => None,
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::Zero => 0,
            Self::One => 1,
            Self::Two => 2,
            Self::Undetermined => 3,
        }
    }
}

/// The changed cells' centre in twentieths of a cell, the row scaled by the record's aspect.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Centroid {
    pub x: u32,
    pub y: u32,
}

/// A most-recent-first history of `HISTORY` entries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct History<T> {
    entries: [Option<T>; HISTORY],
}

impl<T: Copy> History<T> {
    pub const fn empty() -> Self { Self { entries: [None; HISTORY] } }

    const fn filled(value: T) -> Self { Self { entries: [Some(value); HISTORY] } }

    pub fn record(&mut self, value: T) {
        self.entries.copy_within(..HISTORY - 1, 1);
        self.entries[0] = Some(value);
    }

    pub const fn entries(&self) -> &[Option<T>; HISTORY] { &self.entries }

    /// The latest `count` entries, when that many are recorded.
    fn latest(&self, count: usize) -> Option<impl Iterator<Item = T> + '_> {
        self.entries[count - 1].is_some().then(|| self.entries[..count].iter().flatten().copied())
    }
}

/// A history window: the configured count, 1 for 0, at most `HISTORY` (0xfa821c, 0xfa83dc).
fn window(properties: &mut impl Properties, id: Property) -> usize {
    (properties.property(id) as usize).clamp(1, HISTORY)
}

/// The motion record (shared field group 27 at +0x7ec), as the measurement and the classification
/// read and store it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MotionRecord {
    /// 1 when the latest measurement found motion.
    pub found: u32,
    pub speed: u16,
    /// What the calc library published: a kind, a speed level, an area and an exposure.
    pub published_kind: u32,
    pub published_speed: u16,
    pub dominant: u32,
    pub average: u16,
    pub average_scale: u16,
    pub published_area: u32,
    pub published_exposure: u32,
    /// 1 for the half press's measurement, 0 for live view's; the round sets it.
    pub source: u32,
    pub(crate) bytes: [u8; 40],
}

/// The round's recent frame intervals (shared field group 14 at +0x864).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IntervalList {
    pub intervals: [u32; INTERVALS],
    pub count: u16,
    pub(crate) bytes: [u8; 48],
}

impl IntervalList {
    /// The listed intervals' sum; a count above `INTERVALS` sums the list.
    fn total(&self) -> u32 { self.intervals[..usize::from(self.count).min(INTERVALS)].iter().fold(0, |sum, interval| sum.wrapping_add(*interval)) }
}

/// The latest measurement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MotionMeasurement {
    pub cells: u16,
    pub found: bool,
    pub centroid: Option<Centroid>,
    pub previous: Option<Centroid>,
    /// One million over the frame period, 0 when none is known.
    pub rate: u16,
    pub speed: u16,
}

impl Default for MotionMeasurement {
    /// Zeroed, as saclac's memory is before the first measurement.
    fn default() -> Self {
        Self { cells: 0, found: false, centroid: Some(Centroid::default()), previous: Some(Centroid::default()), rate: 0, speed: 0 }
    }
}

/// The two luminance grids a measurement compares, sixteen cells to a row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GridPair {
    pub current: [i16; GRID_CELLS],
    pub previous: [i16; GRID_CELLS],
}

/// What the motion diagnostics export.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MotionDiagnostics<'a> {
    pub current: &'a [i16; GRID_CELLS],
    pub previous: &'a [i16; GRID_CELLS],
    pub measurement: MotionMeasurement,
    pub class: MotionClass,
    pub classes: [Option<MotionClass>; HISTORY],
    pub dominant: MotionClass,
    pub offset: u16,
    pub offsets: [Option<u16>; HISTORY],
    pub average: u16,
}

/// The two flags the controllers set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControllerFlag {
    /// 0xfa7ff8, 0xfa8018.
    First,
    /// Set for a record of scene kind 57 (0xf309d8; 0xfa8038, 0xfa8058).
    Second,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Configuration {
    /// The acquisition's timing word, which the planner reads (0xfa7e8c, 0xfa7eac).
    pub acquisition_timing: u32,
    refresh: bool,
    controller_flags: [bool; 2],
    classes: History<MotionClass>,
    offsets: History<u16>,
    measurement: MotionMeasurement,
    class: MotionClass,
    dominant: MotionClass,
    offset: u16,
    average: u16,
}

impl Default for Configuration {
    /// Both histories hold zeros until the first reset, as saclac's zeroed memory does.
    fn default() -> Self {
        Self {
            acquisition_timing: 0,
            refresh: false,
            controller_flags: [false; 2],
            classes: History::filled(MotionClass::Zero),
            offsets: History::filled(0),
            measurement: MotionMeasurement::default(),
            class: MotionClass::Zero,
            dominant: MotionClass::Zero,
            offset: 0,
            average: 0,
        }
    }
}

/// The row scale as a fraction, numerator in the low half (record word +0x0, 0x74ec30).
fn row_scale(ports: &mut impl ProfileAccess, record: RecordHandle) -> (u32, u32) {
    let word: u32 = ports.profile().read(record, &group_00::AT_0000);
    (word & 0xffff, word >> 16)
}

fn motion_record(ports: &mut impl ProfileAccess, record: RecordHandle) -> MotionRecord { MotionRecord::decode(ports.profile().read_shared(record, &group_27::AT_07EC)) }

/// Stores the motion record; a handle that may not write group 27 stores nothing (0x758300).
fn store_motion_record(ports: &mut impl ProfileStore, record: RecordHandle, motion: &MotionRecord) {
    let _refused = ports.records().write_shared(record, &group_27::AT_07EC, motion.encode());
}

/// The floor of the square root.
fn root(square: u64) -> u32 {
    let mut root = 0u64;
    let mut bit = 1u64 << 62;
    let mut rest = square;
    while bit > square {
        bit >>= 2;
    }
    while bit != 0 {
        if rest >= root + bit {
            rest -= root + bit;
            root = (root >> 1) + bit;
        } else {
            root >>= 1;
        }
        bit >>= 2;
    }
    root as u32
}

impl Configuration {
    /// Set when the search asks for a refresh and property 0x87fd is not 1 (0xfa9554).
    pub fn request_refresh(&mut self, properties: &mut impl Properties, requested: bool) {
        self.refresh = requested && properties.property(stock::REFRESH_SUPPRESSED) != 1;
    }

    /// 0xfa9588.
    pub const fn refresh(&self) -> bool { self.refresh }

    pub const fn controller_flag(&self, flag: ControllerFlag) -> bool { self.controller_flags[flag as usize] }

    pub fn set_controller_flag(&mut self, flag: ControllerFlag, set: bool) { self.controller_flags[flag as usize] = set; }

    pub const fn measurement(&self) -> &MotionMeasurement { &self.measurement }

    pub const fn class(&self) -> MotionClass { self.class }

    pub const fn dominant(&self) -> MotionClass { self.dominant }

    pub const fn offset(&self) -> u16 { self.offset }

    pub const fn average(&self) -> u16 { self.average }

    pub const fn classes(&self) -> &History<MotionClass> { &self.classes }

    pub const fn offsets(&self) -> &History<u16> { &self.offsets }

    /// Empties both histories and resets the library's published selection and diagnostics (0xfa898c).
    pub fn reset_histories(&mut self, library: &mut impl LibraryDiagnostics) {
        self.classes = History::empty();
        self.offsets = History::empty();
        library.reset_selection();
        library.reset_diagnostics();
    }

    /// Measures the subject motion between the grids: the cells whose difference reaches the
    /// threshold, their centre, its distance from `previous` and that distance per frame period.
    /// Stores the result into the record's motion record and leaves the new centre in `previous`.
    pub fn measure_motion(
        &mut self,
        ports: &mut (impl ProfileStore + Properties),
        timing: &mut impl FrameInterval,
        record: RecordHandle,
        grids: &GridPair,
        source: MotionSource,
        previous: &mut Option<Centroid>,
    ) {
        let (numerator, denominator) = row_scale(ports, record);
        let threshold = ports.property(stock::motion_threshold(source)) as i32;
        let minimum = ports.property(stock::motion_minimum_cells(source));
        let period = match source {
            MotionSource::HalfPress => {
                let pipeline: u32 = ports.profile().read_shared(record, &group_26::AT_0230);
                timing.frame_interval(pipeline).unwrap_or(0)
            }
            MotionSource::Live => IntervalList::decode(ports.profile().read_shared(record, &group_14::AT_0864)).total(),
        };
        let rate = if period == 0 { 0 } else { (1_000_000 / period) as u16 };
        let earlier = *previous;

        let (mut cells, mut columns, mut rows) = (0u32, 0u32, 0u32);
        for (index, (current, before)) in grids.current.iter().zip(grids.previous.iter()).enumerate() {
            let difference = current.wrapping_sub(*before).wrapping_abs();
            if i32::from(difference) >= threshold {
                cells += 1;
                columns += (index % GRID_COLUMNS) as u32;
                rows += (index / GRID_COLUMNS) as u32;
            }
        }
        let cells = cells as u16;

        let centroid = (cells != 0 && u32::from(cells) >= minimum).then(|| {
            let cells = u32::from(cells);
            let x = (columns * 20 + cells / 2) / cells;
            let scaled = cells.wrapping_mul(denominator);
            let y = rows.wrapping_mul(numerator).wrapping_mul(20).wrapping_add(scaled / 2).checked_div(scaled).unwrap_or(0);
            Centroid { x, y }
        });
        let distance = match (centroid, earlier) {
            (Some(centroid), Some(earlier)) => {
                let dx = i64::from(centroid.x) - i64::from(earlier.x);
                let dy = i64::from(centroid.y) - i64::from(earlier.y);
                root((dx * dx + dy * dy) as u64) as u16
            }
            _ => 0,
        };
        let speed = ((u32::from(rate) * u32::from(distance) + 15) / 30) as u16;
        *previous = centroid;

        let mut motion = motion_record(ports, record);
        motion.found = u32::from(centroid.is_some());
        motion.speed = speed;
        store_motion_record(ports, record, &motion);

        self.measurement = MotionMeasurement { cells, found: centroid.is_some(), centroid, previous: earlier, rate, speed };
    }

    /// Classifies the latest measurement with what the calc library published, records the class
    /// and an offset in the histories, and stores the dominant class and the offset average into
    /// the record's motion record (0xfa8520).
    pub fn classify_motion(
        &mut self,
        ports: &mut (impl ProfileStore + Properties),
        library: &mut impl LibraryDiagnostics,
        record: RecordHandle,
        source: MotionSource,
    ) {
        library.publish_selection(record);
        let mut motion = motion_record(ports, record);
        let limit = ports.property(stock::offset_limit(source));
        let published_level = ((u32::from(motion.published_speed) * 1000 + 1280) / 2560) as u16;
        let measured_level = ((u32::from(self.measurement.speed) * 1000 + 160) / 320) as u16;

        let enable = ports.property(stock::motion_enable(source));
        let measures = enable & 2 != 0;
        let by_kind = enable & 1 != 0;
        let (class, offset) = if measures && by_kind {
            let class = self.moving_class(ports, &motion, source);
            (class, if u32::from(published_level) < limit { measured_level } else { published_level })
        } else if measures {
            let class = match motion.found {
                0 => MotionClass::Zero,
                1 => MotionClass::One,
                _ => MotionClass::Undetermined,
            };
            (class, measured_level)
        } else if by_kind {
            (MotionClass::from_code(motion.published_kind.min(3)), published_level)
        } else {
            (MotionClass::Undetermined, 0)
        };

        self.classes.record(class);
        let dominant = self.dominant_class(ports, source);
        if offset != u16::MAX {
            self.offsets.record(offset);
        }
        let average = self.offset_average(ports, source);

        motion.dominant = stock::class_code(dominant);
        motion.average = average;
        motion.average_scale = 30;
        store_motion_record(ports, record, &motion);
        self.class = class;
        self.dominant = dominant;
        self.offset = offset;
        self.average = average;
    }

    /// The class of a measurement when both measuring and classifying by kind are on: class 1 for
    /// a moving subject the rules admit, else the class configured for the result and the kind.
    fn moving_class(&self, ports: &mut impl Properties, motion: &MotionRecord, source: MotionSource) -> MotionClass {
        if motion.published_kind == 0 && motion.found == 1 && self.moving(ports, motion, source) {
            return MotionClass::One;
        }
        if motion.found > 3 || motion.published_kind > 3 {
            return MotionClass::Undetermined;
        }
        MotionClass::from_code(ports.property(stock::configured_class(motion.found, motion.published_kind)))
    }

    /// Whether the published area, the changed cells and the measured speed are each in their
    /// configured bounds.
    fn moving(&self, ports: &mut impl Properties, motion: &MotionRecord, source: MotionSource) -> bool {
        let scale = stock::area_factors(source).into_iter().fold(1u32, |scale, id| scale.wrapping_mul(ports.property(id)));
        if ports.property(stock::MOVING_RULES) != 1 {
            return false;
        }
        let bounds = if motion.published_exposure == 0 { stock::MOVING_STILL_EXPOSURE } else { stock::MOVING_EXPOSED };
        let (cells, speed) = (u32::from(self.measurement.cells), u32::from(self.measurement.speed));
        let mut admits = |value: u32, [low, high]: [Property; 2], scale: u32| {
            let low = ports.property(low).wrapping_mul(scale);
            let high = ports.property(high).wrapping_mul(scale);
            low <= value && value < high
        };
        admits(motion.published_area, bounds.area, scale) && admits(cells, bounds.cells, 1) && admits(speed, bounds.speed, 1)
    }

    /// The class most often recorded among the configured window of latest classes; a tie goes to
    /// the first of the preference properties naming a tied class (0xfa83c0). Undetermined while
    /// the window is not full or no preference names a tied class.
    fn dominant_class(&self, ports: &mut impl Properties, source: MotionSource) -> MotionClass {
        let count = window(ports, stock::dominant_window(source));
        let Some(latest) = self.classes.latest(count) else {
            return MotionClass::Undetermined;
        };
        let mut tally = [0u8; 4];
        for class in latest {
            tally[class.index()] += 1;
        }
        let most = tally.iter().copied().max().unwrap_or(0);
        let preferences = stock::DOMINANT_PREFERENCE.map(|id| ports.property(id));
        preferences
            .into_iter()
            .filter_map(MotionClass::exact)
            .find(|class| tally[class.index()] == most)
            .unwrap_or(MotionClass::Undetermined)
    }

    /// The mean of the configured window of latest offsets, 0 unless the window is full (0xfa81fc).
    fn offset_average(&self, ports: &mut impl Properties, source: MotionSource) -> u16 {
        let count = window(ports, stock::average_window(source));
        self.offsets.latest(count).map_or(0, |latest| latest.fold(0u16, u16::wrapping_add) / count as u16)
    }

    /// Exports the grids, the measurement, the classification and both histories to the work
    /// memory after the library's own diagnostics (0xfa91b0).
    pub fn export_diagnostics(&self, memory: &mut impl StockMemory, library: &mut impl LibraryDiagnostics, grids: &GridPair) {
        library.snapshot_diagnostics();
        let diagnostics = MotionDiagnostics {
            current: &grids.current,
            previous: &grids.previous,
            measurement: self.measurement,
            class: self.class,
            classes: *self.classes.entries(),
            dominant: self.dominant,
            offset: self.offset,
            offsets: *self.offsets.entries(),
            average: self.average,
        };
        for (at, word) in stock::motion_diagnostic_words(diagnostics) {
            memory.write_u32(at, word);
        }
    }
}

/// Clears the round's frame-interval list: its intervals and its count (0xfa89b8).
pub fn clear_intervals(ports: &mut impl ProfileStore, record: RecordHandle) {
    let mut list = IntervalList::decode(ports.profile().read_shared(record, &group_14::AT_0864));
    list.intervals = [0; INTERVALS];
    list.count = 0;
    let _refused = ports.records().write_shared(record, &group_14::AT_0864, list.encode());
}
