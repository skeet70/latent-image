#![forbid(unsafe_code)]

//! Production dispatch from the sequence steps to their work and external services.

use crate::absorb::{absorb, Origin};
use crate::boundary::{Completions, Diagnostics, LensReference, Replies, Trace};
use crate::calculation_work::CalculationWork;
use crate::calculations_work::CalculationsWork;
use crate::capture_boundary::CaptureParameters;
use crate::capture_control::CaptureControl;
use crate::exposure_work::ExposureWork;
use crate::finish_work::FinishWork;
use crate::lens_return::LensReturn;
use crate::pipeline_work::PipelineWork;
use crate::ports::*;
use crate::preparation::{record_lens, HalfPressParameters, LensStage, Prepared};
use crate::primary_job::{CaptureSteps, PrimaryJob};
use crate::secondary_job::SecondaryJob;
use crate::state::{ShotState, StateMemory};
use xm1_saclac_interface::statistics::SettingOverrides;
use xm1_saclac_interface::{BlockingRequest, Completed, Completion, RecordHandle};

/// The typed command blocks kept by the controller, not the completion being dispatched.
pub trait ShotCommands {
    fn half_press_parameters(&mut self) -> HalfPressParameters;
    fn live_lens_reference(&mut self) -> LensReference;
    fn capture_parameters(&mut self) -> CaptureParameters;
    fn setting_overrides(&mut self) -> SettingOverrides;
    fn capture_abort(&mut self) -> Option<AbortStatus>;
}

pub trait ShotServices {
    /// Calc 0x3c02 over the shot's calculation input and working result in `stock::calculation`.
    fn request_calculation(&mut self, completion: Completion);
    fn blocking(&mut self, request: BlockingRequest);
    fn fault(&mut self);
}

/// Only external traits belong here; shot-owned operations are on `ShotState`.
pub trait ExternalPorts:
    ShotCommands + ShotServices + crate::preparation::Boundary + crate::exposure_work::Boundary
    + crate::calculation_work::CalculationPorts + crate::pipeline_work::PipelineBoundary
    + crate::finish_work::FinishPorts + crate::grid_boundary::GridLayers + crate::record_load::LoadPorts
    + crate::capture_control::ControlBoundary + crate::lens_return::ReturnBoundary
    + crate::secondary_job::SecondaryBoundary + crate::primary_job::PrimaryBoundary
    + crate::calculations_work::CalculationsBoundary + crate::capture_dump::DumpBoundary
    + crate::statistics_dump::DumpBoundary
{}
impl<T> ExternalPorts for T where
    T: ShotCommands + ShotServices + crate::preparation::Boundary + crate::exposure_work::Boundary
    + crate::calculation_work::CalculationPorts + crate::pipeline_work::PipelineBoundary
    + crate::finish_work::FinishPorts + crate::grid_boundary::GridLayers + crate::record_load::LoadPorts
    + crate::capture_control::ControlBoundary + crate::lens_return::ReturnBoundary
    + crate::secondary_job::SecondaryBoundary + crate::primary_job::PrimaryBoundary
    + crate::calculations_work::CalculationsBoundary + crate::capture_dump::DumpBoundary
    + crate::statistics_dump::DumpBoundary
{}

pub struct ShotPorts<M, X> {
    state: ShotState<M>,
    prepared: Prepared,
    exposure: ExposureWork,
    calculation: CalculationWork,
    pipeline: PipelineWork,
    finish: FinishWork,
    control: CaptureControl,
    lens_return: LensReturn,
    secondary: SecondaryJob,
    primary: PrimaryJob,
    calculations: CalculationsWork,
    external: X,
    branch: Branch,
}

impl<M: StateMemory, X> ShotPorts<M, X> {
    pub fn new(memory: M, external: X) -> Self {
        Self {
            state: ShotState::new(memory), prepared: Prepared::default(), exposure: ExposureWork::default(),
            calculation: CalculationWork::new(), pipeline: PipelineWork::new(), finish: FinishWork::new(),
            control: CaptureControl::new(), lens_return: LensReturn::new(), secondary: SecondaryJob::new(),
            primary: PrimaryJob::new(), calculations: CalculationsWork::new(), external, branch: Branch::HalfPress,
        }
    }

    pub fn state(&mut self) -> &mut ShotState<M> { &mut self.state }
    pub fn external(&mut self) -> &mut X { &mut self.external }
    pub fn upload_done(&self) -> bool { self.pipeline.upload_done() }
    pub fn statistics_pending(&self) -> bool { self.pipeline.statistics_pending() }

    pub fn keep_correction(&mut self, correction: &xm1_saclac_interface::exposure::Correction) {
        self.exposure.keep_correction(correction);
    }

}

impl<M: StateMemory, X: ExternalPorts> ShotPorts<M, X> {
    fn sequence(&mut self, effect: SequenceEffect) {
        match effect {
            SequenceEffect::BindRecord(record) => self.prepared.bind(record),
            SequenceEffect::NoteLiveRecord(record) => self.prepared.note_live_record(record),
            SequenceEffect::PrepareRecords(branch) => {
                self.branch = branch;
                let parameters = match branch {
                    Branch::HalfPress => Some(self.external.half_press_parameters()),
                    Branch::Live => None,
                };
                let reference = match parameters {
                    Some(parameters) => parameters.lens_reference,
                    None => self.external.live_lens_reference(),
                };
                self.prepared.prepare(parameters, reference, &mut self.state, &mut self.external);
            }
            SequenceEffect::SetRefinementPass(pass) => self.prepared.set_refinement_pass(pass),
            SequenceEffect::RecordLensStatus => record_lens(LensStage::Conclusion, &mut self.state, &mut self.external),
            SequenceEffect::ReleaseLiveResources => self.prepared.release_live_resources(&mut self.external),
            SequenceEffect::Replied => Diagnostics::trace(&mut self.external, Trace::HalfPressReplied),
        }
    }

    fn calculation(&mut self, effect: CalculationEffect) {
        match effect {
            CalculationEffect::RequestLive(record) => {
                self.calculation.prepare_live(record, &mut self.state, &mut self.external);
                self.external.request_calculation(Completion::Calculation90);
            }
            CalculationEffect::Request => self.external.request_calculation(Completion::Calculation90),
            CalculationEffect::RecordSkipped => self.calculation.record_skipped(&mut self.state, &mut self.external),
            CalculationEffect::RecordResult => self.calculation.record_result(&mut self.state, &mut self.external),
            CalculationEffect::Absorb => {
                let origin = match self.branch {
                    Branch::HalfPress => Origin::HalfPress(self.prepared.parameters.unwrap_or_default()),
                    Branch::Live => Origin::Live,
                };
                let input = self.state.absorb_input(origin);
                absorb(&input, &mut self.state, &mut self.external);
            }
        }
    }

    fn capture(&mut self, effect: CaptureEffect) {
        match effect {
            CaptureEffect::BindRecord(record) => self.control.bind(record),
            CaptureEffect::RequestStatisticsSetup => {
                let parameters = self.external.capture_parameters();
                self.control.request_statistics_setup(&parameters, &mut self.external);
            }
            CaptureEffect::PrepareSetting => self.control.prepare_setting(&mut self.external),
            CaptureEffect::RequestSetting => {
                let overrides = self.external.setting_overrides();
                self.control.request_setting(self.primary.setting_template(), &overrides, &mut self.external);
            }
            CaptureEffect::CommitSetting => self.control.commit_setting(&mut self.external),
            CaptureEffect::PrepareRecord => self.control.prepare_record(&mut self.external),
            CaptureEffect::PrepareJob => {
                let parameters = self.external.capture_parameters();
                self.control.prepare_job(&parameters, &mut self.prepared, &mut self.state, &mut self.external);
            }
            CaptureEffect::ClearSecondary => self.control.clear_secondary(),
            CaptureEffect::Finalize => {
                let parameters = self.external.capture_parameters();
                self.control.finalize(&parameters, &self.primary, &mut self.state, &mut self.external);
            }
            CaptureEffect::Release => self.control.release(&mut self.external),
        }
    }
}

impl<M: StateMemory, X: ExternalPorts> Ports for ShotPorts<M, X> {
    fn effect(&mut self, effect: Effect) {
        match effect {
            Effect::Sequence(effect) => self.sequence(effect),
            Effect::Exposure(effect) => self.exposure.effect(effect, &self.prepared, &mut self.state, &mut self.external),
            Effect::Calculation(effect) => self.calculation(effect),
            Effect::Pipeline(effect) => self.pipeline.effect(effect, &mut self.prepared, &mut self.state, &mut self.external),
            Effect::Finish(effect) => self.finish.effect(effect, &self.prepared, &self.pipeline, &mut self.state, &mut self.external),
            Effect::Lens(lens) => match lens {
                LensMove::FinishRelative | LensMove::FinishAbsolute => self.finish.lens_move(lens, &self.prepared, &self.pipeline, &mut self.external),
                LensMove::CaptureRelative | LensMove::CaptureAbsolute => self.primary.lens_move(lens, &mut self.external),
                LensMove::ReturnRelative | LensMove::ReturnAbsolute => self.lens_return.lens_move(lens, &mut self.external),
            },
            Effect::Capture(effect) => self.capture(effect),
            Effect::Return(effect) => {
                let parameters = self.external.capture_parameters();
                self.lens_return.effect(effect, &parameters, &self.prepared.lens_return, &mut self.state, &mut self.external);
            }
            Effect::Secondary(effect) => {
                let parameters = self.external.capture_parameters();
                self.secondary.effect(effect, &parameters, self.state.secondary_mut(), &mut self.external);
            }
            Effect::Primary(effect) => {
                let parameters = self.external.capture_parameters();
                let steps = CaptureSteps { control: &self.control, secondary: self.state.secondary() };
                self.primary.effect(effect, &parameters, steps, &mut self.external);
            }
            Effect::Calculations(effect) => self.calculations.effect(effect, self.state.secondary().measurement(), &mut self.external),
        }
    }

    fn query(&mut self, query: Query) -> bool {
        match query {
            Query::HasWork => self.prepared.has_work(),
            Query::TwoPassExposure => self.prepared.two_pass_exposure(&mut self.external),
            Query::SkipLensSynchronization => self.prepared.skips_lens_synchronization(&mut self.external),
            Query::LensStatusPending => self.prepared.lens_status_pending(),
            Query::Exposure(query) => self.exposure.query(query, &mut self.external),
            Query::Pipeline(query) => self.pipeline.query(query, &mut self.external),
            Query::Finish(query) => self.finish.query(query, &self.pipeline, &mut self.external),
            Query::Capture(query) => {
                let parameters = self.external.capture_parameters();
                match query {
                    CaptureQuery::LensReturn => self.control.needs_lens_return(&parameters, &mut self.prepared.lens_return, &mut self.state, &mut self.external),
                    CaptureQuery::SecondaryJob => self.control.secondary_job(&parameters, &mut self.state, &mut self.external),
                }
            }
            Query::Return(query) => {
                let parameters = self.external.capture_parameters();
                self.lens_return.query(query, &mut self.prepared.lens_return, &parameters, &mut self.external)
            }
            Query::Secondary(query) => self.secondary.query(query, self.state.secondary(), &mut self.external),
            Query::Primary(query) => {
                let parameters = self.external.capture_parameters();
                self.primary.query(query, &parameters, &mut self.external)
            }
            Query::Calculations(query) => self.calculations.query(query, &mut self.external),
        }
    }

    fn half_press_mode(&mut self) -> HalfPressMode { self.external.half_press_parameters().mode }
    fn half_press_notifications(&mut self) -> HalfPressNotifications { self.external.half_press_parameters().mask.notifications() }
    fn calculation_plan(&mut self, record: RecordHandle) -> CalculationPlan {
        self.calculation.plan(record, &self.prepared.parameters.unwrap_or_default(), &mut self.state, &mut self.external)
    }
    fn next_evaluation(&mut self) -> EvaluationIndex { self.exposure.next_evaluation(&mut self.external) }
    fn capture_abort(&mut self) -> Option<AbortStatus> { self.external.capture_abort() }
    fn blocking(&mut self, request: BlockingRequest) { self.external.blocking(request); }
    fn reply(&mut self, reply: Reply) { Replies::reply(&mut self.external, reply); }
    fn self_complete(&mut self, completion: Completion) { Completions::self_complete(&mut self.external, completion); }
    fn receive(&mut self, completed: Completed<'_>) {
        self.exposure.receive(completed);
        self.pipeline.receive(completed, &mut self.external);
        self.control.receive(completed);
        self.secondary.receive(completed);
        self.primary.receive(completed);
        self.calculations.receive(completed);
    }
    fn fault(&mut self) { self.external.fault(); }
}
