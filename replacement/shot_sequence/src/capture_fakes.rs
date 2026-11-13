use crate::boundary::{ClientRef, Frame, Lens, LensPoint, LensReading};
use crate::capture_boundary::*;
use std::collections::BTreeMap;
use std::vec::Vec;
use xm1_saclac_interface::calculation::ReferenceRequest;
use xm1_saclac_interface::{Completion, RecordHandle};

#[derive(Clone, Debug, PartialEq)]
pub enum Call {
    SetField(RecordHandle, RecordField, u32),
    SetSlotField(RecordHandle, u8, SlotField, u32),
    StoreSlotEntry(SlotHandle, SlotEntry),
    Absolute { target: u32, profile: MoveProfile, completion: Completion },
    Relative { delta: i16, speed: u32, profile: MoveProfile, completion: Completion },
    RequestReference { request: ReferenceRequest, result: CalculationResult, completion: Completion },
    SetReturnActive(bool),
    RestoreReturnCalculation,
    InstallReturnCalculation,
    AbsorbCalculation,
    LoadReturnRecords,
    Trace(CaptureTrace),
}

pub struct CaptureFake {
    pub calls: Vec<Call>,
    pub fields: BTreeMap<u8, u32>,
    pub lens: BTreeMap<u8, u32>,
    pub frame: Frame,
    pub selection: SlotSelection,
    /// Slot handles in the client's slot list, and each slot record's reference and offset.
    pub slots: Vec<SlotHandle>,
    pub slot_references: BTreeMap<u32, u32>,
    pub slot_offsets: BTreeMap<u32, i16>,
    pub pairs: [ReferencePair; 5],
    pub template: ReferenceRequest,
    pub templates_for: Vec<u32>,
    pub return_request: bool,
}

pub fn field_key(field: RecordField) -> u8 { field as u8 }

impl CaptureFake {
    pub fn new() -> Self {
        Self {
            calls: Vec::new(),
            fields: BTreeMap::new(),
            lens: BTreeMap::new(),
            frame: Frame(0),
            selection: SlotSelection::default(),
            slots: Vec::new(),
            slot_references: BTreeMap::new(),
            slot_offsets: BTreeMap::new(),
            pairs: [ReferencePair::default(); 5],
            template: ReferenceRequest::from_bytes([0; ReferenceRequest::BYTES]),
            templates_for: Vec::new(),
            return_request: false,
        }
    }

    pub fn set(&mut self, field: RecordField, value: u32) { self.fields.insert(field_key(field), value); }
    pub fn set_lens(&mut self, reading: LensReading, value: u32) { self.lens.insert(reading as u8, value); }

    pub fn add_slot(&mut self, handle: u32, reference: u32, offset: i16) {
        self.slots.push(SlotHandle(handle));
        self.slot_references.insert(handle, reference);
        self.slot_offsets.insert(handle, offset);
    }

    pub fn has(&self, call: &Call) -> bool { self.calls.contains(call) }

    pub fn record_writes(&self) -> Vec<Call> {
        self.calls.iter().filter(|call| matches!(call, Call::SetField(..) | Call::SetSlotField(..) | Call::StoreSlotEntry(..))).cloned().collect()
    }

    /// The entry the record layer hands out for `slot` carries the slot index in every byte.
    pub fn entry(slot: u8) -> SlotEntry { SlotEntry([slot; SlotEntry::BYTES]) }
}

impl CaptureClient for CaptureFake {
    fn frame(&mut self, _sources: ClientRef) -> Frame { self.frame }
    fn selection(&mut self, _selection: ClientRef) -> SlotSelection { self.selection }
    fn slot(&mut self, _slots: ClientRef, index: u8) -> SlotHandle { self.slots[usize::from(index)] }
}

impl CaptureRecords for CaptureFake {
    fn field(&mut self, _record: RecordHandle, field: RecordField) -> u32 { self.fields.get(&field_key(field)).copied().unwrap_or(0) }
    fn set_field(&mut self, record: RecordHandle, field: RecordField, value: u32) {
        self.fields.insert(field_key(field), value);
        self.calls.push(Call::SetField(record, field, value));
    }
    fn set_slot_field(&mut self, record: RecordHandle, slot: u8, field: SlotField, value: u32) { self.calls.push(Call::SetSlotField(record, slot, field, value)); }
    fn reference_pair(&mut self, _record: RecordHandle, index: u8) -> ReferencePair { self.pairs[usize::from(index)] }
    fn slot_entry(&mut self, _record: RecordHandle, slot: u8) -> SlotEntry { Self::entry(slot) }
    fn store_slot_entry(&mut self, target: SlotHandle, entry: &SlotEntry) { self.calls.push(Call::StoreSlotEntry(target, *entry)); }
    fn slot_reference(&mut self, slot: SlotHandle) -> u32 { self.slot_references.get(&slot.0).copied().unwrap_or(0) }
    fn slot_offset(&mut self, slot: SlotHandle) -> i16 { self.slot_offsets.get(&slot.0).copied().unwrap_or(0) }
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
    fn convert(&mut self, reference: u32, detail: u32, lens: u32) -> u32 { reference.wrapping_add(detail).wrapping_add(lens) }
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
}

impl ReferenceTemplates for CaptureFake {
    fn reference_request(&mut self, reference: u32) -> ReferenceRequest {
        self.templates_for.push(reference);
        self.template
    }
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
}

impl CaptureDiagnostics for CaptureFake {
    fn trace(&mut self, trace: CaptureTrace) { self.calls.push(Call::Trace(trace)); }
}
