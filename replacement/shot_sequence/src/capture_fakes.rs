use crate::boundary::{ClientRef, FocusResult, Frame, Lens, LensPoint, LensReading, LensReference};
use crate::capture_boundary::*;
use crate::subjects::SubjectList;
use std::collections::BTreeMap;
use std::vec::Vec;
use xm1_saclac_interface::calculation::ReferenceRequest;
use xm1_saclac_interface::statistics::StatisticsSetting;
use xm1_saclac_interface::{Completion, RecordHandle};

#[derive(Clone, Debug, PartialEq)]
pub enum Call {
    SetField(RecordHandle, RecordField, u32),
    SetSlotField(RecordHandle, u8, SlotField, u32),
    StoreSlotEntry(SlotHandle, SlotEntry),
    LoadProfile(RecordHandle, ClientRef),
    SelectSlot(RecordHandle, SlotHandle),
    StoreFocusResult(RecordHandle, FocusResult),
    StoreSettingResult(RecordHandle, SettingResult),
    StoreItem(RecordHandle, u8, ItemEntry),
    PublishSubjects(RecordHandle, SubjectList),
    SelectionResult(ClientRef, u32),
    SelectionHook(ClientRef),
    SetFrameWord(Frame, FrameOffset, u32),
    FillFrame(Frame, FrameOffset, u32, u8),
    CopyTable(Frame, FrameOffset, FrameTable),
    Absolute { target: u32, profile: MoveProfile, completion: Completion },
    Relative { delta: i16, speed: u32, profile: MoveProfile, completion: Completion },
    RequestReference { request: ReferenceRequest, result: CalculationResult, completion: Completion },
    ExportSelection(Frame),
    ExportJointTables(Frame),
    StatisticsSetup { enabled: bool, completion: Completion },
    StatisticsSetting { setting: StatisticsSetting, completion: Completion },
    SetReturnActive(bool),
    RestoreReturnCalculation,
    InstallReturnCalculation,
    AbsorbCalculation,
    LoadReturnRecords,
    SetSecondaryEnabled(bool),
    ResetSecondary,
    ClassifySubjects { kind: u32, source: u32, area: u32 },
    UpdateCaptureSelection(RecordHandle),
    PrepareSetting,
    RefreshSetting,
    MonitorDeviceCommands(bool),
    ExportCapture(Frame),
    ExportPipeline(Frame),
    PrepareDumpArea(Frame, FrameOffset),
    Trace(CaptureTrace),
}

pub struct CaptureFake {
    pub calls: Vec<Call>,
    pub fields: BTreeMap<u8, u32>,
    pub slot_fields: BTreeMap<(u8, u8), u32>,
    pub lens: BTreeMap<u8, u32>,
    pub adjustments: BTreeMap<u8, u8>,
    pub frame: Frame,
    pub frame_words: BTreeMap<u32, u32>,
    pub selection: SlotSelection,
    /// Slot handles in the client's slot list, and each slot record's reference and offset.
    pub slots: Vec<SlotHandle>,
    pub slot_references: BTreeMap<u32, u32>,
    pub slot_offsets: BTreeMap<u32, i16>,
    pub entries: BTreeMap<u8, SlotEntry>,
    pub pairs: [ReferencePair; 5],
    pub template: ReferenceRequest,
    pub templates_for: Vec<u32>,
    pub return_request: bool,
    pub has_work: bool,
    pub secondary_enabled: bool,
    pub subject_refresh: u32,
    pub moved_areas: u8,
    pub sources: [SubjectList; 2],
    pub focus: FocusResult,
    pub capture_busy: bool,
    pub split_limit: i16,
    pub device_value: u32,
}

pub fn field_key(field: RecordField) -> u8 { field as u8 }
fn slot_key(slot: u8, field: SlotField) -> (u8, u8) { (slot, field as u8) }

impl CaptureFake {
    pub fn new() -> Self {
        Self {
            calls: Vec::new(),
            fields: BTreeMap::new(),
            slot_fields: BTreeMap::new(),
            lens: BTreeMap::new(),
            adjustments: BTreeMap::new(),
            frame: Frame(0),
            frame_words: BTreeMap::new(),
            selection: SlotSelection::default(),
            slots: Vec::new(),
            slot_references: BTreeMap::new(),
            slot_offsets: BTreeMap::new(),
            entries: BTreeMap::new(),
            pairs: [ReferencePair::default(); 5],
            template: ReferenceRequest::from_bytes([0; ReferenceRequest::BYTES]),
            templates_for: Vec::new(),
            return_request: false,
            has_work: false,
            secondary_enabled: false,
            subject_refresh: 0,
            moved_areas: 0,
            sources: [SubjectList::default(); 2],
            focus: FocusResult { reference: LensReference(0), valid: 1, kept: 1, refined: 1, extent: 0, bytes: [0; 132] },
            capture_busy: false,
            split_limit: i16::MAX,
            device_value: 0,
        }
    }

    pub fn set(&mut self, field: RecordField, value: u32) { self.fields.insert(field_key(field), value); }
    pub fn set_slot(&mut self, slot: u8, field: SlotField, value: u32) { self.slot_fields.insert(slot_key(slot, field), value); }
    pub fn set_lens(&mut self, reading: LensReading, value: u32) { self.lens.insert(reading as u8, value); }
    pub fn set_adjustment(&mut self, adjustment: CaptureAdjustment, value: u8) { self.adjustments.insert(adjustment as u8, value); }

    pub fn add_slot(&mut self, handle: u32, reference: u32, offset: i16) {
        self.slots.push(SlotHandle(handle));
        self.slot_references.insert(handle, reference);
        self.slot_offsets.insert(handle, offset);
    }

    pub fn has(&self, call: &Call) -> bool { self.calls.contains(call) }

    pub fn record_writes(&self) -> Vec<Call> {
        self.calls.iter().filter(|call| matches!(call, Call::SetField(..) | Call::SetSlotField(..) | Call::StoreSlotEntry(..))).cloned().collect()
    }

    pub fn slot_writes(&self, wanted: SlotField) -> Vec<(u8, u32)> {
        self.calls.iter().filter_map(|call| match call { Call::SetSlotField(_, slot, field, value) if *field == wanted => Some((*slot, *value)), _ => None }).collect()
    }

    /// The entry the record layer hands out for `slot` carries the slot index in every byte
    /// unless a test set one.
    pub fn entry(slot: u8) -> SlotEntry { SlotEntry([slot; SlotEntry::BYTES]) }
}

impl CaptureClient for CaptureFake {
    fn frame(&mut self, _sources: ClientRef) -> Frame { self.frame }
    fn selection(&mut self, _selection: ClientRef) -> SlotSelection { self.selection }
    fn slot(&mut self, _slots: ClientRef, index: u8) -> SlotHandle { self.slots.get(usize::from(index)).copied().unwrap_or(SlotHandle(0)) }
    fn store_selection_result(&mut self, selection: ClientRef, value: u32) { self.calls.push(Call::SelectionResult(selection, value)); }
    fn store_selection_hook(&mut self, selection: ClientRef) { self.calls.push(Call::SelectionHook(selection)); }
    fn frame_word(&mut self, _frame: Frame, offset: FrameOffset) -> u32 { self.frame_words.get(&offset.0).copied().unwrap_or(0) }
    fn set_frame_word(&mut self, frame: Frame, offset: FrameOffset, value: u32) {
        self.frame_words.insert(offset.0, value);
        self.calls.push(Call::SetFrameWord(frame, offset, value));
    }
    fn fill_frame(&mut self, frame: Frame, offset: FrameOffset, bytes: u32, value: u8) { self.calls.push(Call::FillFrame(frame, offset, bytes, value)); }
    fn copy_table(&mut self, frame: Frame, offset: FrameOffset, table: FrameTable) { self.calls.push(Call::CopyTable(frame, offset, table)); }
}

impl CaptureRecords for CaptureFake {
    fn field(&mut self, _record: RecordHandle, field: RecordField) -> u32 { self.fields.get(&field_key(field)).copied().unwrap_or(0) }
    fn set_field(&mut self, record: RecordHandle, field: RecordField, value: u32) {
        self.fields.insert(field_key(field), value);
        self.calls.push(Call::SetField(record, field, value));
    }
    fn slot_field(&mut self, _record: RecordHandle, slot: u8, field: SlotField) -> u32 { self.slot_fields.get(&slot_key(slot, field)).copied().unwrap_or(0) }
    fn set_slot_field(&mut self, record: RecordHandle, slot: u8, field: SlotField, value: u32) { self.calls.push(Call::SetSlotField(record, slot, field, value)); }
    fn reference_pair(&mut self, _record: RecordHandle, index: u8) -> ReferencePair { self.pairs[usize::from(index)] }
    fn slot_entry(&mut self, _record: RecordHandle, slot: u8) -> SlotEntry { self.entries.get(&slot).copied().unwrap_or(Self::entry(slot)) }
    fn store_slot_entry(&mut self, target: SlotHandle, entry: &SlotEntry) { self.calls.push(Call::StoreSlotEntry(target, *entry)); }
    fn slot_reference(&mut self, slot: SlotHandle) -> u32 { self.slot_references.get(&slot.0).copied().unwrap_or(0) }
    fn slot_offset(&mut self, slot: SlotHandle) -> i16 { self.slot_offsets.get(&slot.0).copied().unwrap_or(0) }
    fn load_profile(&mut self, record: RecordHandle, profile: ClientRef) { self.calls.push(Call::LoadProfile(record, profile)); }
    fn select_slot(&mut self, record: RecordHandle, slot: SlotHandle) { self.calls.push(Call::SelectSlot(record, slot)); }
    fn focus_result(&mut self, _record: RecordHandle) -> FocusResult { self.focus }
    fn store_focus_result(&mut self, record: RecordHandle, result: &FocusResult) {
        self.focus = *result;
        self.calls.push(Call::StoreFocusResult(record, *result));
    }
    fn area_reference(&mut self, _record: RecordHandle) -> AreaReference { AreaReference([7; 10]) }
    fn store_setting_result(&mut self, record: RecordHandle, result: &SettingResult) { self.calls.push(Call::StoreSettingResult(record, *result)); }
    fn store_item(&mut self, record: RecordHandle, index: u8, item: &ItemEntry) { self.calls.push(Call::StoreItem(record, index, *item)); }
    fn publish_subjects(&mut self, record: RecordHandle, list: &SubjectList) { self.calls.push(Call::PublishSubjects(record, *list)); }
}

impl Lens for CaptureFake {
    fn read(&mut self, reading: LensReading) -> u32 { self.lens.get(&(reading as u8)).copied().unwrap_or(0) }
    fn take_change(&mut self) -> u32 { 0 }
    fn designated_point(&mut self) -> Option<LensPoint> { None }
    fn target_distance(&mut self, _point: LensPoint, _control: u32) -> u32 { 0 }
    fn target_step(&mut self, _point: LensPoint) -> u16 { 0 }
}

/// Arithmetic that keeps every input visible in the result.
impl LensGeometry for CaptureFake {
    fn reference_offset(&mut self, reference: u32) -> i16 { (reference as i16).wrapping_mul(2) }
    fn rounded(&mut self, value: u32) -> i16 { (value as i16).wrapping_add(1) }
    fn convert(&mut self, base: u32, value: u32, lens: u32) -> u32 { base.wrapping_add(value).wrapping_add(lens) }
    fn mapped(&mut self, value: u32, selector: u32) -> u32 { value ^ selector }
    fn compensated(&mut self, reference: u32, lens: u32) -> i16 { reference.wrapping_sub(lens) as i16 }
    fn remap(&mut self, request: RemapRequest) -> i32 {
        (request.target as i32).wrapping_mul(3).wrapping_add(request.state as i32).wrapping_add(if request.remap { 100 } else { 0 })
    }
}

impl LensMoves for CaptureFake {
    fn absolute(&mut self, target: u32, profile: MoveProfile, completion: Completion) { self.calls.push(Call::Absolute { target, profile, completion }); }
    fn relative(&mut self, delta: i16, speed: u32, profile: MoveProfile, completion: Completion) {
        self.calls.push(Call::Relative { delta, speed, profile, completion });
    }
}

impl CalcLibrary for CaptureFake {
    fn request_reference(&mut self, request: &ReferenceRequest, result: CalculationResult, completion: Completion) {
        self.calls.push(Call::RequestReference { request: *request, result, completion });
    }
    fn selection_rate(&mut self, _record: RecordHandle, sample: LensSample, control: u32) -> u32 { sample.reference + sample.state + u32::from(sample.position) + sample.kind + control }
    fn selection_direction(&mut self, sample: LensSample) -> u32 { sample.reference * 10 + sample.state }
    fn export_selection(&mut self, frame: Frame) { self.calls.push(Call::ExportSelection(frame)); }
    fn export_joint_tables(&mut self, frame: Frame) { self.calls.push(Call::ExportJointTables(frame)); }
}

impl ReferenceTemplates for CaptureFake {
    fn reference_request(&mut self, reference: u32) -> ReferenceRequest {
        self.templates_for.push(reference);
        self.template
    }
}

impl CapturePeers for CaptureFake {
    fn statistics_setup(&mut self, enabled: bool, completion: Completion) { self.calls.push(Call::StatisticsSetup { enabled, completion }); }
    fn statistics_setting(&mut self, setting: &StatisticsSetting, completion: Completion) { self.calls.push(Call::StatisticsSetting { setting: *setting, completion }); }
}

impl CaptureShared for CaptureFake {
    fn active_slot(&mut self, selection: SlotSelection) -> u8 {
        if (1..=selection.count).contains(&selection.requested) && selection.count <= 3 { selection.requested } else { 1 }
    }
    fn set_return_active(&mut self, active: bool) { self.calls.push(Call::SetReturnActive(active)); }
    fn restore_return_calculation(&mut self) { self.calls.push(Call::RestoreReturnCalculation); }
    fn install_return_calculation(&mut self) { self.calls.push(Call::InstallReturnCalculation); }
    fn absorb_calculation(&mut self) { self.calls.push(Call::AbsorbCalculation); }
    fn load_return_records(&mut self) { self.calls.push(Call::LoadReturnRecords); }
    fn take_return_request(&mut self) -> bool { core::mem::take(&mut self.return_request) }
    fn has_work(&mut self) -> bool { self.has_work }
    fn secondary_enabled(&mut self) -> bool { self.secondary_enabled }
    fn set_secondary_enabled(&mut self, enabled: bool) {
        self.secondary_enabled = enabled;
        self.calls.push(Call::SetSecondaryEnabled(enabled));
    }
    fn reset_secondary(&mut self) { self.calls.push(Call::ResetSecondary); }
    fn calculation_word(&mut self, word: CalculationWord) -> u32 {
        match word {
            CalculationWord::SubjectRefresh => self.subject_refresh,
        }
    }
    fn moved_areas(&mut self) -> u8 { self.moved_areas }
    fn classify_subjects(&mut self, _reference: &AreaReference, kind: u32, source: u32, area: u32) { self.calls.push(Call::ClassifySubjects { kind, source, area }); }
    fn update_capture_selection(&mut self, record: RecordHandle) { self.calls.push(Call::UpdateCaptureSelection(record)); }
    fn subject_source(&mut self, primary: bool) -> SubjectList { self.sources[usize::from(!primary)] }
}

impl CapturePlatform for CaptureFake {
    fn adjustment(&mut self, adjustment: CaptureAdjustment) -> u8 { self.adjustments.get(&(adjustment as u8)).copied().unwrap_or(0) }
    fn prepare_setting(&mut self) { self.calls.push(Call::PrepareSetting); }
    fn refresh_setting(&mut self) { self.calls.push(Call::RefreshSetting); }
    fn monitor_device_commands(&mut self, monitored: bool) { self.calls.push(Call::MonitorDeviceCommands(monitored)); }
    fn capture_busy(&mut self) -> bool { self.capture_busy }
    fn slot_rate(&mut self, active: u32, profile: u32) -> u32 { if active != 0 { 4 } else { 2 + u32::from(profile != 0) } }
    fn split_distance(&mut self, distance: i16) -> DistanceSplit {
        if distance > self.split_limit { DistanceSplit { kept: self.split_limit, excess: distance - self.split_limit } } else { DistanceSplit { kept: distance, excess: 0 } }
    }
    fn device_value(&mut self) -> u32 { self.device_value }
    fn export_capture(&mut self, frame: Frame) { self.calls.push(Call::ExportCapture(frame)); }
    fn export_pipeline(&mut self, frame: Frame) { self.calls.push(Call::ExportPipeline(frame)); }
    fn prepare_dump_area(&mut self, frame: Frame, offset: FrameOffset) { self.calls.push(Call::PrepareDumpArea(frame, offset)); }
}

impl CaptureDiagnostics for CaptureFake {
    fn trace(&mut self, trace: CaptureTrace) { self.calls.push(Call::Trace(trace)); }
}
