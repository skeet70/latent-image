#![forbid(unsafe_code)]

//! What the capture steps' leaf work calls outside them: the capture command's client memory, the
//! image-4 record layer, lens geometry and lens moves, the calc library, peer requests, the helpers
//! the shot steps share, platform services and diagnostics. Byte layouts are in `capture_stock`.

use crate::boundary::{ClientRef, FocusResult, Frame};
use crate::subjects::SubjectList;
use xm1_saclac_interface::area_measurement::AreaEntry;
use xm1_saclac_interface::calculation::ReferenceRequest;
use xm1_saclac_interface::statistics::StatisticsSetting;
use xm1_saclac_interface::{Completion, RecordHandle};

/// A slot record the capture command names; the record layer owns it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SlotHandle(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CaptureOptions(pub u32);

impl CaptureOptions {
    pub const fn statistics_setup(self) -> bool { self.0 & 0x40 != 0 }
    /// The profile the parameters name is loaded into the record before its slot is selected.
    pub const fn loads_profile(self) -> bool { self.0 & 0x0a != 0 }
    pub const fn selects_slot(self) -> bool { self.0 & 0x1b5 != 0 }
    /// An unset second limit (-0x8000) takes the default for the first slot.
    pub const fn defaults_second_limit(self) -> bool { self.0 & 0x10 != 0 }
}

/// The command-7 parameters as the client wrote them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureParameters {
    pub options: CaptureOptions,
    /// 3 is a capture without the lens return, the secondary job and the record preparation.
    pub mode: u32,
    pub sources: ClientRef,
    pub profile: ClientRef,
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

/// Where a capture dump goes inside the client's frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameOffset(pub u32);

/// A constant table the firmware copies into a dump.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameTable { SelectionMap }

/// Client memory the command-7 parameters name.
pub trait CaptureClient {
    /// The frame the source descriptor names.
    fn frame(&mut self, sources: ClientRef) -> Frame;
    /// The word the source descriptor keeps after the frame.
    fn frame_detail(&mut self, sources: ClientRef) -> u32;
    fn selection(&mut self, selection: ClientRef) -> SlotSelection;
    fn slot(&mut self, slots: ClientRef, index: u8) -> SlotHandle;
    fn store_selection_result(&mut self, selection: ClientRef, value: u32);
    /// Gives the selection the completion hook the capture leaves for it.
    fn store_selection_hook(&mut self, selection: ClientRef);
    fn frame_word(&mut self, frame: Frame, offset: FrameOffset) -> u32;
    fn set_frame_word(&mut self, frame: Frame, offset: FrameOffset, value: u32);
    fn fill_frame(&mut self, frame: Frame, offset: FrameOffset, bytes: u32, value: u8);
    fn copy_table(&mut self, frame: Frame, offset: FrameOffset, table: FrameTable);
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
    /// 1 lets a capture of the first slot refresh the slot rates and directions.
    SlotMode,
    /// 1 asks for the slot rate of the first slot's entry.
    SubjectKind,
    SubjectSource,
    SubjectArea,
    /// Nonzero ends a subject refresh before the candidates are replaced.
    SubjectsLocked,
    /// Nonzero keeps every candidate instead of the common ones.
    KeepsAllSubjects,
    SlotLock,
    /// 7 enables the secondary job; also blocks it in the secondary decision.
    SecondaryMode,
    /// 1 enables the secondary job and picks the device value for the slot.
    SecondaryLatch,
    SecondaryStyle,
    /// 1 blocks the secondary job.
    SecondaryBlocked,
    JobPrepared,
    /// 1 puts the selection map into the capture dump.
    MapPresent,
    /// Two halfwords the profile geometry takes.
    Layout,
    SecondaryOption,
    ProfileClass,
    FrameFirst,
    FrameSecond,
    /// Nonzero lets the evaluation of the secondary job place the record's first item.
    PlacesItem,
    MeasurementOption,
}

/// Per-slot values of the bound record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SlotField {
    Reference,
    Offset,
    Rate,
    Direction,
    Step,
    Residual,
    State,
    /// Both set for the first slot lets the secondary job run.
    SecondaryFirst,
    SecondarySecond,
    DeviceValue,
    SecondLimit,
    ItemState,
    /// 1 lets the secondary job measure the record's areas.
    MeasuresAreas,
    Distance,
    Trim,
    ProfileSelector,
    MeasuredValue,
    EvaluationWord,
    EvaluationOffset,
}

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

/// The record's area references, as the subject classification reads them.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AreaReference(pub [u32; 10]);


/// The words a setting completion returns and the record keeps.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SettingResult(pub [u32; 5]);

/// The image-4 record layer. A refused write is not observed.
pub trait CaptureRecords {
    fn field(&mut self, record: RecordHandle, field: RecordField) -> u32;
    fn set_field(&mut self, record: RecordHandle, field: RecordField, value: u32);
    fn slot_field(&mut self, record: RecordHandle, slot: u8, field: SlotField) -> u32;
    fn set_slot_field(&mut self, record: RecordHandle, slot: u8, field: SlotField, value: u32);
    fn reference_pair(&mut self, record: RecordHandle, index: u8) -> ReferencePair;
    fn slot_entry(&mut self, record: RecordHandle, slot: u8) -> SlotEntry;
    fn store_slot_entry(&mut self, target: SlotHandle, entry: &SlotEntry);
    fn slot_reference(&mut self, slot: SlotHandle) -> u32;
    fn slot_offset(&mut self, slot: SlotHandle) -> i16;
    fn load_profile(&mut self, record: RecordHandle, profile: ClientRef);
    fn select_slot(&mut self, record: RecordHandle, slot: SlotHandle);
    fn focus_result(&mut self, record: RecordHandle) -> FocusResult;
    fn store_focus_result(&mut self, record: RecordHandle, result: &FocusResult);
    fn area_reference(&mut self, record: RecordHandle) -> AreaReference;
    fn store_setting_result(&mut self, record: RecordHandle, result: &SettingResult);
    fn store_item(&mut self, record: RecordHandle, index: u8, item: &AreaEntry);
    fn publish_subjects(&mut self, record: RecordHandle, list: &SubjectList);
}

/// Lens-reference arithmetic of the image-4 configuration tables.
pub trait LensGeometry {
    /// The position offset of `reference` from the first reference.
    fn reference_offset(&mut self, reference: u32) -> i16;
    fn rounded(&mut self, value: u32) -> i16;
    /// `value` re-expressed at `lens` within the range `base` allows.
    fn convert(&mut self, base: u32, value: u32, lens: u32) -> u32;
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
pub enum MoveProfile { LensReturn, Finish { speed: u32 } }

/// Lens moves through the lens service, each completing with `completion`.
pub trait LensMoves {
    fn absolute(&mut self, target: u32, profile: MoveProfile, completion: Completion);
    fn relative(&mut self, delta: i16, speed: u32, profile: MoveProfile, completion: Completion);
}

/// The lens readings a capture of the first slot hands the calc library.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LensSample {
    pub position: u16,
    pub reference: u32,
    pub state: u32,
    pub kind: u32,
}

/// Where a reference calculation writes its result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CalculationResult { LensReturn }

/// Calc-library entries the capture steps call.
pub trait CalcLibrary {
    fn request_reference(&mut self, request: &ReferenceRequest, result: CalculationResult, completion: Completion);
    fn selection_rate(&mut self, record: RecordHandle, sample: LensSample, control: u32) -> u32;
    fn selection_direction(&mut self, sample: LensSample) -> u32;
    /// Writes the selection adjustment into the frame's dump.
    fn export_selection(&mut self, frame: Frame);
    fn export_joint_tables(&mut self, frame: Frame);
}

/// The reference template shared with the other saclac modules.
pub trait ReferenceTemplates {
    /// The template shifted to `reference`.
    fn reference_request(&mut self, reference: u32) -> ReferenceRequest;
}

/// Peer requests the capture controller sends itself.
pub trait CapturePeers {
    fn statistics_setup(&mut self, enabled: bool, completion: Completion);
    fn statistics_setting(&mut self, setting: &StatisticsSetting, completion: Completion);
}

/// Calculation-context words the capture reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CalculationWord {
    /// Nonzero lets a capture refresh the subjects.
    SubjectRefresh,
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
    /// The half press left work for the capture.
    fn has_work(&mut self) -> bool;
    fn secondary_enabled(&mut self) -> bool;
    fn set_secondary_enabled(&mut self, enabled: bool);
    /// Clears the secondary job's shared state.
    fn reset_secondary(&mut self);
    fn calculation_word(&mut self, word: CalculationWord) -> u32;
    /// Areas whose selection moved since the half press.
    fn moved_areas(&mut self) -> u8;
    fn classify_subjects(&mut self, reference: &AreaReference, kind: u32, source: u32, area: u32);
    /// Updates the capture's selection from the calculation.
    fn update_capture_selection(&mut self, record: RecordHandle);
    /// The selection map stands in the frame's dump.
    fn dump_armed(&mut self) -> bool;
    fn set_dump_armed(&mut self, armed: bool);
    /// The subjects the primary (`true`) or the secondary source left for the capture.
    fn subject_source(&mut self, primary: bool) -> SubjectList;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureAdjustment {
    /// Bit 7 asks for the capture dumps; bit 0 decides the secondary job by `SecondaryStyle`.
    Diagnostics,
    SecondaryStyle,
    /// Bit 0 locks the first slot when a job is prepared.
    SlotLock,
    /// Bits 0..1 enable the subject refresh; nonzero keeps the selection map.
    Subjects,
}

/// The parts of a distance a slot keeps and passes on.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DistanceSplit {
    pub kept: i16,
    pub excess: i16,
}

/// Services outside saclac the capture controller calls.
pub trait CapturePlatform {
    fn adjustment(&mut self, adjustment: CaptureAdjustment) -> u8;
    fn prepare_setting(&mut self);
    fn refresh_setting(&mut self);
    fn monitor_device_commands(&mut self, monitored: bool);
    fn device_status_bit(&mut self) -> bool;
    fn slot_rate(&mut self, active: u32, profile: u32) -> u32;
    fn split_distance(&mut self, distance: i16) -> DistanceSplit;
    fn device_value(&mut self) -> u32;
    fn export_record_snapshot(&mut self, frame: Frame);
    fn export_pipeline(&mut self, frame: Frame);
    fn prepare_dump_area(&mut self, frame: Frame, offset: FrameOffset);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureTrace { ReturnAbsolute, ReturnRelative, PrepareJob, Released }

pub trait CaptureDiagnostics {
    fn trace(&mut self, trace: CaptureTrace);
}
