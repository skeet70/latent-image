use crate::boundary::*;
use crate::exposure_work::ExposureSetup;
use crate::leaf_fakes::Fake;
use crate::ports::{EvaluationIndex, HalfPressMode, Reply};
use crate::subjects::SubjectList;
use std::vec::Vec;
use xm1_saclac_interface::exposure::{Correction, Evaluation, MeasuredEntry, StatisticsRequest, STATISTICS_AREAS_BYTES};
use xm1_saclac_interface::{Completion, RecordHandle};

#[derive(Clone, Debug, PartialEq)]
pub enum ExposureCall {
    Prepare(ExposureSetup),
    SelectMeasured(u8),
    CheckMeasured(u8),
    AcceptMeasured(u8),
    RequestEvaluation(u8),
    AbsorbEvaluation(Evaluation),
    RequestCorrection,
    Export(Option<Frame>, Correction, ExportKind),
    ClearSelection(Option<Frame>, bool),
    FinishPrimary,
    StoreExposure(u8, i32),
    StoreOffset(ExposureOffset, i16),
    StoreArea(usize, i16),
    StoreMetering(MeteringBlock),
    StartRecording,
    RecordEvaluations(u8),
    RequestStatistics(StatisticsRequest, SubjectList),
    ReleaseStatistics,
    Reply(Reply),
    SelfComplete(Completion),
}

#[derive(Debug, Default)]
pub struct ExposureWorld {
    pub calls: Vec<ExposureCall>,
    pub selection: SelectionKind,
    pub primary: bool,
    pub secondary: bool,
    pub area_count: u8,
    pub evaluated: u8,
    pub accepted: bool,
    pub next: u8,
    pub correction_needed: bool,
    pub skip_correction: bool,
    pub levels: ExposureLevels,
    pub alternate: bool,
    pub variant: u32,
    pub exporting: bool,
    pub lens_offset: u16,
    pub calibration: u16,
    pub export_last: bool,
    pub recording: bool,
    pub metering: u8,
    pub metering_source: u8,
}

impl ExposureWorld {
    pub fn stored_exposure(&self) -> Vec<(u8, i32)> {
        self.calls.iter().filter_map(|call| match call { ExposureCall::StoreExposure(slot, value) => Some((*slot, *value)), _ => None }).collect()
    }

    pub fn has(&self, wanted: &ExposureCall) -> bool { self.calls.contains(wanted) }

    pub fn position(&self, wanted: impl Fn(&ExposureCall) -> bool) -> Option<usize> { self.calls.iter().position(wanted) }
}

impl ExposureCalculation for Fake {
    fn selection(&mut self, _record: RecordHandle, mode: HalfPressMode, refinement_pass: bool) -> SelectionKind {
        SelectionKind(self.exposure.selection.0 + mode.0 * 10 + u32::from(refinement_pass) * 100)
    }
    fn selects_primary(&mut self, _selection: SelectionKind) -> bool { self.exposure.primary }
    fn selects_secondary(&mut self, _selection: SelectionKind) -> bool { self.exposure.secondary }
    fn prepare(&mut self, setup: &ExposureSetup) { self.exposure.calls.push(ExposureCall::Prepare(*setup)); }
    fn calculation_mode(&mut self, _record: RecordHandle) -> u32 { 0x21 }
    fn correction_mode(&mut self, _record: RecordHandle, refinement_pass: bool) -> u32 { 0x30 + u32::from(refinement_pass) }
    fn scaled_statistics(&mut self) -> bool { false }
    fn evaluation_areas(&mut self) -> [u8; STATISTICS_AREAS_BYTES] { [0x5a; STATISTICS_AREAS_BYTES] }
    fn evaluation_area_count(&mut self) -> u8 { self.exposure.area_count }
    fn evaluated_count(&mut self) -> u8 { self.exposure.evaluated }
    fn select_measured(&mut self, entry: MeasuredEntry) { self.exposure.calls.push(ExposureCall::SelectMeasured(entry.0)); }
    fn check_measured(&mut self, entry: MeasuredEntry) -> bool {
        self.exposure.calls.push(ExposureCall::CheckMeasured(entry.0));
        self.exposure.accepted
    }
    fn accept_measured(&mut self, entry: MeasuredEntry) { self.exposure.calls.push(ExposureCall::AcceptMeasured(entry.0)); }
    fn next_evaluation(&mut self) -> EvaluationIndex { EvaluationIndex(self.exposure.next) }
    fn request_evaluation(&mut self, index: EvaluationIndex) { self.exposure.calls.push(ExposureCall::RequestEvaluation(index.0)); }
    fn absorb_evaluation(&mut self, evaluation: &Evaluation) { self.exposure.calls.push(ExposureCall::AbsorbEvaluation(*evaluation)); }
    fn correction_needed(&mut self) -> bool { self.exposure.correction_needed }
    fn request_correction(&mut self, _setup: &ExposureSetup) -> CorrectionRequest {
        self.exposure.calls.push(ExposureCall::RequestCorrection);
        if self.exposure.skip_correction { CorrectionRequest::Skipped } else { CorrectionRequest::Sent }
    }
    fn levels(&mut self) -> ExposureLevels { self.exposure.levels }
    fn alternate(&mut self) -> bool { self.exposure.alternate }
    fn variant(&mut self) -> u32 { self.exposure.variant }
    fn exporting(&mut self) -> bool { self.exposure.exporting }
    fn export(&mut self, frame: Option<Frame>, correction: &Correction, kind: ExportKind) {
        self.exposure.calls.push(ExposureCall::Export(frame, *correction, kind));
    }
    fn clear_selection(&mut self, frame: Option<Frame>, primary: bool) { self.exposure.calls.push(ExposureCall::ClearSelection(frame, primary)); }
    fn finish_primary(&mut self) { self.exposure.calls.push(ExposureCall::FinishPrimary); }
}

impl ExposureRecord for Fake {
    fn store_exposure(&mut self, slot: ExposureSlot, value: i32) { self.exposure.calls.push(ExposureCall::StoreExposure(slot.0, value)); }
    fn store_offset(&mut self, offset: ExposureOffset, value: i16) { self.exposure.calls.push(ExposureCall::StoreOffset(offset, value)); }
    fn store_area_exposure(&mut self, index: usize, value: i16) { self.exposure.calls.push(ExposureCall::StoreArea(index, value)); }
    fn metering(&mut self) -> MeteringBlock { MeteringBlock([self.exposure.metering; METERING_BYTES]) }
    fn metering_source(&mut self) -> [u8; METERING_SOURCE_BYTES] { [self.exposure.metering_source; METERING_SOURCE_BYTES] }
    fn store_metering(&mut self, block: &MeteringBlock) { self.exposure.calls.push(ExposureCall::StoreMetering(*block)); }
}

impl ExposureServices for Fake {
    fn lens_offset(&mut self, lens_reference: LensReference) -> u16 { self.exposure.lens_offset + lens_reference.0 as u16 }
    fn calibration_offset(&mut self) -> u16 { self.exposure.calibration }
    fn exports_last_correction(&mut self) -> bool { self.exposure.export_last }
}

impl Recorder for Fake {
    fn recording(&mut self, _mode: RecorderMode) -> bool { self.exposure.recording }
    fn start_recording(&mut self, _mode: RecorderMode) { self.exposure.calls.push(ExposureCall::StartRecording); }
    fn record_evaluations(&mut self, count: u8) { self.exposure.calls.push(ExposureCall::RecordEvaluations(count)); }
}

impl StatisticsPort for Fake {
    fn request_statistics(&mut self, request: &StatisticsRequest, subjects: &SubjectList) {
        self.exposure.calls.push(ExposureCall::RequestStatistics(*request, *subjects));
    }
    fn release_statistics(&mut self) { self.exposure.calls.push(ExposureCall::ReleaseStatistics); }
}

impl Replies for Fake {
    fn reply(&mut self, reply: Reply) { self.exposure.calls.push(ExposureCall::Reply(reply)); }
}

impl Completions for Fake {
    fn self_complete(&mut self, completion: Completion) { self.exposure.calls.push(ExposureCall::SelfComplete(completion)); }
}
