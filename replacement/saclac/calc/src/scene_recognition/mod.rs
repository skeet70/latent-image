//! SR-AUTO scene recognition: the scene and flags the library recognises for a record in live
//! view and at capture, the frames it notes to tell when the scene changed, and the histories its
//! votes settle by.

pub mod detectors;
pub mod history;
pub mod ports;

#[cfg(test)]
mod detector_tests;
#[cfg(test)]
mod fakes;
#[cfg(test)]
mod state_tests;

use xm1_saclac_interface::calc::SharedAddress;

use crate::adjust::configuration::{History, MotionRecord, HISTORY};
use crate::exposure::ports::{Properties, StockMemory};
use crate::exposure::settings;
use crate::stock::scene_recognition as stock;
use history::{Code, ComparisonRecord, FrameNote, IndexRecord, KeyRecord, SnapshotComparison};

pub const CELLS: usize = 256;

/// A 16 by 16 grid of cell levels, row by row.
pub type Grid = [i16; CELLS];

/// A scene, numbered as the trace names them (0xf52f50..0xf53368).
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct Scene(pub u32);

impl Scene {
    pub const AUTO: Self = Self(0);
    pub const PORTRAIT: Self = Self(1);
    pub const BACKLIT_PORTRAIT: Self = Self(4);
    pub const LANDSCAPE: Self = Self(5);
    pub const LANDSCAPE_PORTRAIT: Self = Self(8);
    pub const LANDSCAPE_BACKLIT_PORTRAIT: Self = Self(9);
    pub const HANDHELD_NIGHT: Self = Self(11);
    pub const NIGHT: Self = Self(12);
    pub const NIGHT_PORTRAIT: Self = Self(14);
    pub const NIGHT_PORTRAIT_TRIPOD: Self = Self(15);
    pub const CLOSEUP: Self = Self(48);
    /// Names no scene; the group hint after a reset (0xf4dfb8).
    pub const NO_HINT: Self = Self(58);
}

/// Bit 0: a moving subject; bit 1: digital shake reduction (trace suffixes 0xf53378, 0xf52f38).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SceneFlags(pub u16);

impl SceneFlags {
    pub const NONE: Self = Self(0);
    pub const MOVING: Self = Self(1);
    pub const SHAKE_REDUCTION: Self = Self(2);

    pub const fn with(self, other: Self) -> Self { Self(self.0 | other.0) }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Recognition {
    pub scene: Scene,
    pub flags: SceneFlags,
}

impl Recognition {
    pub const AUTO: Self = Self { scene: Scene::AUTO, flags: SceneFlags::NONE };
}

/// Which recognition a vote makes: live view's (0xf38110) or the shot's (0xefa3f8).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    Live,
    Capture,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LevelBounds {
    pub high: i16,
    pub low: i16,
}

/// What the frame query (0xf4f1e8) reads from the record for the scene detectors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Features {
    /// The query's answer; the vote then stores its own choice here.
    pub selection: Recognition,
    /// The query detectors in the order they were asked, 0 past the last.
    pub tried: [u8; 6],
    /// Record byte 0x5d.
    pub at_005d: u32,
    /// The subject query's count (0xeedc84).
    pub subjects: u32,
    pub level: i16,
    pub primary_level: i16,
    pub alternate_level: i16,
    pub third_level: i16,
    pub upper_level: i16,
    pub lower_level: i16,
    /// The capture record's answer to the level-difference test.
    pub difference_answer: u32,
    pub at_38: i16,
    pub at_3c: u32,
    pub status: u32,
    pub position: i16,
    pub lens_step: i16,
    pub distance: i16,
    pub response: i16,
    pub at_4c: u32,
    pub reference: u32,
    pub state: u32,
    pub selected: u32,
    /// The level bounds of record kind 0, then of kind 11 (0xf4bf88).
    pub bounds: [LevelBounds; 2],
    pub motion: MotionRecord,
    pub stationary: bool,
    pub index_recorded: bool,
}

const ZERO_MOTION: MotionRecord = MotionRecord {
    found: 0,
    speed: 0,
    published_kind: 0,
    published_speed: 0,
    dominant: 0,
    average: 0,
    average_scale: 0,
    published_area: 0,
    published_exposure: 0,
    source: 0,
    bytes: [0; 40],
};

impl Features {
    pub const ZERO: Self = Self {
        selection: Recognition::AUTO,
        tried: [0; 6],
        at_005d: 0,
        subjects: 0,
        level: 0,
        primary_level: 0,
        alternate_level: 0,
        third_level: 0,
        upper_level: 0,
        lower_level: 0,
        difference_answer: 0,
        at_38: 0,
        at_3c: 0,
        status: 0,
        position: 0,
        lens_step: 0,
        distance: 0,
        response: 0,
        at_4c: 0,
        reference: 0,
        state: 0,
        selected: 0,
        bounds: [LevelBounds { high: 0, low: 0 }; 2],
        motion: ZERO_MOTION,
        stationary: false,
        index_recorded: false,
    };
}

/// One run of a change-detector table: each slot's detector index and weight, its answer, the
/// summed weight of the detectors that answered, and the vote's decision.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Tally {
    pub detectors: [u8; 6],
    pub weights: [u8; 6],
    pub answers: [u32; 6],
    pub weight: u8,
    pub decision: u32,
}

impl Tally {
    pub const ZERO: Self = Self { detectors: [0; 6], weights: [0; 6], answers: [0; 6], weight: 0, decision: 0 };
}

/// The live vote's choices between frames.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Choices {
    pub history: Recognition,
    pub majority: Recognition,
    pub holding: bool,
    pub recording: bool,
    pub recorded: u8,
    pub held: u8,
    pub released: bool,
    pub forced: bool,
    pub tally: Tally,
}

impl Choices {
    pub const ZERO: Self = Self {
        history: Recognition::AUTO,
        majority: Recognition::AUTO,
        holding: false,
        recording: false,
        recorded: 0,
        held: 0,
        released: false,
        forced: false,
        tally: Tally::ZERO,
    };
}

/// The evaluation of the latest frame: the features, the choices and both comparisons.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Evaluation {
    pub features: Features,
    pub choices: Choices,
    pub against_reference: ComparisonRecord,
    pub snapshot: FrameNote,
    pub against_snapshot: SnapshotComparison,
}

impl Evaluation {
    pub const ZERO: Self = Self {
        features: Features::ZERO,
        choices: Choices::ZERO,
        against_reference: ComparisonRecord::ZERO,
        snapshot: FrameNote::ZERO,
        against_snapshot: SnapshotComparison::ZERO,
    };
}

/// The lens detectors of the capture vote's table: whether each answered and the direction it
/// reported.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LensDetectors {
    pub matched: bool,
    pub match_direction: u32,
    pub lens_moved: bool,
    pub lens_direction: u32,
    pub lens_changed: bool,
}

impl LensDetectors {
    pub const ZERO: Self = Self { matched: false, match_direction: 0, lens_moved: false, lens_direction: 0, lens_changed: false };
}

/// The rate filter's tests that passed: the position test, and the level test that admits the
/// group.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FilterOutcome {
    pub position: bool,
    pub level: bool,
}

/// The live vote's last decisions as the capture dump reads them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Trace {
    pub query: Scene,
    pub option: u32,
    pub candidate: Scene,
    pub recorded: u32,
    /// The group hints recorded with each noted frame.
    pub hints: History<Code>,
    pub filtered: Scene,
    pub earlier_candidate: Scene,
    pub earlier_filtered: Scene,
    pub selection: Scene,
    pub flags: SceneFlags,
    pub filter: FilterOutcome,
    pub detectors: LensDetectors,
    pub promoted: Recognition,
}

impl Trace {
    const ZERO: Self = Self {
        query: Scene::AUTO,
        option: 0,
        candidate: Scene::AUTO,
        recorded: 0,
        hints: History::filled(Code::ZERO),
        filtered: Scene::AUTO,
        earlier_candidate: Scene::AUTO,
        earlier_filtered: Scene::AUTO,
        selection: Scene::AUTO,
        flags: SceneFlags::NONE,
        filter: FilterOutcome { position: false, level: false },
        detectors: LensDetectors::ZERO,
        promoted: Recognition::AUTO,
    };
}

/// The capture vote's last decisions as the capture dump reads them.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CaptureTrace {
    pub query: Scene,
    pub option: u32,
    pub candidate: Scene,
    pub recorded: u32,
    pub detectors: LensDetectors,
    pub group: Scene,
    pub hint: Scene,
    pub filtered: Scene,
    pub selection: Recognition,
    pub promoted: Recognition,
}

impl CaptureTrace {
    const ZERO: Self = Self {
        query: Scene::AUTO,
        option: 0,
        candidate: Scene::AUTO,
        recorded: 0,
        detectors: LensDetectors::ZERO,
        group: Scene::AUTO,
        hint: Scene::AUTO,
        filtered: Scene::AUTO,
        selection: Recognition::AUTO,
        promoted: Recognition::AUTO,
    };
}

/// The recognition's state between calls (stock: the block at 0x16caf00 and the lists beside it).
#[derive(Debug, Eq, PartialEq)]
pub struct SceneRecognition {
    pub(crate) published: Recognition,
    pub(crate) settled: Recognition,
    /// The subject count the last successful subject query answered.
    pub(crate) last_subjects: u32,
    pub(crate) evaluation: Evaluation,
    pub(crate) ready: bool,
    pub(crate) group_hint: Scene,
    pub(crate) corrected_grid: Grid,
    pub(crate) level_offset: i16,
    pub(crate) lens_offset: i16,
    pub(crate) adjustments_changed: bool,
    pub(crate) candidate: Scene,
    pub(crate) filtered: Scene,
    pub(crate) source_copy: Grid,
    pub(crate) capture: Recognition,
    pub(crate) shifted_grid: Grid,
    pub(crate) saved: Evaluation,
    pub(crate) log_index: u16,
    pub(crate) saved_features: Features,
    pub(crate) saved_capture: Recognition,
    pub(crate) live_trace: Trace,
    pub(crate) capture_trace: CaptureTrace,
    pub(crate) stationary_history: [IndexRecord; HISTORY],
    pub(crate) references: History<KeyRecord>,
    pub(crate) primary_codes: History<Code>,
    pub(crate) secondary_codes: History<Code>,
    pub(crate) changes: History<bool>,
    pub(crate) comparisons: [ComparisonRecord; 5],
}

impl Default for SceneRecognition {
    fn default() -> Self { Self::new() }
}

impl SceneRecognition {
    /// The state before the first reset, every stock byte zero.
    pub const fn new() -> Self {
        Self {
            published: Recognition::AUTO,
            settled: Recognition::AUTO,
            last_subjects: 0,
            evaluation: Evaluation::ZERO,
            ready: false,
            group_hint: Scene::AUTO,
            corrected_grid: [0; CELLS],
            level_offset: 0,
            lens_offset: 0,
            adjustments_changed: false,
            candidate: Scene::AUTO,
            filtered: Scene::AUTO,
            source_copy: [0; CELLS],
            capture: Recognition::AUTO,
            shifted_grid: [0; CELLS],
            saved: Evaluation::ZERO,
            log_index: 0,
            saved_features: Features::ZERO,
            saved_capture: Recognition::AUTO,
            live_trace: Trace::ZERO,
            capture_trace: CaptureTrace::ZERO,
            stationary_history: [IndexRecord::ZERO; HISTORY],
            references: History::filled(KeyRecord::ZERO),
            primary_codes: History::filled(Code::ZERO),
            secondary_codes: History::filled(Code::ZERO),
            changes: History::filled(false),
            comparisons: [ComparisonRecord::ZERO; 5],
        }
    }

    /// Clears the state, empties the code lists, the reference history and the hints, and clears
    /// the comparison history and the work-memory log; the stationary history stays (0xf4df4c).
    pub fn reset(&mut self, memory: &mut impl StockMemory) {
        self.published = Recognition::AUTO;
        self.settled = Recognition::AUTO;
        self.last_subjects = 0;
        self.evaluation = Evaluation::ZERO;
        self.evaluation.against_reference.stationary.count = u8::MAX;
        self.ready = true;
        self.group_hint = Scene::NO_HINT;
        self.corrected_grid = [0; CELLS];
        self.level_offset = 0;
        self.lens_offset = 0;
        self.adjustments_changed = true;
        self.candidate = Scene::AUTO;
        self.filtered = Scene::AUTO;
        self.source_copy = [0; CELLS];
        self.capture = Recognition::AUTO;
        self.shifted_grid = [0; CELLS];
        self.saved = Evaluation::ZERO;
        self.log_index = 0;
        self.saved_features = Features::ZERO;
        self.saved_capture = Recognition::AUTO;
        self.live_trace = Trace::ZERO;
        self.live_trace.hints = History::empty();
        self.capture_trace = CaptureTrace::ZERO;
        self.references = History::empty();
        self.primary_codes = History::empty();
        self.secondary_codes = History::empty();
        self.changes = History::empty();
        self.comparisons = [ComparisonRecord::ZERO; 5];
        memory.fill(stock::LOG, stock::LOG_BYTES as usize, 0);
    }

    /// Notes where the motion measurement keeps its grid and copies the grid (0xf4dd50).
    pub fn set_source(&mut self, memory: &mut impl StockMemory, source: SharedAddress) {
        self.evaluation.against_reference.source = Some(source);
        self.source_copy = read_grid(memory, source);
    }

    /// The lens position the focus search reported (0xf4dd98).
    pub fn report_position(&mut self, position: i16) { self.evaluation.against_reference.position = position; }

    /// Keeps `grid` raised by calibrated level step 2 less `level` (0xf4e8cc).
    pub fn shift_grid(&mut self, properties: &mut impl Properties, grid: &Grid, level: i16) {
        let shift = settings::level_step(properties, 2).wrapping_sub(level);
        for (shifted, cell) in self.shifted_grid.iter_mut().zip(grid) {
            *shifted = cell.wrapping_add(shift);
        }
    }

    /// Keeps the source grid lowered by the level and lens offsets the frame query read
    /// (0xf4e91c). Nothing changes while no source is noted; stock reads from address 0.
    pub fn correct_grid(&mut self, memory: &mut impl StockMemory) {
        let Some(source) = self.evaluation.against_reference.source else { return };
        let offset = self.level_offset.wrapping_add(self.lens_offset);
        for (corrected, cell) in self.corrected_grid.iter_mut().zip(read_grid(memory, source)) {
            *corrected = cell.wrapping_sub(offset);
        }
    }

    /// 0xf4e980.
    pub const fn corrected_grid(&self) -> &Grid { &self.corrected_grid }

    /// The scene group the rate filter tests: none while the adjustments changed, else `hint`,
    /// unless property 0x4a14 names a group (0xf4e99c).
    pub fn choose_group_hint(&mut self, properties: &mut impl Properties, hint: Scene) {
        self.group_hint = if self.adjustments_changed { Scene::NO_HINT } else { hint };
        if let Some(group) = stock::forced_group(properties.property(stock::FORCED_GROUP)) {
            self.group_hint = group;
        }
    }

    /// 0xf4ec04.
    pub fn mark_adjustments_changed(&mut self) { self.adjustments_changed = true; }

    /// Whether the adjustments changed since the last call, clearing the mark (0xf4ebc0, 0xf4ebe4).
    pub fn take_adjustments_changed(&mut self) -> bool { core::mem::take(&mut self.adjustments_changed) }

    /// Writes the capture dump's scene-recognition section into the frame at `frame`: the mark,
    /// five level settings in tenths, the work-memory log and the end mark, with the log entry
    /// the next write takes zeroed (0xf525ec).
    pub fn store_dump_section(&self, ports: &mut (impl Properties + StockMemory), frame: SharedAddress) {
        let at = |offset: u32| SharedAddress(frame.0.wrapping_add(offset));
        ports.write_u32(at(stock::DUMP_SECTION), stock::DUMP_MARK);
        for (index, setting) in stock::DUMP_SETTINGS.into_iter().enumerate() {
            let tenths = ports.property(setting).wrapping_mul(10);
            ports.write_u32(at(stock::DUMP_SETTINGS_AT + 4 * index as u32), tenths);
        }
        for offset in (0..stock::LOG_BYTES).step_by(4) {
            let word = ports.read_u32(SharedAddress(stock::LOG.0 + offset));
            ports.write_u32(at(stock::DUMP_LOG + offset), word);
        }
        ports.write_u32(at(stock::DUMP_END), stock::DUMP_END_MARK);
        let next = stock::DUMP_LOG + stock::LOG_ENTRY_BYTES * u32::from(self.log_index);
        for offset in (0..stock::LOG_ENTRY_BYTES).step_by(4) {
            ports.write_u32(at(next + offset), 0);
        }
    }

    /// The scene and flags last published to the record.
    pub const fn published(&self) -> Recognition { self.published }

    pub const fn evaluation(&self) -> &Evaluation { &self.evaluation }

    /// The evaluation the latest live vote saved for the capture dump.
    pub const fn saved_evaluation(&self) -> &Evaluation { &self.saved }

    /// The features the latest capture vote saved, and the recognition it chose.
    pub const fn saved_features(&self) -> (&Features, Recognition) { (&self.saved_features, self.saved_capture) }

    pub const fn live_trace(&self) -> &Trace { &self.live_trace }

    pub const fn capture_trace(&self) -> &CaptureTrace { &self.capture_trace }

    /// The codes of the noted frames, newest first.
    pub const fn primary_codes(&self) -> &History<Code> { &self.primary_codes }

    /// The primary codes as they stood after the latest live recording.
    pub const fn secondary_codes(&self) -> &History<Code> { &self.secondary_codes }

    /// Whether each of the latest change votes found a change, newest first.
    pub const fn changes(&self) -> &History<bool> { &self.changes }

    pub const fn references(&self) -> &History<KeyRecord> { &self.references }

    pub const fn stationary_history(&self) -> &[IndexRecord; HISTORY] { &self.stationary_history }

    /// The comparisons with the reference record, newest first.
    pub const fn comparisons(&self) -> &[ComparisonRecord; 5] { &self.comparisons }
}

/// The 256 cells at `at`, two bytes each.
pub(crate) fn read_grid(memory: &mut impl StockMemory, at: SharedAddress) -> Grid {
    core::array::from_fn(|index| memory.read_u16(SharedAddress(at.0.wrapping_add(2 * index as u32))) as i16)
}
