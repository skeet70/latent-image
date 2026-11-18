#![forbid(unsafe_code)]

//! X-M1 1.50 layouts of the record load 0xf19634: the image-4 accessors behind `LoadRecords`, the
//! calculation-library exports, the auxiliary hook's arguments and the block it publishes.

use crate::boundary::LensSnapshot;
use crate::record_load::{AuxiliaryCall, LoadExport, ModeFiveBlock, ModeFiveValues, PairSlot, ResumeSource};
use xm1_saclac_interface::calculation::AreaResult;

/// Lens-snapshot halfword +0x28 (0xf199fc, 0xf19a14).
const SNAPSHOT_RESUME_OFFSET: usize = 0x28 - 36;

pub fn with_resume_offset(mut snapshot: LensSnapshot, offset: i16) -> LensSnapshot {
    snapshot.tail[SNAPSHOT_RESUME_OFFSET..SNAPSHOT_RESUME_OFFSET + 2].copy_from_slice(&offset.to_le_bytes());
    snapshot
}

/// The area result's halfword +0 (0xf19b20 reads it from the selected pass).
pub fn area_start(area: &AreaResult) -> i16 { i16::from_le_bytes([area.bytes()[0], area.bytes()[1]]) }

/// Halfwords +0, +2, +4 and the word +0x24 of the block (0xf19748..0xf197cc).
pub fn set_mode_five(block: &mut ModeFiveBlock, values: ModeFiveValues) {
    for (index, half) in values.profile.iter().enumerate() {
        block.0[2 * index..2 * index + 2].copy_from_slice(&half.to_le_bytes());
    }
    block.0[0x24..0x28].copy_from_slice(&values.row_word.to_le_bytes());
}

/// Lens snapshot 0x756840 and its setter 0x756878, both taking the record and the block.
pub const LENS_SNAPSHOT: (u32, u32) = (0x756840, 0x756878);
pub const LENS_REFERENCE: u32 = 0x751b74;
pub const fn resume_offset(source: ResumeSource) -> u32 {
    match source {
        ResumeSource::Profile => 0x74f5ec,
        ResumeSource::Stored => 0x75a440,
    }
}
pub const STORE_RESUME_OFFSET: u32 = 0x75a414;
/// 0x75a464(record, slot, &{reference, detail}).
pub const STORE_REFERENCE_PAIR: u32 = 0x75a464;
pub const fn pair_slot_index(slot: PairSlot) -> u32 { slot.get() as u32 }
/// The stored and the selected first halfwords (0xf19b28, 0xf19b34).
pub const STORE_AREA_STARTS: [u32; 2] = [0x75a324, 0x75a374];
pub const MODE_FIVE_PROFILE: [u32; 3] = [0x750efc, 0x751914, 0x7519ac];
/// 0x7588e8(block, record) and 0x75890c(record, block).
pub const MODE_FIVE_BLOCK: (u32, u32) = (0x7588e8, 0x75890c);
/// Getters of record slot 0, called with (record, 0): the start 0x752990, then the hook's order
/// (0xf19e58..0xf19ee8).
pub const STORED_AREA_START: u32 = 0x752990;
pub const STORED_AREA_REFERENCE: u32 = 0x752a40;
pub const STORED_AREA_SELECTION_CLASS: u32 = 0x7536a0;
pub const STORED_AREA_PLAN_HALF: u32 = 0x753750;
pub const STORED_AREA_PLAN_BITS: [u32; 2] = [0x753800, 0x7538b0];
pub const STORED_AREA_HALVES: [u32; 5] = [0x752ba0, 0x752c50, 0x752d00, 0x752f10, 0x753120];
/// 0x752928(entry, record, slot) and 0x75ae78(client slot, entry).
pub const RECORD_SLOT: (u32, u32) = (0x752928, 0x75ae78);
pub const AUXILIARY_HOOK: u32 = 0x756b9c;

/// 0xf602ec(frame, block, 0x16b4f24, 0x16b3e00, 0x16a026c) and 0xf6030c(frame, block, 0x16a026c),
/// the block being the calculation block 0x16ad740.
pub const fn export_entry(export: LoadExport) -> u32 {
    match export {
        LoadExport::Areas => 0xf602ec,
        LoadExport::Cell => 0xf6030c,
    }
}
pub const EXPORT_AREAS_TABLES: [u32; 3] = [0x16b_4f24, 0x16b_3e00, 0x16a_026c];
pub const EXPORT_CELL_TABLE: u32 = 0x16a_026c;

/// The thirteen words of the hook call (0xf19f48..0xf19fa0); halfword values are sign-extended.
pub fn auxiliary_hook_arguments(call: &AuxiliaryCall) -> [u32; 13] {
    let area = &call.area;
    let half = |value: i16| value as i32 as u32;
    [
        area.selection_class,
        half(area.halves[0]),
        area.reference,
        half(area.plan_half),
        area.plan_bit_two,
        area.plan_bit_five,
        0,
        1,
        half(area.halves[1]),
        half(area.halves[2]),
        half(area.halves[3]),
        half(area.halves[4]),
        call.reference,
    ]
}

/// The publication's address, copied under the interface lock 0x723efc / 0x723f7c.
pub const AUXILIARY_PUBLICATION: u32 = 0x16a_04a8;
pub const AUXILIARY_PUBLICATION_BYTES: usize = 48;

/// The block 0xf19fa4..0xf1a014 builds; stock leaves the halfwords at +0xa and +0x12 as stale
/// stack bytes, written here as zero.
pub fn auxiliary_publication(call: &AuxiliaryCall) -> [u8; AUXILIARY_PUBLICATION_BYTES] {
    let mut bytes = [0; AUXILIARY_PUBLICATION_BYTES];
    let area = &call.area;
    let mut word = |offset: usize, value: u32| bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    word(0x00, call.hook.0);
    word(0x04, area.selection_class);
    word(0x0c, area.reference);
    word(0x14, area.plan_bit_two);
    word(0x18, area.plan_bit_five);
    word(0x1c, 0);
    word(0x20, 1);
    word(0x2c, call.reference);
    let mut half = |offset: usize, value: i16| bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    half(0x08, area.halves[0]);
    half(0x10, area.plan_half);
    for (index, value) in area.halves[1..].iter().enumerate() {
        half(0x24 + 2 * index, *value);
    }
    bytes
}
