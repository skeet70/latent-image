#![forbid(unsafe_code)]

//! X-M1 1.50 boundary values of the capture steps.

use crate::boundary::ClientRef;
use crate::capture_boundary::{CaptureOptions, CaptureParameters, CaptureTrace, MoveProfile, RecordField, SlotField, SlotSelection};

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
        reference: at(0x10),
        slots: ClientRef(at(0x14)),
        selection: ClientRef(at(0x38)),
    })
}

/// Client words the capture steps read through the parameters: the source descriptor's frame at
/// +0x50 (0xf2efc0), a slot handle per word of the slot list (0xf2f0c8), and the selection's
/// requested slot and count bytes at +0 and +1 (0xf2f078, 0xf2f07c).
pub const SOURCE_FRAME: u32 = 0x50;
pub const SELECTION_REQUESTED: u32 = 0;
pub const SELECTION_COUNT: u32 = 1;

pub fn slot_selection(bytes: [u8; 2]) -> SlotSelection { SlotSelection { requested: bytes[0], count: bytes[1] } }

/// Record-layer entries behind each field: getters take the record, setters the record and value.
pub const fn record_field_entry(field: RecordField) -> u32 {
    match field {
        RecordField::ResourceKind => 0x74e7f0,
        RecordField::RemapMode => 0x75093c,
        RecordField::ReferenceFirstLimit => 0x74f5ec,
        RecordField::ReferenceSecondLimit => 0x74f6fc,
        RecordField::ReferenceOption => 0x74f784,
        RecordField::CaptureFrame => 0x75a564,
        RecordField::CaptureSlotCount => 0x75a5ac,
        RecordField::CaptureSlots => 0x75a5f4,
    }
}

/// Setters taking the record, the slot index and the value.
pub const fn slot_field_setter(field: SlotField) -> u32 {
    match field {
        SlotField::Reference => 0x752a98,
        SlotField::Offset => 0x752b48,
    }
}

/// Entries for the other record-layer calls: reference pair 0x75a4ac (record, index, out), slot
/// entry copy out 0x752928 (out, record, slot), slot record store 0x75ae78 (slot, entry), slot
/// record reference 0x75c874 and offset 0x75c8a4.
pub const REFERENCE_PAIR: u32 = 0x75a4ac;
pub const SLOT_ENTRY: u32 = 0x752928;
pub const STORE_SLOT_ENTRY: u32 = 0x75ae78;
pub const SLOT_REFERENCE: u32 = 0x75c874;
pub const SLOT_OFFSET: u32 = 0x75c8a4;

/// The words after the callback and target of the lens service's absolute move 0x76e654
/// (0xf2f2e8..0xf2f304, the finish's 0xf17d7c..0xf17d94).
pub const fn absolute_move_arguments(profile: MoveProfile) -> [u32; 4] {
    match profile {
        MoveProfile::LensReturn => [1, 1, 0, 4],
        MoveProfile::Finish { speed } => [1, 1, 0, speed],
    }
}

/// The words after the callback, delta and speed of the relative move 0x76e1ac (0xf2f3d0..0xf2f3f0,
/// the finish's 0xf1856c..0xf1858c).
pub const fn relative_move_arguments(profile: MoveProfile) -> [u32; 4] {
    match profile {
        MoveProfile::LensReturn => [1, 0, 5, 4],
        MoveProfile::Finish { speed } => [1, 0, speed, 4],
    }
}

/// Words passed to the trace entry 0x72872c.
pub const fn trace_word(trace: CaptureTrace) -> u32 {
    match trace {
        CaptureTrace::ReturnAbsolute => 0x0303_0100,
        CaptureTrace::ReturnRelative => 0x0303_0000,
    }
}
