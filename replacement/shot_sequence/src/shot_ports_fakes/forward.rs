// Test-only composition of the existing external-service fakes.
use super::{ExternalFake, Event};

#[allow(unused_imports)]
mod leaf_fakes {
use super::{ExternalFake, Event};
use crate::boundary::*;
use crate::preparation::{FrameHeader, LensReadings, LensStage, PreparationShot};
use xm1_saclac_interface::RecordHandle;
use crate::subjects::{PrimaryDetection, Selection, Subject, SubjectList, SubjectQuery, SubjectServices, Summary};
use crate::summary::{AreaStatistics, FrameSummary};
use std::collections::BTreeMap;
use std::vec::Vec;
impl Records for ExternalFake {
    fn setting(&mut self, setting: Setting) -> u32 {  Records::setting(&mut self.basic, setting) }
    fn lens_snapshot(&mut self) -> LensSnapshot {  Records::lens_snapshot(&mut self.basic) }
    fn store_lens_snapshot(&mut self, snapshot: &LensSnapshot) {  Records::store_lens_snapshot(&mut self.basic, snapshot) }
    fn focus_result(&mut self) -> FocusResult {  Records::focus_result(&mut self.basic) }
    fn store_focus_result(&mut self, result: &FocusResult) {  Records::store_focus_result(&mut self.basic, result) }
    fn focus_confirmation(&mut self) -> FocusConfirmation {  Records::focus_confirmation(&mut self.basic) }
    fn store_focus_confirmation(&mut self, confirmation: &FocusConfirmation) {  Records::store_focus_confirmation(&mut self.basic, confirmation) }
    fn focus_history(&mut self) -> FocusHistory {  Records::focus_history(&mut self.basic) }
    fn store_focus_history(&mut self, history: &FocusHistory) {  Records::store_focus_history(&mut self.basic, history) }
    fn clear_exposure_record(&mut self) {  Records::clear_exposure_record(&mut self.basic) }
    fn area_statistics(&mut self) -> AreaStatistics {  Records::area_statistics(&mut self.basic) }
    fn publish(&mut self, publication: Publication<'_>) {  Records::publish(&mut self.basic, publication) }
    fn select_first_mode(&mut self) {  Records::select_first_mode(&mut self.basic) }
    fn load_lens_capabilities(&mut self) {  Records::load_lens_capabilities(&mut self.basic) }
}
impl Lens for ExternalFake {
    fn read(&mut self, reading: LensReading) -> u32 {  Lens::read(&mut self.basic, reading) }
    fn take_change(&mut self) -> u32 {  Lens::take_change(&mut self.basic) }
    fn designated_point(&mut self) -> Option<LensPoint> {  Lens::designated_point(&mut self.basic) }
    fn target_distance(&mut self, point: LensPoint, control: u32) -> u32 {  Lens::target_distance(&mut self.basic, point, control) }
    fn target_step(&mut self, point: LensPoint) -> u16 {  Lens::target_step(&mut self.basic, point) }
}
impl Client for ExternalFake {
    fn area_count(&mut self, _area_count: ClientRef) -> u32 {  Client::area_count(&mut self.basic, _area_count) }
    fn area(&mut self, _areas: ClientRef, index: usize) -> u32 {  Client::area(&mut self.basic, _areas, index) }
    fn write_lens_target(&mut self, target: ClientRef, point: LensPoint, step: u16, distance: u32) {  Client::write_lens_target(&mut self.basic, target, point, step, distance) }
    fn frame(&mut self, _slot: ClientRef) -> Option<Frame> {  Client::frame(&mut self.basic, _slot) }
    fn clear_frame_progress(&mut self, slot: ClientRef) {  Client::clear_frame_progress(&mut self.basic, slot) }
    fn announce_frame(&mut self, frame: Frame, header: FrameHeader) {  Client::announce_frame(&mut self.basic, frame, header) }
    fn write_summary(&mut self, frame: Frame, summary: &FrameSummary) {  Client::write_summary(&mut self.basic, frame, summary) }
}
impl Diagnostics for ExternalFake {
    fn trace(&mut self, trace: Trace) { self.events.push(Event::Trace(trace)); Diagnostics::trace(&mut self.basic, trace) }
    fn dump(&mut self, dump: Dump, frame: Frame, argument: u32) {  Diagnostics::dump(&mut self.basic, dump, frame, argument) }
    fn calculation_context_mode(&mut self) -> u32 {  Diagnostics::calculation_context_mode(&mut self.basic) }
    fn dump_calculation(&mut self) {  Diagnostics::dump_calculation(&mut self.basic) }
}
impl Platform for ExternalFake {
    fn hold(&mut self, _hold: Hold, held: bool) {  Platform::hold(&mut self.basic, _hold, held) }
    fn adjustment(&mut self, adjustment: Adjustment) -> u8 {  Platform::adjustment(&mut self.basic, adjustment) }
    fn level(&mut self) -> u8 {  Platform::level(&mut self.basic) }
    fn tick(&mut self) -> u32 {  Platform::tick(&mut self.basic) }
    fn reset_image_progress(&mut self) {  Platform::reset_image_progress(&mut self.basic) }
}
impl SubjectServices for ExternalFake {
    fn lens_distance(&mut self, lens_reference: LensReference) -> u32 {  SubjectServices::lens_distance(&mut self.basic, lens_reference) }
    fn primary_detect(&mut self, limit: u32, _query: &SubjectQuery) -> PrimaryDetection {  SubjectServices::primary_detect(&mut self.basic, limit, _query) }
    fn primary_select(&mut self, limit: u32, _query: &SubjectQuery) -> SubjectList {  SubjectServices::primary_select(&mut self.basic, limit, _query) }
    fn primary_regions(&mut self, _query: &SubjectQuery) -> SubjectList {  SubjectServices::primary_regions(&mut self.basic, _query) }
    fn secondary_detect(&mut self, selection: Selection, _query: &SubjectQuery, list: &mut SubjectList, _summary: &mut Summary) {  SubjectServices::secondary_detect(&mut self.basic, selection, _query, list, _summary) }
    fn secondary_select(&mut self, selection: Selection, _query: &SubjectQuery, list: &mut SubjectList) {  SubjectServices::secondary_select(&mut self.basic, selection, _query, list) }
    fn secondary_regions(&mut self, _query: &SubjectQuery, list: &mut SubjectList) {  SubjectServices::secondary_regions(&mut self.basic, _query, list) }
    fn refine(&mut self, _query: &SubjectQuery, _list: &mut SubjectList) {  SubjectServices::refine(&mut self.basic, _query, _list) }
    fn select(&mut self, kind: u32, option: u32) -> bool {  SubjectServices::select(&mut self.basic, kind, option) }
    fn close_selection(&mut self) {  SubjectServices::close_selection(&mut self.basic) }
}
}

#[allow(unused_imports)]
mod exposure_fakes {
use super::{ExternalFake, Event};
use crate::boundary::*;
use crate::exposure_work::ExposureSetup;
use crate::leaf_fakes::Fake;
use crate::ports::{EvaluationIndex, HalfPressMode, Reply};
use crate::subjects::SubjectList;
use std::vec::Vec;
use xm1_saclac_interface::exposure::{Correction, Evaluation, MeasuredEntry, StatisticsRequest, STATISTICS_AREAS_BYTES};
use xm1_saclac_interface::{Completion, RecordHandle};
impl ExposureCalculation for ExternalFake {
    fn selection(&mut self, _record: RecordHandle, mode: HalfPressMode, refinement_pass: bool) -> SelectionKind {  ExposureCalculation::selection(&mut self.basic, _record, mode, refinement_pass) }
    fn selects_primary(&mut self, _selection: SelectionKind) -> bool {  ExposureCalculation::selects_primary(&mut self.basic, _selection) }
    fn selects_secondary(&mut self, _selection: SelectionKind) -> bool {  ExposureCalculation::selects_secondary(&mut self.basic, _selection) }
    fn prepare(&mut self, setup: &ExposureSetup) {  ExposureCalculation::prepare(&mut self.basic, setup) }
    fn calculation_mode(&mut self, _record: RecordHandle) -> u32 {  ExposureCalculation::calculation_mode(&mut self.basic, _record) }
    fn correction_mode(&mut self, _record: RecordHandle, refinement_pass: bool) -> u32 {  ExposureCalculation::correction_mode(&mut self.basic, _record, refinement_pass) }
    fn scaled_statistics(&mut self) -> bool {  ExposureCalculation::scaled_statistics(&mut self.basic) }
    fn evaluation_areas(&mut self) -> [u8; STATISTICS_AREAS_BYTES] {  ExposureCalculation::evaluation_areas(&mut self.basic) }
    fn evaluation_area_count(&mut self) -> u8 {  ExposureCalculation::evaluation_area_count(&mut self.basic) }
    fn evaluated_count(&mut self) -> u8 {  ExposureCalculation::evaluated_count(&mut self.basic) }
    fn select_measured(&mut self, entry: MeasuredEntry) {  ExposureCalculation::select_measured(&mut self.basic, entry) }
    fn check_measured(&mut self, entry: MeasuredEntry) -> bool {  ExposureCalculation::check_measured(&mut self.basic, entry) }
    fn accept_measured(&mut self, entry: MeasuredEntry) {  ExposureCalculation::accept_measured(&mut self.basic, entry) }
    fn next_evaluation(&mut self) -> EvaluationIndex {  ExposureCalculation::next_evaluation(&mut self.basic) }
    fn request_evaluation(&mut self, index: EvaluationIndex) { self.events.push(Event::Evaluation(index)); ExposureCalculation::request_evaluation(&mut self.basic, index) }
    fn absorb_evaluation(&mut self, evaluation: &Evaluation) {  ExposureCalculation::absorb_evaluation(&mut self.basic, evaluation) }
    fn correction_needed(&mut self) -> bool {  ExposureCalculation::correction_needed(&mut self.basic) }
    fn request_correction(&mut self, _setup: &ExposureSetup) -> CorrectionRequest { self.events.push(Event::Correction); ExposureCalculation::request_correction(&mut self.basic, _setup) }
    fn levels(&mut self) -> ExposureLevels {  ExposureCalculation::levels(&mut self.basic) }
    fn alternate(&mut self) -> bool {  ExposureCalculation::alternate(&mut self.basic) }
    fn variant(&mut self) -> u32 {  ExposureCalculation::variant(&mut self.basic) }
    fn exporting(&mut self) -> bool {  ExposureCalculation::exporting(&mut self.basic) }
    fn export(&mut self, frame: Option<Frame>, correction: &Correction, kind: ExportKind) {  ExposureCalculation::export(&mut self.basic, frame, correction, kind) }
    fn clear_selection(&mut self, frame: Option<Frame>, primary: bool) {  ExposureCalculation::clear_selection(&mut self.basic, frame, primary) }
    fn finish_primary(&mut self) {  ExposureCalculation::finish_primary(&mut self.basic) }
}
impl ExposureRecord for ExternalFake {
    fn store_exposure(&mut self, slot: ExposureSlot, value: i32) {  ExposureRecord::store_exposure(&mut self.basic, slot, value) }
    fn store_offset(&mut self, offset: ExposureOffset, value: i16) {  ExposureRecord::store_offset(&mut self.basic, offset, value) }
    fn store_area_exposure(&mut self, index: usize, value: i16) {  ExposureRecord::store_area_exposure(&mut self.basic, index, value) }
    fn metering(&mut self) -> MeteringBlock {  ExposureRecord::metering(&mut self.basic) }
    fn metering_source(&mut self) -> [u8; METERING_SOURCE_BYTES] {  ExposureRecord::metering_source(&mut self.basic) }
    fn store_metering(&mut self, block: &MeteringBlock) {  ExposureRecord::store_metering(&mut self.basic, block) }
}
impl ExposureServices for ExternalFake {
    fn lens_offset(&mut self, lens_reference: LensReference) -> u16 {  ExposureServices::lens_offset(&mut self.basic, lens_reference) }
    fn calibration_offset(&mut self) -> u16 {  ExposureServices::calibration_offset(&mut self.basic) }
    fn exports_last_correction(&mut self) -> bool {  ExposureServices::exports_last_correction(&mut self.basic) }
}
impl Recorder for ExternalFake {
    fn recording(&mut self, _mode: RecorderMode) -> bool {  Recorder::recording(&mut self.basic, _mode) }
    fn start_recording(&mut self, _mode: RecorderMode) {  Recorder::start_recording(&mut self.basic, _mode) }
    fn record_evaluations(&mut self, count: u8) {  Recorder::record_evaluations(&mut self.basic, count) }
}
impl StatisticsPort for ExternalFake {
    fn request_statistics(&mut self, request: &StatisticsRequest, subjects: &SubjectList) { self.events.push(Event::Statistics(*request)); StatisticsPort::request_statistics(&mut self.basic, request, subjects) }
    fn release_statistics(&mut self) {  StatisticsPort::release_statistics(&mut self.basic) }
}
impl Replies for ExternalFake {
    fn reply(&mut self, reply: Reply) { self.events.push(Event::Reply(reply)); Replies::reply(&mut self.basic, reply) }
}
impl Completions for ExternalFake {
    fn self_complete(&mut self, completion: Completion) { self.events.push(Event::SelfComplete(completion)); Completions::self_complete(&mut self.basic, completion) }
}
}

#[allow(unused_imports)]
mod capture_fakes {
use super::{ExternalFake, Event};
use crate::boundary::{ClientRef, FocusResult, Frame, Lens, LensPoint, LensReading, LensReference};
use crate::capture_boundary::*;
use crate::capture_control::ControlShot;
use crate::capture_dump::DumpShot;
use crate::dump_boundary::ShotWord;
use crate::lens_return::ReturnShot;
use crate::secondary_job::SecondaryResult;
use crate::statistics_dump::StatisticsDump;
use crate::subjects::SubjectList;
use std::collections::BTreeMap;
use std::vec::Vec;
use xm1_saclac_interface::area_measurement::AreaEntry;
use xm1_saclac_interface::calculation::ReferenceRequest;
use xm1_saclac_interface::statistics::StatisticsSetting;
use xm1_saclac_interface::{Completion, RecordHandle};
impl CaptureClient for ExternalFake {
    fn frame(&mut self, _sources: ClientRef) -> Frame {  CaptureClient::frame(&mut self.capture, _sources) }
    fn frame_detail(&mut self, _sources: ClientRef) -> u32 {  CaptureClient::frame_detail(&mut self.capture, _sources) }
    fn selection(&mut self, _selection: ClientRef) -> SlotSelection {  CaptureClient::selection(&mut self.capture, _selection) }
    fn slot(&mut self, _slots: ClientRef, index: u8) -> SlotHandle {  CaptureClient::slot(&mut self.capture, _slots, index) }
    fn store_selection_result(&mut self, selection: ClientRef, value: u32) {  CaptureClient::store_selection_result(&mut self.capture, selection, value) }
    fn store_selection_hook(&mut self, selection: ClientRef) {  CaptureClient::store_selection_hook(&mut self.capture, selection) }
    fn output_marker(&mut self, _selection: ClientRef) -> i16 {  self.output_marker }
    fn frame_word(&mut self, _frame: Frame, offset: FrameOffset) -> u32 {  self.capture.frame_words.get(&offset.0).copied().unwrap_or(0) }
    fn set_frame_word(&mut self, _frame: Frame, offset: FrameOffset, value: u32) { self.capture.frame_words.insert(offset.0, value); }
    fn fill_frame(&mut self, frame: Frame, offset: FrameOffset, bytes: u32, value: u8) {  CaptureClient::fill_frame(&mut self.capture, frame, offset, bytes, value) }
    fn copy_table(&mut self, frame: Frame, offset: FrameOffset, table: FrameTable) {  CaptureClient::copy_table(&mut self.capture, frame, offset, table) }
}
impl CaptureRecords for ExternalFake {
    fn field(&mut self, _record: RecordHandle, field: RecordField) -> u32 {  CaptureRecords::field(&mut self.capture, _record, field) }
    fn set_field(&mut self, record: RecordHandle, field: RecordField, value: u32) {  CaptureRecords::set_field(&mut self.capture, record, field, value) }
    fn slot_field(&mut self, _record: RecordHandle, slot: u8, field: SlotField) -> u32 {  CaptureRecords::slot_field(&mut self.capture, _record, slot, field) }
    fn set_slot_field(&mut self, record: RecordHandle, slot: u8, field: SlotField, value: u32) {  CaptureRecords::set_slot_field(&mut self.capture, record, slot, field, value) }
    fn reference_pair(&mut self, _record: RecordHandle, index: u8) -> ReferencePair {  CaptureRecords::reference_pair(&mut self.capture, _record, index) }
    fn slot_entry(&mut self, _record: RecordHandle, slot: u8) -> SlotEntry {  CaptureRecords::slot_entry(&mut self.capture, _record, slot) }
    fn store_slot_entry(&mut self, target: SlotHandle, entry: &SlotEntry) {  CaptureRecords::store_slot_entry(&mut self.capture, target, entry) }
    fn slot_reference(&mut self, slot: SlotHandle) -> u32 {  CaptureRecords::slot_reference(&mut self.capture, slot) }
    fn slot_offset(&mut self, slot: SlotHandle) -> i16 {  CaptureRecords::slot_offset(&mut self.capture, slot) }
    fn load_profile(&mut self, record: RecordHandle, profile: ClientRef) {  CaptureRecords::load_profile(&mut self.capture, record, profile) }
    fn select_slot(&mut self, record: RecordHandle, slot: SlotHandle) {  CaptureRecords::select_slot(&mut self.capture, record, slot) }
    fn focus_result(&mut self, _record: RecordHandle) -> FocusResult {  CaptureRecords::focus_result(&mut self.capture, _record) }
    fn store_focus_result(&mut self, record: RecordHandle, result: &FocusResult) {  CaptureRecords::store_focus_result(&mut self.capture, record, result) }
    fn area_reference(&mut self, _record: RecordHandle) -> AreaReference {  CaptureRecords::area_reference(&mut self.capture, _record) }
    fn store_setting_result(&mut self, record: RecordHandle, result: &SettingResult) {  CaptureRecords::store_setting_result(&mut self.capture, record, result) }
    fn store_item(&mut self, record: RecordHandle, index: u8, item: &AreaEntry) {  CaptureRecords::store_item(&mut self.capture, record, index, item) }
    fn publish_subjects(&mut self, record: RecordHandle, list: &SubjectList) {  CaptureRecords::publish_subjects(&mut self.capture, record, list) }
}
impl LensGeometry for ExternalFake {
    fn reference_offset(&mut self, reference: u32) -> i16 {  LensGeometry::reference_offset(&mut self.capture, reference) }
    fn rounded(&mut self, value: u32) -> i16 {  LensGeometry::rounded(&mut self.capture, value) }
    fn convert(&mut self, base: u32, value: u32, lens: u32) -> u32 {  LensGeometry::convert(&mut self.capture, base, value, lens) }
    fn mapped(&mut self, value: u32, selector: u32) -> u32 {  LensGeometry::mapped(&mut self.capture, value, selector) }
    fn compensated(&mut self, reference: u32, lens: u32) -> i16 {  LensGeometry::compensated(&mut self.capture, reference, lens) }
    fn remap(&mut self, request: RemapRequest) -> i32 {  LensGeometry::remap(&mut self.capture, request) }
}
impl LensMoves for ExternalFake {
    fn absolute(&mut self, target: u32, profile: MoveProfile, completion: Completion) { self.events.push(Event::Absolute(target, profile, completion)); LensMoves::absolute(&mut self.capture, target, profile, completion) }
    fn relative(&mut self, delta: i16, speed: u32, profile: MoveProfile, completion: Completion) { self.events.push(Event::Relative(delta, speed, profile, completion)); LensMoves::relative(&mut self.capture, delta, speed, profile, completion) }
}
impl CalcLibrary for ExternalFake {
    fn request_reference(&mut self, request: &ReferenceRequest, result: CalculationResult, completion: Completion) { self.events.push(Event::Reference(*request, result, completion)); CalcLibrary::request_reference(&mut self.capture, request, result, completion) }
    fn selection_rate(&mut self, _record: RecordHandle, sample: LensSample, control: u32) -> u32 {  CalcLibrary::selection_rate(&mut self.capture, _record, sample, control) }
    fn selection_direction(&mut self, sample: LensSample) -> u32 {  CalcLibrary::selection_direction(&mut self.capture, sample) }
    fn export_selection(&mut self, frame: Frame) {  CalcLibrary::export_selection(&mut self.capture, frame) }
    fn export_joint_tables(&mut self, frame: Frame) {  CalcLibrary::export_joint_tables(&mut self.capture, frame) }
}
impl CapturePeers for ExternalFake {
    fn statistics_setup(&mut self, enabled: bool, completion: Completion) { self.events.push(Event::Setup(enabled, completion)); CapturePeers::statistics_setup(&mut self.capture, enabled, completion) }
    fn statistics_setting(&mut self, setting: &StatisticsSetting, completion: Completion) {  CapturePeers::statistics_setting(&mut self.capture, setting, completion) }
}
impl CapturePlatform for ExternalFake {
    fn adjustment(&mut self, adjustment: CaptureAdjustment) -> u8 {  CapturePlatform::adjustment(&mut self.capture, adjustment) }
    fn prepare_setting(&mut self) {  CapturePlatform::prepare_setting(&mut self.capture) }
    fn refresh_setting(&mut self) {  CapturePlatform::refresh_setting(&mut self.capture) }
    fn monitor_device_commands(&mut self, monitored: bool) {  CapturePlatform::monitor_device_commands(&mut self.capture, monitored) }
    fn device_status_bit(&mut self) -> bool {  CapturePlatform::device_status_bit(&mut self.capture) }
    fn slot_rate(&mut self, active: u32, profile: u32) -> u32 {  CapturePlatform::slot_rate(&mut self.capture, active, profile) }
    fn split_distance(&mut self, distance: i16) -> DistanceSplit {  CapturePlatform::split_distance(&mut self.capture, distance) }
    fn device_value(&mut self) -> u32 {  CapturePlatform::device_value(&mut self.capture) }
    fn export_record_snapshot(&mut self, frame: Frame) {  CapturePlatform::export_record_snapshot(&mut self.capture, frame) }
    fn export_pipeline(&mut self, frame: Frame) {  CapturePlatform::export_pipeline(&mut self.capture, frame) }
    fn prepare_dump_area(&mut self, frame: Frame, offset: FrameOffset) {  CapturePlatform::prepare_dump_area(&mut self.capture, frame, offset) }
}
impl CaptureDiagnostics for ExternalFake {
    fn trace(&mut self, trace: CaptureTrace) {  CaptureDiagnostics::trace(&mut self.capture, trace) }
}
}

#[allow(unused_imports)]
mod grid_fakes {
use super::{ExternalFake, Event};
use crate::calculation_work::CalculationProfile;
use crate::grid_boundary::{BlendQuery, ClassLimits, ClassQuery, DefaultQuery, GridImaging, GridLibrary, GridProfiles, GridSetting, GridSettings, LevelQuery, LimitInputs, ReferenceQuery};
use crate::state::{ShotState, StateMemory};
use crate::state_tests::Memory;
use crate::stock::state as layout;
use std::collections::BTreeMap;
use std::vec::Vec;
use xm1_saclac_interface::calculation::{AreaAt, AreaHalf, AreaIndex, AreaResult, GridCell, GridPass, ResultPass, TrackedKind};
use xm1_saclac_interface::selection::{CellCode, SelectionClass, SelectionList, SubjectQuery};
use xm1_saclac_interface::RecordHandle;
impl GridProfiles for ExternalFake {
    fn profile(&mut self, _record: RecordHandle, value: CalculationProfile) -> u32 {  GridProfiles::profile(&mut self.grid, _record, value) }
}
impl GridLibrary for ExternalFake {
    fn selection_mode(&mut self, _record: RecordHandle, _refinement: u32) -> u32 {  GridLibrary::selection_mode(&mut self.grid, _record, _refinement) }
    fn selection_flags(&mut self, _record: RecordHandle, _refinement: u32) -> u16 {  GridLibrary::selection_flags(&mut self.grid, _record, _refinement) }
    fn selection_code(&mut self, _record: RecordHandle) -> i16 {  GridLibrary::selection_code(&mut self.grid, _record) }
    fn class_limits(&mut self, _inputs: &LimitInputs) -> ClassLimits {  GridLibrary::class_limits(&mut self.grid, _inputs) }
    fn fixed_class_limits(&mut self) -> ClassLimits {  GridLibrary::fixed_class_limits(&mut self.grid) }
    fn total_class_limits(&mut self, _total: i16) -> ClassLimits {  GridLibrary::total_class_limits(&mut self.grid, _total) }
    fn layout_areas(&mut self, _layout: u32) -> u8 {  GridLibrary::layout_areas(&mut self.grid, _layout) }
    fn target_reference(&mut self, _query: &ReferenceQuery) -> u32 {  GridLibrary::target_reference(&mut self.grid, _query) }
    fn target_level(&mut self, query: &LevelQuery) -> i16 {  GridLibrary::target_level(&mut self.grid, query) }
    fn subject_eligible(&mut self, _query: SubjectQuery) -> bool {  GridLibrary::subject_eligible(&mut self.grid, _query) }
    fn focus_blend(&mut self, _query: &BlendQuery) -> u32 {  GridLibrary::focus_blend(&mut self.grid, _query) }
    fn settle_level(&mut self, level: i16) -> (i16, i16) {  GridLibrary::settle_level(&mut self.grid, level) }
    fn floor_level(&mut self, level: i16) -> i16 {  GridLibrary::floor_level(&mut self.grid, level) }
}
impl GridImaging for ExternalFake {
    fn class_code(&mut self, class: SelectionClass) -> u8 {  GridImaging::class_code(&mut self.grid, class) }
    fn code_class(&mut self, code: u8) -> SelectionClass {  GridImaging::code_class(&mut self.grid, code) }
    fn class_cell(&mut self, class: SelectionClass) -> CellCode {  GridImaging::class_cell(&mut self.grid, class) }
    fn cell_code(&mut self, class: SelectionClass) -> u8 {  GridImaging::cell_code(&mut self.grid, class) }
    fn cell_class(&mut self, cell: CellCode) -> u32 {  GridImaging::cell_class(&mut self.grid, cell) }
    fn cell_kind(&mut self, cell: CellCode) -> u32 {  GridImaging::cell_kind(&mut self.grid, cell) }
    fn cell_level(&mut self, _cell: CellCode) -> i16 {  GridImaging::cell_level(&mut self.grid, _cell) }
    fn cell_unset(&mut self, cell: CellCode) -> bool {  GridImaging::cell_unset(&mut self.grid, cell) }
    fn class_status(&mut self, index: u32) -> u8 {  GridImaging::class_status(&mut self.grid, index) }
    fn level_scale(&mut self, level: i16) -> u32 {  GridImaging::level_scale(&mut self.grid, level) }
    fn level_class(&mut self, _level: i32, value: i32) -> SelectionClass {  GridImaging::level_class(&mut self.grid, _level, value) }
    fn exposure_value(&mut self) -> i32 {  GridImaging::exposure_value(&mut self.grid) }
    fn first_class_index(&mut self) -> u32 {  GridImaging::first_class_index(&mut self.grid) }
    fn class_level(&mut self, _index: u32) -> u16 {  GridImaging::class_level(&mut self.grid, _index) }
    fn class_changed(&mut self, _query: &ClassQuery) -> bool {  GridImaging::class_changed(&mut self.grid, _query) }
    fn record_class(&mut self, _query: &ClassQuery) -> u32 {  GridImaging::record_class(&mut self.grid, _query) }
    fn record_mode(&mut self, _record: RecordHandle) -> u32 {  GridImaging::record_mode(&mut self.grid, _record) }
    fn record_flags(&mut self, _record: RecordHandle) -> u16 {  GridImaging::record_flags(&mut self.grid, _record) }
    fn default_class(&mut self, _query: &DefaultQuery) -> SelectionClass {  GridImaging::default_class(&mut self.grid, _query) }
}
impl GridSettings for ExternalFake {
    fn setting(&mut self, setting: GridSetting) -> i32 {  GridSettings::setting(&mut self.grid, setting) }
    fn lens_threshold(&mut self, step: u8) -> i16 {  GridSettings::lens_threshold(&mut self.grid, step) }
    fn lens_band(&mut self, _level: i16) -> u32 {  GridSettings::lens_band(&mut self.grid, _level) }
    fn reference_level(&mut self, _reference: u32, _lens_reference: u32) -> i16 {  GridSettings::reference_level(&mut self.grid, _reference, _lens_reference) }
    fn reference_offset(&mut self, reference: u32) -> i16 {  GridSettings::reference_offset(&mut self.grid, reference) }
}
}

#[allow(unused_imports)]
mod calculation_fakes {
use super::{ExternalFake, Event};
use crate::absorb::*;
use crate::boundary::*;
use crate::capture_boundary::{AreaReference, RecordField, ReferencePair, SlotEntry, SlotHandle};
use crate::ports::Reply;
use crate::record_load::*;
use xm1_saclac_interface::stock::StockCallbackWord;
use crate::preparation::FrameHeader;
use crate::summary::FrameSummary;
use std::cell::RefCell;
use std::rc::Rc;
use std::vec::Vec;
use xm1_saclac_interface::calculation::{AreaAt, AreaIndex, AreaResult, AreaRow, CellSummary, FocusSummary, GridCell, GridPass, ResultPass, TrackedKind};
use xm1_saclac_interface::stock::record_handle_from_a2;
use xm1_saclac_interface::RecordHandle;
impl AbsorbLibrary for ExternalFake {
    fn subject_source(&mut self, _record: RecordHandle, _refinement: u32) -> u32 {  AbsorbLibrary::subject_source(&mut self.absorb, _record, _refinement) }
    fn grid_mode(&mut self, _record: RecordHandle, mode: u32) -> u32 {  AbsorbLibrary::grid_mode(&mut self.absorb, _record, mode) }
    fn evaluate_distances(&mut self, pass: ResultPass, _cell: GridCell, index: AreaIndex) {  AbsorbLibrary::evaluate_distances(&mut self.absorb, pass, _cell, index) }
    fn selection_changed(&mut self, _record: RecordHandle, _source: u32, _reference: u32) -> bool {  AbsorbLibrary::selection_changed(&mut self.absorb, _record, _source, _reference) }
    fn compare_reference(&mut self, _record: RecordHandle, _source: u32) -> (bool, u32) {  AbsorbLibrary::compare_reference(&mut self.absorb, _record, _source) }
    fn compensated(&mut self, target: u32) -> i16 {  AbsorbLibrary::compensated(&mut self.absorb, target) }
    fn classify_level(&mut self, value: i16) -> u32 {  AbsorbLibrary::classify_level(&mut self.absorb, value) }
    fn split_reference(&mut self) -> u32 {  AbsorbLibrary::split_reference(&mut self.absorb) }
    fn lower_reference(&mut self, _lens_reference: LensReference) -> u32 {  AbsorbLibrary::lower_reference(&mut self.absorb, _lens_reference) }
}
impl AbsorbDiagnostics for ExternalFake {
    fn adjustment(&mut self, adjustment: AbsorbAdjustment) -> u8 {  AbsorbDiagnostics::adjustment(&mut self.absorb, adjustment) }
    fn export_result(&mut self, _frame: Option<Frame>, export: ResultExport, enabled: bool) {  AbsorbDiagnostics::export_result(&mut self.absorb, _frame, export, enabled) }
    fn clear_channel_diagnostics(&mut self, _frame: Option<Frame>, cleared: bool) {  AbsorbDiagnostics::clear_channel_diagnostics(&mut self.absorb, _frame, cleared) }
    fn draw_rate_status(&mut self, _reference: &AreaReference) {  AbsorbDiagnostics::draw_rate_status(&mut self.absorb, _reference) }
    fn release_rate_status(&mut self) {  AbsorbDiagnostics::release_rate_status(&mut self.absorb) }
}
impl AbsorbRecords for ExternalFake {
    fn store_area(&mut self, _record: RecordHandle, index: AreaIndex, store: &AreaStore) {  AbsorbRecords::store_area(&mut self.absorb, _record, index, store) }
    fn store_area_detail(&mut self, _record: RecordHandle, index: AreaIndex, detail: &AreaDetail) {  AbsorbRecords::store_area_detail(&mut self.absorb, _record, index, detail) }
    fn store_auxiliary(&mut self, _record: RecordHandle, index: AreaIndex, auxiliary: &AuxiliaryReference) {  AbsorbRecords::store_auxiliary(&mut self.absorb, _record, index, auxiliary) }
    fn store_cell(&mut self, _record: RecordHandle, cell: &CellSummary, weight: u32) {  AbsorbRecords::store_cell(&mut self.absorb, _record, cell, weight) }
    fn store_focus_summary(&mut self, _record: RecordHandle, summary: &FocusSummary, level: u32) {  AbsorbRecords::store_focus_summary(&mut self.absorb, _record, summary, level) }
    fn area_lens_reference(&mut self, _record: RecordHandle, index: u8) -> u32 {  AbsorbRecords::area_lens_reference(&mut self.absorb, _record, index) }
    fn extended_areas(&mut self, _record: RecordHandle) -> bool {  AbsorbRecords::extended_areas(&mut self.absorb, _record) }
    fn selection_source(&mut self, _record: RecordHandle) -> u32 {  AbsorbRecords::selection_source(&mut self.absorb, _record) }
    fn metering(&mut self, _record: RecordHandle) -> MeteringBlock {  AbsorbRecords::metering(&mut self.absorb, _record) }
    fn store_metering(&mut self, _record: RecordHandle, block: &MeteringBlock) {  AbsorbRecords::store_metering(&mut self.absorb, _record, block) }
    fn value(&mut self, _record: RecordHandle, field: RecordField) -> u32 {  AbsorbRecords::value(&mut self.absorb, _record, field) }
    fn area_references(&mut self, _record: RecordHandle) -> AreaReference {  AbsorbRecords::area_references(&mut self.absorb, _record) }
}
impl LoadRecords for ExternalFake {
    fn record_lens_snapshot(&mut self, _record: RecordHandle) -> LensSnapshot {  LoadRecords::record_lens_snapshot(&mut self.absorb, _record) }
    fn store_record_lens_snapshot(&mut self, _record: RecordHandle, snapshot: &LensSnapshot) {  LoadRecords::store_record_lens_snapshot(&mut self.absorb, _record, snapshot) }
    fn lens_reference(&mut self, _record: RecordHandle) -> u32 {  LoadRecords::lens_reference(&mut self.absorb, _record) }
    fn resume_offset(&mut self, _record: RecordHandle, source: ResumeSource) -> i16 {  LoadRecords::resume_offset(&mut self.absorb, _record, source) }
    fn store_resume_offset(&mut self, _record: RecordHandle, offset: i16) {  LoadRecords::store_resume_offset(&mut self.absorb, _record, offset) }
    fn store_reference_pair(&mut self, _record: RecordHandle, slot: PairSlot, pair: ReferencePair) {  LoadRecords::store_reference_pair(&mut self.absorb, _record, slot, pair) }
    fn store_area_starts(&mut self, _record: RecordHandle, starts: AreaStarts) {  LoadRecords::store_area_starts(&mut self.absorb, _record, starts) }
    fn mode_five_profile(&mut self, _record: RecordHandle) -> [i16; 3] {  LoadRecords::mode_five_profile(&mut self.absorb, _record) }
    fn mode_five_block(&mut self, _record: RecordHandle) -> ModeFiveBlock {  LoadRecords::mode_five_block(&mut self.absorb, _record) }
    fn store_mode_five_block(&mut self, _record: RecordHandle, block: &ModeFiveBlock) {  LoadRecords::store_mode_five_block(&mut self.absorb, _record, block) }
    fn stored_area(&mut self, _record: RecordHandle) -> StoredArea {  LoadRecords::stored_area(&mut self.absorb, _record) }
    fn record_slot(&mut self, _record: RecordHandle, slot: u8) -> SlotEntry {  LoadRecords::record_slot(&mut self.absorb, _record, slot) }
    fn store_client_slot(&mut self, target: SlotHandle, entry: &SlotEntry) {  LoadRecords::store_client_slot(&mut self.absorb, target, entry) }
    fn auxiliary_hook(&mut self, _record: RecordHandle) -> Option<StockCallbackWord> {  LoadRecords::auxiliary_hook(&mut self.absorb, _record) }
}
impl LoadEffects for ExternalFake {
    fn export_loaded(&mut self, frame: Option<Frame>, export: LoadExport) {  LoadEffects::export_loaded(&mut self.absorb, frame, export) }
    fn call_auxiliary_hook(&mut self, call: &AuxiliaryCall) {  LoadEffects::call_auxiliary_hook(&mut self.absorb, call) }
    fn publish_auxiliary_call(&mut self, call: &AuxiliaryCall) {  LoadEffects::publish_auxiliary_call(&mut self.absorb, call) }
}
}

#[allow(unused_imports)]
mod calculation_work_tests {
use super::{ExternalFake, Event};
use crate::boundary::*;
use crate::calculation::Calculation;
use crate::calculation_work::*;
use crate::ports::{Branch, CalculationEffect, CalculationPlan, Effect};
use crate::preparation::{HalfPressMask, HalfPressParameters};
use crate::ports::HalfPressMode;
use crate::summary::AreaStatistics;
use crate::tests::{record, Call, Recorder};
use crate::Outcome;
use std::cell::Cell;
use std::rc::Rc;
use xm1_saclac_interface::{Completion, RecordHandle};
impl CalculationLibrary for ExternalFake {
    fn selection_mode(&mut self, _record: RecordHandle, refinement: u32) -> u32 {  CalculationLibrary::selection_mode(&mut self.calculation, _record, refinement) }
    fn row_data(&mut self, kind: u32, mode: u32, evaluation: u32, limit: u32, option: u32) -> u32 {  CalculationLibrary::row_data(&mut self.calculation, kind, mode, evaluation, limit, option) }
    fn adjustment_status(&mut self, kind: u32, mode: u32, option: u32) -> u8 {  CalculationLibrary::adjustment_status(&mut self.calculation, kind, mode, option) }
    fn class_skips_selection(&mut self, _class: u32) -> bool {  CalculationLibrary::class_skips_selection(&mut self.calculation, _class) }
    fn display_class(&mut self, _record: RecordHandle, refinement: u32) -> u32 {  CalculationLibrary::display_class(&mut self.calculation, _record, refinement) }
    fn selection_policy(&mut self, kind: u32, display: u32, evaluation: u32, option: u32) -> u32 {  CalculationLibrary::selection_policy(&mut self.calculation, kind, display, evaluation, option) }
    fn build_input(&mut self, area: [u32; 2], selection: u32, kind: u32, exposure: [u16; 5]) { CalculationLibrary::build_input(&mut self.calculation, area, selection, kind, exposure) }
    fn class_reuses_result(&mut self, _class: u32) -> bool {  CalculationLibrary::class_reuses_result(&mut self.calculation, _class) }
}
impl CalculationProfiles for ExternalFake {
    fn profile(&mut self, _record: RecordHandle, value: CalculationProfile) -> u32 {  CalculationProfiles::profile(&mut self.calculation, _record, value) }
    fn store_resume_offset(&mut self, _record: RecordHandle, offset: u16) {  CalculationProfiles::store_resume_offset(&mut self.calculation, _record, offset) }
    fn reuse_control(&mut self) -> u32 {  CalculationProfiles::reuse_control(&mut self.calculation) }
    fn client_pair(&mut self, at: ClientRef) -> [u32; 2] {  CalculationProfiles::client_pair(&mut self.calculation, at) }
}
}

#[allow(unused_imports)]
mod pipeline_work_tests {
use super::{ExternalFake, Event};
use std::vec::Vec;
use crate::boundary::{Client, ClientRef, Frame, Lens, LensPoint, LensReading, LensReference};
use crate::finish_work::{AreaPositions, CorrectionRecord, FinishPipeline};
use crate::pipeline_work::*;
use crate::ports::{HalfPressMode, PipelineEffect as E, PipelinePhase, PipelineQuery as Q, PipelineRequest as R};
use crate::preparation::{FrameHeader, HalfPressMask, HalfPressParameters, Prepared};
use crate::summary::FrameSummary;
use xm1_saclac_interface::pipeline::*;
use xm1_saclac_interface::stock::pipeline as payloads;
use xm1_saclac_interface::{stock, Completed, Completion, RecordHandle};
impl PipelineLibrary for ExternalFake {
    fn plan(&mut self, input: &PlanInput, candidates: &crate::subjects::Candidates, selection_reference: u32) -> Option<Plan> { PipelineLibrary::plan(&mut self.pipeline, input, candidates, selection_reference) }
    fn refresh_channels(&mut self) {  PipelineLibrary::refresh_channels(&mut self.pipeline) }
    fn channel_item_ready(&mut self, _record: RecordHandle) -> bool {  PipelineLibrary::channel_item_ready(&mut self.pipeline, _record) }
    fn apply_adjustment(&mut self, _adjustment: &[PlanBlock; 7], _pass: AdjustmentPass) {  PipelineLibrary::apply_adjustment(&mut self.pipeline, _adjustment, _pass) }
    fn adjustment_gated(&mut self) -> bool {  PipelineLibrary::adjustment_gated(&mut self.pipeline) }
    fn build_scan_plan(&mut self, scan: PlanBlock) -> u32 { PipelineLibrary::build_scan_plan(&mut self.pipeline, scan) }
    fn focus_result(&mut self, _scan: PlanBlock, _kind: ResultKind) -> FocusResultOut {  PipelineLibrary::focus_result(&mut self.pipeline, _scan, _kind) }
    fn selected_value(&mut self) -> u32 {  PipelineLibrary::selected_value(&mut self.pipeline) }
    fn needs_raw(&mut self) -> bool {  PipelineLibrary::needs_raw(&mut self.pipeline) }
    fn conclude_raw(&mut self) {  PipelineLibrary::conclude_raw(&mut self.pipeline) }
    fn store_converted_input(&mut self) {  PipelineLibrary::store_converted_input(&mut self.pipeline) }
    fn begin_scan(&mut self) {  PipelineLibrary::begin_scan(&mut self.pipeline) }
    fn active_channels(&mut self) -> u8 {  PipelineLibrary::active_channels(&mut self.pipeline) }
    fn position_correction(&mut self) -> u32 {  PipelineLibrary::position_correction(&mut self.pipeline) }
    fn scaled_value(&mut self, _scan: PlanBlock, index: u8) -> u16 {  PipelineLibrary::scaled_value(&mut self.pipeline, _scan, index) }
    fn scaled_levels(&mut self, _scan: PlanBlock, value: u16, index: u8) -> [i16; TONE_LEVELS] {  PipelineLibrary::scaled_levels(&mut self.pipeline, _scan, value, index) }
    fn store_channel_samples(&mut self, _scan: PlanBlock, _snapshot: &SampleSnapshot) {  PipelineLibrary::store_channel_samples(&mut self.pipeline, _scan, _snapshot) }
    fn store_channel_sample(&mut self, _scan: PlanBlock, sample: i16, index: u8) {  PipelineLibrary::store_channel_sample(&mut self.pipeline, _scan, sample, index) }
    fn select_channel(&mut self, channel: u8) {  PipelineLibrary::select_channel(&mut self.pipeline, channel) }
    fn indexed_selection(&mut self, _record: RecordHandle) -> u32 {  PipelineLibrary::indexed_selection(&mut self.pipeline, _record) }
    fn adjust_direction(&mut self, point: &LensPoint) -> i16 {  PipelineLibrary::adjust_direction(&mut self.pipeline, point) }
    fn phase_attribute(&mut self, phase: u32, _attribute: PhaseAttribute) -> bool {  PipelineLibrary::phase_attribute(&mut self.pipeline, phase, _attribute) }
    fn request_raw(&mut self, _plan: &Plan, completion: Completion) {  PipelineLibrary::request_raw(&mut self.pipeline, _plan, completion) }
    fn request_tone(&mut self, request: &ToneRequest, _completion: Completion) {  PipelineLibrary::request_tone(&mut self.pipeline, request, _completion) }
    fn request_gradation(&mut self, request: &GradationRequest, _completion: Completion) {  PipelineLibrary::request_gradation(&mut self.pipeline, request, _completion) }
}
impl PlanMemory for ExternalFake {
    fn subject(&mut self, _block: PlanBlock) -> SubjectBlock {  PlanMemory::subject(&mut self.pipeline, _block) }
    fn capture(&mut self, _block: PlanBlock) -> CaptureBlock {  PlanMemory::capture(&mut self.pipeline, _block) }
    fn lens(&mut self, _block: PlanBlock) -> LensBlock {  PlanMemory::lens(&mut self.pipeline, _block) }
    fn scan(&mut self, _block: PlanBlock) -> ScanBlock {  PlanMemory::scan(&mut self.pipeline, _block) }
    fn raw_selection(&mut self, _block: PlanBlock) -> u32 {  PlanMemory::raw_selection(&mut self.pipeline, _block) }
    fn raw_first_word(&mut self, _block: PlanBlock) -> u32 {  PlanMemory::raw_first_word(&mut self.pipeline, _block) }
    fn store_scan(&mut self, _block: PlanBlock, store: ScanStore) {  PlanMemory::store_scan(&mut self.pipeline, _block, store) }
    fn mark_lens_driven(&mut self, _block: PlanBlock) {  PlanMemory::mark_lens_driven(&mut self.pipeline, _block) }
    fn store_raw_calculation(&mut self, block: PlanBlock, result: &RawCalculation) {  PlanMemory::store_raw_calculation(&mut self.pipeline, block, result) }
}
impl PipelineRecords for ExternalFake {
    fn stamp_frame(&mut self, frame: Frame, _lens_reference: u32) {  PipelineRecords::stamp_frame(&mut self.pipeline, frame, _lens_reference) }
    fn resource(&mut self, _record: RecordHandle) -> u32 {  PipelineRecords::resource(&mut self.pipeline, _record) }
    fn resource_bank(&mut self, _record: RecordHandle) -> u32 {  PipelineRecords::resource_bank(&mut self.pipeline, _record) }
    fn outcome_history(&mut self, _record: RecordHandle) -> OutcomeHistory {  PipelineRecords::outcome_history(&mut self.pipeline, _record) }
    fn store_outcome_history(&mut self, _record: RecordHandle, history: &OutcomeHistory) {  PipelineRecords::store_outcome_history(&mut self.pipeline, _record, history) }
    fn store_outcome(&mut self, _record: RecordHandle, value: OutcomeValue) {  PipelineRecords::store_outcome(&mut self.pipeline, _record, value) }
    fn store_outcome_correction(&mut self, _record: RecordHandle, correction: &CorrectionRecord) {  PipelineRecords::store_outcome_correction(&mut self.pipeline, _record, correction) }
    fn store_area_block(&mut self, _record: RecordHandle, slot: AreaSlot, areas: &AreaBlock) {  PipelineRecords::store_area_block(&mut self.pipeline, _record, slot, areas) }
    fn store_outcome_positions(&mut self, _record: RecordHandle, positions: &AreaPositions) {  PipelineRecords::store_outcome_positions(&mut self.pipeline, _record, positions) }
    fn statistics_values(&mut self, _record: RecordHandle) -> StatisticsRecordValues {  PipelineRecords::statistics_values(&mut self.pipeline, _record) }
    fn lens_value(&mut self, value: PipelineLens) -> u32 {  PipelineRecords::lens_value(&mut self.pipeline, value) }
    fn position_code(&mut self, point: &LensPoint, _drive: u32) -> u16 {  PipelineRecords::position_code(&mut self.pipeline, point, _drive) }
    fn tone_level(&mut self, level: i16, _reference: u32, _state: u32, _geometry: u32, _mode: u32) -> u32 {  PipelineRecords::tone_level(&mut self.pipeline, level, _reference, _state, _geometry, _mode) }
    fn level_bounds(&mut self, _state: u32, _reference: u32) -> (u32, u32) {  PipelineRecords::level_bounds(&mut self.pipeline, _state, _reference) }
    fn level_value(&mut self, level: u32) -> u32 {  PipelineRecords::level_value(&mut self.pipeline, level) }
    fn channel_threshold(&mut self, _reference: u32, _state: u32, scaled: u32) -> u32 {  PipelineRecords::channel_threshold(&mut self.pipeline, _reference, _state, scaled) }
    fn correction_base(&mut self, _tolerance_kind: u32) -> u16 {  PipelineRecords::correction_base(&mut self.pipeline, _tolerance_kind) }
}
impl PipelineProfiles for ExternalFake {
    fn requires_adjustment(&mut self, _profile: u32) -> bool {  PipelineProfiles::requires_adjustment(&mut self.pipeline, _profile) }
    fn record_mode_available(&mut self, _mode: u32) -> bool {  PipelineProfiles::record_mode_available(&mut self.pipeline, _mode) }
    fn begin_record_mode(&mut self, _mode: u32) {  PipelineProfiles::begin_record_mode(&mut self.pipeline, _mode) }
    fn finish_record_mode(&mut self, threshold: u8) {  PipelineProfiles::finish_record_mode(&mut self.pipeline, threshold) }
    fn configure_extended_operations(&mut self) {  PipelineProfiles::configure_extended_operations(&mut self.pipeline) }
    fn configure_secondary_operations(&mut self) {  PipelineProfiles::configure_secondary_operations(&mut self.pipeline) }
}
impl PipelinePeers for ExternalFake {
    fn request_raw_frame(&mut self, request: &RawFrameRequest, _completion: Completion) { self.events.push(Event::RawFrame(*request)); PipelinePeers::request_raw_frame(&mut self.pipeline, request, _completion) }
    fn request_frame(&mut self, request: &FrameRequest, _completion: Completion) { self.events.push(Event::Frame(*request)); PipelinePeers::request_frame(&mut self.pipeline, request, _completion) }
    fn request_scan(&mut self, request: &ScanRequest, progress: Completion, status: Completion) {  PipelinePeers::request_scan(&mut self.pipeline, request, progress, status) }
    fn request_selection(&mut self, request: SelectionRequest) {  PipelinePeers::request_selection(&mut self.pipeline, request) }
    fn request_scan_selection(&mut self) {  PipelinePeers::request_scan_selection(&mut self.pipeline) }
    fn request_output(&mut self, _completion: Completion) { self.events.push(Event::Output); PipelinePeers::request_output(&mut self.pipeline, _completion) }
    fn request_commit(&mut self, _completion: Completion) { self.events.push(Event::Commit); PipelinePeers::request_commit(&mut self.pipeline, _completion) }
    fn request_statistics(&mut self, setup: &StatisticsSetup, _completion: Completion) {  PipelinePeers::request_statistics(&mut self.pipeline, setup, _completion) }
    fn request_statistics_commit(&mut self, _completion: Completion) {  PipelinePeers::request_statistics_commit(&mut self.pipeline, _completion) }
    fn store_statistics_weights(&mut self, target: u32, weights: &StatisticsWeights) {  PipelinePeers::store_statistics_weights(&mut self.pipeline, target, weights) }
    fn statistics_target(&mut self, _lens: PlanBlock) -> u32 {  PipelinePeers::statistics_target(&mut self.pipeline, _lens) }
    fn capture_idle(&mut self) -> bool {  PipelinePeers::capture_idle(&mut self.pipeline) }
    fn take_sample(&mut self, _level: u16, _targets: &[PlanBlock; 5]) -> Option<i16> {  PipelinePeers::take_sample(&mut self.pipeline, _level, _targets) }
    fn channel_sample(&mut self, index: u8) -> i16 {  PipelinePeers::channel_sample(&mut self.pipeline, index) }
    fn sample_snapshot(&mut self) -> SampleSnapshot {  PipelinePeers::sample_snapshot(&mut self.pipeline) }
}
impl PipelinePlatform for ExternalFake {
    fn setting(&mut self, setting: PipelineSetting) -> u32 {  PipelinePlatform::setting(&mut self.pipeline, setting) }
    fn load_test_state(&mut self) -> u32 {  PipelinePlatform::load_test_state(&mut self.pipeline) }
    fn load_test_setting(&mut self) -> u32 {  PipelinePlatform::load_test_setting(&mut self.pipeline) }
    fn load_test_register(&mut self, register: u8) -> u8 {  PipelinePlatform::load_test_register(&mut self.pipeline, register) }
    fn set_load_test_register(&mut self, register: u8, value: u8) {  PipelinePlatform::set_load_test_register(&mut self.pipeline, register, value) }
    fn trace(&mut self, trace: PipelineTrace) {  PipelinePlatform::trace(&mut self.pipeline, trace) }
    fn stamp_pipeline_frame(&mut self, _frame: Frame, words: &PipelineFrame) {  PipelinePlatform::stamp_pipeline_frame(&mut self.pipeline, _frame, words) }
    fn clear_frame_work(&mut self, frame: Frame) {  PipelinePlatform::clear_frame_work(&mut self.pipeline, frame) }
    fn store_bound_frame(&mut self, frame: Option<Frame>) {  PipelinePlatform::store_bound_frame(&mut self.pipeline, frame) }
    fn restart_marked(&mut self) -> bool {  PipelinePlatform::restart_marked(&mut self.pipeline) }
    fn mark_restart(&mut self) {  PipelinePlatform::mark_restart(&mut self.pipeline) }
    fn store_work(&mut self, area: WorkArea, store: WorkStore) {  PipelinePlatform::store_work(&mut self.pipeline, area, store) }
    fn work_marked(&mut self, _area: WorkArea) -> bool {  PipelinePlatform::work_marked(&mut self.pipeline, _area) }
    fn flush_work(&mut self, area: WorkArea) {  PipelinePlatform::flush_work(&mut self.pipeline, area) }
    fn publish_statistics_progress(&mut self, enabled: bool) { PipelinePlatform::publish_statistics_progress(&mut self.pipeline, enabled); }
}
}

#[allow(unused_imports)]
mod finish_work_tests {
use super::{ExternalFake, Event};
use crate::boundary::*;
use crate::capture_boundary::{LensMoves, MoveProfile};
use crate::finish_work::*;
use crate::ports::{FinishEffect, FinishQuery, HalfPressMode, LensMove};
use crate::preparation::{FrameHeader, HalfPressMask, HalfPressParameters, Prepared};
use crate::summary::{AreaStatistics, FrameSummary};
use crate::tests::record;
use std::vec::Vec;
use xm1_saclac_interface::{Completion, RecordHandle};
impl FinishLayers for ExternalFake {
    fn profile(&mut self, _record: RecordHandle, value: FinishProfile) -> u32 {  FinishLayers::profile(&mut self.finish, _record, value) }
    fn lens(&mut self, value: FinishLensValue) -> u32 {  FinishLayers::lens(&mut self.finish, value) }
    fn lens_pair(&mut self) -> (u32, u16) {  FinishLayers::lens_pair(&mut self.finish) }
    fn configuration(&mut self, value: FinishConfiguration) -> u32 {  FinishLayers::configuration(&mut self.finish, value) }
    fn load_test(&mut self) -> u32 {  FinishLayers::load_test(&mut self.finish) }
    fn trace(&mut self, trace: FinishTrace) {  FinishLayers::trace(&mut self.finish, trace) }
    fn designated_point(&mut self, _record: RecordHandle) -> LensPoint {  FinishLayers::designated_point(&mut self.finish, _record) }
    fn area_positions(&mut self, _record: RecordHandle) -> AreaPositions {  FinishLayers::area_positions(&mut self.finish, _record) }
    fn store_area_positions(&mut self, _record: RecordHandle, positions: &AreaPositions) {  FinishLayers::store_area_positions(&mut self.finish, _record, positions) }
    fn correction_record(&mut self, _record: RecordHandle) -> CorrectionRecord {  FinishLayers::correction_record(&mut self.finish, _record) }
    fn store_correction_record(&mut self, _record: RecordHandle, correction: &CorrectionRecord) {  FinishLayers::store_correction_record(&mut self.finish, _record, correction) }
    fn clamp_step(&mut self, position: i16, _kind: u32, direction: u32) -> i16 {  FinishLayers::clamp_step(&mut self.finish, position, _kind, direction) }
    fn adjust(&mut self, position: i16, _point: &LensPoint, _area_flag: bool) -> i16 {  FinishLayers::adjust(&mut self.finish, position, _point, _area_flag) }
    fn near_bounds(&mut self, position: i16, distance: u32, target: u32, _drive: u32, _direction: u32, _step: u32) -> [u32; 3] {  FinishLayers::near_bounds(&mut self.finish, position, distance, target, _drive, _direction, _step) }
    fn far_bounds(&mut self, position: i16, pair: (u32, u16), target: u32, _drive: u32, _direction: u32, _step: u32) -> [u32; 3] {  FinishLayers::far_bounds(&mut self.finish, position, pair, target, _drive, _direction, _step) }
    fn set_lens_target(&mut self, position: i16, reference: LensReference, _drive: u32, kind: u32) {  FinishLayers::set_lens_target(&mut self.finish, position, reference, _drive, kind) }
    fn store_area_selection(&mut self, _record: RecordHandle, index: u8, selection: u32) {  FinishLayers::store_area_selection(&mut self.finish, _record, index, selection) }
    fn store_area_mode(&mut self, _record: RecordHandle, index: u8, mode: u32) {  FinishLayers::store_area_mode(&mut self.finish, _record, index, mode) }
    fn notify_bounds(&mut self, bounds: [u32; 3]) {  FinishLayers::notify_bounds(&mut self.finish, bounds) }
}
impl FinishSources for ExternalFake {
    fn target_source(&mut self) -> TargetSource {  FinishSources::target_source(&mut self.finish) }
    fn capture_slots(&mut self) -> (u8, u8) {  FinishSources::capture_slots(&mut self.finish) }
    fn capture_target(&mut self, index: usize) -> u32 {  FinishSources::capture_target(&mut self.finish, index) }
    fn frame_drive(&mut self, _frame: Frame) -> u32 {  FinishSources::frame_drive(&mut self.finish, _frame) }
    fn store_frame_correction(&mut self, _frame: Frame, correction: u32) {  FinishSources::store_frame_correction(&mut self.finish, _frame, correction) }
    fn capture_status(&mut self) -> u32 {  FinishSources::capture_status(&mut self.finish) }
}
impl FinishLibrary for ExternalFake {
    fn point_selection(&mut self, point: &LensPoint, _lens_reference: LensReference, _state: u32) -> u32 {  FinishLibrary::point_selection(&mut self.finish, point, _lens_reference, _state) }
    fn target_selection(&mut self, _record: RecordHandle, _lens_reference: LensReference, _position: i16, _control: u32, _kind: u32, _lens_control: u32, _frame: u32) -> u32 {  FinishLibrary::target_selection(&mut self.finish, _record, _lens_reference, _position, _control, _kind, _lens_control, _frame) }
    fn mode_selection(&mut self, _record: RecordHandle, _mode: u32) -> u32 {  FinishLibrary::mode_selection(&mut self.finish, _record, _mode) }
    fn correction(&mut self, _point: &LensPoint, _drive: u32, _frame_drive: u32) -> u32 {  FinishLibrary::correction(&mut self.finish, _point, _drive, _frame_drive) }
    fn adjust_position(&mut self, adjustment: PositionAdjustment) -> i16 {  FinishLibrary::adjust_position(&mut self.finish, adjustment) }
}
}

#[allow(unused_imports)]
mod primary_job_tests {
use super::{ExternalFake, Event};
use crate::boundary::{ClientRef, Frame, Lens, LensPoint, LensReading};
use crate::calculations_work::{BinnedPlanes, CaptureCommand, CaptureProcessing, Processing, ProcessingSelection, StatisticsPlanes, GRID, PLANES};
use crate::capture_boundary::*;
use crate::capture_control::CaptureControl;
use crate::capture_stock;
use crate::ports::{CalculationsJob, LensMove, PrimaryEffect, PrimaryQuery};
use crate::primary_boundary::*;
use crate::primary_job::{CaptureSteps, PrimaryJob};
use crate::secondary_boundary::{CropGeometry, CropRequest, GeometryRequest, MeasurementCalc, MeasurementProfiles, ProfileGeometry, ScalePair};
use crate::subjects::SubjectList;
use crate::tests::record;
use std::collections::BTreeMap;
use std::vec::Vec;
use xm1_saclac_interface::area_measurement::{AreaEntry, CaptureSelection};
use xm1_saclac_interface::calc::AreaGeometry;
use xm1_saclac_interface::capture_job::{CaptureEnd, CaptureRequest, CaptureWindow, DisplayRequest, OutputSelection};
use xm1_saclac_interface::statistics::StatisticsSetting;
use xm1_saclac_interface::{stock, Completed, Completion, RecordHandle};
impl CaptureJobClient for ExternalFake {
    fn source_words(&mut self, _sources: ClientRef) -> SourceWords {  CaptureJobClient::source_words(&mut self.primary, _sources) }
    fn store_source_output(&mut self, _sources: ClientRef, marker: u16, value: u32) {  CaptureJobClient::store_source_output(&mut self.primary, _sources, marker, value) }
    fn selection_option(&mut self, _selection: ClientRef) -> u8 {  CaptureJobClient::selection_option(&mut self.primary, _selection) }
    fn readout_mode(&mut self, _selection: ClientRef) -> u32 {  CaptureJobClient::readout_mode(&mut self.primary, _selection) }
    fn store_output(&mut self, _selection: ClientRef, output: &OutputSelection, offset: u32, rate: u32) {  CaptureJobClient::store_output(&mut self.primary, _selection, output, offset, rate) }
    fn store_capture_end(&mut self, _selection: ClientRef, end: &CaptureEnd) {  CaptureJobClient::store_capture_end(&mut self.primary, _selection, end) }
    fn calibration_block(&mut self, _block: ClientRef) -> ClientCalibration {  CaptureJobClient::calibration_block(&mut self.primary, _block) }
    fn statistics_window(&mut self, _block: ClientRef) -> [u32; 4] {  CaptureJobClient::statistics_window(&mut self.primary, _block) }
    fn statistics_selector(&mut self, _block: ClientRef) -> u32 {  CaptureJobClient::statistics_selector(&mut self.primary, _block) }
    fn clear_area_list(&mut self, _buffer: ClientRef, list: DeliveredList) {  CaptureJobClient::clear_area_list(&mut self.primary, _buffer, list) }
    fn area_count(&mut self, _buffer: ClientRef, list: DeliveredList) -> u32 {  CaptureJobClient::area_count(&mut self.primary, _buffer, list) }
    fn area(&mut self, _buffer: ClientRef, list: DeliveredList, index: u8) -> AreaEntry {  CaptureJobClient::area(&mut self.primary, _buffer, list, index) }
    fn set_area(&mut self, _buffer: ClientRef, list: DeliveredList, index: u8, area: &AreaEntry) {  CaptureJobClient::set_area(&mut self.primary, _buffer, list, index, area) }
}
impl CaptureJobRecords for ExternalFake {
    fn record_calibration(&mut self, _record: RecordHandle) -> Calibration {  CaptureJobRecords::record_calibration(&mut self.primary, _record) }
    fn statistics_option(&mut self, _record: RecordHandle) -> (u32, u8) {  CaptureJobRecords::statistics_option(&mut self.primary, _record) }
    fn slot_positions(&mut self, _record: RecordHandle) -> SlotPositions {  CaptureJobRecords::slot_positions(&mut self.primary, _record) }
    fn setting_result(&mut self, _record: RecordHandle) -> SettingResult {  CaptureJobRecords::setting_result(&mut self.primary, _record) }
    fn slot_record(&mut self, slot: SlotHandle) -> SlotEntry {  CaptureJobRecords::slot_record(&mut self.primary, slot) }
    fn delivered_count(&mut self, _record: RecordHandle, list: DeliveredList) -> u32 {  CaptureJobRecords::delivered_count(&mut self.primary, _record, list) }
    fn delivered_area(&mut self, _record: RecordHandle, list: DeliveredList, index: u8) -> AreaEntry {  CaptureJobRecords::delivered_area(&mut self.primary, _record, list, index) }
    fn store_delivered_count(&mut self, _record: RecordHandle, list: DeliveredList, count: u32) {  CaptureJobRecords::store_delivered_count(&mut self.primary, _record, list, count) }
    fn store_delivered_area(&mut self, _record: RecordHandle, list: DeliveredList, index: u8, area: &AreaEntry) {  CaptureJobRecords::store_delivered_area(&mut self.primary, _record, list, index, area) }
}
impl CapturePipeline for ExternalFake {
    fn capture(&mut self, request: &CaptureRequest, completions: CaptureCompletions) { self.events.push(Event::Capture(*request, completions)); CapturePipeline::capture(&mut self.primary, request, completions) }
    fn request_command_c(&mut self) { self.events.push(Event::CommandC); CapturePipeline::request_command_c(&mut self.primary) }
    fn request_command_28(&mut self) { self.events.push(Event::Command28); CapturePipeline::request_command_28(&mut self.primary) }
    fn display(&mut self, request: &DisplayRequest) {  CapturePipeline::display(&mut self.primary, request) }
    fn readout_processing(&mut self, _source: u8) -> Option<Processing> { self.readout_processing }
    fn keeps_template(&mut self, _subject_source: u32, _options: u32, _mode: u8, _profile_class: u32) -> bool {  CapturePipeline::keeps_template(&mut self.primary, _subject_source, _options, _mode, _profile_class) }
    fn statistics_ready(&mut self) -> bool {  CapturePipeline::statistics_ready(&mut self.primary) }
}
impl ProfileCalibration for ExternalFake {
    fn calibrate(&mut self, request: &FactorRequest) -> Calibration {  ProfileCalibration::calibrate(&mut self.primary, request) }
    fn clamp_geometry(&mut self, word: u32, calibration: &Calibration, rate: u8, exposure: i16) -> Calibration {  ProfileCalibration::clamp_geometry(&mut self.primary, word, calibration, rate, exposure) }
    fn apply_positive(&mut self, exposure: i32, calibration: &mut Calibration) {  ProfileCalibration::apply_positive(&mut self.primary, exposure, calibration) }
    fn clamp_output(&mut self, calibration: &Calibration) -> Calibration {  ProfileCalibration::clamp_output(&mut self.primary, calibration) }
}
impl CaptureJobCalc for ExternalFake {
    fn default_calibration(&mut self, seed: &CalibrationSeed) -> (Calibration, u32) {  CaptureJobCalc::default_calibration(&mut self.primary, seed) }
    fn output_scale(&mut self, _index: i16, _distance: u32, _reference: u32, _words: [u32; 2]) -> i32 {  CaptureJobCalc::output_scale(&mut self.primary, _index, _distance, _reference, _words) }
}
impl CaptureProfiles for ExternalFake {
    fn area_selector(&mut self, profile: u32, index: u8) -> u32 {  CaptureProfiles::area_selector(&mut self.primary, profile, index) }
    fn display_layout(&mut self, _selector: u32, _kind: u8) -> DisplayLayout {  CaptureProfiles::display_layout(&mut self.primary, _selector, _kind) }
    fn paired(&mut self, _selector: u32) -> bool {  CaptureProfiles::paired(&mut self.primary, _selector) }
    fn sensor_window(&mut self, crop: &CropGeometry) -> [u32; 4] {  CaptureProfiles::sensor_window(&mut self.primary, crop) }
    fn generate_areas(&mut self, mode: AreaMode, selector: u32, frame: &AreaFrame, _buffer: ClientRef) {  CaptureProfiles::generate_areas(&mut self.primary, mode, selector, frame, _buffer) }
    fn frame_areas(&mut self, words: &GeometryWords, corner: [u32; 2], _buffer: ClientRef) {  CaptureProfiles::frame_areas(&mut self.primary, words, corner, _buffer) }
    fn display_extent(&mut self, selector: u32, pair: [u32; 2], words: &GeometryWords) -> [u32; 2] {  CaptureProfiles::display_extent(&mut self.primary, selector, pair, words) }
    fn measurement_table(&mut self) -> [u32; 18] {  CaptureProfiles::measurement_table(&mut self.primary) }
}
impl CaptureReplies for ExternalFake {
    fn reply(&mut self, handle: RecordHandle, status: CaptureStatus) { self.events.push(Event::Reply(crate::ports::Reply { command: xm1_saclac_interface::CommandKind::Command07, record: Some(handle), status: xm1_saclac_interface::CompletionStatus(crate::capture_stock::capture_status(status) as i32) })); CaptureReplies::reply(&mut self.primary, handle, status) }
}
}

#[allow(unused_imports)]
mod secondary_job_tests {
use super::{ExternalFake, Event};
use crate::boundary::{ClientRef, Frame};
use crate::calculations_work::Processing;
use crate::capture_boundary::*;
use crate::capture_stock;
use crate::finish_work::FinishLensValue;
use crate::ports::{SecondaryEffect, SecondaryQuery};
use crate::secondary_boundary::*;
use crate::secondary_job::SecondaryJob;
use std::collections::BTreeMap;
use std::vec::Vec;
use xm1_saclac_calc::geometry::{ClipRule, ClipSettings};
use xm1_saclac_interface::area_measurement::{AreaEntry, CaptureSelection, EvaluatedList, MeasurementRequest, TransferRequest};
use xm1_saclac_interface::calc::AreaGeometry;
use xm1_saclac_interface::{stock, Completed, Completion, RecordHandle};
impl AreaLists for ExternalFake {
    fn area_count(&mut self, _record: RecordHandle, list: AreaList) -> u32 {  AreaLists::area_count(&mut self.secondary, _record, list) }
    fn area(&mut self, _record: RecordHandle, list: AreaList, index: u8) -> AreaEntry {  AreaLists::area(&mut self.secondary, _record, list, index) }
    fn list_detail(&mut self, _record: RecordHandle, list: AreaList) -> u32 {  AreaLists::list_detail(&mut self.secondary, _record, list) }
    fn prefers_second_list(&mut self, _record: RecordHandle) -> bool {  AreaLists::prefers_second_list(&mut self.secondary, _record) }
    fn store_slot_extent(&mut self, _record: RecordHandle, extent: [u32; 4]) {  AreaLists::store_slot_extent(&mut self.secondary, _record, extent) }
    fn store_slot_cells(&mut self, _record: RecordHandle, cells: [u32; 4]) {  AreaLists::store_slot_cells(&mut self.secondary, _record, cells) }
}
impl MeasurementCalc for ExternalFake {
    fn capture_selection(&mut self, _record: RecordHandle) -> CaptureSelection {  MeasurementCalc::capture_selection(&mut self.secondary, _record) }
    fn statistics_key(&mut self, processing: &Processing) -> [i16; 2] {  MeasurementCalc::statistics_key(&mut self.secondary, processing) }
    fn scale_step(&mut self, index: u16) -> i16 {  MeasurementCalc::scale_step(&mut self.secondary, index) }
    fn capture_offset(&mut self, _record: RecordHandle) -> u32 {  MeasurementCalc::capture_offset(&mut self.secondary, _record) }
    fn curve_within(&mut self, distance: u32, selection: u32, drive: u32, limit: i32) -> bool {  MeasurementCalc::curve_within(&mut self.secondary, distance, selection, drive, limit) }
    fn request_evaluation(&mut self, _record: RecordHandle, _processing: &Processing, frames: [u32; 2], geometry: &AreaGeometry, completions: [Completion; 2]) {  MeasurementCalc::request_evaluation(&mut self.secondary, _record, _processing, frames, geometry, completions) }
}
impl MeasurementProfiles for ExternalFake {
    fn profile_geometry(&mut self, request: &GeometryRequest) -> ProfileGeometry {  MeasurementProfiles::profile_geometry(&mut self.secondary, request) }
    fn crop_geometry(&mut self, request: &CropRequest) -> CropGeometry {  MeasurementProfiles::crop_geometry(&mut self.secondary, request) }
    fn flagged_scale_pair(&mut self, _selector: u32) -> ScalePair {  MeasurementProfiles::flagged_scale_pair(&mut self.secondary, _selector) }
    fn profile_extent(&mut self, _selector: u32, inset: bool) -> [u32; 4] {  MeasurementProfiles::profile_extent(&mut self.secondary, _selector, inset) }
}
impl MeasurementLens for ExternalFake {
    fn lens(&mut self, value: FinishLensValue) -> u32 {  MeasurementLens::lens(&mut self.secondary, value) }
    fn clamp_step(&mut self, position: i16, _kind: u32, _direction: u32) -> i16 {  MeasurementLens::clamp_step(&mut self.secondary, position, _kind, _direction) }
    fn reference_value(&mut self, index: i32) -> u32 {  MeasurementLens::reference_value(&mut self.secondary, index) }
}
impl MeasurementPipeline for ExternalFake {
    fn prepare_statistics(&mut self, source: u8, store: StatisticsStore, processing: &mut Processing) -> bool {  MeasurementPipeline::prepare_statistics(&mut self.secondary, source, store, processing) }
    fn measurement_word(&mut self, word: MeasurementWord) -> u32 {  MeasurementPipeline::measurement_word(&mut self.secondary, word) }
    fn release_measurement(&mut self) {  MeasurementPipeline::release_measurement(&mut self.secondary) }
    fn measure_areas(&mut self, request: &MeasurementRequest, areas: &[AreaEntry; 3], completions: [Completion; 2]) {
        self.events.push(Event::Measure(*request, completions));
        MeasurementPipeline::measure_areas(&mut self.secondary, request, areas, completions)
    }
    fn transfer(&mut self, request: &TransferRequest, completion: Completion) {  MeasurementPipeline::transfer(&mut self.secondary, request, completion) }
}
impl MeasurementSettings for ExternalFake {
    fn measurement_setting(&mut self, setting: MeasurementSetting) -> u32 {  MeasurementSettings::measurement_setting(&mut self.secondary, setting) }
}
impl ClipSettings for ExternalFake {
    fn clip_rule(&mut self) -> ClipRule {  ClipSettings::clip_rule(&mut self.secondary) }
    fn minimum_visible_percent(&mut self) -> u32 {  ClipSettings::minimum_visible_percent(&mut self.secondary) }
}
}

#[allow(unused_imports)]
mod calculations_work_tests {
use super::{ExternalFake, Event};
use crate::calculations_work::*;
use crate::jobs::Calculations;
use crate::ports::*;
use crate::tests::record;
use crate::Outcome;
use std::vec::Vec;
use xm1_saclac_interface::capture_results::{FirstResult, FourthResult, SecondResult, ThirdResult};
use xm1_saclac_interface::{stock, BlockingRequest, Completed, Completion, RecordHandle};
impl CaptureCommand for ExternalFake {
    fn jobs(&mut self) -> u32 {  CaptureCommand::jobs(&mut self.calculations) }
    fn detail(&mut self) -> u32 {  CaptureCommand::detail(&mut self.calculations) }
    fn processing_word(&mut self) -> u32 {  CaptureCommand::processing_word(&mut self.calculations) }
    fn fourth_target(&mut self) -> bool {  CaptureCommand::fourth_target(&mut self.calculations) }
    fn publish_readout(&mut self, mode: u32, value: u32) { self.events.push(Event::Readout(mode, value)); CaptureCommand::publish_readout(&mut self.calculations, mode, value) }
    fn publish_planes(&mut self, planes: &BinnedPlanes, extent: [u16; 2]) {  CaptureCommand::publish_planes(&mut self.calculations, planes, extent) }
    fn publish(&mut self, job: CalculationsJob, _record: RecordHandle) { self.events.push(Event::Published(job)); CaptureCommand::publish(&mut self.calculations, job, _record) }
}
impl CaptureProcessing for ExternalFake {
    fn configure(&mut self, source: u8, statistics: bool, processing: &mut Processing, client: u32, detail: u32, selection: &ProcessingSelection) -> u32 {
        self.events.push(Event::Configured { source, statistics, selection: *selection });
        CaptureProcessing::configure(&mut self.calculations, source, statistics, processing, client, detail, selection)
    }
    fn statistics(&mut self) -> StatisticsPlanes {  CaptureProcessing::statistics(&mut self.calculations) }
}
impl CalculationsLibrary for ExternalFake {
    fn request_first(&mut self, _record: RecordHandle, _processing: &Processing) { self.events.push(Event::Job(crate::ports::CalculationsJob::First)); CalculationsLibrary::request_first(&mut self.calculations, _record, _processing) }
    fn request_second(&mut self, input: &SecondJobInput, _processing: &Processing) { self.events.push(Event::SecondJob(*input)); CalculationsLibrary::request_second(&mut self.calculations, input, _processing) }
    fn request_third(&mut self, _record: RecordHandle, _processing: &Processing, detail: u32) { self.events.push(Event::Job(crate::ports::CalculationsJob::Third)); CalculationsLibrary::request_third(&mut self.calculations, _record, _processing, detail) }
    fn request_fourth(&mut self, _record: RecordHandle, _processing: &Processing, detail: u32) { self.events.push(Event::Job(crate::ports::CalculationsJob::Fourth)); CalculationsLibrary::request_fourth(&mut self.calculations, _record, _processing, detail) }
}
impl CalculationsRecords for ExternalFake {
    fn configuration(&mut self) -> u32 {  CalculationsRecords::configuration(&mut self.calculations) }
    fn offset_unit(&mut self) -> u16 {  CalculationsRecords::offset_unit(&mut self.calculations) }
    fn paired_mode(&mut self, _record: RecordHandle) -> u32 {  CalculationsRecords::paired_mode(&mut self.calculations, _record) }
    fn profile_kind(&mut self, _record: RecordHandle) -> u32 {  CalculationsRecords::profile_kind(&mut self.calculations, _record) }
    fn slot(&mut self, _record: RecordHandle) -> [u8; 12] {  CalculationsRecords::slot(&mut self.calculations, _record) }
    fn store_first(&mut self, _record: RecordHandle, result: &FirstResult) { self.events.push(Event::First(*result)); CalculationsRecords::store_first(&mut self.calculations, _record, result) }
    fn store_second(&mut self, _record: RecordHandle, result: &SecondResult) { self.events.push(Event::Second(*result)); CalculationsRecords::store_second(&mut self.calculations, _record, result) }
    fn store_third(&mut self, _record: RecordHandle, result: ThirdResult) { self.events.push(Event::Third(result)); CalculationsRecords::store_third(&mut self.calculations, _record, result) }
    fn store_fourth(&mut self, _record: RecordHandle, result: FourthResult) { self.events.push(Event::Fourth(result)); CalculationsRecords::store_fourth(&mut self.calculations, _record, result) }
}
}

#[allow(unused_imports)]
mod capture_dump_tests {
use super::{ExternalFake, Event};
use crate::boundary::{ClientRef, FocusResult, Frame, LensReference};
use crate::calculations_work::Processing;
use crate::capture_boundary::*;
use crate::capture_control::CaptureControl;
use crate::capture_dump;
use crate::dump_boundary::*;
use crate::primary_job::PrimaryJob;
use crate::secondary_boundary::MeasurementCalc;
use crate::subjects::SubjectList;
use std::collections::BTreeMap;
use std::vec::Vec;
use xm1_saclac_interface::area_measurement::{AreaEntry, CaptureSelection};
use xm1_saclac_interface::calc::AreaGeometry;
use xm1_saclac_interface::{stock, Completion, RecordHandle};
impl DumpWorkspaces for ExternalFake {
    fn workspace_field(&mut self, workspace: Workspace, field: WorkspaceField) -> u32 {  DumpWorkspaces::workspace_field(&mut self.dump, workspace, field) }
}
impl DumpSettings for ExternalFake {
    fn adjustment_byte(&mut self, key: AdjustmentKey) -> u8 {  DumpSettings::adjustment_byte(&mut self.dump, key) }
    fn settings_class(&mut self) -> u32 {  DumpSettings::settings_class(&mut self.dump) }
    fn export_configuration(&mut self, frame: Frame, distance: u32) {  DumpSettings::export_configuration(&mut self.dump, frame, distance) }
}
impl DumpShared for ExternalFake {
    fn capture_window(&mut self, mode: u32, layout: [u16; 2]) -> Option<[u32; 2]> {  DumpShared::capture_window(&mut self.dump, mode, layout) }
}
}
