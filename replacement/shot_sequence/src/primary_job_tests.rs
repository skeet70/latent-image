use crate::boundary::{ClientRef, Frame, Lens, LensPoint, LensReading};
use crate::calculations_work::{BinnedPlanes, CaptureCommand, CaptureProcessing, Processing, ProcessingSelection, StatisticsPlanes, GRID, PLANES};
use crate::capture_boundary::*;
use crate::capture_control::CaptureControl;
use crate::capture_stock;
use crate::ports::{CalculationsJob, LensMove, PrimaryEffect, PrimaryQuery};
use crate::primary_boundary::*;
use crate::primary_job::{CaptureSteps, PrimaryJob};
use crate::secondary_boundary::{CropGeometry, CropRequest, GeometryRequest, MeasurementCalc, MeasurementProfiles, ProfileGeometry, ScalePair};
use crate::secondary_job::SecondaryJob;
use crate::subjects::SubjectList;
use crate::tests::record;
use std::collections::BTreeMap;
use std::vec::Vec;
use xm1_saclac_interface::area_measurement::{AreaEntry, CaptureSelection};
use xm1_saclac_interface::calc::AreaGeometry;
use xm1_saclac_interface::capture_job::{CaptureEnd, CaptureRequest, CaptureWindow, DisplayRequest, OutputSelection};
use xm1_saclac_interface::statistics::StatisticsSetting;
use xm1_saclac_interface::{stock, Completed, Completion, RecordHandle};

const FRAME: Frame = Frame(0x9000);
const CAPTURE_OFFSET: u32 = 1000;
const LENS_STATE: u32 = 0x44;

#[derive(Clone, Debug, PartialEq)]
enum Call {
    SetSlotField(SlotField, u32),
    FrameWord(FrameOffset, u32),
    Fill(FrameOffset, u32),
    Capture(CaptureRequest, CaptureCompletions),
    CommandC,
    Command28,
    Display(DisplayRequest),
    Statistics(StatisticsSetting, Completion),
    Reply(CaptureStatus),
    SourceOutput(u16, u32),
    Output(OutputSelection, u32, u32),
    CaptureEnd(CaptureEnd),
    Readout(u32, u32),
    Configure { source: u8, statistics: bool, client: u32, detail: u32, selection: ProcessingSelection },
    ClearList(DeliveredList),
    SetArea(DeliveredList, u8, [u32; 2]),
    Generate(AreaMode, u32, AreaFrame),
    FrameAreas(GeometryWords, [u32; 2]),
    StoreCount(DeliveredList, u32),
    StoreArea(DeliveredList, u8, u32),
    StoreSetting(SettingResult),
    Calibrate(FactorRequest),
    Clamp(u32, Calibration, u8, i16),
    Positive(i32),
    Default(CalibrationSeed),
    Trace(CaptureTrace),
    Absolute(u32, MoveProfile, Completion),
    Relative(i16, u32, MoveProfile, Completion),
    Monitor(bool),
}

struct World {
    calls: Vec<Call>,
    fields: BTreeMap<u8, u32>,
    slot_fields: BTreeMap<u8, u32>,
    adjustments: BTreeMap<u8, u8>,
    lens: BTreeMap<u8, u32>,
    selection: SlotSelection,
    slots: Vec<SlotHandle>,
    slot_records: BTreeMap<u32, SlotEntry>,
    positions: SlotPositions,
    lists: [Vec<AreaEntry>; 3],
    list_counts: [u32; 3],
    stored_lists: [Vec<AreaEntry>; 3],
    stored_setting: SettingResult,
    keeps_template: bool,
    statistics_ready: bool,
    processing: Option<Processing>,
    readout_mode: u32,
    scale: i32,
    crops: BTreeMap<u16, CropGeometry>,
    display: DisplayLayout,
}

fn list_index(list: DeliveredList) -> usize { list as usize }

impl World {
    fn new() -> Self {
        let mut world = Self {
            calls: Vec::new(),
            fields: BTreeMap::new(),
            slot_fields: BTreeMap::new(),
            adjustments: BTreeMap::new(),
            lens: BTreeMap::new(),
            selection: SlotSelection { requested: 0, count: 1 },
            slots: Vec::new(),
            slot_records: BTreeMap::new(),
            positions: SlotPositions::default(),
            lists: [Vec::new(), Vec::new(), Vec::new()],
            list_counts: [0; 3],
            stored_lists: [Vec::new(), Vec::new(), Vec::new()],
            stored_setting: SettingResult([0; 5]),
            keeps_template: true,
            statistics_ready: true,
            processing: None,
            readout_mode: 0,
            scale: 0x10,
            crops: BTreeMap::new(),
            display: DisplayLayout::default(),
        };
        world.lens.insert(LensReading::State as u8, LENS_STATE);
        world
    }

    fn field(&mut self, field: RecordField, value: u32) { self.fields.insert(field as u8, value); }

    fn slot_field(&mut self, field: SlotField, value: u32) { self.slot_fields.insert(field as u8, value); }

    fn replies(&self) -> Vec<CaptureStatus> { self.calls.iter().filter_map(|call| match call { Call::Reply(status) => Some(*status), _ => None }).collect() }

    fn captured(&self) -> (CaptureRequest, CaptureCompletions) {
        self.calls.iter().find_map(|call| match call { Call::Capture(request, completions) => Some((*request, *completions)), _ => None }).unwrap()
    }

    fn statistics(&self) -> StatisticsSetting {
        self.calls.iter().find_map(|call| match call { Call::Statistics(setting, _) => Some(*setting), _ => None }).unwrap()
    }
}

impl CaptureRecords for World {
    fn field(&mut self, _record: RecordHandle, field: RecordField) -> u32 { self.fields.get(&(field as u8)).copied().unwrap_or(0) }
    fn set_field(&mut self, _record: RecordHandle, _field: RecordField, _value: u32) {}
    fn slot_field(&mut self, _record: RecordHandle, slot: u8, field: SlotField) -> u32 {
        assert_eq!(slot, 0);
        self.slot_fields.get(&(field as u8)).copied().unwrap_or(0)
    }
    fn set_slot_field(&mut self, _record: RecordHandle, slot: u8, field: SlotField, value: u32) {
        assert_eq!(slot, 0);
        self.calls.push(Call::SetSlotField(field, value));
    }
    fn reference_pair(&mut self, _record: RecordHandle, _index: u8) -> ReferencePair { ReferencePair::default() }
    fn slot_entry(&mut self, _record: RecordHandle, _slot: u8) -> SlotEntry { SlotEntry([0; SlotEntry::BYTES]) }
    fn store_slot_entry(&mut self, _target: SlotHandle, _entry: &SlotEntry) {}
    fn slot_reference(&mut self, _slot: SlotHandle) -> u32 { 0 }
    fn slot_offset(&mut self, _slot: SlotHandle) -> i16 { 0 }
    fn load_profile(&mut self, _record: RecordHandle, _profile: ClientRef) {}
    fn select_slot(&mut self, _record: RecordHandle, _slot: SlotHandle) {}
    fn focus_result(&mut self, _record: RecordHandle) -> crate::boundary::FocusResult {
        crate::boundary::FocusResult { reference: crate::boundary::LensReference(0), valid: 0, kept: 0, refined: 0, extent: 0, bytes: [0; 132] }
    }
    fn store_focus_result(&mut self, _record: RecordHandle, _result: &crate::boundary::FocusResult) {}
    fn area_reference(&mut self, _record: RecordHandle) -> AreaReference { AreaReference::default() }
    fn store_setting_result(&mut self, _record: RecordHandle, result: &SettingResult) {
        self.stored_setting = *result;
        self.calls.push(Call::StoreSetting(*result));
    }
    fn store_item(&mut self, _record: RecordHandle, _index: u8, _item: &AreaEntry) {}
    fn publish_subjects(&mut self, _record: RecordHandle, _list: &SubjectList) {}
}

impl CaptureClient for World {
    fn frame(&mut self, _sources: ClientRef) -> Frame { FRAME }
    fn frame_detail(&mut self, _sources: ClientRef) -> u32 { 0 }
    fn selection(&mut self, _selection: ClientRef) -> SlotSelection { self.selection }
    fn slot(&mut self, _slots: ClientRef, index: u8) -> SlotHandle { self.slots.get(usize::from(index)).copied().unwrap_or(SlotHandle(0)) }
    fn store_selection_result(&mut self, _selection: ClientRef, _value: u32) {}
    fn store_selection_hook(&mut self, _selection: ClientRef) {}
    fn output_marker(&mut self, _selection: ClientRef) -> i16 { 0 }
    fn frame_word(&mut self, _frame: Frame, _offset: FrameOffset) -> u32 { 0 }
    fn set_frame_word(&mut self, frame: Frame, offset: FrameOffset, value: u32) {
        assert_eq!(frame, FRAME);
        self.calls.push(Call::FrameWord(offset, value));
    }
    fn fill_frame(&mut self, _frame: Frame, offset: FrameOffset, bytes: u32, value: u8) {
        assert_eq!(value, 0);
        self.calls.push(Call::Fill(offset, bytes));
    }
    fn copy_table(&mut self, _frame: Frame, _offset: FrameOffset, _table: FrameTable) {}
}

impl CapturePlatform for World {
    fn adjustment(&mut self, adjustment: CaptureAdjustment) -> u8 { self.adjustments.get(&(adjustment as u8)).copied().unwrap_or(0) }
    fn prepare_setting(&mut self) {}
    fn refresh_setting(&mut self) {}
    fn monitor_device_commands(&mut self, monitored: bool) { self.calls.push(Call::Monitor(monitored)); }
    fn device_status_bit(&mut self) -> bool { false }
    fn slot_rate(&mut self, _active: u32, _profile: u32) -> u32 { 0 }
    fn split_distance(&mut self, _distance: i16) -> DistanceSplit { DistanceSplit::default() }
    fn device_value(&mut self) -> u32 { 0 }
    fn export_record_snapshot(&mut self, _frame: Frame) {}
    fn export_pipeline(&mut self, _frame: Frame) {}
    fn prepare_dump_area(&mut self, _frame: Frame, _offset: FrameOffset) {}
}

impl CaptureDiagnostics for World {
    fn trace(&mut self, trace: CaptureTrace) { self.calls.push(Call::Trace(trace)); }
}

impl CapturePeers for World {
    fn statistics_setup(&mut self, _enabled: bool, _completion: Completion) {}
    fn statistics_setting(&mut self, setting: &StatisticsSetting, completion: Completion) { self.calls.push(Call::Statistics(*setting, completion)); }
}

impl CaptureShared for World {
    fn active_slot(&mut self, selection: SlotSelection) -> u8 {
        if (1..=selection.count).contains(&selection.requested) && selection.count <= 3 { selection.requested } else { 1 }
    }
    fn set_return_active(&mut self, _active: bool) {}
    fn restore_return_calculation(&mut self) {}
    fn install_return_calculation(&mut self) {}
    fn absorb_calculation(&mut self) {}
    fn load_return_records(&mut self) {}
    fn take_return_request(&mut self) -> bool { false }
    fn has_work(&mut self) -> bool { false }
    fn secondary_enabled(&mut self) -> bool { false }
    fn set_secondary_enabled(&mut self, _enabled: bool) {}
    fn reset_secondary(&mut self) {}
    fn calculation_word(&mut self, _word: CalculationWord) -> u32 { 0 }
    fn moved_areas(&mut self) -> u8 { 0 }
    fn classify_subjects(&mut self, _reference: &AreaReference, _kind: u32, _source: u32, _area: u32) {}
    fn update_capture_selection(&mut self, _record: RecordHandle) {}
    fn dump_armed(&mut self) -> bool { false }
    fn set_dump_armed(&mut self, _armed: bool) {}
    fn subject_source(&mut self, _primary: bool) -> SubjectList { SubjectList::default() }
}

impl Lens for World {
    fn read(&mut self, reading: LensReading) -> u32 { self.lens.get(&(reading as u8)).copied().unwrap_or(0) }
    fn take_change(&mut self) -> u32 { 0 }
    fn designated_point(&mut self) -> Option<LensPoint> { None }
    fn target_distance(&mut self, _point: LensPoint, _control: u32) -> u32 { 0 }
    fn target_step(&mut self, _point: LensPoint) -> u16 { 0 }
}

/// Arithmetic that keeps every input visible in the result.
impl LensGeometry for World {
    fn reference_offset(&mut self, _reference: u32) -> i16 { 0 }
    fn rounded(&mut self, _value: u32) -> i16 { 0 }
    fn convert(&mut self, _base: u32, _value: u32, _lens: u32) -> u32 { 0 }
    fn mapped(&mut self, _value: u32, _selector: u32) -> u32 { 0 }
    fn compensated(&mut self, _reference: u32, _lens: u32) -> i16 { 0 }
    fn remap(&mut self, request: RemapRequest) -> i32 {
        (request.target as i32) * 3 + request.state as i32 + request.reference as i32 + if request.remap { 100 } else { 0 }
    }
}

impl LensMoves for World {
    fn absolute(&mut self, target: u32, profile: MoveProfile, completion: Completion) { self.calls.push(Call::Absolute(target, profile, completion)); }
    fn relative(&mut self, delta: i16, speed: u32, profile: MoveProfile, completion: Completion) { self.calls.push(Call::Relative(delta, speed, profile, completion)); }
}

impl MeasurementCalc for World {
    fn capture_selection(&mut self, _record: RecordHandle) -> CaptureSelection { CaptureSelection::default() }
    fn statistics_key(&mut self, _processing: &Processing) -> [i16; 2] { [0; 2] }
    fn scale_step(&mut self, _index: u16) -> i16 { 0 }
    fn capture_offset(&mut self, _record: RecordHandle) -> u32 { CAPTURE_OFFSET }
    fn curve_within(&mut self, _distance: u32, _selection: u32, _drive: u32, _limit: i32) -> bool { false }
    fn request_evaluation(&mut self, _record: RecordHandle, _processing: &Processing, _frames: [u32; 2], _geometry: &AreaGeometry, _completions: [Completion; 2]) {}
}

impl MeasurementProfiles for World {
    fn profile_geometry(&mut self, _request: &GeometryRequest) -> ProfileGeometry { ProfileGeometry::default() }
    fn crop_geometry(&mut self, request: &CropRequest) -> CropGeometry {
        assert_eq!((request.offset, request.full), (CAPTURE_OFFSET, false));
        self.crops.get(&request.layout[0]).copied().unwrap_or_default()
    }
    fn flagged_scale_pair(&mut self, _selector: u32) -> ScalePair { ScalePair::default() }
    fn profile_extent(&mut self, _selector: u32, inset: bool) -> [u32; 4] {
        assert!(!inset);
        [1, 2, 3, 4]
    }
}

impl CaptureCommand for World {
    fn jobs(&mut self) -> u32 { 0 }
    fn detail(&mut self) -> u32 { 0 }
    fn processing_word(&mut self) -> u32 { 0xc0 }
    fn fourth_target(&mut self) -> bool { false }
    fn publish_readout(&mut self, mode: u32, value: u32) { self.calls.push(Call::Readout(mode, value)); }
    fn publish_planes(&mut self, _planes: &BinnedPlanes, _extent: [u16; 2]) {}
    fn publish(&mut self, _job: CalculationsJob, _record: RecordHandle) {}
}

impl CaptureProcessing for World {
    fn configure(&mut self, source: u8, statistics: bool, _processing: &mut Processing, client: u32, detail: u32, selection: &ProcessingSelection) -> u32 {
        self.calls.push(Call::Configure { source, statistics, client, detail, selection: *selection });
        1
    }
    fn statistics(&mut self) -> StatisticsPlanes { [[0; GRID * GRID]; PLANES] }
}

impl CaptureJobClient for World {
    fn source_words(&mut self, _sources: ClientRef) -> SourceWords { SourceWords { first: 0x10, words: [0x11, 0x12, 0x13], byte: 0x14, option: 0x15 } }
    fn store_source_output(&mut self, _sources: ClientRef, marker: u16, value: u32) { self.calls.push(Call::SourceOutput(marker, value)); }
    fn selection_option(&mut self, _selection: ClientRef) -> u8 { 0x21 }
    fn readout_mode(&mut self, _selection: ClientRef) -> u32 { self.readout_mode }
    fn store_output(&mut self, _selection: ClientRef, output: &OutputSelection, offset: u32, rate: u32) { self.calls.push(Call::Output(*output, offset, rate)); }
    fn store_capture_end(&mut self, _selection: ClientRef, end: &CaptureEnd) { self.calls.push(Call::CaptureEnd(*end)); }
    fn calibration_block(&mut self, _block: ClientRef) -> ClientCalibration { ClientCalibration { word: 0x31, calibration: Calibration([1, 2, 3]), rate: 7 } }
    fn statistics_window(&mut self, _block: ClientRef) -> [u32; 4] { [5, 6, 7, 8] }
    fn statistics_selector(&mut self, _block: ClientRef) -> u32 { 0x41 }
    fn clear_area_list(&mut self, _buffer: ClientRef, list: DeliveredList) {
        self.lists[list_index(list)].clear();
        self.list_counts[list_index(list)] = 0;
        self.calls.push(Call::ClearList(list));
    }
    fn area_count(&mut self, _buffer: ClientRef, list: DeliveredList) -> u32 { self.list_counts[list_index(list)] }
    fn area(&mut self, _buffer: ClientRef, list: DeliveredList, index: u8) -> AreaEntry { self.lists[list_index(list)][usize::from(index)] }
    fn set_area(&mut self, _buffer: ClientRef, list: DeliveredList, index: u8, area: &AreaEntry) {
        self.lists[list_index(list)][usize::from(index)] = *area;
        self.calls.push(Call::SetArea(list, index, [area.rectangle()[0], area.centre()[1]]));
    }
}

impl CaptureJobRecords for World {
    fn record_calibration(&mut self, _record: RecordHandle) -> Calibration { Calibration([9, 8, 7]) }
    fn statistics_option(&mut self, _record: RecordHandle) -> (u32, u8) { (0x51, 0x52) }
    fn slot_positions(&mut self, _record: RecordHandle) -> SlotPositions { self.positions }
    fn setting_result(&mut self, _record: RecordHandle) -> SettingResult { self.stored_setting }
    fn slot_record(&mut self, slot: SlotHandle) -> SlotEntry { self.slot_records[&slot.0] }
    fn delivered_count(&mut self, _record: RecordHandle, list: DeliveredList) -> u32 { self.stored_lists[list_index(list)].len() as u32 }
    fn delivered_area(&mut self, _record: RecordHandle, list: DeliveredList, index: u8) -> AreaEntry { self.stored_lists[list_index(list)][usize::from(index)] }
    fn store_delivered_count(&mut self, _record: RecordHandle, list: DeliveredList, count: u32) {
        self.stored_lists[list_index(list)].clear();
        self.calls.push(Call::StoreCount(list, count));
    }
    fn store_delivered_area(&mut self, _record: RecordHandle, list: DeliveredList, index: u8, area: &AreaEntry) {
        self.stored_lists[list_index(list)].push(*area);
        self.calls.push(Call::StoreArea(list, index, area.rectangle()[0]));
    }
}

impl CapturePipeline for World {
    fn capture(&mut self, request: &CaptureRequest, completions: CaptureCompletions) { self.calls.push(Call::Capture(*request, completions)); }
    fn request_command_c(&mut self) { self.calls.push(Call::CommandC); }
    fn request_command_28(&mut self) { self.calls.push(Call::Command28); }
    fn display(&mut self, request: &DisplayRequest) { self.calls.push(Call::Display(*request)); }
    fn readout_processing(&mut self, source: u8) -> Option<Processing> {
        assert_eq!(source, 6);
        self.processing
    }
    fn keeps_template(&mut self, _subject_source: u32, _options: u32, _mode: u8, _profile_class: u32) -> bool { self.keeps_template }
    fn statistics_ready(&mut self) -> bool { self.statistics_ready }
}

/// Each step adds its own constant to the first value, so the chain stays visible.
impl ProfileCalibration for World {
    fn calibrate(&mut self, request: &FactorRequest) -> Calibration {
        self.calls.push(Call::Calibrate(*request));
        Calibration([request.calibration.0[0] + 10, request.calibration.0[1], request.calibration.0[2]])
    }
    fn clamp_geometry(&mut self, word: u32, calibration: &Calibration, rate: u8, exposure: i16) -> Calibration {
        self.calls.push(Call::Clamp(word, *calibration, rate, exposure));
        Calibration([calibration.0[0] + 100, calibration.0[1], calibration.0[2]])
    }
    fn apply_positive(&mut self, exposure: i32, calibration: &mut Calibration) {
        self.calls.push(Call::Positive(exposure));
        calibration.0[0] += 1000;
    }
    fn clamp_output(&mut self, calibration: &Calibration) -> Calibration { Calibration([calibration.0[0] + 10000, calibration.0[1], calibration.0[2]]) }
}

impl CaptureJobCalc for World {
    fn default_calibration(&mut self, seed: &CalibrationSeed) -> (Calibration, u32) {
        self.calls.push(Call::Default(*seed));
        (Calibration([4, 5, 6]), 0x61)
    }
    fn output_scale(&mut self, _index: i16, _distance: u32, _reference: u32, _words: [u32; 2]) -> i32 { self.scale }
}

impl CaptureProfiles for World {
    fn area_selector(&mut self, profile: u32, index: u8) -> u32 { profile * 10 + u32::from(index) }
    fn display_layout(&mut self, _selector: u32, _kind: u8) -> DisplayLayout { self.display }
    fn paired(&mut self, _selector: u32) -> bool { false }
    fn sensor_window(&mut self, crop: &CropGeometry) -> [u32; 4] { [crop.window[0] + 50, crop.window[1] + 60, 70, 80] }
    fn generate_areas(&mut self, mode: AreaMode, selector: u32, frame: &AreaFrame, _buffer: ClientRef) {
        self.calls.push(Call::Generate(mode, selector, *frame));
        if mode == AreaMode::Second {
            let mut area = AreaEntry::EMPTY;
            area.set_rectangle([500, 600, 1, 1]);
            area.set_centre([510, 610]);
            self.lists[1] = std::vec![area; 12];
            self.list_counts[1] = 12;
        }
    }
    fn frame_areas(&mut self, words: &GeometryWords, corner: [u32; 2], _buffer: ClientRef) { self.calls.push(Call::FrameAreas(*words, corner)); }
    fn display_extent(&mut self, selector: u32, pair: [u32; 2], words: &GeometryWords) -> [u32; 2] { [selector + pair[0], words.offset + pair[1]] }
    fn measurement_table(&mut self) -> [u32; 18] { core::array::from_fn(|index| 0x700 + index as u32) }
}

impl CaptureReplies for World {
    fn reply(&mut self, handle: RecordHandle, status: CaptureStatus) {
        assert_eq!(handle, record());
        self.calls.push(Call::Reply(status));
    }
}

fn parameters(options: u32) -> CaptureParameters {
    CaptureParameters {
        options: CaptureOptions(options),
        mode: 0,
        sources: ClientRef(0x100),
        profile: ClientRef(0),
        slots: ClientRef(0x300),
        selection: ClientRef(0x400),
        calibration: ClientRef(0x500),
        window: ClientRef(0),
        areas: ClientRef(0x600),
        statistics_selector: ClientRef(0x700),
    }
}

struct Steps { control: CaptureControl, secondary: SecondaryJob }

impl Steps {
    fn new() -> Self { Self { control: CaptureControl::new(), secondary: SecondaryJob::new() } }
    fn view(&self) -> CaptureSteps<'_> { CaptureSteps { control: &self.control, secondary: &self.secondary } }
}

fn run(job: &mut PrimaryJob, effects: &[PrimaryEffect], options: u32, world: &mut World) {
    let steps = Steps::new();
    for effect in effects { job.effect(*effect, &parameters(options), steps.view(), world); }
}

fn begun(world: &mut World) -> PrimaryJob {
    let mut job = PrimaryJob::new();
    run(&mut job, &[PrimaryEffect::Begin(record()), PrimaryEffect::ResetFlags], 0, world);
    job
}

fn deliver(job: &mut PrimaryJob, completion: Completion, payload: &[u8]) { job.receive(Completed { completion, parameters: stock::parameters(payload) }); }

fn put(bytes: &mut [u8], offset: usize, value: &[u8]) { bytes[offset..offset + value.len()].copy_from_slice(value); }

/// An output payload: status, then the 88 bytes, with `delivered` outputs made of `outputs`.
fn output_payload(outputs: u8, delivered: u8) -> [u8; 92] {
    let mut payload = [0; 92];
    let output = &mut payload[4..];
    put(output, 0x0c, &0x1234u16.to_le_bytes());
    put(output, 0x10, &0x5678u32.to_le_bytes());
    put(output, 0x14, &(-3i16).to_le_bytes());
    put(output, 0x16, &4u16.to_le_bytes());
    put(output, 0x18, &5u16.to_le_bytes());
    put(output, 0x1a, &6u16.to_le_bytes());
    put(output, 0x20, &0x77u32.to_le_bytes());
    put(output, 0x24, &0x88u32.to_le_bytes());
    put(output, 0x28, &0x99u16.to_le_bytes());
    put(output, 0x34, &0xa1u32.to_le_bytes());
    put(output, 0x38, &0xa2u32.to_le_bytes());
    output[0x3c] = outputs;
    output[0x3e] = delivered;
    put(output, 0x40, &0xb1u32.to_le_bytes());
    put(output, 0x44, &0xb2u16.to_le_bytes());
    output[0x46] = 0xb3;
    put(output, 0x48, &0xb4u32.to_le_bytes());
    put(output, 0x4c, &0xb5u32.to_le_bytes());
    put(output, 0x50, &0xb6u32.to_le_bytes());
    payload
}

fn word(bytes: &[u8], offset: usize) -> u32 { u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) }

fn half(bytes: &[u8], offset: usize) -> u16 { u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap()) }

#[test]
fn the_capture_request_carries_the_record_the_display_windows_and_the_other_steps() {
    let mut world = World::new();
    world.field(RecordField::Layout, 0x0002_0001);
    world.field(RecordField::SubjectSource, 0x2a);
    world.field(RecordField::ReferenceSecondLimit, 0xffff_8000);
    world.slot_field(SlotField::Offset, (-2i16) as u16 as u32);
    world.slot_field(SlotField::ProfileSelector, 0x66);
    world.positions.count = 3;
    world.display.areas[0] = DisplayArea { origin: 4, window: [0, 30, 300, 120], extent: [0, 10, 400, 0] };
    let mut job = begun(&mut world);
    run(&mut job, &[PrimaryEffect::PrepareTransfer, PrimaryEffect::Request], 0x201, &mut world);
    let (request, completions) = world.captured();
    assert_eq!(completions, CaptureCompletions { outputs: true, readout: false, delivery: true });
    assert_eq!(request.windows[0], CaptureWindow { left: 180, width: 124, half_height: 60 }, "200 - 30 + 10 from the left, 300 + 4 wide");
    assert_eq!(request.transfer.reference, LENS_STATE);
    assert_eq!((request.layout, request.subject_source, request.slot_offset, request.profile_selector), ([1, 2], 0x2a, -2, 0x66));
    assert_eq!((request.source, request.source_words, request.source_byte, request.source_option), (0x10, [0x11, 0x12, 0x13], 0x14, 0x15));
    assert_eq!((request.held, request.secondary_pending, request.secondary_enabled), (false, false, false));
    assert_eq!((request.second_limit_unset, request.slot_positions, request.selection_byte, request.options), (true, 3, 0x21, 0x201));
}

#[test]
fn a_later_selected_slot_with_a_lens_reference_is_where_the_lens_goes() {
    let mut world = World::new();
    world.selection = SlotSelection { requested: 1, count: 2 };
    world.slots = std::vec![SlotHandle(0x10), SlotHandle(0x20)];
    let mut entry = SlotEntry([0; SlotEntry::BYTES]);
    put(&mut entry.0, 4, &0x55u32.to_le_bytes());
    put(&mut entry.0, 0xa, &7u16.to_le_bytes());
    world.slot_records.insert(0x20, entry);
    let mut job = begun(&mut world);
    run(&mut job, &[PrimaryEffect::PrepareTransfer], 0, &mut world);
    assert!(job.query(PrimaryQuery::NeedsAbsolute, &parameters(0), &mut world));
    job.lens_move(LensMove::CaptureAbsolute, &mut world);
    assert_eq!(world.calls, [Call::Trace(CaptureTrace::CaptureAbsolute), Call::Absolute(0x55, MoveProfile::Capture, Completion::Lens3a)]);
    run(&mut job, &[PrimaryEffect::Request], 0, &mut world);
    assert_eq!(world.captured().0.transfer.second_limit, 7);

    let mut entry = SlotEntry([0; SlotEntry::BYTES]);
    put(&mut entry.0, 4, &0x80u32.to_le_bytes());
    world.slot_records.insert(0x20, entry);
    run(&mut job, &[PrimaryEffect::PrepareTransfer], 0, &mut world);
    assert!(!job.query(PrimaryQuery::NeedsAbsolute, &parameters(0), &mut world), "a slot without a reference leaves the lens where it is");
}

#[test]
fn a_held_readout_mode_needs_no_lens_move() {
    let mut world = World::new();
    world.lens.insert(LensReading::State as u8, LENS_STATE + 1);
    let mut job = begun(&mut world);
    run(&mut job, &[PrimaryEffect::PrepareTransfer], 0, &mut world);
    world.lens.insert(LensReading::State as u8, LENS_STATE + 2);
    assert!(job.query(PrimaryQuery::NeedsAbsolute, &parameters(0), &mut world));
    world.readout_mode = 2;
    assert!(!job.query(PrimaryQuery::NeedsAbsolute, &parameters(0), &mut world));
}

#[test]
fn a_positioned_source_moves_between_the_requested_slot_and_the_one_before() {
    let mut world = World::new();
    world.field(RecordField::SubjectSource, 0x32);
    world.positions = SlotPositions {
        count: 3,
        positions: [
            SlotPosition { position: 100, speed: 1 },
            SlotPosition { position: 140, speed: 2 },
            SlotPosition { position: 90, speed: 3 },
            SlotPosition::default(),
        ],
    };
    for (requested, move_) in [(2, (-50, 3)), (0, (40, 2)), (7, (40, 2))] {
        world.selection = SlotSelection { requested, count: 3 };
        world.calls.clear();
        let mut job = begun(&mut world);
        assert!(job.query(PrimaryQuery::NeedsRelative, &parameters(0), &mut world));
        run(&mut job, &[PrimaryEffect::PrepareRelative], 0, &mut world);
        job.lens_move(LensMove::CaptureRelative, &mut world);
        assert_eq!(world.calls, [Call::Trace(CaptureTrace::CaptureRelative), Call::Relative(move_.0, move_.1, MoveProfile::Capture, Completion::Lens35)]);
    }
}

#[test]
fn another_source_moves_the_lens_to_the_transfer_point_through_the_remap() {
    let mut world = World::new();
    world.field(RecordField::RemapMode, 1);
    world.lens.insert(LensReading::Position as u8, 1000);
    world.lens.insert(LensReading::Kind as u8, 6);
    let mut job = begun(&mut world);
    deliver(&mut job, Completion::Image60, &output_payload(1, 1));
    run(&mut job, &[PrimaryEffect::PrepareTransfer, PrimaryEffect::Output, PrimaryEffect::PrepareRelative], 0, &mut world);
    world.calls.clear();
    job.lens_move(LensMove::CaptureRelative, &mut world);
    let remapped = LENS_STATE as i32 * 3 + LENS_STATE as i32 + 0x88 + 100;
    assert_eq!(world.calls[1], Call::Relative((remapped - 1000) as i16, 6, MoveProfile::Capture, Completion::Lens35));
}

#[test]
fn the_statistics_take_the_client_calibration_through_the_calibration_chain() {
    let mut world = World::new();
    world.slot_field(SlotField::Trim, (-9i16) as u16 as u32);
    world.slot_field(SlotField::Step, 33);
    world.field(RecordField::At0049, 0x49);
    let mut job = begun(&mut world);
    let mut start = [0u8; 20];
    put(&mut start, 0x10, &1u32.to_le_bytes());
    deliver(&mut job, Completion::Image62, &start);
    run(&mut job, &[PrimaryEffect::RequestStatistics], 0, &mut world);
    assert!(world.calls.contains(&Call::Calibrate(FactorRequest {
        record_words: [0x49, 0],
        slot_reference: 0,
        kind: 0,
        calibration: Calibration([1, 2, 3]),
        mode: 2,
    })));
    assert!(world.calls.contains(&Call::Clamp(0x31, Calibration([11, 2, 3]), 7, -9)));
    assert!(world.calls.contains(&Call::Positive(33)));
    let setting = world.statistics();
    let bytes = setting.bytes();
    assert_eq!((word(bytes, 0x18), word(bytes, 0x20)), (1, 0x41), "flagged start, client selector");
    assert_eq!([word(bytes, 0x38), word(bytes, 0x44)], [0, 640], "the whole frame without a client window");
    assert_eq!((word(bytes, 0x78), half(bytes, 0x7c)), (0x31, 11111));
    assert_eq!((word(bytes, 0x88), bytes[0x8c]), (0x51, 0x52));
    assert_eq!(world.calls.last(), Some(&Call::Statistics(setting, Completion::Statistics7d)));
    assert_eq!(job.setting_template(), &setting);
}

#[test]
fn the_default_calibration_starts_from_the_capture_start_and_the_record() {
    let mut world = World::new();
    let mut job = begun(&mut world);
    let mut start = [0u8; 20];
    put(&mut start, 4, &3u32.to_le_bytes());
    put(&mut start, 8, &0x0022_0011u32.to_le_bytes());
    put(&mut start, 0xc, &5u32.to_le_bytes());
    deliver(&mut job, Completion::Image62, &start);
    run(&mut job, &[PrimaryEffect::RequestStatistics], 0x2, &mut world);
    assert!(world.calls.contains(&Call::Default(CalibrationSeed { record: Some(record()), base: Calibration([9, 8, 7]), pair: [0x11, 0x22], words: [5, 0] })));
    assert!(world.calls.contains(&Call::Clamp(0x61, Calibration([14, 5, 6]), 7, 0)));
}

#[test]
fn a_statistics_record_the_pipeline_does_not_keep_leaves_the_template() {
    let mut world = World::new();
    world.keeps_template = false;
    let mut job = begun(&mut world);
    run(&mut job, &[PrimaryEffect::RequestStatistics], 0, &mut world);
    assert_ne!(job.setting_template(), &world.statistics());
    assert_eq!(job.setting_template(), &StatisticsSetting::from_bytes([0; StatisticsSetting::BYTES]));
}

#[test]
fn an_output_reaches_the_first_slot_the_source_and_the_selection() {
    let mut world = World::new();
    let mut job = begun(&mut world);
    deliver(&mut job, Completion::Image5f, &output_payload(2, 1));
    run(&mut job, &[PrimaryEffect::Output], 0, &mut world);
    assert_eq!(world.calls[..8], [
        Call::SetSlotField(SlotField::Reference, 0x77),
        Call::SetSlotField(SlotField::SecondLimit, (-3i32) as u32),
        Call::SetSlotField(SlotField::Trim, 4),
        Call::SetSlotField(SlotField::At0186, 5),
        Call::SetSlotField(SlotField::At0184, 6),
        Call::SetSlotField(SlotField::At01b8, 0xa2),
        Call::SetSlotField(SlotField::Distance, 0x88),
        Call::SetSlotField(SlotField::At0204, 0xa1),
    ]);
    let selection = OutputSelection { marker: 0x99, word: 0xb1, pair: 0xb2, byte: 0xb3, words: [0xb4, 0xb5, 0xb6] };
    assert_eq!(world.calls[8..], [Call::SourceOutput(0x1234, 0x5678), Call::Output(selection, CAPTURE_OFFSET, 0xff_ffff / 0x10)]);

    world.scale = 0;
    world.calls.clear();
    run(&mut job, &[PrimaryEffect::Output], 0, &mut world);
    assert_eq!(world.calls.last(), Some(&Call::Output(selection, CAPTURE_OFFSET, 0xff_ffff)), "a zero scale divides by one");
}

#[test]
fn the_client_hears_of_progress_once_after_an_output_and_the_delivery() {
    let mut world = World::new();
    let mut job = begun(&mut world);
    run(&mut job, &[PrimaryEffect::PublishProgress], 0, &mut world);
    deliver(&mut job, Completion::Image5f, &output_payload(2, 1));
    run(&mut job, &[PrimaryEffect::Output, PrimaryEffect::PublishProgress], 0, &mut world);
    assert!(world.replies().is_empty());
    deliver(&mut job, Completion::Image64, &[0; 8]);
    run(&mut job, &[PrimaryEffect::Delivered, PrimaryEffect::PublishProgress, PrimaryEffect::PublishProgress], 0, &mut world);
    assert_eq!(world.replies(), [CaptureStatus::Progress]);
    assert!(job.query(PrimaryQuery::Delivered, &parameters(0), &mut world));
    run(&mut job, &[PrimaryEffect::ResetFlags], 0, &mut world);
    assert!(!job.query(PrimaryQuery::Delivered, &parameters(0), &mut world));
}

#[test]
fn accounting_waits_for_an_output_the_wanted_statistics_and_the_pipeline() {
    let mut world = World::new();
    world.statistics_ready = false;
    let mut job = begun(&mut world);
    let options = 0x20 | 0x80;
    run(&mut job, &[PrimaryEffect::Account], options, &mut world);
    deliver(&mut job, Completion::Image60, &output_payload(1, 1));
    run(&mut job, &[PrimaryEffect::Output, PrimaryEffect::Account], options, &mut world);
    assert!(job.query(PrimaryQuery::TransferPending, &parameters(options), &mut world));
    run(&mut job, &[PrimaryEffect::MarkStatisticsSeen, PrimaryEffect::Account], options, &mut world);
    assert!(!job.query(PrimaryQuery::TransferPending, &parameters(options), &mut world));
    assert!(world.replies().is_empty());
    world.statistics_ready = true;
    run(&mut job, &[PrimaryEffect::Account, PrimaryEffect::Account], options, &mut world);
    assert_eq!(world.replies(), [CaptureStatus::Accounted]);
}

#[test]
fn a_new_request_forgets_the_last_output() {
    let mut world = World::new();
    let mut job = begun(&mut world);
    deliver(&mut job, Completion::Image60, &output_payload(1, 1));
    run(&mut job, &[PrimaryEffect::Output, PrimaryEffect::Request, PrimaryEffect::Account], 0, &mut world);
    assert!(world.replies().is_empty());
}

#[test]
fn a_delivery_without_areas_clears_the_buffer_and_stores_empty_lists() {
    let mut world = World::new();
    let mut job = begun(&mut world);
    deliver(&mut job, Completion::Image64, &[0; 8]);
    run(&mut job, &[PrimaryEffect::Delivered], 0, &mut world);
    assert_eq!(world.calls, [
        Call::ClearList(DeliveredList::Second),
        Call::ClearList(DeliveredList::First),
        Call::ClearList(DeliveredList::Third),
        Call::StoreCount(DeliveredList::Second, 0),
        Call::StoreCount(DeliveredList::First, 0),
        Call::StoreCount(DeliveredList::Third, 0),
    ]);
}

#[test]
fn a_delivery_with_areas_generates_moves_and_stores_them() {
    let mut world = World::new();
    world.field(RecordField::Layout, 0x0006_0005);
    world.field(RecordField::FrameFirst, 0xf1);
    world.crops.insert(5, CropGeometry { origin: [10, 20], frame: [400, 600], window: [110, 220, 30, 40] });
    world.crops.insert(3, CropGeometry { origin: [1, 2], frame: [0; 2], window: [5, 6, 0, 0] });
    let mut job = begun(&mut world);
    let mut start = [0u8; 20];
    put(&mut start, 4, &4u32.to_le_bytes());
    deliver(&mut job, Completion::Image62, &start);
    run(&mut job, &[PrimaryEffect::RequestStatistics], 0x20, &mut world);
    deliver(&mut job, Completion::Image64, &[1, 0, 0, 0, 0, 0, 0, 0]);
    world.calls.clear();
    run(&mut job, &[PrimaryEffect::Delivered], 0, &mut world);
    let frame = AreaFrame { crop: [400, 600, 100, 200, 30, 40], sensor: [54, 64, 70, 80] };
    assert_eq!(world.calls[..2], [Call::Generate(AreaMode::Second, 41, frame), Call::Generate(AreaMode::Third, 41, frame)]);
    assert_eq!(world.calls[2], Call::SetArea(DeliveredList::Second, 0, [400, 410]), "corners move into the crop's window");
    let words = GeometryWords { offset: CAPTURE_OFFSET, frame_words: [0xf1, 0], layout: [5, 6] };
    assert_eq!(world.calls[12], Call::FrameAreas(words, [30, 40]));
    assert_eq!(world.calls[13..16], [
        Call::StoreCount(DeliveredList::Second, 12),
        Call::StoreCount(DeliveredList::First, 0),
        Call::StoreCount(DeliveredList::Third, 0),
    ]);
    let stored: Vec<_> = world.calls[16..].iter().filter(|call| matches!(call, Call::StoreArea(..))).collect();
    assert_eq!(stored.len(), 10, "the record keeps ten areas of a list");
}

#[test]
fn diagnostics_mark_the_dump_area_and_dump_the_second_delivered_list() {
    let mut world = World::new();
    world.adjustments.insert(CaptureAdjustment::Diagnostics as u8, 0x80);
    world.field(RecordField::SubjectSource, 0x2a);
    let mut job = begun(&mut world);
    run(&mut job, &[PrimaryEffect::PrepareRecords], 0, &mut world);
    let (start, end) = capture_stock::PRIMARY_DUMP_MARKS;
    assert_eq!(world.calls, [
        Call::Fill(capture_stock::PRIMARY_DUMP, capture_stock::PRIMARY_DUMP_BYTES),
        Call::FrameWord(capture_stock::PRIMARY_DUMP, start),
        Call::FrameWord(capture_stock::PRIMARY_DUMP_END, end),
    ]);
    for (source, options) in [(0x35, 0), (0x39, 0), (0x2a, 0x10)] {
        world.field(RecordField::SubjectSource, source);
        world.calls.clear();
        run(&mut job, &[PrimaryEffect::PrepareRecords], options, &mut world);
        assert_eq!(world.calls.len(), 1, "source {source:#x} options {options:#x} clears without marks");
    }

    let mut area = AreaEntry::EMPTY;
    area.set_rectangle([1, 2, 3, 4]);
    area.0[0x20] = 9;
    world.stored_lists[1] = std::vec![area];
    world.calls.clear();
    deliver(&mut job, Completion::Image64, &[0; 8]);
    run(&mut job, &[PrimaryEffect::Delivered], 0, &mut world);
    let dumped: BTreeMap<u32, u32> = world.calls.iter().filter_map(|call| match call { Call::FrameWord(offset, value) => Some((offset.0, *value)), _ => None }).collect();
    assert_eq!(dumped[&capture_stock::PRIMARY_DUMP_COUNT.0], 0, "the delivery stored no area");
    assert_eq!(dumped[&capture_stock::PRIMARY_DUMP_EXTENT[0].0], 0x701, "the table overwrites the extent");
    assert_eq!(dumped[&(capture_stock::PRIMARY_DUMP.0 + 0x4e4)], 0x711);
    assert!(!dumped.contains_key(&(capture_stock::PRIMARY_DUMP.0 + 0x4e8)));
}

#[test]
fn a_blocked_readout_configures_its_processing_without_the_jobs() {
    let mut world = World::new();
    let mut job = begun(&mut world);
    let mut readout = [0u8; 0x24];
    readout[4] = 6;
    readout[5] = 9;
    put(&mut readout, 0x1c, &3u32.to_le_bytes());
    put(&mut readout, 0x20, &0x42u32.to_le_bytes());
    deliver(&mut job, Completion::Image61, &readout);
    assert!(!job.query(PrimaryQuery::BlockReady, &parameters(0), &mut world));
    let mut processing = Processing::default();
    processing.0[0x8c] = 1;
    world.processing = Some(processing);
    assert!(job.query(PrimaryQuery::BlockReady, &parameters(0), &mut world));
    run(&mut job, &[PrimaryEffect::Block], 0, &mut world);
    assert_eq!(world.calls, [
        Call::Readout(3, 0x42),
        Call::Configure { source: 6, statistics: false, client: 0xc0, detail: 0, selection: ProcessingSelection { variant: 9, variant_mode: true, mode: 3, value: 0x42 } },
    ]);
}

#[test]
fn the_display_stores_the_statistics_result_and_shows_its_extent() {
    let mut world = World::new();
    world.slot_field(SlotField::ProfileSelector, 0x30);
    let mut job = begun(&mut world);
    let mut payload = [0u8; 24];
    for index in 0..6 { put(&mut payload, 4 * index, &(index as u32 + 1).to_le_bytes()); }
    deliver(&mut job, Completion::Statistics7d, &payload);
    run(&mut job, &[PrimaryEffect::RequestDisplay], 0x200 | 0x20, &mut world);
    assert_eq!(world.calls, [
        Call::StoreSetting(SettingResult([2, 3, 4, 5, 6])),
        Call::Display(DisplayRequest { setting_word: 5, setting_pair: [2, 3], extent: [0x32, CAPTURE_OFFSET + 3] }),
        Call::Reply(CaptureStatus::Displayed),
    ]);
    world.calls.clear();
    run(&mut job, &[PrimaryEffect::RequestDisplay], 0, &mut world);
    assert_eq!(world.calls, [Call::StoreSetting(SettingResult([2, 3, 4, 5, 6]))]);
}

#[test]
fn the_capture_end_reaches_the_selection_and_the_finish_is_replied() {
    let mut world = World::new();
    let mut job = begun(&mut world);
    deliver(&mut job, Completion::Image5e, &[0, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0]);
    run(&mut job, &[PrimaryEffect::RestoreLens, PrimaryEffect::PublishFinish, PrimaryEffect::Abort], 0, &mut world);
    assert_eq!(world.calls, [
        Call::CaptureEnd(CaptureEnd { readout: [1, 2], detail: 3 }),
        Call::Reply(CaptureStatus::Finished),
        Call::Monitor(true),
        Call::Trace(CaptureTrace::Released),
    ]);
}

#[test]
fn client_commands_marked_before_the_capture_are_pending_until_cleared() {
    let mut world = World::new();
    let mut job = begun(&mut world);
    let pending = |job: &mut PrimaryJob, world: &mut World| {
        [PrimaryQuery::CommandCPending, PrimaryQuery::Command28Pending].map(|query| job.query(query, &parameters(0), world))
    };
    assert_eq!(pending(&mut job, &mut world), [false, false]);
    run(&mut job, &[PrimaryEffect::MarkCommandC, PrimaryEffect::MarkCommand28], 0, &mut world);
    assert_eq!(pending(&mut job, &mut world), [true, true]);
    run(&mut job, &[PrimaryEffect::ClearCommandC], 0, &mut world);
    assert_eq!(pending(&mut job, &mut world), [false, true]);
    run(&mut job, &[PrimaryEffect::RequestCommandC, PrimaryEffect::RequestCommand28], 0, &mut world);
    assert_eq!(world.calls, [Call::CommandC, Call::Command28]);
}

#[test]
fn the_command_options_decide_what_the_capture_reports() {
    let mut world = World::new();
    let mut job = begun(&mut world);
    let query = |job: &mut PrimaryJob, world: &mut World, query, options, mode| {
        let mut parameters = parameters(options);
        parameters.mode = mode;
        job.query(query, &parameters, world)
    };
    assert!(query(&mut job, &mut world, PrimaryQuery::ImageFirst, 0, 3));
    assert!(!query(&mut job, &mut world, PrimaryQuery::ImageFirst, 0x2, 0));
    assert!(query(&mut job, &mut world, PrimaryQuery::ImageVariant, 0x100, 0));
    assert!(!query(&mut job, &mut world, PrimaryQuery::ImageVariant, 0x41, 0));
    assert!(query(&mut job, &mut world, PrimaryQuery::StatisticsWanted, 0x20, 0));
    assert!(query(&mut job, &mut world, PrimaryQuery::DisplayEnabled, 0x200, 0));
    assert!(!query(&mut job, &mut world, PrimaryQuery::StatisticsWanted, 0x1df, 0));
}

#[test]
fn a_capture_setting_starts_from_the_primary_jobs_template() {
    let mut world = World::new();
    let mut job = begun(&mut world);
    run(&mut job, &[PrimaryEffect::RequestStatistics], 0, &mut world);
    let template = *job.setting_template();
    let mut fake = crate::capture_fakes::CaptureFake::new();
    let overrides = xm1_saclac_interface::statistics::SettingOverrides::default();
    CaptureControl::new().request_setting(job.setting_template(), &overrides, &mut fake);
    let mut expected = template;
    expected.apply(&overrides);
    assert_eq!(fake.calls, [crate::capture_fakes::Call::StatisticsSetting { setting: expected, completion: Completion::Statistics85 }]);
}
