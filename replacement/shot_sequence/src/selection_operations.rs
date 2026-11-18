#![forbid(unsafe_code)]

//! The shot's selection operations as the focus round and the controllers make them, over the
//! shot's state and the grid's layers.

use crate::absorb::CalculationResults;
use crate::classification::classify_subjects;
use crate::grid::{area_at, update_grid};
use crate::grid_boundary::{GridLayers, GridSettings};
use crate::selection::{record_list, ListSource};
use crate::state::{ShotState, StateMemory};
use xm1_saclac_interface::calculation::{AreaIndex, AreaResult, ReferenceRequest, ResultPass, TrackedKind};
use xm1_saclac_interface::selection::{CellCode, ClassifyStage, ResultBuffer, RetainedReference, SelectionList, SelectionSlot, ShotSelection, SubjectQuery, SubjectShare};
use xm1_saclac_interface::RecordHandle;

pub struct SelectionOperations<M, L> {
    state: ShotState<M>,
    layers: L,
}

impl<M: StateMemory, L: GridLayers> SelectionOperations<M, L> {
    pub const fn new(memory: M, layers: L) -> Self { Self { state: ShotState::new(memory), layers } }
}

/// The retained result keeps the auxiliary source and, for an untracked area, the auxiliary
/// reference and position (0xf1d5d4).
pub fn retain_reference<M: StateMemory>(retained: RetainedReference, state: &mut ShotState<M>) {
    state.set_retained_source(retained.source);
    if let Some(at) = retained.untracked { state.set_retained_reference(at, retained.reference, retained.position); }
}

/// The saved calculation input moved to `reference`: its positions shift by the offset between
/// the two references (0xf1fc5c).
pub fn reference_request<M: StateMemory>(reference: u32, state: &mut ShotState<M>, settings: &mut impl GridSettings) -> ReferenceRequest {
    let mut request = state.saved_input();
    let shift = settings.reference_offset(request.reference()).wrapping_sub(settings.reference_offset(reference));
    request.shift_positions(shift);
    request.set_reference(reference);
    request
}

impl<M: StateMemory, L: GridLayers> ShotSelection for SelectionOperations<M, L> {
    fn request_word(&mut self) -> u32 { self.state.request_word() }

    fn default_selection(&mut self, record: RecordHandle) -> SelectionList { record_list(record, ListSource::Record, &mut self.state, &mut self.layers) }

    fn saved_selection(&mut self) -> SelectionList { self.state.saved_selection() }

    fn mark_auxiliary_retained(&mut self, retained: bool) { self.state.set_auxiliary_retained(retained); }

    fn area(&mut self, kind: Option<TrackedKind>, pass: ResultPass, cell: CellCode, index: AreaIndex) -> AreaResult {
        self.state.area(area_at(kind, pass, cell.cell(), index))
    }

    fn selection_cell(&mut self, slot: SelectionSlot) -> CellCode { self.state.selection_cell(slot) }

    fn classify_subjects(&mut self, stage: ClassifyStage, share: SubjectShare, query: SubjectQuery) {
        classify_subjects(stage, share, query, &mut self.state, &mut self.layers);
    }

    fn subject_kind(&mut self, stage: ClassifyStage, index: AreaIndex) -> Option<TrackedKind> { self.state.subject_kind(stage, index) }

    fn update_grid(&mut self, record: RecordHandle, reset: bool, keep_gates: bool, lens_reference: u32) {
        update_grid(record, reset, keep_gates, lens_reference, &mut self.state, &mut self.layers);
    }

    fn restore_result(&mut self) { self.state.restore_result(); }

    fn retain_result(&mut self, from: ResultBuffer) { self.state.retain_result_from(from); }

    fn retain_reference(&mut self, retained: RetainedReference) { retain_reference(retained, &mut self.state); }

    fn reference_request(&mut self, reference: u32) -> ReferenceRequest { reference_request(reference, &mut self.state, &mut self.layers) }

    fn grid_selected(&mut self) -> bool { self.state.grid_selected() }

    fn selection_mode(&mut self) -> u32 { CalculationResults::selection_mode(&mut self.state) }

    fn tracking(&mut self) -> bool { CalculationResults::tracking(&mut self.state) }

    fn retained_targets(&mut self) -> (u32, u16) { self.state.retained_targets() }
}
