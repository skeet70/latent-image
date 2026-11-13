#![forbid(unsafe_code)]

//! The shot's area selection and the operations the focus round and the controllers make on the
//! shot's selection state. A selection list keeps, for each area of the plain pass, of each grid
//! pass and of each tracked kind, a class and the grid cell the class selects.

use crate::calculation::{AreaIndex, AreaResult, GridCell, ReferenceRequest, ResultPass, TrackedKind};
use crate::RecordHandle;

/// Where a selection list keeps an area's class and cell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelectionSlot {
    pub kind: Option<TrackedKind>,
    pub pass: ResultPass,
    pub index: AreaIndex,
}

impl SelectionSlot {
    pub const COUNT: usize = 84;

    pub const fn plain(index: AreaIndex) -> Self { Self { kind: None, pass: ResultPass::Plain, index } }

    /// Index into the class and cell arrays (X-M1 1.50 id5 0xf1b050, 0xf1c324).
    pub const fn position(self) -> usize {
        let index = self.index.get() as usize;
        match (self.kind, self.pass) {
            (None, ResultPass::Plain) => index,
            (Some(kind), ResultPass::Plain) => 3 + 3 * kind.get() as usize + index,
            (None, ResultPass::Grid(pass)) => 21 + 3 * pass.index() as usize + index,
            (Some(kind), ResultPass::Grid(pass)) => 30 + 9 * kind.get() as usize + 3 * pass.index() as usize + index,
        }
    }
}

/// The class a selection keeps for an area.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SelectionClass(pub u32);

/// The two bytes of a selection cell. A coordinate above 2 addresses the last column or row
/// (0xf1ae20).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CellCode {
    pub column: u8,
    pub row: u8,
}

impl CellCode {
    pub const fn cell(self) -> GridCell {
        let column = if self.column > 2 { 2 } else { self.column };
        let row = if self.row > 2 { 2 } else { self.row };
        GridCell { column, row }
    }
}

/// A selection list as the shot keeps it (X-M1 1.50: 0x218 bytes at 0x16a026c, the saved
/// default at 0x4171783c).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelectionList([u8; SelectionList::BYTES]);

impl Default for SelectionList {
    fn default() -> Self { Self([0; Self::BYTES]) }
}

impl SelectionList {
    pub const BYTES: usize = 0x218;
    const FIXED: usize = 0;
    const AREA_COUNT: usize = 4;
    const CLASSES: usize = 8;
    const CELLS: usize = 0x158;
    const WEIGHTS: usize = 0x200;
    /// Five halfwords the exposure conclusion leaves (0xf1f69c..0xf1f6cc).
    const EXPOSURE: usize = 0x20c;
    pub const EXPOSURE_HALVES: usize = 5;
    const UPPER_CODE: usize = 0x216;

    pub const fn from_bytes(bytes: [u8; Self::BYTES]) -> Self { Self(bytes) }

    pub const fn bytes(&self) -> &[u8; Self::BYTES] { &self.0 }

    /// The classes are given; the grid update keeps them instead of choosing its own.
    pub fn fixed(&self) -> bool { self.word(Self::FIXED) != 0 }

    pub fn set_fixed(&mut self, fixed: bool) { self.put_word(Self::FIXED, u32::from(fixed)); }

    pub const fn area_count(&self) -> u8 { self.0[Self::AREA_COUNT] }

    pub fn set_area_count(&mut self, count: u8) { self.0[Self::AREA_COUNT] = count; }

    pub fn class(&self, slot: SelectionSlot) -> SelectionClass { SelectionClass(self.word(Self::CLASSES + 4 * slot.position())) }

    pub fn set_class(&mut self, slot: SelectionSlot, class: SelectionClass) { self.put_word(Self::CLASSES + 4 * slot.position(), class.0); }

    pub fn cell(&self, slot: SelectionSlot) -> CellCode {
        let at = Self::CELLS + 2 * slot.position();
        CellCode { column: self.0[at], row: self.0[at + 1] }
    }

    pub fn set_cell(&mut self, slot: SelectionSlot, cell: CellCode) {
        let at = Self::CELLS + 2 * slot.position();
        self.0[at] = cell.column;
        self.0[at + 1] = cell.row;
    }

    pub fn weight(&self, index: AreaIndex) -> u32 { self.word(Self::WEIGHTS + 4 * index.get() as usize) }

    pub fn set_weight(&mut self, index: AreaIndex, weight: u32) { self.put_word(Self::WEIGHTS + 4 * index.get() as usize, weight); }

    pub fn exposure_half(&self, index: usize) -> u16 {
        let at = Self::EXPOSURE + 2 * index;
        u16::from_le_bytes([self.0[at], self.0[at + 1]])
    }

    pub fn set_exposure_half(&mut self, index: usize, half: u16) {
        let at = Self::EXPOSURE + 2 * index;
        self.0[at..at + 2].copy_from_slice(&half.to_le_bytes());
    }

    /// The upper class code the record's limits allow.
    pub const fn upper_code(&self) -> u8 { self.0[Self::UPPER_CODE] }

    pub fn set_upper_code(&mut self, code: u8) { self.0[Self::UPPER_CODE] = code; }

    fn word(&self, at: usize) -> u32 { u32::from_le_bytes([self.0[at], self.0[at + 1], self.0[at + 2], self.0[at + 3]]) }

    fn put_word(&mut self, at: usize, value: u32) { self.0[at..at + 4].copy_from_slice(&value.to_le_bytes()); }
}

/// Which caller classifies the subjects (the classification's first argument, 0 to 3). Stages 0
/// and 1 share one bank of per-area kinds; the others keep their own.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClassifyStage { Absorb, RecordLoad, Round, CaptureJob }

/// The two values of a subject reference the classification reads.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SubjectShare {
    /// Kept as the bank's mode; 3 lets a configured class cap the area kinds.
    pub mode: u32,
    /// Percent of each area's scaled level that decides its kind; 0 leaves every area untracked.
    pub percent: u16,
}

/// The values the calculation library decides subject eligibility from.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SubjectQuery {
    pub kind: u32,
    pub source: u32,
    pub area: u32,
}

/// The stock address of a calc 0x3c02 result buffer another task fills.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResultBuffer(pub u32);

/// The auxiliary source the retained result keeps and the reference and position it gives an
/// area that holds no tracked kind (0xf1d5d4 replaces only when its kind argument is 7).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetainedReference {
    pub source: u32,
    pub reference: u32,
    pub position: i16,
    pub untracked: Option<crate::calculation::AreaAt>,
}

/// Operations on the shot's selection state that the focus round and the controllers make
/// (X-M1 1.50 id5 entries named on each method).
pub trait ShotSelection {
    /// 0xf160d0: the request word the shot's preparation stored.
    fn request_word(&mut self) -> u32;
    /// 0xf162e4 with the record's defaults: a selection list built from the record alone.
    fn default_selection(&mut self, record: RecordHandle) -> SelectionList;
    /// 0xf17abc: the saved default list.
    fn saved_selection(&mut self) -> SelectionList;
    /// 0xf17af8.
    fn mark_auxiliary_retained(&mut self, retained: bool);
    /// 0xf1af38: an area of the working result at a cell.
    fn area(&mut self, kind: Option<TrackedKind>, pass: ResultPass, cell: CellCode, index: AreaIndex) -> AreaResult;
    /// 0xf1b050: the cell the selection list holds.
    fn selection_cell(&mut self, slot: SelectionSlot) -> CellCode;
    /// 0xf1c6b4.
    fn classify_subjects(&mut self, stage: ClassifyStage, share: SubjectShare, query: SubjectQuery);
    /// The kind the classification left for an area in a stage's bank.
    fn subject_kind(&mut self, stage: ClassifyStage, index: AreaIndex) -> Option<TrackedKind>;
    /// 0xf1cf68: a reset takes the saved list back; `keep_gates` leaves the target gates as they
    /// are (its third argument is 1).
    fn update_grid(&mut self, record: RecordHandle, reset: bool, keep_gates: bool, lens_reference: u32);
    /// 0xf1d0d8 into the working result.
    fn restore_result(&mut self);
    /// 0xf1d134 from a result buffer.
    fn retain_result(&mut self, from: ResultBuffer);
    /// 0xf1d5d4.
    fn retain_reference(&mut self, retained: RetainedReference);
    /// 0xf1fc5c: the saved calculation input moved to `reference`.
    fn reference_request(&mut self, reference: u32) -> ReferenceRequest;
    /// The calculation block's grid-selected word (+0x6410; 0xf1c930).
    fn grid_selected(&mut self) -> bool;
    /// The calculation block's selection mode word (+0x6414; 0xf1c944, 0xf1ed70).
    fn selection_mode(&mut self) -> u32;
    /// The plan's tracking word (+0x6568; 0xf1c958).
    fn tracking(&mut self) -> bool;
    /// The target word the last finish left (0x16ad738; 0xf367fc) and the halfword the last result
    /// left (0x16b3d38; 0xf36790).
    fn retained_targets(&mut self) -> (u32, u16);
}
