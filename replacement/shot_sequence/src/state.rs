#![forbid(unsafe_code)]

//! The shot's state where other tasks read it: the calculation block with its selection grid,
//! the retained and set-aside results, the shot context and the saved calculation input.
//! `ShotState` reads and writes it through `StateMemory`; `stock::state` places each value.
//! `ShotState` also holds what only the shot's steps share: the secondary job's result and the
//! frame dump's tag.

use crate::absorb::{AuxiliaryOutcome, CalculationResults};
use crate::secondary_job::SecondaryResult;
use crate::statistics_dump::StatisticsDump;
use crate::stock::state as layout;
use xm1_saclac_interface::calculation::{AreaAt, AreaIndex, AreaResult, AreaRow, AreaTarget, CellSummary, FocusSummary, GridCell, GridPass, ReferenceRequest, ResultPass, TrackedKind};
use xm1_saclac_interface::selection::{CellCode, ClassifyStage, ResultBuffer, SelectionClass, SelectionList, SelectionSlot};

/// A location of shot state; only `stock::state` makes one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StateAddress(pub(crate) u32);

impl StateAddress {
    pub const fn get(self) -> u32 { self.0 }

    pub(crate) const fn offset(self, bytes: usize) -> Self { Self(self.0 + bytes as u32) }
}

/// Memory holding the shot's state. Copies move bytes in ascending order.
pub trait StateMemory {
    fn read(&mut self, at: StateAddress, into: &mut [u8]);
    fn write(&mut self, at: StateAddress, bytes: &[u8]);
    fn copy(&mut self, to: StateAddress, from: StateAddress, bytes: usize);
}

/// The shot's state.
pub struct ShotState<M> {
    memory: M,
    secondary: SecondaryResult,
    frame_dump: StatisticsDump,
}

impl<M: StateMemory> ShotState<M> {
    pub const fn new(memory: M) -> Self { Self { memory, secondary: SecondaryResult::new(), frame_dump: StatisticsDump { armed: false } } }

    pub fn memory(&mut self) -> &mut M { &mut self.memory }

    pub fn secondary(&self) -> &SecondaryResult { &self.secondary }

    pub fn secondary_mut(&mut self) -> &mut SecondaryResult { &mut self.secondary }

    pub fn frame_dump(&mut self) -> &mut StatisticsDump { &mut self.frame_dump }

    pub(crate) fn bytes<const N: usize>(&mut self, at: StateAddress) -> [u8; N] {
        let mut bytes = [0; N];
        self.memory.read(at, &mut bytes);
        bytes
    }

    pub(crate) fn word(&mut self, at: StateAddress) -> u32 { u32::from_le_bytes(self.bytes(at)) }

    pub(crate) fn half(&mut self, at: StateAddress) -> u16 { u16::from_le_bytes(self.bytes(at)) }

    pub(crate) fn byte(&mut self, at: StateAddress) -> u8 { self.bytes::<1>(at)[0] }

    pub(crate) fn set_word(&mut self, at: StateAddress, value: u32) { self.memory.write(at, &value.to_le_bytes()); }

    pub(crate) fn set_half(&mut self, at: StateAddress, value: u16) { self.memory.write(at, &value.to_le_bytes()); }

    /// The retained target word and its copy in the saved calculation input.
    pub fn retain_target_word(&mut self, word: u32) {
        for at in layout::RETAINED_TARGET_WORD { self.set_word(at, word); }
    }
}

impl<M: StateMemory> CalculationResults for ShotState<M> {
    fn result_areas(&mut self) -> u8 { u8::try_from(self.word(layout::AREA_COUNT)).unwrap_or(u8::MAX) }

    fn tracking(&mut self) -> bool { self.word(layout::TRACKING) != 0 }

    fn area_kind(&mut self, bank: ClassifyStage, index: AreaIndex) -> u8 { self.byte(layout::subject_kind(bank, index)) }

    fn grid_selected(&mut self) -> bool { self.word(layout::GRID_SELECTED) != 0 }

    fn selection_mode(&mut self) -> u32 { self.word(layout::SELECTION_MODE) }

    fn result_area(&mut self, at: AreaAt) -> AreaResult { AreaResult::from_bytes(self.bytes(layout::area(at))) }

    fn marker(&mut self, pass: ResultPass, kind: Option<TrackedKind>, index: AreaIndex) -> u32 { self.word(layout::marker(pass, kind, index)) }

    fn plan_value(&mut self, pass: ResultPass, cell: GridCell, index: AreaIndex) -> u32 { self.word(layout::plan_value(pass, cell, index)) }

    fn plan_half(&mut self, pass: ResultPass, cell: GridCell, index: AreaIndex) -> i16 { self.half(layout::plan_half(pass, cell, index)) as i16 }

    fn row(&mut self, pass: ResultPass, cell: GridCell, index: AreaIndex) -> AreaRow { AreaRow(self.bytes(layout::row(pass, cell, index))) }

    fn row_word(&mut self, pass: ResultPass, cell: GridCell) -> u32 { self.word(layout::row_word(pass, cell)) }

    fn cell_summary(&mut self, pass: ResultPass, cell: GridCell) -> CellSummary { CellSummary::from_bytes(self.bytes(layout::cell_summary(pass, cell))) }

    fn focus_summary(&mut self) -> FocusSummary { layout::focus_summary(|at| self.word(at)) }

    fn retained_half(&mut self) -> u16 { self.half(layout::RETAINED_HALF) }

    fn auxiliary(&mut self) -> AuxiliaryOutcome { layout::auxiliary_outcome(self.word(layout::AUXILIARY)) }

    fn set_auxiliary(&mut self, outcome: AuxiliaryOutcome) { self.set_word(layout::AUXILIARY, layout::auxiliary_word(outcome)); }

    fn set_working_aside(&mut self) { self.memory.copy(layout::ASIDE_RESULT, layout::RESULT, layout::RESULT_BYTES); }

    fn restore_working(&mut self) { self.memory.copy(layout::RESULT, layout::ASIDE_RESULT, layout::RESULT_BYTES); }

    fn adopt_grid_area(&mut self, pass: GridPass, cell: GridCell, index: AreaIndex, kind: Option<TrackedKind>) {
        let grid = ResultPass::Grid(pass);
        self.memory.copy(layout::area(AreaAt::Cell { pass: ResultPass::Plain, cell, index }), layout::area(AreaAt::Cell { pass: grid, cell, index }), AreaResult::BYTES);
        if let Some(kind) = kind {
            self.memory.copy(layout::area(AreaAt::Tracked { pass: ResultPass::Plain, kind, index }), layout::area(AreaAt::Tracked { pass: grid, kind, index }), AreaResult::BYTES);
        }
    }

    fn adopt_grid_tables(&mut self, pass: GridPass, cell: GridCell, index: AreaIndex) {
        let grid = ResultPass::Grid(pass);
        let plain = ResultPass::Plain;
        self.memory.copy(layout::plan_value(plain, cell, index), layout::plan_value(grid, cell, index), 4);
        self.memory.copy(layout::plan_half(plain, cell, index), layout::plan_half(grid, cell, index), 2);
        self.memory.copy(layout::row(plain, cell, index), layout::row(grid, cell, index), AreaRow::BYTES);
    }

    fn adopt_grid_cell(&mut self, pass: GridPass, cell: GridCell) {
        self.memory.copy(layout::cell_summary(ResultPass::Plain, cell), layout::cell_summary(ResultPass::Grid(pass), cell), CellSummary::BYTES);
    }
}

impl<M: StateMemory> ShotState<M> {
    /// The grid cell the selection holds for an area of a pass, or of a tracked kind's areas.
    pub fn cell_of(&mut self, kind: Option<TrackedKind>, pass: ResultPass, index: AreaIndex) -> GridCell {
        self.selection_cell(SelectionSlot { kind, pass, index }).cell()
    }

    pub fn set_cell_of(&mut self, kind: Option<TrackedKind>, pass: ResultPass, index: AreaIndex, cell: GridCell) {
        self.memory.write(layout::cell(SelectionSlot { kind, pass, index }), &[cell.column, cell.row]);
    }

    pub fn retain_result(&mut self) { self.memory.copy(layout::RETAINED_RESULT, layout::RESULT, layout::RESULT_BYTES); }

    pub fn restore_result(&mut self) { self.memory.copy(layout::RESULT, layout::RETAINED_RESULT, layout::RESULT_BYTES); }

    pub fn set_auxiliary_retained(&mut self, retained: bool) { self.set_word(layout::AUXILIARY_RETAINED, u32::from(retained)); }

    pub fn requires_eligibility(&mut self) -> bool { self.word(layout::ELIGIBILITY) != 0 }

    pub fn area_weight(&mut self, index: AreaIndex) -> u32 { self.word(layout::area_weight(index)) }

    /// The retained target halfword and its copy in the saved calculation input.
    pub fn retain_target_half(&mut self, half: u16) {
        for at in layout::RETAINED_TARGET_HALF { self.set_half(at, half); }
    }

    pub fn request_word(&mut self) -> u32 { self.word(layout::REQUEST_WORD) }

    /// The block's area count as the selection grid reads it (a byte).
    pub fn area_count(&mut self) -> u8 { self.byte(layout::AREA_COUNT) }

    /// The tracked kinds the grid walks: none unless the plan tracks.
    pub fn tracked_kinds(&mut self) -> u8 { if self.word(layout::TRACKING) == 0 { 0 } else { self.byte(layout::TRACKED_KINDS) } }

    pub fn grid_selected(&mut self) -> bool { self.word(layout::GRID_SELECTED) != 0 }

    /// The pass the selection mode names when a grid is selected; the plain pass otherwise.
    pub fn selected_pass(&mut self) -> ResultPass {
        if !self.grid_selected() { return ResultPass::Plain; }
        GridPass::new(self.word(layout::SELECTION_MODE)).map_or(ResultPass::Plain, ResultPass::Grid)
    }

    pub fn exposure_columns(&mut self) -> u8 { self.byte(layout::EXPOSURE_COLUMNS) }

    pub fn exposure_rows(&mut self) -> u8 { self.byte(layout::EXPOSURE_ROWS) }

    pub fn refinement(&mut self) -> u32 { self.word(layout::EXPOSURE_REFINEMENT) }

    pub fn exposure_lens_reference(&mut self) -> u32 { self.word(layout::EXPOSURE_LENS_REFERENCE) }

    pub fn exposure_target_word(&mut self) -> u32 { self.word(layout::EXPOSURE_TARGET_WORD) }

    pub fn exposure_focus_mode(&mut self) -> u32 { self.word(layout::EXPOSURE_FOCUS_MODE) }

    pub fn selection(&mut self) -> SelectionList { SelectionList::from_bytes(self.bytes(layout::SELECTION_LIST)) }

    pub fn set_selection(&mut self, list: &SelectionList) { self.memory.write(layout::SELECTION_LIST, list.bytes()); }

    pub fn saved_selection(&mut self) -> SelectionList { SelectionList::from_bytes(self.bytes(layout::SAVED_SELECTION)) }

    pub fn save_selection(&mut self) { self.memory.copy(layout::SAVED_SELECTION, layout::SELECTION_LIST, layout::SELECTION_BYTES); }

    /// Takes the saved list back, keeping the list's weights.
    pub fn load_saved_selection(&mut self) {
        let weights: [u8; layout::SELECTION_WEIGHT_BYTES] = self.bytes(layout::SELECTION_WEIGHTS);
        self.memory.copy(layout::SELECTION_LIST, layout::SAVED_SELECTION, layout::SELECTION_BYTES);
        self.memory.write(layout::SELECTION_WEIGHTS, &weights);
    }

    pub fn selection_fixed(&mut self) -> bool { self.word(layout::SELECTION_LIST) != 0 }

    pub fn selection_class(&mut self, slot: SelectionSlot) -> SelectionClass { SelectionClass(self.word(layout::class(slot))) }

    pub fn selection_cell(&mut self, slot: SelectionSlot) -> CellCode {
        let [column, row] = self.bytes(layout::cell(slot));
        CellCode { column, row }
    }

    pub fn set_selection_entry(&mut self, slot: SelectionSlot, class: SelectionClass, cell: CellCode) {
        self.set_word(layout::class(slot), class.0);
        self.memory.write(layout::cell(slot), &[cell.column, cell.row]);
    }

    pub fn area(&mut self, at: AreaAt) -> AreaResult { AreaResult::from_bytes(self.bytes(layout::area(at))) }

    pub fn set_area(&mut self, at: AreaAt, area: &AreaResult) { self.memory.write(layout::area(at), area.bytes()); }

    pub fn target(&mut self, kind: TrackedKind, pass: ResultPass, cell: GridCell, index: AreaIndex) -> AreaTarget {
        AreaTarget::from_bytes(self.bytes(layout::target(kind, pass, cell, index)))
    }

    pub fn set_target(&mut self, kind: TrackedKind, pass: ResultPass, cell: GridCell, index: AreaIndex, target: AreaTarget) {
        self.memory.write(layout::target(kind, pass, cell, index), &target.to_bytes());
    }

    pub fn target_gate(&mut self, kind: TrackedKind, pass: ResultPass, cell: GridCell, index: AreaIndex) -> bool {
        self.word(layout::target_gate(kind, pass, cell, index)) != 0
    }

    pub fn set_target_gate(&mut self, kind: TrackedKind, pass: ResultPass, cell: GridCell, index: AreaIndex, open: bool) {
        self.set_word(layout::target_gate(kind, pass, cell, index), u32::from(open));
    }

    pub fn set_marker(&mut self, pass: ResultPass, kind: Option<TrackedKind>, index: AreaIndex, marker: u32) { self.set_word(layout::marker(pass, kind, index), marker); }

    pub fn row_flags(&mut self, pass: ResultPass, cell: GridCell, index: AreaIndex) -> u8 { self.byte(layout::row(pass, cell, index)) }

    /// Sets bits of an area's plan word.
    pub fn mark_plan(&mut self, pass: ResultPass, cell: GridCell, index: AreaIndex, bits: u8) {
        let at = layout::plan_value(pass, cell, index);
        let word = self.word(at);
        self.set_word(at, word | u32::from(bits));
    }

    pub fn subject_kind(&mut self, stage: ClassifyStage, index: AreaIndex) -> Option<TrackedKind> {
        u8::try_from(self.word(layout::subject_kind(stage, index))).ok().and_then(TrackedKind::new)
    }

    pub fn set_subject_kind(&mut self, stage: ClassifyStage, index: AreaIndex, kind: Option<TrackedKind>) {
        self.set_word(layout::subject_kind(stage, index), kind.map_or(UNTRACKED, |kind| u32::from(kind.get())));
    }

    pub fn subject_level(&mut self, stage: ClassifyStage, index: AreaIndex) -> u64 { u64::from_le_bytes(self.bytes(layout::subject_level(stage, index))) }

    pub fn set_subject_level(&mut self, stage: ClassifyStage, index: AreaIndex, level: u64) { self.memory.write(layout::subject_level(stage, index), &level.to_le_bytes()); }

    pub fn subject_mode(&mut self, stage: ClassifyStage) -> u32 { self.word(layout::subject_mode(stage)) }

    pub fn set_subject_mode(&mut self, stage: ClassifyStage, mode: u32) { self.set_word(layout::subject_mode(stage), mode); }

    /// Copies another task's result buffer over the retained result.
    pub fn retain_result_from(&mut self, from: ResultBuffer) { self.memory.copy(layout::RETAINED_RESULT, layout::result_buffer(from), layout::RESULT_BYTES); }

    pub fn retained_source(&mut self) -> u32 { self.word(layout::RETAINED_SOURCE) }

    pub fn set_retained_source(&mut self, source: u32) { self.set_word(layout::RETAINED_SOURCE, source); }

    /// Replaces an area's reference and position in the retained result.
    pub fn set_retained_reference(&mut self, at: AreaAt, reference: u32, position: i16) {
        let at = layout::retained_area(at);
        let mut area = AreaResult::from_bytes(self.bytes(at));
        area.set_reference(reference, position);
        self.memory.write(at, area.bytes());
    }

    /// The target word the last finish left and the halfword the last result left.
    pub fn retained_targets(&mut self) -> (u32, u16) {
        let [word, _] = layout::RETAINED_TARGET_WORD;
        let [half, _] = layout::RETAINED_TARGET_HALF;
        (self.word(word), self.half(half))
    }

    pub fn retained_area(&mut self, at: AreaAt) -> AreaResult { AreaResult::from_bytes(self.bytes(layout::retained_area(at))) }

    pub fn saved_input(&mut self) -> ReferenceRequest { ReferenceRequest::from_bytes(self.bytes(layout::SAVED_INPUT)) }
}

/// The subject kind word of an area no tracked kind holds.
pub const UNTRACKED: u32 = 7;
