#![forbid(unsafe_code)]

use core::cell::RefCell;
use xm1_saclac_calc::exposure::ports::Placement;
use xm1_saclac_imaging::driver::Isp;
use xm1_saclac_interface::{Blocking, BlockingRequest, Calc, Calc1, Command, CommandKind, Completion, ImageProcessor, RecordHandle, exposure::{Correction, Evaluation, MeasuredEntry, STATISTICS_AREAS_BYTES}, statistics::SettingOverrides, stock::StockCallbackWord};
use xm1_saclac_profile_geometry::controller::{ProfileGeometry, Tables};
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_shared::stock::Machine;
use xm1_shot_sequence::{boundary::{Client, ClientRef, CorrectionRequest, ExportKind, ExposureCalculation, ExposureLevels, Frame, LensPoint, LensReference}, capture_boundary::CaptureParameters, exposure_work::ExposureSetup, ports::{AbortStatus, EvaluationIndex, HalfPressMode}, preparation::{FrameHeader, HalfPressParameters}, shot_ports::{ShotCommands, ShotServices}};
use crate::{calculation_services::Calculation, command_source::CommandSource, exposure_services::{ExposureServices, ExposureState}, geometry::GeometryBindings, services::Services, stock::geometry::Memory as GeometryMemory};

/// The record argument of the current Shot entry, not another record store.
pub trait BindRecord { fn bind_record(&mut self, record: RecordHandle); }

#[derive(Clone, Copy)]
pub struct Callbacks<'a> {
    pub exposure: crate::exposure_services::Callbacks,
    pub calculation: StockCallbackWord,
    pub statistics: StockCallbackWord,
    pub capture_selection_hook: StockCallbackWord,
    pub capture_calculations: CaptureCalculationCallbacks,
    pub moves: LensMoveCallbacks,
    pub capture_statistics: CaptureStatisticsCallbacks,
    pub measurement: MeasurementCallbacks,
    pub capture_image: CaptureImageCallbacks,
    pub pipeline: PipelineCallbacks<'a>,
}

#[derive(Clone, Copy)]
pub struct CaptureCalculationCallbacks {
    pub first: StockCallbackWord,
    pub second: StockCallbackWord,
    pub third: StockCallbackWord,
    pub fourth: StockCallbackWord,
    /// Evaluation94, then refinement95; each native callback takes an84-byte result pointer.
    pub area_evaluation: [StockCallbackWord; 2],
}

#[derive(Clone, Copy)]
pub struct LensMoveCallbacks {
    pub finish_relative: StockCallbackWord,
    pub finish_absolute: StockCallbackWord,
    pub capture_relative: StockCallbackWord,
    pub capture_absolute: StockCallbackWord,
}

#[derive(Clone, Copy)]
pub struct CaptureStatisticsCallbacks {
    pub setup: StockCallbackWord,
    pub setting: StockCallbackWord,
    pub primary_setting: StockCallbackWord,
}

#[derive(Clone, Copy)]
pub struct MeasurementCallbacks {
    pub finished: StockCallbackWord,
    pub ready: StockCallbackWord,
    pub transferred: StockCallbackWord,
}

#[derive(Clone, Copy)]
pub struct CaptureImageCallbacks {
    pub end: StockCallbackWord,
    pub outputs: StockCallbackWord,
    pub readout: StockCallbackWord,
    pub delivery: StockCallbackWord,
    pub complete: StockCallbackWord,
    pub notice: StockCallbackWord,
    pub command_c: StockCallbackWord,
    pub command_28: StockCallbackWord,
}

#[derive(Clone, Copy)]
pub struct PipelineCallbacks<'a> {
    pub raw_calculated: StockCallbackWord,
    pub tone_calculated: StockCallbackWord,
    pub gradation_calculated: StockCallbackWord,
    pub raw_checked: StockCallbackWord,
    pub frame_prepared: StockCallbackWord,
    pub tone_progress: StockCallbackWord,
    pub tone_finished: StockCallbackWord,
    pub output: StockCallbackWord,
    pub commit: StockCallbackWord,
    pub statistics: StockCallbackWord,
    pub statistics_commit: StockCallbackWord,
    pub statistics_progress: StockCallbackWord,
    pub progress_gate: &'a crate::statistics_results::ReportingGate,
}

/// Persistent library capabilities over the same configuration, records and exposure context.
/// `peers` is another handle to the same queues; it holds no model state and permits posting
/// without borrowing the Services owner a second time.
pub struct Libraries<'a, M, R, E, P, I, T, A, Q> {
    pub services: &'a RefCell<Services<M, R, E, P>>,
    pub geometry: &'a RefCell<ProfileGeometry<GeometryMemory<M>, GeometryBindings<M, P, I>, T>>,
    pub recognition: &'a RefCell<xm1_saclac_calc::scene_recognition::SceneRecognition>,
    pub placement: &'a RefCell<A>,
    pub exposure: &'a ExposureState,
    pub peers: Q,
    pub record: RecordHandle,
    /// LiveSearch's current record (native16c21fc), not the current Shot argument.
    pub live_search_record: &'a xm1_saclac_search_bindings::RecordBinding,
    pub subjects: &'a RefCell<crate::shot_subjects::Workspace>,
    pub capture: &'a RefCell<crate::shot_capture_workspace::Workspace>,
    pub motion: &'a RefCell<Option<xm1_saclac_calc::adjust::motion::state::MotionState>>,
    pub callbacks: Callbacks<'a>,
}
impl<M, R, E, P, I, T, A, Q> BindRecord for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn bind_record(&mut self, record: RecordHandle) { self.record = record; }
}
impl<M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, A, Q> Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn machine(&self) -> M { self.services.borrow().machine.clone() }
}
impl<M: CommandSource + Clone, R: Memory, E: StockEntries, P, I, T, A, Q> Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn command<U>(&self, kind: CommandKind, body: impl FnOnce(Command<'_>) -> U) -> U {
        self.machine().with_command(kind, body)
    }
}
impl<M: CommandSource + Clone, R: Memory, E: StockEntries, P, I, T, A, Q> ShotCommands for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn half_press_parameters(&mut self) -> HalfPressParameters {
        self.command(CommandKind::HalfPress, |command| xm1_shot_sequence::stock::half_press_parameters(command.parameters.bytes()).expect("stored HalfPress block"))
    }
    fn live_lens_reference(&mut self) -> LensReference {
        self.command(CommandKind::Command02, |command| xm1_shot_sequence::stock::live_lens_reference(command.parameters.bytes()).expect("stored Command02 block"))
    }
    fn capture_parameters(&mut self) -> CaptureParameters {
        self.command(CommandKind::Command07, |command| xm1_shot_sequence::capture_stock::capture_parameters(command.parameters.bytes()).expect("stored Command07 block"))
    }
    fn setting_overrides(&mut self) -> SettingOverrides {
        let machine = self.machine();
        self.command(CommandKind::Command08, |command| {
            let parameters = xm1_shot_sequence::capture_stock::setting_parameters(command.parameters.bytes()).expect("stored Command08 block");
            xm1_shot_sequence::capture_stock::setting_overrides(&parameters, |address, bytes| {
                for (index, byte) in bytes.iter_mut().enumerate() { *byte = machine.read_u8(address.0.wrapping_add(index as u32)); }
            })
        })
    }
    fn capture_abort(&mut self) -> Option<AbortStatus> {
        xm1_shot_sequence::stock::abort_status(self.machine().read_u32(crate::stock::CURRENT_COMPLETION_STATUS))
    }
}
impl<M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, A, Q: Calc + Calc1 + Blocking> ShotServices for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn request_calculation(&mut self, completion: Completion) {
        assert_eq!(completion, Completion::Calculation90);
        let mut machine = self.machine();
        let mut peers = crate::acquisition::CalculationPeers { machine: &mut machine, peers: &mut self.peers };
        assert!(xm1_saclac_calc::requests::Peers::calc(&mut peers, &xm1_shot_sequence::stock::calculation::request(self.callbacks.calculation)).is_ok(), "saclac calculation enqueue failed");
    }
    fn blocking(&mut self, request: BlockingRequest) {
        assert!(Blocking::execute(&mut self.peers, request).is_ok(), "saclac blocking rendezvous failed");
    }
    fn fault(&mut self) { self.machine().enter(crate::stock::FAULT, &[]); }
}
impl<M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, A, Q> Client for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn area_count(&mut self, area_count: ClientRef) -> u32 { self.machine().read_u32(area_count.0) }
    fn area(&mut self, areas: ClientRef, index: usize) -> u32 { self.machine().read_u32(areas.0.wrapping_add(index as u32 * 4)) }
    fn write_lens_target(&mut self, target: ClientRef, point: LensPoint, step: u16, distance: u32) {
        let mut machine = self.machine();
        xm1_shot_sequence::stock::lens_target_words(point, step, distance, |offset, bytes| {
            for (index, &byte) in bytes.iter().enumerate() { machine.write_u8(target.0.wrapping_add(offset).wrapping_add(index as u32), byte); }
        });
    }
    fn frame(&mut self, slot: ClientRef) -> Option<Frame> { let address = self.machine().read_u32(slot.0); (address != 0).then_some(Frame(address)) }
    fn clear_frame_progress(&mut self, slot: ClientRef) {
        let mut machine = self.machine();
        let frame = machine.read_u32(slot.0);
        machine.write_u32(frame.wrapping_add(xm1_shot_sequence::stock::FRAME_PROGRESS), 0);
    }
    fn announce_frame(&mut self, frame: Frame, header: FrameHeader) {
        let mut machine = self.machine();
        for (offset, word) in xm1_shot_sequence::stock::frame_header_words(header) { machine.write_u32(frame.0.wrapping_add(offset), word); }
        machine.write_u32(crate::stock::CALC_FRAME_HEADER_SOURCE, frame.0.wrapping_add(xm1_shot_sequence::stock::FRAME_HEADER));
    }
    fn write_summary(&mut self, frame: Frame, summary: &xm1_shot_sequence::summary::FrameSummary) {
        let mut machine = self.machine();
        xm1_shot_sequence::stock::summary_words(summary, |offset, word| machine.write_u32(frame.0.wrapping_add(xm1_shot_sequence::stock::FRAME_SUMMARY).wrapping_add(offset), word));
    }
}
impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor, I: Isp, T: Tables, A: Placement, Q: Calc + Calc1> Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn exposure<U>(&mut self, body: impl FnOnce(&mut ExposureServices<'_, M, R, E, P, I, T, A, Q>) -> U) -> U {
        body(&mut ExposureServices {
            calculation: Calculation { services: &mut self.services.borrow_mut(), geometry: &mut self.geometry.borrow_mut(), placement: &mut self.placement.borrow_mut() },
            state: self.exposure, peers: &mut self.peers, callbacks: self.callbacks.exposure,
        })
    }
    pub fn build_calculation_input(&mut self, area: [u32; 2], plan: xm1_saclac_calc::exposure::calculation::PlanInput, selection: bool) {
        let input = xm1_saclac_calc::exposure::calculation::build_input(
            &mut Calculation { services: &mut self.services.borrow_mut(), geometry: &mut self.geometry.borrow_mut(), placement: &mut *self.placement.borrow_mut() },
            &mut self.exposure.context.borrow_mut(), area, plan, selection,
        );
        crate::shot_input::store(&mut self.machine(), &input);
    }
}
macro_rules! exposure {
    ($(fn $name:ident(&mut self $(, $argument:ident: $type:ty)*) $(-> $result:ty)?;)*) => {
        impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor, I: Isp, T: Tables, A: Placement, Q: Calc + Calc1>
            ExposureCalculation for Libraries<'_, M, R, E, P, I, T, A, Q> {
            $(fn $name(&mut self $(, $argument: $type)*) $(-> $result)? { self.exposure(|view| view.$name($($argument),*)) })*
        }
    };
}
exposure! {
    fn selection(&mut self, record: RecordHandle, mode: HalfPressMode, refinement_pass: bool) -> xm1_shot_sequence::boundary::SelectionKind;
    fn selects_primary(&mut self, selection: xm1_shot_sequence::boundary::SelectionKind) -> bool;
    fn selects_secondary(&mut self, selection: xm1_shot_sequence::boundary::SelectionKind) -> bool;
    fn prepare(&mut self, setup: &ExposureSetup);
    fn calculation_mode(&mut self, record: RecordHandle) -> u32;
    fn correction_mode(&mut self, record: RecordHandle, refinement_pass: bool) -> u32;
    fn scaled_statistics(&mut self) -> bool;
    fn evaluation_areas(&mut self) -> [u8; STATISTICS_AREAS_BYTES];
    fn evaluation_area_count(&mut self) -> u8;
    fn evaluated_count(&mut self) -> u8;
    fn select_measured(&mut self, entry: MeasuredEntry);
    fn check_measured(&mut self, entry: MeasuredEntry) -> bool;
    fn accept_measured(&mut self, entry: MeasuredEntry);
    fn next_evaluation(&mut self) -> EvaluationIndex;
    fn request_evaluation(&mut self, index: EvaluationIndex);
    fn absorb_evaluation(&mut self, evaluation: &Evaluation);
    fn correction_needed(&mut self) -> bool;
    fn request_correction(&mut self, setup: &ExposureSetup) -> CorrectionRequest;
    fn levels(&mut self) -> ExposureLevels;
    fn alternate(&mut self) -> bool;
    fn variant(&mut self) -> u32;
    fn exporting(&mut self) -> bool;
    fn export(&mut self, frame: Option<Frame>, correction: &Correction, kind: ExportKind);
    fn clear_selection(&mut self, frame: Option<Frame>, primary: bool);
    fn finish_primary(&mut self);
}

impl<M, R, E, P, I, T, A, Q: xm1_shot_sequence::boundary::Completions> xm1_shot_sequence::boundary::Completions for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn self_complete(&mut self, completion: Completion) {
        xm1_shot_sequence::boundary::Completions::self_complete(&mut self.peers, completion);
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor, I: Isp, T: Tables, A: Placement, Q: Calc + Calc1>
    xm1_shot_sequence::calculation_work::CalculationLibrary for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn selection_mode(&mut self, record: RecordHandle, refinement: u32) -> u32 {
        use xm1_saclac_calc::exposure::ports::ProfileAccess;
        self.services.borrow_mut().profile().selection_kind(record, refinement != 0)
    }
    fn row_data(&mut self, kind: u32, mode: u32, evaluation: u32, limit: u32, option: u32) -> u32 {
        use xm1_saclac_profile_records::{IntervalTracking, MeasurementSelection};
        // f60c04/10/20 compare full words before narrowing their predicates.
        let query = xm1_saclac_calc::record_policy::RowDataQuery {
            selection: if kind == 1 { MeasurementSelection::Record } else { MeasurementSelection::Unselected },
            kind: xm1_saclac_calc::policy::SelectionKind(mode), evaluation,
            layout: if matches!(limit, 4 | 5) { limit as u8 } else { 0 },
            tracking: IntervalTracking(u8::from(option != 0)),
        };
        u32::from(xm1_saclac_calc::record_policy::row_data_allowed(&mut *self.services.borrow_mut(), &query))
    }
    fn adjustment_status(&mut self, kind: u32, mode: u32, option: u32) -> u8 {
        xm1_saclac_calc::record_policy::adjustment_status(
            u8::from(kind == 1), xm1_saclac_calc::policy::SelectionKind(mode), u8::from(option != 0),
        ).code()
    }
    fn class_skips_selection(&mut self, class: u32) -> bool {
        xm1_saclac_calc::policy::ExposureMode(class).uses_primary_limit()
    }
    fn display_class(&mut self, record: RecordHandle, refinement: u32) -> u32 {
        use xm1_saclac_calc::exposure::ports::ProfileAccess;
        u32::from(self.services.borrow_mut().profile().selection_style(record, refinement != 0))
    }
    fn selection_policy(&mut self, kind: u32, display: u32, evaluation: u32, option: u32) -> u32 {
        u32::from(xm1_saclac_calc::policy::SelectionKind(display).qualifies(kind == 1, evaluation == 1, option != 0))
    }
    fn build_input(&mut self, area: [u32; 2], selection: u32, kind: u32, exposure: [u16; 5]) {
        self.build_calculation_input(area, xm1_saclac_calc::exposure::calculation::PlanInput { kind, exposure }, selection != 0);
    }
    fn class_reuses_result(&mut self, class: u32) -> bool {
        xm1_saclac_calc::policy::ExposureMode(class).uses_adjustment()
    }
}
