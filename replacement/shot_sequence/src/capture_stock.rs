#![forbid(unsafe_code)]

//! X-M1 1.50 boundary values of the capture steps.

use crate::boundary::ClientRef;
use crate::capture_boundary::{
    CaptureAdjustment, CaptureOptions, CaptureParameters, CaptureTrace, FrameOffset, MoveProfile, RecordField, SettingResult, SlotEntry, SlotField,
    SlotSelection,
};
use xm1_saclac_interface::statistics::SettingOverrides;

fn word(block: &[u8], offset: usize) -> Option<u32> {
    let bytes: [u8; 4] = block.get(offset..offset + 4)?.try_into().ok()?;
    Some(u32::from_le_bytes(bytes))
}

pub const CAPTURE_BLOCK_BYTES: usize = 60;

/// The command-7 block (0x169d09c, 0xef9c00), as the capture steps read it.
pub fn capture_parameters(block: &[u8]) -> Option<CaptureParameters> {
    let block = block.get(..CAPTURE_BLOCK_BYTES)?;
    let at = |offset| word(block, offset).unwrap_or(0);
    Some(CaptureParameters {
        options: CaptureOptions(at(0x4)),
        mode: at(0x8),
        sources: ClientRef(at(0xc)),
        profile: ClientRef(at(0x10)),
        slots: ClientRef(at(0x14)),
        selection: ClientRef(at(0x38)),
    })
}

/// Client words the capture steps read through the parameters: the source descriptor's frame at
/// +0x50 (0xf2efc0), a slot handle per word of the slot list (0xf2f0c8), the selection's requested
/// slot and count bytes at +0 and +1 (0xf2f078, 0xf2f07c); they store the selection result at
/// +0x64 (0xf2a7a0) and the hook word at +0x68 (0xf2ab38).
pub const SOURCE_FRAME: u32 = 0x50;
pub const SELECTION_REQUESTED: u32 = 0;
pub const SELECTION_COUNT: u32 = 1;
pub const SELECTION_RESULT: u32 = 0x64;
pub const SELECTION_HOOK: u32 = 0x68;
/// 0x10029a8, an empty function of the capture pipeline.
pub const SELECTION_HOOK_WORD: u32 = 0x0100_29a8;

pub fn slot_selection(bytes: [u8; 2]) -> SlotSelection { SlotSelection { requested: bytes[0], count: bytes[1] } }

/// The capture-setting command block (0x169d0d8, 0xef9c1c) as 0xf2a14c..0xf2a1a0 read it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingParameters {
    pub words: [u32; 5],
    pub selector: ClientRef,
    pub window: ClientRef,
    pub option: ClientRef,
}

pub const SETTING_BLOCK_BYTES: usize = 40;

pub fn setting_parameters(block: &[u8]) -> Option<SettingParameters> {
    let block = block.get(..SETTING_BLOCK_BYTES)?;
    let at = |offset| word(block, offset).unwrap_or(0);
    Some(SettingParameters {
        words: [at(0x4), at(0xc), at(0x10), at(0x8), at(0x20)],
        selector: ClientRef(at(0x24)),
        window: ClientRef(at(0x18)),
        option: ClientRef(at(0x1c)),
    })
}

/// Reads the client memory the setting parameters name: the word at +4 of `selector`, six bytes
/// at `window`, the byte at +4 of `option`.
pub fn setting_overrides(parameters: &SettingParameters, mut read: impl FnMut(ClientRef, &mut [u8])) -> SettingOverrides {
    let mut selector = [0; 4];
    read(ClientRef(parameters.selector.0.wrapping_add(4)), &mut selector);
    let mut window = [0; 6];
    read(parameters.window, &mut window);
    let mut option = [0; 1];
    read(ClientRef(parameters.option.0.wrapping_add(4)), &mut option);
    SettingOverrides { selector: u32::from_le_bytes(selector), words: parameters.words, window, option: option[0] }
}

/// The 0x85 payload (0xf2903c): the status word, then the words the record keeps (0x7585dc
/// stores four of them at 0xf2a1e4).
pub fn setting_result(payload: &[u8]) -> Option<SettingResult> {
    let mut words = [0; 4];
    for (index, slot) in words.iter_mut().enumerate() {
        *slot = word(payload, 4 + 4 * index)?;
    }
    Some(SettingResult(words))
}

/// The record-layer getter behind each field, taking the record.
pub const fn record_field_getter(field: RecordField) -> Option<u32> {
    match field {
        RecordField::ResourceKind => Some(0x74e7f0),
        RecordField::RemapMode => Some(0x75093c),
        RecordField::ReferenceFirstLimit => Some(0x74f5ec),
        RecordField::ReferenceSecondLimit => Some(0x74f6fc),
        RecordField::ReferenceOption => Some(0x74f784),
        RecordField::SlotMode => Some(0x75a238),
        RecordField::SubjectKind => Some(0x75060c),
        RecordField::SubjectSource => Some(0x74e648),
        RecordField::SubjectArea => Some(0x7503ec),
        RecordField::SubjectsLocked => Some(0x74eba8),
        RecordField::KeepsAllSubjects => Some(0x74fc64),
        RecordField::SecondaryMode => Some(0x74f1ac),
        RecordField::SecondaryLatch => Some(0x74f344),
        RecordField::SecondaryBlocked => Some(0x74f2bc),
        RecordField::MapPresent => Some(0x750474),
        RecordField::CaptureFrame
        | RecordField::CaptureSlotCount
        | RecordField::CaptureSlots
        | RecordField::SlotLock
        | RecordField::SecondaryStyle
        | RecordField::JobPrepared => None,
    }
}

/// The record-layer setter behind each field, taking the record and the value.
pub const fn record_field_setter(field: RecordField) -> Option<u32> {
    match field {
        RecordField::CaptureFrame => Some(0x75a564),
        RecordField::CaptureSlotCount => Some(0x75a5ac),
        RecordField::CaptureSlots => Some(0x75a5f4),
        RecordField::SlotLock => Some(0x74f388),
        RecordField::SecondaryStyle => Some(0x74f278),
        RecordField::JobPrepared => Some(0x758360),
        _ => None,
    }
}

/// Getters taking the record and the slot index.
pub const fn slot_field_getter(field: SlotField) -> Option<u32> {
    match field {
        SlotField::SecondaryFirst => Some(0x753960),
        SlotField::SecondarySecond => Some(0x753a10),
        _ => None,
    }
}

/// Setters taking the record, the slot index and the value.
pub const fn slot_field_setter(field: SlotField) -> Option<u32> {
    match field {
        SlotField::Reference => Some(0x752a98),
        SlotField::Offset => Some(0x752b48),
        SlotField::Rate => Some(0x754098),
        SlotField::Direction => Some(0x754148),
        SlotField::Step => Some(0x752eb8),
        SlotField::Residual => Some(0x752e08),
        SlotField::State => Some(0x753c78),
        SlotField::DeviceValue => Some(0x753e88),
        SlotField::SecondLimit => Some(0x752bf8),
        SlotField::ItemState => Some(0x755b58),
        SlotField::SecondaryFirst | SlotField::SecondarySecond => None,
    }
}

/// Entries for the other record-layer calls: reference pair 0x75a4ac (record, index, out), slot
/// entry copy out 0x752928 (out, record, slot), slot record store 0x75ae78 (slot, entry), slot
/// record reference 0x75c874 and offset 0x75c8a4, profile load 0x74e1a4 (record, client block),
/// slot selection 0x74e284 (record, slot, 0), focus result 0x756734/0x756754, area reference
/// 0x7582c0 (out, record), setting result 0x7585dc (record, words), item 0x755c00 (record, index,
/// item), subject publication 0x756478 (record, entries).
pub const REFERENCE_PAIR: u32 = 0x75a4ac;
pub const SLOT_ENTRY: u32 = 0x752928;
pub const STORE_SLOT_ENTRY: u32 = 0x75ae78;
pub const SLOT_REFERENCE: u32 = 0x75c874;
pub const SLOT_OFFSET: u32 = 0x75c8a4;
pub const LOAD_PROFILE: u32 = 0x74e1a4;
pub const SELECT_SLOT: u32 = 0x74e284;
pub const FOCUS_RESULT: u32 = 0x756734;
pub const STORE_FOCUS_RESULT: u32 = 0x756754;
pub const AREA_REFERENCE: u32 = 0x7582c0;
pub const STORE_SETTING_RESULT: u32 = 0x7585dc;
pub const STORE_ITEM: u32 = 0x755c00;
pub const PUBLISH_SUBJECTS: u32 = 0x756478;

fn entry_half(entry: &SlotEntry, offset: usize) -> i16 { i16::from_le_bytes([entry.0[offset], entry.0[offset + 1]]) }

fn put_entry_half(entry: &mut SlotEntry, offset: usize, value: i16) { entry.0[offset..offset + 2].copy_from_slice(&value.to_le_bytes()); }

/// Slot-entry fields, as 0xf2a7e8..0xf2a820, 0xf2a790 and 0xf1dd14 read and write them.
impl SlotEntry {
    pub fn offset(&self) -> i16 { entry_half(self, 0x8) }
    pub fn trims(&self) -> [i16; 2] { [entry_half(self, 0xc), entry_half(self, 0xe)] }
    pub fn residual(&self) -> i16 { entry_half(self, 0x2e) }
    pub fn set_residual(&mut self, value: i16) { put_entry_half(self, 0x2e, value); }
    pub fn step(&self) -> i16 { entry_half(self, 0x30) }
    pub fn set_step(&mut self, value: i16) { put_entry_half(self, 0x30, value); }
    pub fn rate_active(&self) -> u32 { word(&self.0, 0x24).unwrap_or(0) }
    pub fn rate_profile(&self) -> u32 { word(&self.0, 0x28).unwrap_or(0) }
    pub fn primary_subjects(&self) -> bool { word(&self.0, 0xa0) == Some(1) }
}

/// Bytes the adjustment reads 0x69f8fc take.
pub const fn adjustment_id(adjustment: CaptureAdjustment) -> u16 {
    match adjustment {
        CaptureAdjustment::Diagnostics => 0xa8,
        CaptureAdjustment::SecondaryStyle => 0xb1,
        CaptureAdjustment::SlotLock => 0xe1,
        CaptureAdjustment::Subjects => 0x5257,
    }
}

/// The capture trace area of the frame, cleared except for four words (0xf2aa48..0xf2aab0).
pub const TRACE_AREA: FrameOffset = FrameOffset(0xa_074c);
pub const TRACE_AREA_BYTES: u32 = 0x5394;
pub const TRACE_KEPT: [FrameOffset; 4] = [FrameOffset(0xa_5680), FrameOffset(0xa_5684), FrameOffset(0xa_5688), FrameOffset(0xa_568c)];

/// The selection map dump of the frame (0xf49138): an area with marks at both ends, a table that
/// holds the map, and the parts filled with 0xff when the subjects are off.
pub const MAP_AREA: FrameOffset = FrameOffset(0x8_4f74);
pub const MAP_AREA_BYTES: u32 = 0x1_7f5c;
pub const MAP_END: FrameOffset = FrameOffset(0x9_cecc);
pub const MAP_MARKS: (u32, u32) = (0x5354_4d44, 0x4c41_4d44);
pub const MAP_TABLE: FrameOffset = FrameOffset(0x9_1064);
pub const MAP_TABLE_BYTES: u32 = 0xbe68;
pub const MAP_UNSET: [(FrameOffset, u32); 3] = [(FrameOffset(0x9_1064), 0x2c00), (FrameOffset(0x9_3c7c), 0x8400), (FrameOffset(0x9_c07c), 0x168)];
/// The selection map table the firmware copies (0xf49238).
pub const SELECTION_MAP_TABLE: u32 = 0x5ffd_b9ec;

/// The words after the callback and target of the lens service's absolute move 0x76e654
/// (0xf2f2e8..0xf2f304).
pub const fn absolute_move_arguments(profile: MoveProfile) -> [u32; 4] {
    match profile {
        MoveProfile::LensReturn => [1, 1, 0, 4],
    }
}

/// The words after the callback, delta and speed of the relative move 0x76e1ac (0xf2f3d0..0xf2f3f0).
pub const fn relative_move_arguments(profile: MoveProfile) -> [u32; 4] {
    match profile {
        MoveProfile::LensReturn => [1, 0, 5, 4],
    }
}

/// Words passed to the trace entry 0x72872c.
pub const fn trace_word(trace: CaptureTrace) -> u32 {
    match trace {
        CaptureTrace::ReturnAbsolute => 0x0303_0100,
        CaptureTrace::ReturnRelative => 0x0303_0000,
        CaptureTrace::PrepareJob => 0x0300_0000,
        CaptureTrace::Released => 0x1300_0000,
    }
}

/// Words of the statistics setup request 0x1039dd4 after the enable flag (0xf2917c..0xf29180).
pub const STATISTICS_SETUP_SECOND_WORD: u32 = 1;
