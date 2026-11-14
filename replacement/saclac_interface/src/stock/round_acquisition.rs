#![forbid(unsafe_code)]

//! X-M1 1.50 bytes of the measurement round's acquisition payloads: the calc1 0x3d00 record
//! (0xf3579c, S1 0xf242ec) and its grid, the 0x3d01 record (0xf36eac, S1 0xf24bd0) with the
//! window and setting records 0xf367a4 builds where calc1 reads them, the result records the
//! 0x87/0x88 (S1 0x9b/0x9c) completions point at, and the tables posted to imgdc 0x3f2c, 0x3f2d
//! and 0x3f45.

use crate::calc::{AcquisitionGrid, SharedAddress};
use crate::round_acquisition::{
    AcquisitionRequest, AcquisitionResult, ChangeNotice, FocusTable, PrimaryLevels, RestoredSetting, SavedExposure,
    SavedSelection, SelectionEntry, SelectionLevels, SelectionProgram, SelectionRequest, SelectionResult,
    SelectionSetting, SelectionTable, SelectionWindow, ShotRecordLevels,
};

fn put_word(bytes: &mut [u8], offset: usize, value: u32) { bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes()); }
fn put_half(bytes: &mut [u8], offset: usize, value: u16) { bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes()); }
fn word(bytes: &[u8], offset: usize) -> u32 { u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]]) }
fn half(bytes: &[u8], offset: usize) -> u16 { u16::from_le_bytes([bytes[offset], bytes[offset + 1]]) }
fn halves<const N: usize>(bytes: &[u8], offset: usize) -> [u16; N] { core::array::from_fn(|index| half(bytes, offset + 2 * index)) }

/// The grid's values the round reads: its first byte and word, the byte at +0x2e and the
/// halfword at +0x2a (0xf33328, 0xf33370, 0xf368a8, 0xf368f0, 0xf36e08).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GridLead {
    pub first_byte: u8,
    pub first_word: u32,
    pub flag: u8,
    pub halfword: u16,
}

pub fn grid_lead(grid: &AcquisitionGrid) -> GridLead {
    let bytes = &grid.0;
    GridLead { first_byte: bytes[4], first_word: word(bytes, 4), flag: bytes[0x2e], halfword: half(bytes, 0x2a) }
}

/// 0xf35b40..0xf35b48.
pub fn set_grid_count(grid: &mut AcquisitionGrid, count: u32) { put_word(&mut grid.0, 0, count); }

/// 0xf35ca0..0xf35cac: an empty grid's first word.
pub fn clear_grid_lead(grid: &mut AcquisitionGrid) { put_word(&mut grid.0, 4, 0); }

/// 0xf35c30..0xf35c8c: the inputs 0xfa7bb8 fills the live grid from, at +0x418.
pub fn set_grid_inputs(grid: &mut AcquisitionGrid, timing: u32, lens_word: u32, positions: [u32; 2], window: u32) {
    let bytes = &mut grid.0;
    put_word(bytes, 0x418, timing);
    put_word(bytes, 0x41c, lens_word);
    put_word(bytes, 0x420, positions[0]);
    put_word(bytes, 0x424, positions[1]);
    put_word(bytes, 0x428, window);
}

/// 0xf35cc8..0xf35cd0.
pub fn set_grid_configuration(grid: &mut AcquisitionGrid, configuration: u8) { grid.0[0x458] = configuration; }

/// Entries the grid holds from +0x45c.
pub const GRID_ENTRIES: usize = 10;

/// 0xf35b64..0xf35c2c: the grid copied from a primary snapshot, its entries named by the
/// snapshot's halfword at +0x118. Stock copies that many 8-byte entries unchecked, past the grid
/// into the acquisition's initialization levels from the eleventh; owned stops at the tenth.
pub fn grid_from_snapshot(grid: &mut AcquisitionGrid, snapshot: &[u8; 0x5c4]) {
    let bytes = &mut grid.0;
    bytes[4..0x418].copy_from_slice(&snapshot[0x118..0x52c]);
    bytes[0x418..0x430].copy_from_slice(&snapshot[0x52c..0x544]);
    bytes[0x430..0x458].copy_from_slice(&snapshot[0x544..0x56c]);
    let entries = usize::from(half(snapshot, 0x118)).min(GRID_ENTRIES);
    for entry in 0..entries {
        bytes[0x45c + 8 * entry..0x464 + 8 * entry].copy_from_slice(&snapshot[0x84 + 8 * entry..0x8c + 8 * entry]);
    }
    put_word(bytes, 0x4b0, 320);
    put_word(bytes, 0x4ac, 0x5ffe_d1c4);
}

/// The grid's three regions the record publishes, each a lead of three words and its body
/// (0xf33754..0xf337d0).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GridPublication<'a> {
    pub cells: ([u32; 3], &'a [u8]),
    pub window: ([u32; 3], &'a [u8]),
    pub entries: ([u32; 3], &'a [u8]),
}

pub fn grid_publication(grid: &AcquisitionGrid) -> GridPublication<'_> {
    let bytes = &grid.0;
    let lead = |offset: usize| [word(bytes, offset), word(bytes, offset + 4), word(bytes, offset + 8)];
    GridPublication {
        cells: (lead(4), &bytes[0x10..0x418]),
        window: (lead(0x430), &bytes[0x43c..0x458]),
        entries: (lead(0x45c), &bytes[0x468..0x4ac]),
    }
}

pub const ACQUISITION_REQUEST_BYTES: usize = 168;

/// The record at 0x16bfb88 (S1: on the stack). Bytes 0x26..0x28, 0x36..0x38, 0x6d and 0x72..0x74
/// are never written; the live record keeps them zero, the S1 record leaves stack residue in
/// them and in bytes 4..6, which owned sends as zero.
pub fn acquisition_request(request: &AcquisitionRequest) -> [u8; ACQUISITION_REQUEST_BYTES] {
    let mut bytes = [0; ACQUISITION_REQUEST_BYTES];
    let (profile, sample) = (&request.profile, &request.sample);
    put_word(&mut bytes, 0x00, request.mode);
    put_half(&mut bytes, 0x04, profile.halfword);
    put_word(&mut bytes, 0x08, profile.updated_position);
    put_word(&mut bytes, 0x0c, u32::from(request.first));
    put_word(&mut bytes, 0x10, u32::from(sample.complete));
    put_word(&mut bytes, 0x14, u32::from(profile.selection));
    put_word(&mut bytes, 0x18, profile.adjustment);
    put_half(&mut bytes, 0x1c, profile.reference);
    put_word(&mut bytes, 0x20, profile.word);
    put_half(&mut bytes, 0x24, request.relative_level);
    put_word(&mut bytes, 0x28, sample.exposure);
    put_half(&mut bytes, 0x2c, sample.level);
    put_half(&mut bytes, 0x2e, sample.reference_level);
    put_word(&mut bytes, 0x30, sample.timing);
    put_half(&mut bytes, 0x34, sample.halfword);
    put_word(&mut bytes, 0x38, sample.extent[0]);
    put_word(&mut bytes, 0x3c, sample.extent[1]);
    put_word(&mut bytes, 0x40, sample.size[0]);
    put_word(&mut bytes, 0x44, sample.size[1]);
    put_word(&mut bytes, 0x48, sample.origin[0]);
    put_word(&mut bytes, 0x4c, sample.origin[1]);
    for (index, value) in [100, 100, 256, 100, 100, 1].into_iter().enumerate() { put_half(&mut bytes, 0x50 + 2 * index, value); }
    put_word(&mut bytes, 0x5c, 0x5ffe_c1b4);
    put_word(&mut bytes, 0x60, 0x5ffe_d1b4);
    put_word(&mut bytes, 0x64, 4096);
    put_word(&mut bytes, 0x68, 16);
    bytes[0x6c] = request.configuration;
    put_word(&mut bytes, 0x6e, profile.window);
    put_word(&mut bytes, 0x74, sample.gain_step);
    put_word(&mut bytes, 0x78, request.level_category);
    put_word(&mut bytes, 0x7c, sample.adjustment);
    put_half(&mut bytes, 0x80, request.correction);
    put_half(&mut bytes, 0x82, request.aspect);
    put_half(&mut bytes, 0x84, request.decade);
    put_half(&mut bytes, 0x86, sample.offset_level);
    for (index, value) in profile.area.into_iter().enumerate() { put_word(&mut bytes, 0x88 + 4 * index, value); }
    put_word(&mut bytes, 0x9c, profile.category);
    put_word(&mut bytes, 0xa0, profile.ratio[0]);
    put_word(&mut bytes, 0xa4, profile.ratio[1]);
    bytes
}

/// Where calc1 reads an acquisition's window and setting records and the shot's saved reference.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelectionRegion {
    pub window: SharedAddress,
    pub setting: SharedAddress,
    pub shot_copy: SharedAddress,
}

/// 0xf37670..0xf3770c, beside the selection guard words at 0x41717f6c.
pub const LIVE_REGION: SelectionRegion = SelectionRegion {
    window: SharedAddress(0x4171_7f78),
    setting: SharedAddress(0x4171_8020),
    shot_copy: SharedAddress(0x4171_814c),
};

/// 0xf250c4..0xf25140.
pub const S1_REGION: SelectionRegion = SelectionRegion {
    window: SharedAddress(0x4171_7b80),
    setting: SharedAddress(0x4171_7c28),
    shot_copy: SharedAddress(0x4171_7d54),
};

pub const SELECTION_REQUEST_BYTES: usize = 228;

/// The record at 0x16bf9d0 (S1: 0x16b5f44). Bytes 0x28..0x2a, 0x52..0x54, 0x5b and 0xde..0xe0
/// are never written.
pub fn selection_request(request: &SelectionRequest, region: SelectionRegion) -> [u8; SELECTION_REQUEST_BYTES] {
    let mut bytes = [0; SELECTION_REQUEST_BYTES];
    let words: [(usize, u32); 36] = [
        (0x00, u32::from(request.first)), (0x04, request.mode), (0x0c, request.third_word.0), (0x10, request.zero_word.0),
        (0x14, request.mode_word), (0x1c, request.lens), (0x20, request.converted_step), (0x2c, request.selection_word),
        (0x30, request.selection), (0x34, request.exposure), (0x44, request.lens_reference), (0x4c, request.gain_step),
        (0x50, u32::from(request.unrestored)), (0x5c, request.eighth_word.0), (0x60, request.grid_count), (0x64, request.second_word.0),
        (0x68, request.adjustment), (0x70, request.seventh_word.0), (0x74, request.fourth_word.0), (0x78, request.fifth_word.0),
        (0x7c, request.focus_word), (0x80, request.word), (0x88, request.mode_flag), (0x8c, request.kind),
        (0x90, request.ninth_word.0), (0x94, request.option), (0x98, u32::from(request.area[1])), (0x9c, u32::from(request.area[0])),
        (0xa0, u32::from(request.area[2])), (0xa4, request.pair_word), (0xa8, request.program_word), (0xac, u32::from(request.rate_flag)),
        (0xb0, request.program), (0xb4, request.request), (0xb8, request.eligible), (0xc8, u32::from(request.scale_change)),
    ];
    for (offset, value) in words { put_word(&mut bytes, offset, value); }
    let halves: [(usize, u16); 17] = [
        (0x08, request.halfword), (0x1a, request.second_halfword), (0x24, request.converted_level), (0x26, request.reference),
        (0x2a, request.restore_adjustment), (0x38, request.summary_level), (0x3a, request.limits.step), (0x3c, request.limits.lower),
        (0x3e, request.limits.upper), (0x42, request.lens_halfword), (0x48, request.limit_halfwords[0]), (0x4a, request.limit_halfwords[1]),
        (0x54, request.correction), (0x56, request.aspect), (0x58, request.decade), (0x6c, request.signed_word), (0x84, request.rate_mode),
    ];
    for (offset, value) in halves { put_half(&mut bytes, offset, value); }
    put_half(&mut bytes, 0x86, request.resource_level);
    put_half(&mut bytes, 0xd8, request.s1.halfword);
    put_half(&mut bytes, 0xda, request.s1.margin);
    bytes[0x18] = request.byte;
    bytes[0x40] = request.limits.byte;
    bytes[0x5a] = request.adjustment_class;
    bytes[0xdc] = u8::from(request.recording);
    bytes[0xdd] = request.s1.byte;
    if request.window {
        put_word(&mut bytes, 0xbc, region.window.0);
        put_word(&mut bytes, 0xc0, region.setting.0);
    }
    if request.shot_copy { put_word(&mut bytes, 0xc4, region.shot_copy.0); }
    put_word(&mut bytes, 0xcc, request.program_changed);
    put_word(&mut bytes, 0xd0, request.program_flag);
    put_word(&mut bytes, 0xd4, request.offset_mode);
    put_word(&mut bytes, 0xe0, request.s1.position);
    bytes
}

/// 0xf36bac..0xf36e6c: the window's fields, written in place where calc1 reads them; `grid` is
/// the grid's address. Bytes 0x2c..0x30, 0x5e..0x60 and 0xa6..0xa8 are left as they are.
pub fn selection_window(window: &SelectionWindow, grid: SharedAddress, mut put: impl FnMut(usize, &[u8])) {
    let words: [(usize, u32); 27] = [
        (0x00, window.kind), (0x04, window.selection), (0x08, window.updated_position), (0x10, window.focus_word),
        (0x14, window.lens), (0x18, window.lens_reference), (0x1c, window.mode_rule), (0x20, window.second_word.0),
        (0x24, window.mode_word), (0x28, window.request), (0x30, window.clock), (0x34, window.rate),
        (0x38, window.context[0]), (0x3c, window.context[1]), (0x40, window.context[2]), (0x60, window.rate_limit),
        (0x68, window.ratio[0]), (0x6c, window.ratio[1]), (0x70, u32::from(window.converted)), (0x74, window.both_modes),
        (0x78, window.one_mode), (0x7c, grid.0.wrapping_add(4)), (0x80, grid.0.wrapping_add(0x430)), (0x84, window.context_word),
        (0x98, window.change_category), (0x9c, window.category), (0xa0, u32::from(window.position_flag)),
    ];
    for (offset, value) in words { put(offset, &value.to_le_bytes()); }
    for offset in [0x48, 0x4c, 0x50, 0x54, 0x88, 0x8c, 0x90, 0x94] { put(offset, &[0; 4]); }
    let halves: [(usize, u16); 7] = [
        (0x0c, window.position_halfword), (0x58, window.position_level), (0x5a, window.relative_level),
        (0x5c, window.resource_level), (0x64, window.rate_mode), (0x66, window.target_level), (0xa4, window.limit_halfword),
    ];
    for (offset, value) in halves { put(offset, &value.to_le_bytes()); }
    put(0x44, &[window.context_byte]);
}

/// 0xf367c8..0xf36ba8: the setting's fields, written in place; `setting` is its address. The
/// calc library's position at +0x1c and bytes 0x18..0x1c, 0x38..0x3c, 0x4e..0x50 and 0x104..0x118
/// are left as they are.
pub fn selection_setting(setting: &SelectionSetting, address: SharedAddress, mut put: impl FnMut(usize, &[u8])) {
    let words: [(usize, u32); 41] = [
        (0x00, 1), (0x04, 3), (0x08, setting.lens), (0x0c, setting.shot_word), (0x14, setting.category), (0x30, 0),
        (0x34, setting.one_mode), (0x40, setting.context_word), (0x50, setting.selection), (0x54, setting.updated_position),
        (0x5c, setting.mode_word), (0x64, setting.setting_word), (0x68, setting.grid_kind), (0x6c, setting.focus_word),
        (0x70, setting.fourth_word.0), (0x74, setting.request), (0x78, setting.rate_mode), (0x7c, setting.word),
        (0x84, setting.selection_word), (0x8c, setting.lens_reference), (0x98, setting.eighth_word.0), (0x9c, setting.mode_rule),
        (0xa0, setting.category_word), (0xa4, setting.change_category), (0xa8, setting.second_word.0), (0xac, setting.mode_flag),
        (0xb0, u32::from(setting.rate_flag)), (0xb4, setting.fifth_word.0), (0xb8, setting.ninth_word.0), (0xdc, setting.tenth_word.0),
        (0xe0, setting.lens_limit), (0xe4, setting.lens_word_index), (0xe8, setting.kind), (0xec, setting.kind_override),
        (0xf0, setting.adjustment), (0xf4, setting.profile_index), (0xf8, setting.program_word), (0xfc, setting.restore_position),
        (0x100, setting.lens), (0x118, address.0.wrapping_add(0x1c)), (0x11c, u32::from(setting.hardware_ready)),
    ];
    for (offset, value) in words { put(offset, &value.to_le_bytes()); }
    for (index, flag) in setting.profile_flags.into_iter().enumerate() { put(0xbc + 4 * index, &flag.to_le_bytes()); }
    put(0x120, &4u32.to_le_bytes());
    put(0x124, &0u16.to_le_bytes());
    put(0x128, &0u32.to_le_bytes());
    let halves: [(usize, u16); 13] = [
        (0x10, setting.margin), (0x44, 0), (0x46, 0), (0x48, 0), (0x4a, 0), (0x4c, 0), (0x58, setting.position_halfword),
        (0x62, setting.halfword), (0x80, setting.reference), (0x82, setting.restore_adjustment), (0x88, setting.lens_halfword),
        (0x90, setting.halfwords[0]), (0x92, setting.halfwords[1]),
    ];
    for (offset, value) in halves { put(offset, &value.to_le_bytes()); }
    put(0x94, &setting.halfwords[2].to_le_bytes());
    put(0x3c, &[3, 3, 0, 0]);
    put(0x60, &[setting.byte]);
}

/// 0xf376dc..0xf37704 (S1: 0xf25104..0xf2512c): the fields the round writes over a setting the
/// shot restored.
pub fn restored_setting(setting: &RestoredSetting, mut put: impl FnMut(usize, &[u8])) {
    put(0x88, &setting.lens_halfword.to_le_bytes());
    put(0x8c, &setting.lens_reference.to_le_bytes());
    put(0x80, &setting.reference.to_le_bytes());
    put(0x10, &setting.margin.to_le_bytes());
}

/// The setting's fields 0xf356f0..0xf3574c reads back after the selection: hardware ready at
/// +0x11c, the updated position at +0x54, its halfword at +0x58 and the shot word at +0xc.
pub fn setting_echo(setting: &[u8; 300]) -> (u32, u32, u16, u32) {
    (word(setting, 0x11c), word(setting, 0x54), half(setting, 0x58), word(setting, 0xc))
}

pub const ACQUISITION_RESULT_BYTES: usize = 0x50;

/// 0xf3515c..0xf35248 and 0xf336cc.
pub fn acquisition_result(address: SharedAddress, bytes: &[u8; ACQUISITION_RESULT_BYTES]) -> AcquisitionResult {
    AcquisitionResult {
        address,
        levels: PrimaryLevels { base: half(bytes, 0), levels: halves(bytes, 2), word: word(bytes, 0x10) },
        statistics: core::array::from_fn(|index| SharedAddress(word(bytes, 0x18 + 4 * index))),
        exposure: word(bytes, 0x28),
        level: half(bytes, 0x2c),
        second: half(bytes, 0x2e),
        offset: half(bytes, 0x30),
        aspect: half(bytes, 0x32),
        halfword: half(bytes, 0x34),
        gain_offset: half(bytes, 0x36),
        correction: half(bytes, 0x38),
        count: word(bytes, 0x3c),
        statistics_valid: word(bytes, 0x44) == 1,
    }
}

pub const SELECTION_RESULT_BYTES: usize = 0x114;

/// 0xf335b0, 0xf3557c, 0xf35d14, 0xf2476c, 0xf254bc and 0xf255ac.
pub fn selection_result(address: SharedAddress, bytes: &[u8; SELECTION_RESULT_BYTES]) -> SelectionResult {
    let program = |exposure: usize, levels: usize| SelectionProgram {
        exposure: word(bytes, exposure),
        levels: halves(bytes, levels),
        first: halves(bytes, levels + 8),
        second: halves(bytes, levels + 16),
        adjustment: halves(bytes, levels + 24),
    };
    SelectionResult {
        address,
        word: word(bytes, 4),
        levels: SelectionLevels {
            base: half(bytes, 0),
            levels: halves(bytes, 0x16),
            zeroed: halves(bytes, 0x1c),
            unsigned: half(bytes, 0x28),
            word: word(bytes, 0x2c),
            last: half(bytes, 0x30),
            reference: half(bytes, 0x9c),
            tail: halves(bytes, 0xb4),
        },
        secondary: program(0x34, 0x38),
        primary: program(0x58, 0x5c),
        restore_levels: halves(bytes, 0x84),
        position: word(bytes, 0x88),
        halfwords: [half(bytes, 0x8c), half(bytes, 0x8e)],
        words: [word(bytes, 0x90), word(bytes, 0x94), word(bytes, 0x98)],
        table_reference: half(bytes, 0x9e),
        table_word: word(bytes, 0xa0),
        primary_only: word(bytes, 0xa4),
        program_argument: word(bytes, 0xa8),
        alternate: word(bytes, 0xb0),
        kind: word(bytes, 0xbc),
        shot_record: SharedAddress(word(bytes, 0xc4)),
        saved_record: SharedAddress(word(bytes, 0xc8)),
        selected: word(bytes, 0xcc),
        shot_data: word(bytes, 0xd0) != 0,
        keeps_program: word(bytes, 0xd8) != 0,
        adjustment: word(bytes, 0xdc),
        program_word: word(bytes, 0xe0),
        limit: bytes[0xe4],
        program_flag: word(bytes, 0xe8),
        tail: halves(bytes, 0xec),
        level_halfword: half(bytes, 0xf8),
        position_word: word(bytes, 0xfc),
        position_flag: bytes[0x100],
        save: SavedSelection {
            word: word(bytes, 0x104),
            index: word(bytes, 0x108),
            bytes: [bytes[0x10c], bytes[0x10d]],
            level: half(bytes, 0x10e) as i16,
            flag: bytes[0x110],
        },
    }
}

/// 0xf3572c..0xf35734: the halfwords at +4 and +0x18 of the record a result names for the shot,
/// 0x7c bytes.
pub fn shot_record_levels(record: &[u8; 0x7c]) -> ShotRecordLevels {
    ShotRecordLevels { first: half(record, 4), last: half(record, 0x18) }
}

/// 0xf2568c..0xf256c8: the saved record's fields the S1 restore publishes.
pub fn saved_exposure(record: &[u8; 0x44]) -> SavedExposure {
    SavedExposure {
        exposure: word(record, 8),
        level: half(record, 0xe),
        second: half(record, 0x3e),
        aspect: half(record, 0x14),
        offset: half(record, 0x40),
    }
}

/// 0xf35510..0xf35564 then 0xfea5f0: imgdc 0x3f2d carries the positions twice.
pub fn change_notice(notice: &ChangeNotice) -> [u8; 44] {
    let mut bytes = [0; 44];
    let words = [
        notice.category, notice.mode_is_one, notice.lens, notice.positions[0], notice.positions[1], notice.unbounded,
        notice.positions[0], notice.positions[1], notice.pair[0], notice.pair[1], notice.kind,
    ];
    for (index, value) in words.into_iter().enumerate() { put_word(&mut bytes, 4 * index, value); }
    bytes
}

fn entry(bytes: &mut [u8], entry: &SelectionEntry) {
    put_word(bytes, 0, entry.timing);
    put_half(bytes, 4, entry.code);
    bytes[6] = entry.selected;
    put_word(bytes, 8, entry.gain_step);
    put_word(bytes, 0xc, entry.exposure);
    put_half(bytes, 0x10, entry.level);
    put_half(bytes, 0x12, entry.second);
    put_half(bytes, 0x14, entry.offset);
    put_word(bytes, 0x18, entry.word);
    put_half(bytes, 0x1c, entry.reference);
    put_word(bytes, 0x20, entry.decade);
}

pub const ENTRY_BYTES: usize = 36;

/// 0xf35f6c..0xf3619c and 0xf36364..0xf3638c; 0xfea5c0 copies the 296 bytes to 0x16f4488 and posts
/// imgdc 0x3f2c without a payload.
pub fn selection_table(table: &SelectionTable) -> [u8; 296] {
    let mut bytes = [0; 296];
    for (index, candidate) in table.primary.iter().enumerate() { entry(&mut bytes[ENTRY_BYTES * index..], candidate); }
    for (index, candidate) in table.secondary.iter().enumerate() { entry(&mut bytes[0x90 + ENTRY_BYTES * index..], candidate); }
    for (index, level) in table.levels.into_iter().enumerate() { put_half(&mut bytes, 0x120 + 2 * index, level); }
    bytes
}

/// 0xf24910..0xf249cc; 0xfeab3c copies the 80 bytes to 0x16f224c and posts imgdc 0x3f45 without a
/// payload.
pub fn focus_table(table: &FocusTable) -> [u8; 80] {
    let mut bytes = [0; 80];
    entry(&mut bytes[..], &table.primary);
    entry(&mut bytes[ENTRY_BYTES..], &table.secondary);
    bytes
}
