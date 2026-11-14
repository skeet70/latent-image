#![forbid(unsafe_code)]

//! The shot's operations over its state: the exposure context and calculation block the
//! calculation step binds, the finish's words and record load, the shot context words a
//! preparation writes, and the selection grid the absorb and the record load use.

use crate::absorb::{absorb, store_areas, Absorb, AbsorbPorts, AreaTarget, AuxiliaryReference, CalculationResults, Form, Origin, SelectionGrid};
use crate::boundary::{Client, ClientRef, LensReference, SelectionKind};
use crate::calculation_work::{CalculationBlock, CalculationContext, CalculationProfiles, ExposureWord, PlanWords};
use crate::capture_boundary::AreaReference;
use crate::capture_control::ControlShot;
use crate::capture_dump::DumpShot;
use crate::capture_stock::subject_share;
use crate::classification::classify_subjects;
use crate::dump_boundary::ShotWord;
use crate::finish_work::FinishCalculation;
use crate::grid::{selected_cell, update_grid};
use crate::grid_boundary::{GridLayers, GridSettings};
use crate::lens_return::ReturnShot;
use crate::preparation::{HalfPressParameters, LensReadings, LensStage, PreparationShot};
use crate::record_load::{load, LoadOrigin, LoadPorts, RecordLoad};
use crate::secondary_job::SecondaryResult;
use crate::selection::{areas, record_list, ListSource};
use crate::selection_operations::{reference_request, retain_reference};
use crate::state::{ShotState, StateMemory};
use crate::statistics_dump::StatisticsDump;
use crate::stock::{self, state as layout};
use crate::subjects::SubjectList;
use xm1_saclac_interface::calculation::{AreaIndex, GridCell, GridPass, ReferenceRequest, ResultPass};
use xm1_saclac_interface::selection::{ClassifyStage, RetainedReference, SubjectQuery};
use xm1_saclac_interface::stock::{record_handle_from_a2, record_handle_word};
use xm1_saclac_interface::RecordHandle;

/// The half-press mode whose preparation carries the first carried word into the eligibility word.
const CARRYING_MODE: u32 = 7;

impl<M: StateMemory> ShotState<M> {
    /// The record the calculation block is bound to.
    pub fn bound_record(&mut self) -> RecordHandle { record_handle_from_a2(self.word(layout::BLOCK)) }

    /// The lens reference the preparation bound.
    pub fn lens_reference(&mut self) -> LensReference { LensReference(self.word(layout::REQUEST_WORD)) }

    pub fn exposure(&mut self, word: ExposureWord) -> u32 { self.word(layout::exposure_word(word)) }

    /// What the absorb reads from the exposure context and the calculation block.
    pub fn absorb_input(&mut self, origin: Origin) -> Absorb {
        Absorb {
            origin,
            record: self.bound_record(),
            exposure_record: record_handle_from_a2(self.word(layout::EXPOSURE_RECORD[1])),
            selection: SelectionKind(self.exposure(ExposureWord::Class)),
            lens_reference: LensReference(self.word(layout::EXPOSURE_LENS_REFERENCE)),
            refinement: self.exposure(ExposureWord::Refinement),
            repeat: self.exposure(ExposureWord::Repeat) != 0,
            live: origin == Origin::Live,
        }
    }

    /// What the record load reads from the exposure context and the calculation block.
    pub fn load_input(&mut self, origin: LoadOrigin) -> RecordLoad {
        RecordLoad { origin, record: self.bound_record(), refinement: self.exposure(ExposureWord::Refinement), repeat: self.exposure(ExposureWord::Repeat) != 0 }
    }

    /// The shot context's lens reference, lens readings and lens movement flags.
    pub fn shot_word(&mut self, word: ShotWord) -> u32 { self.word(layout::lens_reading(word)) }

    /// 0xf16160.
    pub fn set_lens_notice(&mut self, notice: bool) { self.set_word(layout::LENS_NOTICE, u32::from(notice)); }

    /// Reads and clears the request a mode-7 half press left for the capture's lens return
    /// (0xf17b1c, 0xf2a458).
    pub fn take_return_request(&mut self) -> bool {
        let requested = self.word(layout::AUXILIARY_RETAINED) != 0;
        self.set_word(layout::AUXILIARY_RETAINED, 0);
        requested
    }

    /// 0xf1fd60.
    pub fn set_return_active(&mut self, active: bool) { self.set_word(layout::RETURN_ACTIVE, u32::from(active)); }

    pub fn return_active(&mut self) -> bool { self.word(layout::RETURN_ACTIVE) == 1 }

    /// The retained result into the lens return's result, then that into the working result
    /// (0xf2efe8).
    pub fn restore_return_result(&mut self) {
        self.memory().copy(layout::RETURN_RESULT, layout::RETAINED_RESULT, layout::RESULT_BYTES);
        self.install_return_result();
    }

    /// The lens return's result into the working result (0xf1d680).
    pub fn install_return_result(&mut self) { self.memory().copy(layout::RESULT, layout::RETURN_RESULT, layout::RESULT_BYTES); }

    /// Areas whose kind in the capture job's bank differs from the first bank's (0xf1dbb0..0xf1dbf8).
    pub fn moved_areas(&mut self) -> u8 {
        let count = self.area_count();
        let mut moved = 0u8;
        for index in areas(count) {
            let kept = self.word(layout::subject_kind(ClassifyStage::Absorb, index));
            if kept != self.word(layout::subject_kind(ClassifyStage::CaptureJob, index)) { moved = moved.wrapping_add(1); }
        }
        moved
    }

    /// The candidate list the capture's subject refresh copies for the primary subjects or the
    /// others.
    pub fn subject_source(&mut self, primary: bool) -> SubjectList { stock::subject_list(&self.bytes(layout::subject_source(primary))) }

    /// The carried words at the end of a preparation in `mode` (0xf177bc..0xf17824).
    pub fn carry_words(&mut self, mode: Option<u32>) {
        if mode == Some(CARRYING_MODE) {
            let carried = self.word(layout::CARRIED);
            self.set_word(layout::ELIGIBILITY, carried);
        } else {
            self.set_word(layout::ELIGIBILITY, 0);
            if matches!(mode, Some(0 | 5)) { self.set_word(layout::AUXILIARY_RETAINED, 0); }
        }
        self.set_word(layout::CARRIED, 0);
    }
}

impl<M: StateMemory> LensReadings for ShotState<M> {
    /// 0xf161f0.
    fn begin_lens_track(&mut self, reading: u32) {
        for word in [ShotWord::StartReference, ShotWord::SecondReference, ShotWord::ThirdReference] { self.set_word(layout::lens_reading(word), layout::UNREAD_LENS); }
        for word in [ShotWord::Moved, ShotWord::MovedLate] { self.set_word(layout::lens_reading(word), 0); }
        if reading != self.lens_reference().0 { self.set_word(layout::lens_reading(ShotWord::Moved), 1); }
        self.set_word(layout::lens_reading(ShotWord::StartReference), reading);
    }

    /// 0xf1626c for the pipeline, 0xf162a8 for the conclusion.
    fn record_lens(&mut self, stage: LensStage, reading: u32, changed: bool) {
        let (at, flag) = match stage {
            LensStage::Pipeline => (ShotWord::SecondReference, ShotWord::Moved),
            LensStage::Conclusion => (ShotWord::ThirdReference, ShotWord::MovedLate),
        };
        if changed { self.set_word(layout::lens_reading(flag), 1); }
        self.set_word(layout::lens_reading(at), reading);
    }

    fn set_lens_notice(&mut self, notice: bool) { ShotState::set_lens_notice(self, notice); }
}

impl<M: StateMemory, P: GridLayers + Client + CalculationProfiles + crate::statistics_dump::DumpBoundary> PreparationShot<P> for ShotState<M> {
    fn lens_reference(&mut self) -> LensReference { ShotState::lens_reference(self) }

    /// 0xf160ac.
    fn bind_lens_reference(&mut self, reference: LensReference) { self.set_word(layout::REQUEST_WORD, reference.0); }

    /// 0xf162e4 into the list at +0x1cec; the client's area count block holds the count and the
    /// address of the classes.
    fn prepare_selection(&mut self, record: RecordHandle, area_count: Option<ClientRef>, live: bool, ports: &mut P) {
        let source = if live {
            ListSource::Live
        } else {
            let [count, classes] = area_count.map_or([0; 2], |at| ports.client_pair(at));
            let count = count as u8;
            let classes = core::array::from_fn(|index| if index < usize::from(count) { ports.area(ClientRef(classes), index) } else { 0 });
            ListSource::Client { count, classes }
        };
        let list = record_list(record, source, self, ports);
        self.set_selection(&list);
    }

    fn dump_statistics(&mut self, frame: crate::boundary::Frame, first_capture: bool, ports: &mut P) {
        self.frame_dump().write(frame, first_capture, ports);
    }

    fn carry(&mut self, mode: Option<u32>) { self.carry_words(mode); }
}

impl<M: StateMemory, P: GridLayers> SelectionGrid<P> for ShotState<M> {
    fn cell(&mut self, pass: ResultPass, index: AreaIndex) -> GridCell { selected_cell(pass, index, self).cell() }

    fn update_grid(&mut self, record: RecordHandle, reset: bool, keep_gates: bool, lens_reference: LensReference, layers: &mut P) {
        update_grid(record, reset, keep_gates, lens_reference.0, self, layers);
    }

    fn classify_subjects(&mut self, stage: ClassifyStage, reference: &AreaReference, kind: u32, source: u32, area: u32, layers: &mut P) {
        classify_subjects(stage, subject_share(reference), SubjectQuery { kind, source, area }, self, layers);
    }

    fn retain_result(&mut self) { ShotState::retain_result(self); }

    fn restore_result(&mut self) { ShotState::restore_result(self); }

    fn retain_auxiliary(&mut self, auxiliary: &AuxiliaryReference, untracked: Option<xm1_saclac_interface::calculation::AreaAt>) {
        retain_reference(RetainedReference { source: auxiliary.source, reference: auxiliary.target, position: auxiliary.normalized, untracked }, self);
    }

    fn mark_auxiliary_retained(&mut self) { self.set_auxiliary_retained(true); }

    fn requires_eligibility(&mut self) -> bool { ShotState::requires_eligibility(self) }

    fn area_weight(&mut self, index: AreaIndex) -> u32 { ShotState::area_weight(self, index) }

    fn retain_target_half(&mut self, half: u16) { ShotState::retain_target_half(self, half); }

    fn retain_target_word(&mut self, word: u32) { ShotState::retain_target_word(self, word); }
}

impl<M: StateMemory, P: GridSettings> CalculationContext<P> for ShotState<M> {
    fn exposure(&mut self, word: ExposureWord) -> u32 { ShotState::exposure(self, word) }

    fn bind_exposure(&mut self, record: RecordHandle, lens_reference: u32, option: u32) {
        for at in layout::EXPOSURE_RECORD { self.set_word(at, record_handle_word(record)); }
        self.set_word(layout::EXPOSURE_LENS_REFERENCE, lens_reference);
        self.set_word(layout::EXPOSURE_OPTION, option);
    }

    fn clear_repeat(&mut self) { self.set_word(layout::exposure_word(ExposureWord::Repeat), 0); }

    fn retained_targets(&mut self) -> (u32, u16) { ShotState::retained_targets(self) }

    fn set_input_targets(&mut self, word: u32, half: u16) {
        self.set_word(layout::INPUT_TARGET_WORD, word);
        self.set_half(layout::INPUT_TARGET_HALF, half);
    }

    fn save_input(&mut self) { self.memory().copy(layout::SAVED_INPUT, layout::INPUT, layout::INPUT_BYTES); }

    fn resume_input(&mut self, lens_reference: u32, layers: &mut P) {
        let request = reference_request(lens_reference, self, layers);
        self.memory().write(layout::INPUT, request.bytes());
    }

    fn set_resume_offsets(&mut self, lead: u16, lens_word: u32, offset: u16) {
        self.set_half(layout::INPUT_RESUME_LEAD, lead);
        self.set_word(layout::INPUT_RESUME_LENS_WORD, lens_word);
        self.set_half(layout::INPUT_RESUME_OFFSET, offset);
    }
}

impl<M: StateMemory> CalculationBlock for ShotState<M> {
    fn bind_calculation(&mut self, record: RecordHandle) { self.set_word(layout::BLOCK, record_handle_word(record)); }

    fn reset_plan(&mut self, plan: PlanWords) { self.memory().write(layout::PLAN, &stock::calculation::plan_bytes(plan)); }

    fn set_selection(&mut self, selection: u32) { self.set_word(layout::GRID_SELECTED, selection); }

    fn mark_requested(&mut self) { self.set_word(layout::SELECTION_MODE, stock::calculation::REQUESTED); }

    fn restore_result(&mut self) { ShotState::restore_result(self); }

    fn retain_result(&mut self) { ShotState::retain_result(self); }

    fn result_word(&mut self) -> u32 { self.word(layout::RESULT_WORD) }
    fn input_plan(&mut self) -> (u32, [u16; 5]) {
        let kind = self.word(layout::SELECTION_LIST);
        let bytes: [u8; 10] = self.bytes(layout::SELECTION_EXPOSURES);
        (kind, core::array::from_fn(|index| u16::from_le_bytes([bytes[2 * index], bytes[2 * index + 1]])))
    }
}

impl<M: StateMemory, P: LoadPorts + GridLayers> FinishCalculation<P> for ShotState<M> {
    fn calculation_record(&mut self) -> RecordHandle { self.bound_record() }

    fn calculation_areas(&mut self) -> u8 { self.area_count() }

    fn calculation_selection(&mut self) -> u32 { self.word(layout::GRID_SELECTED) }

    fn selection_mode(&mut self) -> u32 { self.word(layout::SELECTION_MODE) }

    fn set_selection_mode(&mut self, mode: u32) { self.set_word(layout::SELECTION_MODE, mode); }

    fn exposure_held(&mut self) -> bool { self.return_active() }

    fn set_saved_selection_mode(&mut self, mode: u32) { self.set_word(layout::SAVED_SELECTION_MODE, mode); }

    fn lens_reference(&mut self) -> LensReference { ShotState::lens_reference(self) }

    fn set_lens_notice(&mut self, notice: bool) { ShotState::set_lens_notice(self, notice); }

    /// 0xf19634 for the half press.
    fn load_finish_record(&mut self, parameters: &HalfPressParameters, ports: &mut P) {
        let input = self.load_input(LoadOrigin::HalfPress(*parameters));
        load(&input, self, ports);
    }
}

impl<M: StateMemory, P: AbsorbPorts + GridLayers> ControlShot<P> for ShotState<M> {
    fn take_return_request(&mut self) -> bool { ShotState::take_return_request(self) }

    fn secondary(&mut self) -> &mut SecondaryResult { self.secondary_mut() }

    /// The tracking word 0xf1db30 tests.
    fn tracks_subjects(&mut self) -> bool { CalculationResults::tracking(self) }

    fn classify_subjects(&mut self, reference: &AreaReference, kind: u32, source: u32, area: u32, ports: &mut P) {
        SelectionGrid::classify_subjects(self, ClassifyStage::CaptureJob, reference, kind, source, area, ports);
    }

    fn moved_areas(&mut self) -> u8 { ShotState::moved_areas(self) }

    /// 0xf1d770(record, 3, 0, 1, 0), then with a grid selected 0xf19204 with the same arguments
    /// over the selection mode's pass.
    fn update_capture_selection(&mut self, record: RecordHandle, ports: &mut P) {
        let target = AreaTarget { record, mode: 0, skip_first: false, bank: ClassifyStage::CaptureJob, eligibility: true };
        store_areas(Form::Short, ResultPass::Plain, target, self, ports);
        if !self.grid_selected() { return; }
        if let Some(pass) = GridPass::new(CalculationResults::selection_mode(self)) { store_areas(Form::Short, ResultPass::Grid(pass), target, self, ports); }
    }

    fn subject_source(&mut self, primary: bool) -> SubjectList { ShotState::subject_source(self, primary) }

    fn frame_dump(&mut self) -> &mut StatisticsDump { ShotState::frame_dump(self) }
}

impl<M: StateMemory, P: LoadPorts + GridLayers> ReturnShot<P> for ShotState<M> {
    fn reference_request(&mut self, reference: u32, ports: &mut P) -> ReferenceRequest { reference_request(reference, self, ports) }

    fn set_return_active(&mut self, active: bool) { ShotState::set_return_active(self, active); }

    fn restore_return_result(&mut self) { ShotState::restore_return_result(self); }

    fn absorb_return_result(&mut self, pending: bool, ports: &mut P) {
        self.install_return_result();
        let input = self.absorb_input(Origin::Return { pending });
        absorb(&input, self, ports);
    }

    fn load_return_records(&mut self, pending: bool, ports: &mut P) {
        let input = self.load_input(LoadOrigin::Return { pending });
        load(&input, self, ports);
    }
}

impl<M: StateMemory> DumpShot for ShotState<M> {
    fn shot_word(&mut self, word: ShotWord) -> u32 { ShotState::shot_word(self, word) }
}
