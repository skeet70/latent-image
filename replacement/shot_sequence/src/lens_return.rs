#![forbid(unsafe_code)]

//! Leaf work of the lens return after a capture: it publishes the capture into its record,
//! restores the saved calculation or the saved slot positions, asks for a reference calculation
//! when neither will do, and moves the lens back to the first slot's reference.

use crate::boundary::{Lens, LensReading};
use crate::capture_boundary::{
    CalcLibrary, CalculationResult, CaptureClient, CaptureDiagnostics, CaptureParameters, CaptureRecords, CaptureTrace, LensGeometry, LensMoves,
    MoveProfile, RecordField, RemapRequest, SlotField,
};
use crate::ports::{LensMove, ReturnEffect, ReturnQuery};
use xm1_saclac_interface::calculation::{ReferenceLimits, ReferenceRequest};
use xm1_saclac_interface::RecordHandle;

/// Slots whose positions a capture saves for the lens return.
pub const SAVED_SLOTS: usize = 3;

/// The resource kind whose saved slot positions the lens return restores.
const RESTORING_KIND: u32 = 13;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SavedSlots {
    pub references: [u32; SAVED_SLOTS],
    pub offsets: [i16; SAVED_SLOTS],
}

/// The lens-return state a capture leaves for the lens return; the preparation resets it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReturnPoint {
    /// Slot positions converted to the capture's lens reference; `None` leaves the restore to a
    /// calculation.
    pub saved: Option<SavedSlots>,
    /// The lens left the record's fifth reference before the capture.
    pub armed: bool,
    /// `armed` as the lens return last took it; the shared shot steps read it.
    pub pending: bool,
}

pub trait ReturnBoundary: CaptureRecords + CaptureClient + Lens + LensGeometry + LensMoves + CalcLibrary + CaptureDiagnostics {}
impl<T> ReturnBoundary for T where T: CaptureRecords + CaptureClient + Lens + LensGeometry + LensMoves + CalcLibrary + CaptureDiagnostics {}

/// The shot state the lens return uses; `P` is what its operations call outside the shot.
pub trait ReturnShot<P: ?Sized> {
    /// The saved calculation input moved to `reference`.
    fn reference_request(&mut self, reference: u32, ports: &mut P) -> ReferenceRequest;
    /// Holds the exposure for the lens return.
    fn set_return_active(&mut self, active: bool);
    /// The retained result becomes the lens return's result and the working result.
    fn restore_return_result(&mut self);
    /// The lens return's result becomes the working result and is absorbed into the record;
    /// `pending` while the lens point is still to be reached.
    fn absorb_return_result(&mut self, pending: bool, ports: &mut P);
    /// Loads the absorbed result into the record for the lens return's conclusion.
    fn load_return_records(&mut self, pending: bool, ports: &mut P);
}

/// The lens return's own state. The calc task reads `request` until the calculation completes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LensReturn {
    record: Option<RecordHandle>,
    request: ReferenceRequest,
    lens_state: u32,
    target: u32,
    delta: i16,
    speed: u32,
}

impl Default for LensReturn {
    fn default() -> Self { Self::new() }
}

impl LensReturn {
    pub const fn new() -> Self {
        Self { record: None, request: ReferenceRequest::from_bytes([0; ReferenceRequest::BYTES]), lens_state: 0, target: 0, delta: 0, speed: 0 }
    }

    pub fn effect<P: ReturnBoundary>(&mut self, effect: ReturnEffect, parameters: &CaptureParameters, point: &ReturnPoint, shot: &mut impl ReturnShot<P>, boundary: &mut P) {
        match effect {
            ReturnEffect::Begin(record) => self.record = Some(record),
            ReturnEffect::PrepareRecords => self.publish_capture(parameters, boundary),
            ReturnEffect::MarkActive(active) => shot.set_return_active(active),
            ReturnEffect::PrepareCalculation => shot.restore_return_result(),
            ReturnEffect::AbsorbCalculation => shot.absorb_return_result(point.pending, boundary),
            ReturnEffect::ConcludeExposure => shot.load_return_records(point.pending, boundary),
            ReturnEffect::RequestCalculation => self.request_calculation(shot, boundary),
            ReturnEffect::PrepareMoves => self.copy_slots(parameters, boundary),
            ReturnEffect::PrepareRelative => self.prepare_relative(boundary),
        }
    }

    pub fn query(&mut self, query: ReturnQuery, point: &mut ReturnPoint, parameters: &CaptureParameters, boundary: &mut impl ReturnBoundary) -> bool {
        match query {
            ReturnQuery::NeedsCalculation => self.needs_calculation(point, parameters, boundary),
            ReturnQuery::NeedsAbsoluteMove => self.needs_absolute_move(parameters, boundary),
        }
    }

    /// Starts `ReturnAbsolute` or `ReturnRelative`; other moves are not the lens return's.
    pub fn lens_move(&self, lens_move: LensMove, boundary: &mut (impl LensMoves + CaptureDiagnostics)) {
        match lens_move {
            LensMove::ReturnAbsolute => {
                boundary.trace(CaptureTrace::ReturnAbsolute);
                boundary.absolute(self.target, MoveProfile::LensReturn, lens_move.completion());
            }
            LensMove::ReturnRelative => {
                boundary.trace(CaptureTrace::ReturnRelative);
                boundary.relative(self.delta, self.speed, MoveProfile::LensReturn, lens_move.completion());
            }
            _ => {}
        }
    }

    pub fn request(&self) -> &ReferenceRequest { &self.request }

    fn publish_capture(&self, parameters: &CaptureParameters, boundary: &mut (impl CaptureRecords + CaptureClient)) {
        let Some(record) = self.record else { return };
        let frame = boundary.frame(parameters.sources);
        boundary.set_field(record, RecordField::CaptureFrame, frame.0);
        let count = boundary.selection(parameters.selection).count;
        boundary.set_field(record, RecordField::CaptureSlotCount, u32::from(count));
        boundary.set_field(record, RecordField::CaptureSlots, parameters.slots.0);
    }

    fn needs_calculation(&self, point: &mut ReturnPoint, parameters: &CaptureParameters, boundary: &mut impl ReturnBoundary) -> bool {
        point.pending = point.armed;
        if point.armed { return true; }
        let Some(record) = self.record else { return false };
        if boundary.field(record, RecordField::ResourceKind) != RESTORING_KIND { return false; }
        let Some(saved) = point.saved else { return true };
        let count = boundary.selection(parameters.selection).count;
        for slot in 0..count.min(SAVED_SLOTS as u8) {
            let index = usize::from(slot);
            boundary.set_slot_field(record, slot, SlotField::Reference, saved.references[index]);
            boundary.set_slot_field(record, slot, SlotField::Offset, i32::from(saved.offsets[index]) as u32);
        }
        false
    }

    fn request_calculation<P: ReturnBoundary>(&mut self, shot: &mut impl ReturnShot<P>, boundary: &mut P) {
        let Some(record) = self.record else { return };
        let limits = ReferenceLimits {
            first: boundary.field(record, RecordField::ReferenceFirstLimit) as i16,
            second: boundary.field(record, RecordField::ReferenceSecondLimit) as i16,
            option: boundary.field(record, RecordField::ReferenceOption),
        };
        let lens = boundary.read(LensReading::Reference);
        let mut request = shot.reference_request(lens, boundary);
        let pair = boundary.reference_pair(record, 4);
        let offset = boundary.reference_offset(lens).wrapping_sub(boundary.reference_offset(pair.reference));
        let converted = boundary.convert(pair.reference, pair.detail, lens);
        let rounding = boundary.rounded(pair.detail).wrapping_sub(boundary.rounded(converted));
        request.shift_positions(offset.wrapping_sub(rounding));
        request.set_limits(limits);
        self.request = request;
        boundary.request_reference(&self.request, CalculationResult::LensReturn, xm1_saclac_interface::Completion::Calculation90);
    }

    /// Copies each slot of the record into the client's slot record when the first slot is the
    /// selected one.
    fn copy_slots(&self, parameters: &CaptureParameters, boundary: &mut (impl CaptureRecords + CaptureClient)) {
        let Some(record) = self.record else { return };
        let selection = boundary.selection(parameters.selection);
        if selection.active() != 1 { return; }
        for slot in selection.indices() {
            let entry = boundary.slot_entry(record, slot);
            let target = boundary.slot(parameters.slots, slot);
            boundary.store_slot_entry(target, &entry);
        }
    }

    fn needs_absolute_move(&mut self, parameters: &CaptureParameters, boundary: &mut impl ReturnBoundary) -> bool {
        self.lens_state = boundary.read(LensReading::State);
        let first = boundary.slot(parameters.slots, 0);
        self.target = boundary.slot_reference(first);
        self.target != self.lens_state
    }

    fn prepare_relative(&mut self, boundary: &mut impl ReturnBoundary) {
        let remap = self.record.is_some_and(|record| boundary.field(record, RecordField::RemapMode) == 1);
        let position = boundary.read(LensReading::Position);
        let request = RemapRequest {
            position,
            reference: boundary.read(LensReading::Reference),
            state: boundary.read(LensReading::State),
            target: self.target,
            remap,
            kind: boundary.read(LensReading::Kind),
        };
        self.delta = boundary.remap(request).wrapping_sub(position as i32) as i16;
        self.speed = request.kind;
    }
}
