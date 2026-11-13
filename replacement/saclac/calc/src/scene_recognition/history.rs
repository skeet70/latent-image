//! The records the recognition notes about frames and the lists it keeps of them.

use xm1_saclac_interface::calc::SharedAddress;

use super::{read_grid, Grid, Recognition, Scene, SceneFlags, SceneRecognition, CELLS};
use crate::exposure::ports::StockMemory;
use crate::stock::scene_recognition::CODED_SCENES;

/// A recognition as the history lists hold it: an index into the 29 coded scenes plus 29 times
/// the two flag bits, 0..=115.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Code(u8);

/// Codes above this decode as AUTO without flags (0xf4a028).
const HIGHEST_DECODED: u8 = 86;

impl Code {
    pub const ZERO: Self = Self(0);

    /// A scene outside the 29 codes as 0, whatever its flags (0xf4a070).
    pub fn of(recognition: Recognition) -> Self {
        let flags = (recognition.flags.0 & 3) as u8;
        if recognition.scene == Scene::AUTO {
            return Self(29 * flags);
        }
        match CODED_SCENES[1..].iter().position(|&scene| u32::from(scene) == recognition.scene.0) {
            Some(index) => Self(index as u8 + 1 + 29 * flags),
            None => Self(0),
        }
    }

    /// 0xf4a014.
    pub fn recognition(self) -> Recognition {
        if self.0 > HIGHEST_DECODED {
            return Recognition::default();
        }
        Recognition { scene: Scene(u32::from(CODED_SCENES[usize::from(self.0 % 29)])), flags: SceneFlags(u16::from(self.0 / 29)) }
    }

    pub const fn byte(self) -> u8 { self.0 }

    /// Code 100 marks an empty entry, so a recognition coding to it is never recorded (snow with
    /// both flags; 0xf4d8d4, 0xf4d9d4).
    pub const fn recordable(self) -> bool { self.0 != crate::stock::scene_recognition::EMPTY_CODE }
}

/// What the recognition notes about a frame to compare later frames with.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameNote {
    pub grid: Grid,
    pub position: i16,
    pub subject: bool,
    pub reference: u32,
    pub motion: u32,
    pub stationary: bool,
}

impl FrameNote {
    pub const ZERO: Self = Self { grid: [0; CELLS], position: 0, subject: false, reference: 0, motion: 0, stationary: false };
}

/// A noted frame and the recognition it was noted with.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyRecord {
    pub code: Code,
    pub note: FrameNote,
}

impl KeyRecord {
    pub const ZERO: Self = Self { code: Code::ZERO, note: FrameNote::ZERO };
}

/// One stationary decision (0xf4c210).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct IndexRecord {
    pub stationary: bool,
    /// What the steadiness query 0xee9728 answered.
    pub steady: bool,
    /// Decisions since the last steady answer, saturating at 255.
    pub count: u8,
}

impl IndexRecord {
    pub const ZERO: Self = Self { stationary: false, steady: false, count: 0 };
}

/// The latest comparison of the live frame with the reference record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ComparisonRecord {
    pub reference: KeyRecord,
    /// Where the motion measurement keeps the grid the comparisons read (0xf4dd50).
    pub source: Option<SharedAddress>,
    /// The source grid as the last grid comparison read it.
    pub compared: Grid,
    /// The weighted mean absolute difference of that comparison.
    pub mean: u16,
    pub position: i16,
    pub previous_position: i16,
    pub position_change: u16,
    pub subject_present: bool,
    pub motion: u32,
    pub stationary: IndexRecord,
}

impl ComparisonRecord {
    pub const ZERO: Self = Self {
        reference: KeyRecord::ZERO,
        source: None,
        compared: [0; CELLS],
        mean: 0,
        position: 0,
        previous_position: 0,
        position_change: 0,
        subject_present: false,
        motion: 0,
        stationary: IndexRecord::ZERO,
    };
}

/// The latest comparison of the live frame with the snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SnapshotComparison {
    pub compared: Grid,
    pub mean: u16,
    pub previous_position: i16,
    pub position_change: u16,
}

impl SnapshotComparison {
    pub const ZERO: Self = Self { compared: [0; CELLS], mean: 0, previous_position: 0, position_change: 0 };
}

/// The code most frequent among the latest `window` entries, the newer of equally frequent codes;
/// AUTO without flags while fewer than `window` are recorded (0xf4d790).
pub fn majority(entries: &[Option<Code>], window: usize) -> Recognition { leading(entries, window).map_or(Recognition::AUTO, |(_, code)| code.recognition()) }

/// The majority's code and the newest entry holding it (0xf50914..0xf50994).
pub(crate) fn leading(entries: &[Option<Code>], window: usize) -> Option<(usize, Code)> {
    let Some(Some(_)) = entries.get(window - 1) else { return None };
    let mut counts = [0u8; 116];
    let mut best = (0, Code(0));
    for (index, code) in entries[..window].iter().enumerate().rev() {
        let Some(code) = code else { continue };
        let count = &mut counts[usize::from(code.0)];
        *count = count.wrapping_add(1);
        let count = *count;
        if counts[usize::from(best.1 .0)] <= count {
            best = (index, *code);
        }
    }
    Some(best)
}

/// Pushes `entry` in front of `entries`, dropping the oldest.
pub fn push_front<T: Copy>(entries: &mut [T], entry: T) {
    let last = entries.len() - 1;
    entries.copy_within(..last, 1);
    entries[0] = entry;
}

impl SceneRecognition {
    /// Records `recognition` in the primary codes and copies them to the secondary codes
    /// (0xf4d8bc).
    pub(crate) fn record_code(&mut self, recognition: Recognition) {
        let code = Code::of(recognition);
        if code.recordable() {
            self.primary_codes.record(code);
        }
        self.secondary_codes = self.primary_codes;
    }

    /// Notes the frame as a reference with `recognition`, makes the primary codes the reference
    /// codes, and records the group hint (0xf4d968). Without a source the note's grid is zero.
    pub(crate) fn note_reference(&mut self, memory: &mut impl StockMemory, recognition: Recognition) {
        let code = Code::of(recognition);
        if code.recordable() {
            let comparison = &self.evaluation.against_reference;
            let features = &self.evaluation.features;
            let grid = comparison.source.map_or([0; CELLS], |source| read_grid(memory, source));
            let note = FrameNote {
                grid,
                position: comparison.position,
                subject: features.subjects != 0,
                reference: features.reference,
                motion: comparison.motion,
                stationary: comparison.stationary.stationary,
            };
            self.references.record(KeyRecord { code, note });
        }
        self.primary_codes = self.references.map(|record| record.code);
        self.live_trace.hints.record(Code::of(Recognition { scene: self.group_hint, flags: SceneFlags::NONE }));
    }

    /// 0xf4dea4.
    pub(crate) fn push_comparison(&mut self) { push_front(&mut self.comparisons, self.evaluation.against_reference); }
}
