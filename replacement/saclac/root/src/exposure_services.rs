use core::cell::{Cell, RefCell};
use xm1_saclac_calc::{
    adjust::context_trace,
    diagnostics::{self, CorrectionExport},
    exposure::{context::{Binding, ExposureContext, Subject, SubjectList}, correction::{self, CorrectionPost}, evaluation, ports::{Placement, ProfileAccess}, prepare},
    policy::ExposureMode, record_policy,
    selection::{RangeLimits, RowIndex, RowUpdate, SelectionSettings},
};
use xm1_saclac_imaging::driver::Isp;
use xm1_saclac_interface::{calc::SharedAddress, exposure::{Correction, Evaluation, MeasuredEntry, STATISTICS_AREAS_BYTES}, stock::StockCallbackWord, Calc, Calc1, ImageProcessor, RecordHandle};
use xm1_saclac_profile_geometry::controller::Tables;
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_shared::stock::Machine;
use xm1_shot_sequence::{boundary::{CorrectionRequest, ExportKind, ExposureCalculation, ExposureLevels, Frame, SelectionKind}, exposure_work::ExposureSetup, ports::{EvaluationIndex, HalfPressMode}};
use crate::{acquisition::CalculationPeers, calculation_services::Calculation, services::Services};

/// The engine context and its selected measured row persist across shot events.
#[derive(Default)]
pub struct ExposureState {
    pub context: RefCell<ExposureContext>,
    selected: Cell<Option<RowIndex>>,
}

#[derive(Clone, Copy)]
pub struct Callbacks {
    pub evaluation: StockCallbackWord,
    pub correction: StockCallbackWord,
}

/// Peer transport is a view of the same poster, independent of the service-state borrow.
pub struct ExposureServices<'a, M, R, E, P, I, T, A, Q> {
    pub calculation: Calculation<'a, M, R, E, P, I, T, A>,
    pub state: &'a ExposureState,
    pub peers: &'a mut Q,
    pub callbacks: Callbacks,
}

fn binding(setup: &ExposureSetup) -> Binding {
    Binding {
        mask: setup.mask.0, mode: setup.mode.0, exposure_mode: ExposureMode(setup.selection.0),
        frame: setup.frame.map(|frame| SharedAddress(frame.0)),
        lens_reference: setup.lens_reference.0, source: setup.source, option: setup.option,
        snapshot_record: setup.record, record: setup.record,
        subjects: SubjectList { count: setup.subjects.count, entries: core::array::from_fn(|index| {
            // Calc's first word is native offset0, not the shot's identity at native+5c.
            let bytes = xm1_shot_sequence::stock::subject_bytes(&setup.subjects.entries[index]);
            Subject { id: u32::from_le_bytes(bytes[..4].try_into().expect("subject first word")), detail: bytes[4..].try_into().expect("subject remaining bytes") }
        }) },
        summary: setup.summary, refinement_pass: setup.refinement_pass, live: setup.live,
    }
}

impl<M: Machine, R, E, P> SelectionSettings for Services<M, R, E, P> {
    fn exact_range(&mut self) -> bool { self.machine.enter(0x69_f8fc, &[0xdf]) & 1 != 0 }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor, I: Isp, T: Tables, A: Placement, Q: Calc + Calc1>
    ExposureCalculation for ExposureServices<'_, M, R, E, P, I, T, A, Q> {
    fn selection(&mut self, record: RecordHandle, mode: HalfPressMode, refinement_pass: bool) -> SelectionKind {
        SelectionKind(record_policy::exposure_mode(&mut self.calculation, record, mode.0, refinement_pass).0)
    }
    fn selects_primary(&mut self, selection: SelectionKind) -> bool { ExposureMode(selection.0).is_primary() }
    fn selects_secondary(&mut self, selection: SelectionKind) -> bool { ExposureMode(selection.0).uses_adjustment() }
    fn prepare(&mut self, setup: &ExposureSetup) {
        self.state.selected.set(None);
        prepare::prepare(&mut self.calculation, &mut self.state.context.borrow_mut(), binding(setup));
    }
    fn calculation_mode(&mut self, record: RecordHandle) -> u32 { self.calculation.profile().chosen_word(record) }
    fn correction_mode(&mut self, record: RecordHandle, refinement_pass: bool) -> u32 { self.calculation.profile().selection_kind(record, refinement_pass) }
    fn scaled_statistics(&mut self) -> bool { false }
    fn evaluation_areas(&mut self) -> [u8; STATISTICS_AREAS_BYTES] {
        xm1_saclac_calc::stock::exposure::evaluation_areas(&self.state.context.borrow().areas)
    }
    fn evaluation_area_count(&mut self) -> u8 { self.state.context.borrow().selection.count }
    fn evaluated_count(&mut self) -> u8 { self.state.context.borrow().selection.evaluated_count() }
    fn select_measured(&mut self, entry: MeasuredEntry) { self.state.selected.set(RowIndex::new(entry.0)); }
    fn check_measured(&mut self, entry: MeasuredEntry) -> bool {
        let Some(row) = self.state.selected.get().filter(|row| row.get() == usize::from(entry.0)) else { return false };
        let view = evaluation::measured_row(self.calculation.placement, &self.state.context.borrow().selection, row);
        let buffers = view.buffers.map_or([0; 2], |buffers| buffers.map(|buffer| buffer.0));
        // f1f09c..f1f0e4 supplies this ordering to fea884/fec494. The latter returns 1/0.
        self.calculation.services.machine.enter(0xfe_c494, &[
            usize::from(entry.0), buffers[0] as usize, view.column_buffer.map_or(0, |buffer| buffer.0) as usize,
            buffers[1] as usize, view.pairs[0].0 as usize, view.regions.0 as usize,
            view.pairs[1].0 as usize, view.pairs[2].0 as usize, view.pairs[3].0 as usize,
            view.reply.0 as usize, view.statistics.0 as usize,
        ]) != 0
    }
    fn accept_measured(&mut self, entry: MeasuredEntry) {
        let row = RowIndex::new(entry.0).expect("accepted exposure row");
        self.state.context.borrow_mut().selection.enable_row(row.get());
    }
    fn next_evaluation(&mut self) -> EvaluationIndex {
        self.state.context.borrow().selection.open_row().map_or(EvaluationIndex::FINAL, |row| EvaluationIndex(row as u8))
    }
    fn request_evaluation(&mut self, index: EvaluationIndex) {
        let row = RowIndex::new(index.0).expect("requested exposure row");
        let mut machine = self.calculation.services.machine.clone();
        let mut peers = CalculationPeers { machine: &mut machine, peers: self.peers };
        evaluation::request_row(&mut self.calculation, &mut peers, &mut self.state.context.borrow_mut(), row, self.callbacks.evaluation).expect("exposure evaluation post");
    }
    fn absorb_evaluation(&mut self, evaluation: &Evaluation) {
        let row = RowIndex::new(evaluation.entry).expect("completed exposure row");
        self.state.context.borrow_mut().selection.apply(RowUpdate {
            row: row.get(),
            first: RangeLimits { lowest: evaluation.levels[0] != 0, highest: evaluation.levels[1] != 0 },
            second: RangeLimits { lowest: evaluation.levels[2] != 0, highest: evaluation.levels[3] != 0 },
        });
    }
    fn correction_needed(&mut self) -> bool {
        self.state.context.borrow().selection.range_is_valid(self.calculation.services)
    }
    fn request_correction(&mut self, setup: &ExposureSetup) -> CorrectionRequest {
        let mut context = self.state.context.borrow_mut();
        if setup.mode.0 == 7 { context.binding = binding(setup); }
        let mut machine = self.calculation.services.machine.clone();
        let mut peers = CalculationPeers { machine: &mut machine, peers: self.peers };
        match correction::post(&mut self.calculation, &mut peers, &mut context, self.callbacks.correction).expect("exposure correction post") {
            CorrectionPost::Posted => CorrectionRequest::Sent,
            CorrectionPost::Skipped => CorrectionRequest::Skipped,
        }
    }
    fn levels(&mut self) -> ExposureLevels {
        let context = self.state.context.borrow();
        ExposureLevels { base: context.levels.target, reference: context.levels.start as u16 }
    }
    fn alternate(&mut self) -> bool { self.state.context.borrow().outcome.treated }
    fn variant(&mut self) -> u32 { u32::from(self.state.context.borrow().outcome.first_treatment) }
    fn exporting(&mut self) -> bool { self.state.context.borrow().outcome.focus_reused }
    fn export(&mut self, frame: Option<Frame>, correction: &Correction, kind: ExportKind) {
        let context = self.state.context.borrow();
        diagnostics::export_correction(&mut self.calculation, frame.map(|frame| SharedAddress(frame.0)), correction, &CorrectionExport {
            first_treatment: context.outcome.first_treatment,
            subjects: context.snapshot.focus.subject_count as u16,
            clear_samples: kind != ExportKind::Plain,
        });
    }
    fn clear_selection(&mut self, frame: Option<Frame>, primary: bool) {
        diagnostics::clear_selection(&mut self.calculation, frame.map(|frame| SharedAddress(frame.0)), primary);
    }
    fn finish_primary(&mut self) { context_trace::trace(&mut self.calculation, &self.state.context.borrow()); }
}
