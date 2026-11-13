#![forbid(unsafe_code)]

//! Leaf work of the exposure step: binding the exposure to the record, the statistics and
//! calculation requests, the results their completions deliver, and the conclusion that writes the
//! corrected exposure into the record.

use crate::boundary::{
    Client, Completions, CorrectionRequest, Diagnostics, ExportKind, ExposureCalculation, ExposureOffset, ExposureRecord, ExposureServices,
    ExposureSlot, Frame,
    LensReference, Recorder, RecorderMode, Records, Replies, SelectionKind, Setting, StatisticsPort, Trace,
};
use crate::ports::{Branch, EvaluationIndex, ExposureEffect, ExposureQuery, HalfPressMode, Reply};
use crate::preparation::{HalfPressMask, Prepared};
use crate::stock::exposure as stock;
use crate::subjects::SubjectList;
use xm1_saclac_interface::exposure::{Correction, Evaluation, MeasuredEntry, ReleaseStatus, StatisticsRequest};
use xm1_saclac_interface::stock::exposure as wire;
use xm1_saclac_interface::{CommandKind, Completed, Completion, CompletionStatus, RecordHandle};

pub const SUMMARY_WORDS: usize = 10;

/// What the exposure binds before the calculation library prepares its tables.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExposureSetup {
    pub record: Option<RecordHandle>,
    pub live: bool,
    pub mask: HalfPressMask,
    pub mode: HalfPressMode,
    pub selection: SelectionKind,
    pub lens_reference: LensReference,
    pub source: u32,
    pub option: u32,
    pub refinement_pass: bool,
    pub subjects: SubjectList,
    pub summary: [u32; SUMMARY_WORDS],
    pub frame: Option<Frame>,
}

pub trait Boundary:
    Records + Client + Diagnostics + ExposureCalculation + ExposureRecord + ExposureServices + Recorder + StatisticsPort + Replies + Completions
{
}
impl<T> Boundary for T where
    T: Records + Client + Diagnostics + ExposureCalculation + ExposureRecord + ExposureServices + Recorder + StatisticsPort + Replies + Completions
{
}

/// The exposure step's state between its events.
#[derive(Debug, Default, Eq, PartialEq)]
pub struct ExposureWork {
    pub setup: ExposureSetup,
    measured: MeasuredEntry,
    checked: MeasuredEntry,
    release: ReleaseStatus,
    evaluation: Evaluation,
    correction: Correction,
    last_correction: Correction,
    area_values: [i16; 10],
    return_mode: u32,
}

impl ExposureWork {
    /// Records the result a completion delivers.
    pub fn receive(&mut self, completed: Completed<'_>) {
        let payload = completed.parameters.bytes();
        match completed.completion {
            Completion::Image4d => {
                if let Some(entry) = wire::measured(payload) { self.measured = entry; }
            }
            Completion::Image4c => {
                if let Some(status) = wire::release(payload) { self.release = status; }
            }
            Completion::Calculation8e => {
                if let Some(evaluation) = wire::evaluation(payload) { self.evaluation = evaluation; }
            }
            Completion::Calculation8f => {
                if let Some(correction) = wire::correction(payload) { self.last_correction = correction; }
                self.correction = self.last_correction;
            }
            _ => {}
        }
    }

    pub fn correction(&self) -> &Correction { &self.correction }

    /// Exposure values the conclusion derived for the ten areas.
    pub fn area_values(&self) -> [i16; 10] { self.area_values }

    pub fn return_mode(&self) -> u32 { self.return_mode }

    pub fn set_return_mode(&mut self, mode: u32) { self.return_mode = mode; }

    pub fn effect(&mut self, effect: ExposureEffect, prepared: &mut Prepared, boundary: &mut impl Boundary) {
        match effect {
            ExposureEffect::Begin { branch: Branch::HalfPress, record } => self.begin_half_press(record, prepared, boundary),
            ExposureEffect::Begin { branch: Branch::Live, record } => self.begin_live(record, prepared, boundary),
            ExposureEffect::RequestEvaluation(index) => {
                boundary.trace(Trace::EvaluationRequested);
                boundary.request_evaluation(index);
            }
            ExposureEffect::RequestCorrection => self.request_correction(boundary),
            ExposureEffect::RequestStatistics => self.request_statistics(boundary),
            ExposureEffect::RequestStatisticsRelease => boundary.release_statistics(),
            ExposureEffect::AbsorbStatistics => boundary.select_measured(self.measured),
            ExposureEffect::AbsorbStatisticsResult => boundary.accept_measured(self.checked),
            ExposureEffect::AbsorbCalculation => boundary.absorb_evaluation(&self.evaluation),
            ExposureEffect::Conclude => self.conclude(prepared, boundary),
        }
    }

    pub fn query(&mut self, query: ExposureQuery, boundary: &mut impl Boundary) -> bool {
        match query {
            ExposureQuery::CorrectionFirst => self.correction_first(boundary),
            ExposureQuery::StatisticsAccepted => {
                self.checked = self.measured;
                boundary.check_measured(self.measured)
            }
            ExposureQuery::CorrectionNeeded => boundary.correction_needed(),
            ExposureQuery::ReleaseStatusAtMostOne => self.release.0 <= 1,
        }
    }

    pub fn next_evaluation(&mut self, boundary: &mut impl Boundary) -> EvaluationIndex { boundary.next_evaluation() }

    fn begin_half_press(&mut self, record: RecordHandle, prepared: &Prepared, boundary: &mut impl Boundary) {
        boundary.trace(Trace::ExposureBegun);
        let parameters = prepared.parameters.unwrap_or_default();
        let refinement_pass = prepared.refinement_pass;
        let mut summary = [0; SUMMARY_WORDS];
        summary.copy_from_slice(&prepared.candidates.summary.0[..SUMMARY_WORDS]);
        let mut setup = ExposureSetup {
            record: Some(record),
            live: false,
            mask: parameters.mask,
            mode: parameters.mode,
            selection: boundary.selection(record, parameters.mode, refinement_pass),
            lens_reference: prepared.lens_reference,
            source: boundary.setting(Setting::ExposureSource),
            option: parameters.option,
            refinement_pass,
            subjects: prepared.candidates.lists[0],
            summary,
            frame: None,
        };
        self.return_mode = 0;
        if parameters.mode.0 != 7 {
            setup.frame = parameters.frame_slot.and_then(|slot| boundary.frame(slot));
            self.setup = setup;
            boundary.prepare(&self.setup);
        } else {
            self.setup = setup;
        }
    }

    fn begin_live(&mut self, record: RecordHandle, prepared: &Prepared, boundary: &mut impl Boundary) {
        boundary.trace(Trace::ExposureBegun);
        self.setup = ExposureSetup {
            record: Some(record),
            live: true,
            mask: HalfPressMask(1),
            mode: HalfPressMode(4),
            selection: SelectionKind(0),
            lens_reference: prepared.lens_reference,
            source: 0,
            option: 1,
            refinement_pass: true,
            subjects: SubjectList::default(),
            summary: [0; SUMMARY_WORDS],
            frame: None,
        };
        boundary.prepare(&self.setup);
    }

    fn correction_first(&self, boundary: &mut impl Boundary) -> bool {
        let mask = self.setup.mask;
        mask.publishes_area_selection()
            || (mask.publishes_areas() && self.setup.mode.0 == 1)
            || !boundary.selects_primary(self.setup.selection)
            || boundary.evaluation_area_count() == 0
    }

    fn request_correction(&mut self, boundary: &mut impl Boundary) {
        boundary.trace(Trace::CorrectionRequested);
        if boundary.request_correction(&self.setup) == CorrectionRequest::Skipped { boundary.self_complete(Completion::Calculation8f); }
    }

    fn request_statistics(&mut self, boundary: &mut impl Boundary) {
        if boundary.recording(RecorderMode::Exposure) { boundary.start_recording(RecorderMode::Exposure); }
        let record = self.setup.record;
        let mut area_layout = boundary.setting(Setting::AreaLayout);
        let mut single_area = false;
        if !self.setup.live {
            let mode = self.setup.mode.0;
            if mode == 2 { area_layout = 0x0003_0002; }
            single_area = boundary.setting(Setting::CaptureVariant) == 1 && mode == 1 && self.setup.mask.publishes_areas();
        }
        let request = StatisticsRequest {
            lens_reference: self.setup.lens_reference.0,
            calculation_mode: record.map_or(0, |record| boundary.calculation_mode(record)),
            subject_query: [boundary.setting(Setting::SubjectQueryA), boundary.setting(Setting::SubjectQueryB)],
            area_layout,
            context_mode: boundary.setting(Setting::ContextMode),
            correction_mode: record.map_or(0, |record| boundary.correction_mode(record, self.setup.refinement_pass)),
            statistics_option: boundary.setting(Setting::StatisticsOption),
            single_area,
            areas: boundary.evaluation_areas(),
            area_count: boundary.evaluation_area_count(),
            scaled: boundary.scaled_statistics(),
        };
        boundary.request_statistics(&request, &self.setup.subjects);
    }

    fn conclude(&mut self, prepared: &mut Prepared, boundary: &mut impl Boundary) {
        if boundary.recording(RecorderMode::Exposure) {
            let evaluated = boundary.evaluated_count();
            if evaluated != 0 { boundary.record_evaluations(evaluated); }
        }
        let frame = self.setup.frame;
        let selection = self.setup.selection;
        if boundary.exporting() && !boundary.selects_secondary(selection) {
            let kind = match boundary.setting(Setting::CorrectionExport) {
                1 => ExportKind::First,
                2 => ExportKind::Second,
                _ => ExportKind::Plain,
            };
            if boundary.exports_last_correction() {
                let last = self.last_correction;
                boundary.export(frame, &last, kind);
            } else {
                boundary.export(frame, &self.correction, kind);
                boundary.clear_selection(frame, false);
            }
        }
        if !boundary.exporting() && boundary.selects_primary(selection) { boundary.clear_selection(frame, true); }
        stock::plan_take_exposure(&mut prepared.selection_plan, &self.correction);
        if !self.setup.live && self.setup.mode.0 != 7 && boundary.selects_primary(selection) { boundary.finish_primary(); }
        self.store_record(boundary);
        if let (false, Some(record)) = (self.setup.live, self.setup.record) {
            boundary.reply(Reply { command: CommandKind::HalfPress, record: Some(record), status: CompletionStatus(3) });
        }
        boundary.trace(Trace::ExposureConcluded);
    }

    /// Writes the correction into the record's exposure table, relative to the lens and the
    /// calculation's reference level.
    fn store_record(&mut self, boundary: &mut impl Boundary) {
        let lens = boundary.lens_offset(self.setup.lens_reference);
        let variant = boundary.variant();
        stock::select_corrected_level(&mut self.correction, variant != 0);
        let levels = boundary.levels();
        let relative = |value: u16| lens.wrapping_add(value).wrapping_sub(levels.reference) as i16;
        for (slot, value) in stock::corrected_slots(&self.correction) {
            boundary.store_exposure(slot, i32::from(relative(value)));
        }
        boundary.store_exposure(ExposureSlot(20), i32::from(levels.base));
        boundary.store_exposure(ExposureSlot(21), i32::from(levels.reference.wrapping_sub(levels.base as u16) as i16));
        boundary.store_exposure(ExposureSlot(22), self.setup.lens_reference.0 as i32);
        boundary.store_exposure(ExposureSlot(23), 0);
        let offsets = stock::correction_offsets(&self.correction);
        let calibrated = |value: u16, calibration: u16| relative(value).wrapping_add(calibration as i16);
        if boundary.alternate() {
            boundary.store_exposure(ExposureSlot(13), i32::from(relative(offsets.alternate)));
            boundary.store_exposure(ExposureSlot(24), variant as i32);
            let calibration = boundary.calibration_offset();
            boundary.store_offset(ExposureOffset::First, calibrated(offsets.first, calibration));
            let calibration = boundary.calibration_offset();
            boundary.store_offset(ExposureOffset::Second, calibrated(offsets.alternate, calibration));
            boundary.store_offset(ExposureOffset::Third, offsets.third);
            for (index, value) in stock::correction_areas(&self.correction).into_iter().enumerate() {
                let value = relative(value);
                boundary.store_area_exposure(index, value);
                self.area_values[index] = value;
            }
        } else {
            boundary.store_exposure(ExposureSlot(13), 0);
            boundary.store_exposure(ExposureSlot(24), variant as i32);
            let calibration = boundary.calibration_offset();
            boundary.store_offset(ExposureOffset::First, calibrated(offsets.first, calibration));
            boundary.store_offset(ExposureOffset::Second, 0);
            boundary.store_offset(ExposureOffset::Third, 0);
            for index in 0..self.area_values.len() {
                boundary.store_area_exposure(index, 0);
                self.area_values[index] = 0;
            }
        }
        let metering = boundary.metering();
        boundary.store_metering(&metering);
    }
}
