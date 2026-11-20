use core::cell::RefCell;
use xm1_focus_measurement_round::{
    acquisition::{ports::*, selection::SelectionMemory}, auxiliary,
    history::MeasurementHistory, image, live::{Effects as LiveEffects, Measurement},
    lanes::Selection,
    pipeline::{Exposure, GainStep, HistoryStatistics, Pipeline, PipelineSnapshot, PipelineWord, Statistics, TimingWord},
    planner::{Cadence, Mode, Work},
    profile::{FrameInterval, FrameRate, GridCell, IntervalHistory, Profile, ProfileGeometry as RecordGeometry, ProfileRecords, ProfileSelection, PublishedResult, RetainedWord, StatisticsInUse, WindowPosition, WindowRecord, WindowStatus},
    record::{Adjustment, AdjustmentWords, AlternateSearch, Power, Request as RecordRequest, Settings, Trace, TraceMark},
    replies::{Effect, Effects}, round::{LaneSignal, LaneStatus}, source, window,
};
use xm1_saclac_calc::scene_recognition::SceneRecognition;
use xm1_saclac_controllers::{children::{ChildEvent, ChildOutcome, MeasurementControl}, search::SearchTerminal};
use xm1_saclac_focus_search::{contrast::Context, live::{self, LiveSearch}};
use xm1_saclac_imaging::driver::Isp;
use xm1_saclac_interface::{calc::SharedAddress, round::GeometryCalibration, round_acquisition::*, stock::{round_acquisition::GridLead, StockCallbackWord}, Calc, Calc1, Completed, ImageProcessor, ImgDc, RecordHandle, Statistics as StatisticsPeer};
use xm1_saclac_profile_geometry::controller::{ProfileGeometry, Tables};
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_search_bindings::RecordBinding;
use xm1_saclac_shared::stock::Machine;
use xm1_shot_sequence::{grid_boundary::GridLayers, shot_ports::ShotPorts, state::StateMemory};
use crate::{geometry::GeometryBindings, round_ports::RoundPorts, search_runtime::SearchSession, services::Services, stock::geometry::Memory as GeometryMemory};

type Geometry<M, P, I, T> = ProfileGeometry<GeometryMemory<M>, GeometryBindings<M, P, I>, T>;

/// The outer caller retains a strategy terminal result without mistaking it for stop completion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Dispatch {
    pub outcome: ChildOutcome,
    pub terminal: Option<SearchTerminal>,
}

pub struct MeasurementRuntime<'s, M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, S, X> {
    services: &'s RefCell<Services<M, R, E, P>>,
    geometry: &'s RefCell<Geometry<M, P, I, T>>,
    recognition: &'s RefCell<SceneRecognition>,
    shot: &'s RefCell<ShotPorts<S, X>>,
    session: SearchSession<'s, M, R, E, P>,
    measurement: Measurement,
    search: LiveSearch<SearchSession<'s, M, R, E, P>>,
}

impl<'s, M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor + Calc + Calc1 + ImgDc + StatisticsPeer, I: Isp, T: Tables, S, X>
    MeasurementRuntime<'s, M, R, E, P, I, T, S, X> {
    pub fn new(services: &'s RefCell<Services<M, R, E, P>>, geometry: &'s RefCell<Geometry<M, P, I, T>>, recognition: &'s RefCell<SceneRecognition>, record: &'s RecordBinding, shot: &'s RefCell<ShotPorts<S, X>>) -> Self {
        let session = SearchSession::new(services, record);
        Self { services, geometry, recognition, shot, session, measurement: Measurement::new(), search: LiveSearch::new(session, Context::new()) }
    }

    pub fn handle(&mut self, event: ChildEvent<'_, MeasurementControl>) -> Dispatch where S: StateMemory, X: GridLayers {
        let mut effects = MeasurementEffects { services: self.services, geometry: self.geometry, recognition: self.recognition, shot: self.shot, session: self.session, search: &mut self.search, auxiliary_buffer: None, terminal: None };
        let outcome = self.measurement.handle(event, &mut effects);
        Dispatch { outcome, terminal: effects.terminal }
    }

    pub fn history(&self) -> &MeasurementHistory { self.measurement.history() }

    pub(crate) fn handle_s1_acquisition(&self, child: &mut xm1_focus_measurement_round::children::S1Acquisition, event: ChildEvent<'_, xm1_saclac_controllers::children::AcquisitionControl>) -> ChildOutcome where S: StateMemory, X: GridLayers {
        child.handle(event, &mut crate::acquisition::Acquisition::new(self.services, self.recognition, self.shot))
    }
}

struct MeasurementEffects<'a, 's, M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, S, X> {
    services: &'s RefCell<Services<M, R, E, P>>,
    geometry: &'s RefCell<Geometry<M, P, I, T>>,
    recognition: &'s RefCell<SceneRecognition>,
    shot: &'s RefCell<ShotPorts<S, X>>,
    session: SearchSession<'s, M, R, E, P>,
    search: &'a mut LiveSearch<SearchSession<'s, M, R, E, P>>,
    auxiliary_buffer: Option<u32>,
    terminal: Option<SearchTerminal>,
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, S, X> MeasurementEffects<'_, '_, M, R, E, P, I, T, S, X> {
    fn with_services<V>(&mut self, operation: impl FnOnce(&mut Services<M, R, E, P>) -> V) -> V {
        operation(&mut self.services.borrow_mut())
    }
    fn with_acquisition<V>(&mut self, operation: impl FnOnce(&mut crate::acquisition::Acquisition<'_, M, R, E, P, S, X>) -> V) -> V {
        operation(&mut crate::acquisition::Acquisition::new(self.services, self.recognition, self.shot))
    }
    fn with_round<V>(&mut self, operation: impl FnOnce(&mut RoundPorts<'_, M, R, E, P, I, T>) -> V) -> V {
        let mut services = self.services.borrow_mut();
        let mut geometry = self.geometry.borrow_mut();
        let mut ports = RoundPorts::new(&mut services, &mut geometry, self.recognition, self.session.record());
        ports.auxiliary_buffer = self.auxiliary_buffer.take();
        let value = operation(&mut ports);
        self.auxiliary_buffer = ports.auxiliary_buffer.take();
        value
    }
    fn invalidate_auxiliary(&mut self) {
        if let Some(buffer) = self.auxiliary_buffer.take() {
            self.services.borrow().machine.enter(0x74_0e74, &[buffer as usize, 12]);
        }
    }
}
impl<M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, S, X> Drop for MeasurementEffects<'_, '_, M, R, E, P, I, T, S, X> {
    fn drop(&mut self) { self.invalidate_auxiliary(); }
}

macro_rules! delegate {
    ($scope:ident, $contract:path { $(fn $name:ident(&mut self $(, $argument:ident: $type:ty)*) $(-> $result:ty)?;)* } $($extra:item)*) => {
        impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor + Calc + Calc1 + ImgDc + StatisticsPeer, I: Isp, T: Tables, S: StateMemory, X: GridLayers>
            $contract for MeasurementEffects<'_, '_, M, R, E, P, I, T, S, X> {
            $(fn $name(&mut self $(, $argument: $type)*) $(-> $result)? {
                self.$scope(|ports| <_ as $contract>::$name(ports $(, $argument)*))
            })*
            $($extra)*
        }
    };
}

delegate!(with_services, ProfileRecords {
    fn offered(&mut self) -> Option<Selection>;
    fn statistics_in_use(&mut self) -> StatisticsInUse;
    fn set_statistics_in_use(&mut self, record: RecordHandle, in_use: StatisticsInUse);
    fn detach_capture_buffer(&mut self, record: RecordHandle);
    fn invalidate(&mut self, record: RecordHandle, result: PublishedResult);
    fn invalidate_focus_result(&mut self);
    fn grants_measurement_history(&mut self, record: RecordHandle) -> bool;
    fn acquisition_level(&mut self) -> i16;
    fn interval_history(&mut self) -> IntervalHistory;
    fn set_interval_history(&mut self, record: RecordHandle, history: IntervalHistory);
    fn profile(&mut self, record: RecordHandle) -> Profile;
    fn geometry(&mut self, record: RecordHandle) -> RecordGeometry;
    fn window(&mut self) -> WindowRecord;
    fn store_window_status(&mut self, record: RecordHandle, status: WindowStatus);
    fn store_window_position(&mut self, record: RecordHandle, position: WindowPosition);
});
delegate!(with_services, Settings {
    fn search_permitted(&mut self) -> bool;
    fn alternate_search(&mut self) -> AlternateSearch;
    fn cadence(&mut self, work: Work, mode: Mode) -> Cadence;
    fn frame_rate_threshold(&mut self) -> FrameRate;
    fn keeps_interval_history(&mut self) -> bool;
    fn geometry_calibration(&mut self) -> GeometryCalibration;
    fn traces_measurement(&mut self) -> bool;
});
delegate!(with_services, Pipeline {
    fn capture(&mut self, statistics: Statistics);
    fn snapshot(&mut self, statistics: Statistics) -> PipelineSnapshot;
    fn timing(&mut self) -> TimingWord;
    fn frame_interval(&mut self, timing: TimingWord) -> Option<FrameInterval>;
    fn request_mode_six_statistics(&mut self);
    fn take_window_refresh(&mut self) -> bool;
    fn history_statistics(&mut self, cell: GridCell) -> HistoryStatistics;
});
delegate!(with_services, Exposure {
    fn gain_offset(&mut self, step: GainStep) -> u32;
    fn exposure_value(&mut self, exposure: [PipelineWord; 2]) -> i16;
    fn timing_offset(&mut self, timing: TimingWord) -> u16;
    fn relative_exposure(&mut self, exposure: PipelineWord) -> i16;
});
delegate!(with_services, Power { fn request_measurement_power(&mut self, held: bool); });
delegate!(with_services, Adjustment { fn adjustment_words(&mut self) -> AdjustmentWords; });
delegate!(with_services, Trace { fn trace(&mut self, mark: TraceMark); });
delegate!(with_services, Effects { fn emit(&mut self, effect: Effect<'_>); });
delegate!(with_services, window::Ports { fn window_result(&mut self, completed: Completed<'_>) -> Option<window::WindowResult>; });

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor + Calc + Calc1 + ImgDc + StatisticsPeer, I: Isp, T: Tables, S: StateMemory, X: GridLayers>
    xm1_focus_measurement_round::record::Ports for MeasurementEffects<'_, '_, M, R, E, P, I, T, S, X> {
    fn request(&mut self, request: RecordRequest) {
        // A next auxiliary request can be planned in this same completion dispatch.
        if matches!(request, RecordRequest::Auxiliary(_)) { self.invalidate_auxiliary(); }
        self.with_services(|ports| xm1_focus_measurement_round::record::Ports::request(ports, request));
    }
}

delegate!(with_acquisition, xm1_focus_measurement_round::acquisition::ports::Ports {
    fn acquisition_settings(&mut self) -> AcquisitionSettings;
    fn record_view(&mut self, record: RecordHandle) -> RecordView;
    fn s1_rate_enabled(&mut self, record: RecordHandle) -> bool;
    fn published_exposure(&mut self, record: RecordHandle) -> PublishedExposure;
    fn publish_exposure(&mut self, record: RecordHandle, publication: &ExposurePublication);
    fn publish_primary_levels(&mut self, record: RecordHandle, levels: &PrimaryLevels);
    fn publish_grid(&mut self, record: RecordHandle, branch: Branch);
    fn publish_selection_levels(&mut self, record: RecordHandle, levels: &SelectionLevels);
    fn update_exposure_record(&mut self, record: RecordHandle, update: ExposureRecordUpdate);
    fn store_lens(&mut self, record: RecordHandle, lens: auxiliary::ResourceIndex);
    fn store_reference(&mut self, record: RecordHandle, reference: u16);
    fn store_selection_reference(&mut self, record: RecordHandle, reference: u16);
    fn program_kind(&mut self, record: RecordHandle) -> u32;
    fn store_program_kind(&mut self, record: RecordHandle, kind: u32);
    fn stored_program(&mut self, record: RecordHandle) -> Program;
    fn store_program(&mut self, record: RecordHandle, program: Program);
    fn store_program_word(&mut self, record: RecordHandle, word: [u32; 2]);
    fn store_program_choice(&mut self, record: RecordHandle, choice: &ProgramChoice);
    fn acquisition_block(&mut self, record: RecordHandle) -> AcquisitionBlock;
    fn store_acquisition_block(&mut self, record: RecordHandle, block: &AcquisitionBlock);
    fn selection_record(&mut self, record: RecordHandle) -> SelectionRecord;
    fn store_selection_record(&mut self, record: RecordHandle, selection: &SelectionRecord);
    fn selection_slot(&mut self, record: RecordHandle) -> SelectionSlot;
    fn publish_selection_slot(&mut self, record: RecordHandle, slot: &SelectionSlot);
    fn store_slot_word(&mut self, record: RecordHandle, word: u32);
    fn position_pair(&mut self, record: RecordHandle, slot: PositionSlot) -> [u32; 2];
    fn store_position_pair(&mut self, record: RecordHandle, slot: PositionSlot, pair: [u32; 2]);
    fn store_restore_levels(&mut self, record: RecordHandle, reference: u16, levels: [u16; 2]);
    fn store_s1_level(&mut self, record: RecordHandle, level: i16);
    fn clear_grid_selection(&mut self, record: RecordHandle);
    fn rate_record(&mut self, record: RecordHandle) -> RateRecord;
    fn store_rate_selection(&mut self, record: RecordHandle, selected: u32);
    fn clear_selection_reuse(&mut self, record: RecordHandle);
    fn clear_controller_reuse(&mut self);
    fn notify_selection(&mut self, record: RecordHandle, notice: &SelectionNotice);
    fn notify_rate(&mut self, record: RecordHandle, notice: &RateNotice);
    fn current_lens(&mut self) -> auxiliary::ResourceIndex;
    fn lens_word(&mut self, record: RecordHandle, lens: Option<auxiliary::ResourceIndex>) -> u32;
    fn collapse_index(&mut self, exposure: PipelineWord) -> u32;
    fn mapped_value(&mut self, first: u32, second: u32) -> u32;
    fn lens_position(&mut self, pair: [u32; 2], position: u32) -> u32;
    fn gain_step_for(&mut self, offset: i32) -> GainStep;
    fn acquisition_gain_step(&mut self, adjustment: u32) -> GainStep;
    fn kind_decade(&mut self, kind: u32) -> u32;
    fn change_positions(&mut self, record: RecordHandle) -> [u32; 2];
    fn select_program(&mut self, inputs: &ProgramInputs) -> Option<Program>;
    fn program_adjustment(&mut self, program: Program) -> u32;
    fn program_range_limit(&mut self, program: Program) -> u32;
    fn program_policy(&mut self, program: Program) -> u32;
    fn program_secondary_gate(&mut self, program: Program) -> u32;
    fn program_scale(&mut self, program: Program) -> u32;
    fn program_percent(&mut self, program: Program) -> u16;
    fn program_target(&mut self, inputs: &TargetInputs) -> ProgramTarget;
    fn program_range(&mut self, inputs: &RangeInputs) -> Range;
    fn program_lower_bound(&mut self, program: Program, lens: u32) -> u32;
    fn profile_percent(&mut self, program: Program, combined: u16, lens: auxiliary::ResourceIndex, bound: u32) -> u16;
    fn grid_count(&mut self, selection: ProfileSelection, mode: AcquisitionMode, adjustment: u32, grid_kind: u32) -> u32;
    fn indexed_configuration(&mut self, selection: ProfileSelection, adjustment: u32, index: u32) -> u8;
    fn level_category(&mut self, level: i16, selection: ProfileSelection, adjustment: u32) -> u32;
    fn selection_limits(&mut self, selection: ProfileSelection, adjustment: u32, grid_count: u32, request: u32) -> SelectionLimits;
    fn grid_lead(&mut self, branch: Branch) -> GridLead;
    fn set_grid_count(&mut self, branch: Branch, count: u32);
    fn clear_grid_lead(&mut self, branch: Branch);
    fn set_grid_configuration(&mut self, branch: Branch, configuration: u8);
    fn set_grid_inputs(&mut self, branch: Branch, inputs: &GridInputs);
    fn fill_grid(&mut self, branch: Branch, inputs: &GridInputs);
    fn snapshot_grid(&mut self, branch: Branch, statistics: Statistics);
    fn adjustment_class(&mut self, adjustment: u32, reference: i16) -> u8;
    fn adjustment_option(&mut self) -> u32;
    fn refreshed_position(&mut self) -> u32;
    fn mode_one(&mut self, inputs: &ModeInputs) -> u32;
    fn both_modes(&mut self, inputs: &ModeInputs) -> u32;
    fn five_parameter_adjustment(&mut self, selection: ProfileSelection, updated_position: u32, mode_flag: u32, modes: u32, grid_word: u32) -> u32;
    fn rate_divisor(&mut self, updated_position: u32, change_category: u32, lens: auxiliary::ResourceIndex, grid_halfword: u16) -> u32;
    fn rate_limit(&mut self, selection: ProfileSelection, updated_position: u32, position_halfword: u16) -> u32;
    fn rate_mode(&mut self, updated_position: u32, position_halfword: u16, word: u32, mode_flag: u32) -> u16;
    fn selection_mode_category(&mut self, mode_word: u32) -> u32;
    fn offset_position(&mut self, level: i16, reference: u16, flag: u32) -> u16;
    fn clamp_selection_difference(&mut self, mode: AcquisitionMode, lens: auxiliary::ResourceIndex, level: i16, lens_halfword: u16, lens_reference: u32, target: i16) -> i16;
    fn has_valid_offset(&mut self, selection: ProfileSelection, mode: AcquisitionMode, lens_halfword: i16) -> u32;
    fn allows_unflagged_selection(&mut self, updated_position: u32, halfword: u16) -> bool;
    fn capture_selection_position(&mut self, branch: Branch, inputs: &PositionInputs) -> SelectionPosition;
    fn copy_rate_grid_source(&mut self, buffer: StatisticsBuffer);
    fn rate_pair(&mut self, record: RecordHandle, result: SharedAddress, flag: u32, pair: &mut [u32; 2]);
    fn clear_rate_configuration(&mut self, record: RecordHandle);
    fn rate_flag(&mut self, record: RecordHandle, flag: u32);
    fn draw_rate(&mut self, result: SharedAddress, flag: u32);
    fn draw_rate_status(&mut self, level: u16, word: u32, flag: u32);
    fn rate_position(&mut self, record: RecordHandle) -> (u32, u16);
    fn statistics_buffer(&mut self, buffer: StatisticsBuffer) -> [u16; 256];
    fn extra_statistics(&mut self, values: &mut [u16]);
    fn store_release(&mut self, release: &Release<'_>);
    fn set_pipeline_reference(&mut self, level: i16);
    fn prepare_s1_sample(&mut self);
    fn s1_sample(&mut self) -> PipelineSnapshot;
    fn post_change_notice(&mut self, notice: &ChangeNotice);
    fn post_selection_table(&mut self, table: &SelectionTable);
    fn post_focus_table(&mut self, table: &FocusTable);
    fn shot_reference_level(&mut self) -> u16;
    fn shot_selection_word(&mut self) -> u32;
    fn prepare_selection_defaults(&mut self, record: RecordHandle) -> ShotContext;
    fn restore_selection_setting(&mut self, branch: Branch, lens: auxiliary::ResourceIndex);
    fn copy_reference_backup(&mut self, branch: Branch);
    fn write_saved_settings(&mut self, record: SharedAddress);
    fn copy_selection_to_saved(&mut self, record: SharedAddress);
    fn restore_selection_if_enabled(&mut self, record: RecordHandle, position: u32);
    fn apply_selection_value_change(&mut self);
    fn save_selection(&mut self, position: u32, saved: &SavedSelection, lens_position: u32);
    fn selected_grid_reference(&mut self, record: RecordHandle, reference: GridReference) -> GridReference;
    fn refresh_hardware_state(&mut self);
    fn hardware_ready(&mut self) -> bool;
    fn clock_word(&mut self) -> u32;
    fn store_selection_window(&mut self, branch: Branch, window: &SelectionWindow);
    fn store_selection_setting(&mut self, branch: Branch, setting: &SelectionSetting);
    fn store_restored_setting(&mut self, branch: Branch, setting: &RestoredSetting);
    fn setting_echo(&mut self, branch: Branch) -> SettingEcho;
    fn request_acquisition(&mut self, branch: Branch, request: &AcquisitionRequest, grid: EvaluationGrid);
    fn request_selection(&mut self, branch: Branch, request: &SelectionRequest);
    fn acquisition_result(&mut self, completed: Completed<'_>) -> Option<AcquisitionResult>;
    fn selection_result(&mut self, completed: Completed<'_>) -> Option<SelectionResult>;
    fn shot_record_levels(&mut self, record: SharedAddress) -> ShotRecordLevels;
    fn saved_exposure(&mut self, record: SharedAddress) -> SavedExposure;
    fn publish_saved_exposure(&mut self, record: RecordHandle, exposure: &SavedExposure);
    fn release_result(&mut self, released: ReleasedResult);
}
    fn with_selection<V>(&mut self, operation: impl FnOnce(&mut SelectionMemory) -> V) -> V {
        self.with_acquisition(|ports| xm1_focus_measurement_round::acquisition::ports::Ports::with_selection(ports, operation))
    }
);

delegate!(with_round, auxiliary::Ports {
    fn reading(&mut self, completed: Completed<'_>) -> Option<auxiliary::AuxiliaryReading>;
    fn auxiliary_settings(&mut self) -> auxiliary::AuxiliarySettings;
    fn default_levels(&mut self) -> [u16; 3];
    fn curve(&mut self, inputs: &auxiliary::CurveInputs) -> auxiliary::Curve;
    fn lens_resource(&mut self) -> auxiliary::ResourceIndex;
    fn resource_level(&mut self, resource: auxiliary::ResourceIndex) -> i16;
    fn follow_limit(&mut self, selection: ProfileSelection, profile: RetainedWord, timing: TimingWord) -> u32;
    fn rate(&mut self, size: [PipelineWord; 2]) -> u32;
    fn level_class(&mut self, level: i16, selection: ProfileSelection, window: RetainedWord) -> RetainedWord;
    fn refresh_grid_selection(&mut self, record: RecordHandle);
    fn auxiliary_continues(&mut self, record: RecordHandle) -> bool;
    fn auxiliary_profile(&mut self, record: RecordHandle) -> auxiliary::AuxiliaryProfile;
    fn request_profile(&mut self, record: RecordHandle) -> auxiliary::RequestProfile;
    fn follow_profile(&mut self, record: RecordHandle) -> RetainedWord;
    fn curve_offset(&mut self, record: RecordHandle) -> i16;
    fn auxiliary_publication(&mut self, record: RecordHandle) -> auxiliary::AuxiliaryPublication;
    fn publish_auxiliary(&mut self, record: RecordHandle, publication: auxiliary::AuxiliaryPublication);
    fn store_auxiliary_value(&mut self, record: RecordHandle, value: i16);
    fn grid_selection(&mut self, record: RecordHandle) -> bool;
    fn record_capture_levels(&mut self, record: RecordHandle, levels: [u16; 3]);
    fn rotate_adjustment_snapshot(&mut self, record: RecordHandle);
});
delegate!(with_round, source::Ports {
    fn source_profile(&mut self, record: RecordHandle) -> source::SourceProfile;
    fn sample_profile(&mut self, record: RecordHandle) -> source::SampleProfile;
    fn source_settings(&mut self) -> source::SourceSettings;
    fn processing_summary(&mut self, request: &source::ProcessingRequest) -> source::Summary;
    fn enable_processing_callbacks(&mut self, enabled: bool);
    fn published_strength(&mut self) -> source::SourceStrength;
    fn update_source_values(&mut self, summary: &source::Summary, means: &source::Summary);
    fn publish_source(&mut self, record: RecordHandle, publication: source::SourcePublication<'_>);
});
delegate!(with_round, image::ReplyPorts {
    fn channel_count(&mut self) -> image::ChannelCount;
    fn lens_report(&mut self) -> image::LensReport;
});
delegate!(with_round, image::Ports {
    fn image_client_callback(&mut self) -> Option<StockCallbackWord>;
    fn notify_image_client(&mut self, callback: StockCallbackWord);
    fn prepare_image_area(&mut self, channel: xm1_saclac_interface::round_image::ImageChannel, area: xm1_saclac_interface::round_image::ImageArea) -> xm1_saclac_interface::round_image::ImageArea;
    fn image_logging(&mut self) -> image::ImageLogging;
    fn log_image(&mut self, entry: image::ImageLog);
    fn request_image(&mut self, request: xm1_saclac_interface::round_image::ImageLaneRequest);
});

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor + Calc + Calc1 + ImgDc + StatisticsPeer, I: Isp, T: Tables, S: StateMemory, X: GridLayers>
    LiveEffects for MeasurementEffects<'_, '_, M, R, E, P, I, T, S, X> {
    fn fault(&mut self) { self.with_services(|ports| crate::stock::acquisition::fault(&ports.machine)); }
    fn search(&mut self, signal: LaneSignal<'_>, record: RecordHandle, history: &MeasurementHistory) -> LaneStatus {
        self.session.set_record(record);
        let kept = self.with_services(|ports| ProfileRecords::grants_measurement_history(ports, record));
        self.session.set_history(history, kept);
        let event = match signal {
            LaneSignal::Initialize => live::LaneEvent::Initialize,
            LaneSignal::Enable => live::LaneEvent::Enable,
            LaneSignal::Stop => live::LaneEvent::Stop,
            LaneSignal::Suspend => live::LaneEvent::Suspend,
            LaneSignal::Resume => live::LaneEvent::Resume,
            LaneSignal::Work(_) => live::LaneEvent::Work,
            LaneSignal::Completed(completed) => self.session.live_completion(completed).unwrap_or(live::LaneEvent::Other),
            LaneSignal::Command(_) => live::LaneEvent::Other,
        };
        match self.session.handle_live(self.search, event) {
            live::LaneStatus::Refused => LaneStatus::Refused,
            live::LaneStatus::Complete => LaneStatus::Complete,
            live::LaneStatus::Pending => LaneStatus::Pending,
            live::LaneStatus::Unhandled => LaneStatus::Unhandled,
            live::LaneStatus::Idle => LaneStatus::Idle,
            live::LaneStatus::Terminal(terminal) => {
                self.terminal = Some(terminal);
                // Keep the fan pending until the lane drains; Dispatch retains the typed result.
                LaneStatus::Pending
            }
        }
    }
}

#[cfg(saclac_qualification)]
pub mod qualification;
