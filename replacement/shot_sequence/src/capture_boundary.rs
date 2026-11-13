#![forbid(unsafe_code)]

//! What the capture steps' leaf work calls outside them: the capture command's client memory, the
//! image-4 record layer, lens geometry and lens moves, the calc library, the helpers the shot
//! steps share, and diagnostics. Byte layouts are in `capture_stock`.

use crate::boundary::{ClientRef, Frame};
use xm1_saclac_interface::calculation::ReferenceRequest;
use xm1_saclac_interface::{Completion, RecordHandle};

/// A slot record the capture command names; the record layer owns it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SlotHandle(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CaptureOptions(pub u32);

/// The command-7 parameters as the client wrote them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureParameters {
    pub options: CaptureOptions,
    /// 3 is a capture without the lens return, the secondary job and the record preparation.
    pub mode: u32,
    pub sources: ClientRef,
    pub reference: u32,
    pub slots: ClientRef,
    pub selection: ClientRef,
}

impl CaptureParameters {
    pub const REDUCED_MODE: u32 = 3;

    pub const fn reduced(&self) -> bool { self.mode == Self::REDUCED_MODE }
}

/// The selection the client names: the slot it asks for and how many slots it passes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SlotSelection {
    pub requested: u8,
    pub count: u8,
}

impl SlotSelection {
    pub fn indices(self) -> core::ops::Range<u8> { 0..self.count }
}

/// Client memory the command-7 parameters name.
pub trait CaptureClient {
    /// The frame the source descriptor names.
    fn frame(&mut self, sources: ClientRef) -> Frame;
    fn selection(&mut self, selection: ClientRef) -> SlotSelection;
    fn slot(&mut self, slots: ClientRef, index: u8) -> SlotHandle;
}

/// Bound-record values the capture steps read or publish.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordField {
    /// 2 compares the lens with the record's fifth reference; 13 lets the lens return restore
    /// saved slot positions.
    ResourceKind,
    /// 1 passes both remap flags when the lens return converts its target.
    RemapMode,
    ReferenceFirstLimit,
    ReferenceSecondLimit,
    ReferenceOption,
    CaptureFrame,
    CaptureSlotCount,
    CaptureSlots,
}

/// Per-slot values of the bound record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SlotField { Reference, Offset }

/// A reference and its detail word, as the record keeps five of them.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReferencePair {
    pub reference: u32,
    pub detail: u32,
}

/// A slot entry as the record layer copies it between records and slot records.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SlotEntry(pub [u8; SlotEntry::BYTES]);

impl SlotEntry {
    pub const BYTES: usize = 168;
}

/// The image-4 record layer. A refused write is not observed.
pub trait CaptureRecords {
    fn field(&mut self, record: RecordHandle, field: RecordField) -> u32;
    fn set_field(&mut self, record: RecordHandle, field: RecordField, value: u32);
    fn set_slot_field(&mut self, record: RecordHandle, slot: u8, field: SlotField, value: u32);
    fn reference_pair(&mut self, record: RecordHandle, index: u8) -> ReferencePair;
    fn slot_entry(&mut self, record: RecordHandle, slot: u8) -> SlotEntry;
    fn store_slot_entry(&mut self, target: SlotHandle, entry: &SlotEntry);
    fn slot_reference(&mut self, slot: SlotHandle) -> u32;
    fn slot_offset(&mut self, slot: SlotHandle) -> i16;
}

/// Lens-reference arithmetic of the image-4 configuration tables.
pub trait LensGeometry {
    /// The position offset of `reference` from the first reference.
    fn reference_offset(&mut self, reference: u32) -> i16;
    fn rounded(&mut self, value: u32) -> i16;
    /// `reference` re-expressed at `lens`, given the record's detail word.
    fn convert(&mut self, reference: u32, detail: u32, lens: u32) -> u32;
    fn mapped(&mut self, value: u32, selector: u32) -> u32;
    /// The slot offset of `reference` at `lens`.
    fn compensated(&mut self, reference: u32, lens: u32) -> i16;
    /// The lens step a relative move to `target` ends at.
    fn remap(&mut self, request: RemapRequest) -> i32;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RemapRequest {
    pub position: u32,
    pub reference: u32,
    pub state: u32,
    pub target: u32,
    pub remap: bool,
    pub kind: u32,
}

/// Which step's move arguments a lens move carries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MoveProfile { LensReturn }

/// Lens moves through the lens service, each completing with `completion`.
pub trait LensMoves {
    fn absolute(&mut self, target: u32, profile: MoveProfile, completion: Completion);
    fn relative(&mut self, delta: i16, speed: u32, profile: MoveProfile, completion: Completion);
}

/// Where a reference calculation writes its result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CalculationResult { LensReturn }

/// Calc-library entries the capture steps call.
pub trait CalcLibrary {
    fn request_reference(&mut self, request: &ReferenceRequest, result: CalculationResult, completion: Completion);
}

/// The reference template shared with the other saclac modules.
pub trait ReferenceTemplates {
    /// The template shifted to `reference`.
    fn reference_request(&mut self, reference: u32) -> ReferenceRequest;
}

/// Helpers the shot steps share and the shot context they keep.
pub trait CaptureShared {
    /// The selected slot, 1-based: `requested` when 1 <= requested <= count <= 3, else 1.
    fn active_slot(&mut self, selection: SlotSelection) -> u8;
    fn set_return_active(&mut self, active: bool);
    /// Copies the saved calculation into the lens return's result.
    fn restore_return_calculation(&mut self);
    /// Makes the lens return's result the working calculation.
    fn install_return_calculation(&mut self);
    fn absorb_calculation(&mut self);
    fn load_return_records(&mut self);
    /// Reads and clears the request a half press left for the capture's lens return.
    fn take_return_request(&mut self) -> bool;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureTrace { ReturnAbsolute, ReturnRelative }

pub trait CaptureDiagnostics {
    fn trace(&mut self, trace: CaptureTrace);
}
