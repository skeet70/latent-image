#![forbid(unsafe_code)]

//! The shot's state where other tasks read it: the calculation block with its selection grid,
//! the retained and set-aside results, the shot context and the saved calculation input.
//! `ShotState` reads and writes it through `StateMemory`; `stock::state` places each value.

use crate::absorb::{AuxiliaryOutcome, CalculationResults};
use crate::stock::state as layout;
use xm1_saclac_interface::calculation::{AreaAt, AreaIndex, AreaResult, AreaRow, CellSummary, FocusSummary, GridCell, GridPass, ResultPass, TrackedKind};

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

/// The shot's state over its memory.
pub struct ShotState<M> {
    memory: M,
}

impl<M: StateMemory> ShotState<M> {
    pub const fn new(memory: M) -> Self { Self { memory } }

    pub fn memory(&mut self) -> &mut M { &mut self.memory }

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

    fn area_kind(&mut self, index: AreaIndex) -> u8 { self.byte(layout::area_kind(index)) }

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
        let [column, row] = self.bytes(layout::cell(kind, pass, index));
        GridCell::new(column, row).unwrap_or_default()
    }

    pub fn set_cell_of(&mut self, kind: Option<TrackedKind>, pass: ResultPass, index: AreaIndex, cell: GridCell) {
        self.memory.write(layout::cell(kind, pass, index), &[cell.column, cell.row]);
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
}
