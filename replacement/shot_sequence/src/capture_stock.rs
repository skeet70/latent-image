#![forbid(unsafe_code)]

//! X-M1 1.50 boundary values of the capture steps.

use crate::boundary::ClientRef;
use crate::calculations_work::Processing;
use crate::capture_boundary::{
    CaptureAdjustment, CaptureOptions, CaptureParameters, CaptureTrace, FrameOffset, MoveProfile, RecordField, SettingResult, SlotEntry, SlotField,
    SlotSelection,
};
use crate::primary_boundary::{AreaMode, CaptureStatus};
use crate::secondary_boundary::{AreaList, MeasurementSetting, MeasurementWord, StatisticsStore};
use xm1_saclac_interface::statistics::SettingOverrides;

fn word(block: &[u8], offset: usize) -> Option<u32> {
    let bytes: [u8; 4] = block.get(offset..offset + 4)?.try_into().ok()?;
    Some(u32::from_le_bytes(bytes))
}

pub const CAPTURE_BLOCK_BYTES: usize = 60;

/// The command-7 block (0x169d09c, 0xef9c00), as the capture steps read it: the primary job
/// reads the calibration block at +0x1c (0xf2cc94, 0xf2cd2c), the window at +0x24 (0xf2ce14),
/// the area buffer at +0x28 (0xf2de24) and the selector block at +0x30 (0xf2cdf4).
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
        calibration: ClientRef(at(0x1c)),
        window: ClientRef(at(0x24)),
        areas: ClientRef(at(0x28)),
        statistics_selector: ClientRef(at(0x30)),
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

/// The 0x85 and 0x7d payloads (0xf2903c, 0xf2cad8): the status word, then the five words the
/// record keeps (0x7585dc copies 20 bytes).
pub fn setting_result(payload: &[u8]) -> Option<SettingResult> {
    let mut words = [0; 5];
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
        RecordField::Layout => Some(0x74ec30),
        RecordField::SecondaryOption => Some(0x74fcec),
        RecordField::ProfileClass => Some(0x74e7a0),
        RecordField::FrameFirst => Some(0x7527f8),
        RecordField::FrameSecond => Some(0x752890),
        RecordField::PlacesItem => Some(0x74eb20),
        RecordField::MeasurementOption => Some(0x74f9a4),
        RecordField::StatisticsWord => Some(0x75a200),
        RecordField::StatisticsByte => Some(0x759140),
        RecordField::At0035 => Some(0x74ece4),
        RecordField::At0036 => Some(0x74ed6c),
        RecordField::At0038 => Some(0x74ee7c),
        RecordField::At0039 => Some(0x74ef04),
        RecordField::At003a => Some(0x74ef8c),
        RecordField::At003b => Some(0x74f014),
        RecordField::At003e => Some(0x74f1f0),
        RecordField::At0042 => Some(0x74f3cc),
        RecordField::At0049 => Some(0x74fdfc),
        RecordField::At004a => Some(0x74fe84),
        RecordField::At004d => Some(0x7501cc),
        RecordField::At0053 => Some(0x7507a4),
        RecordField::At006c => Some(0x750d54),
        RecordField::At20b0 => Some(0x754d9c),
        RecordField::At20b4 => Some(0x754e3c),
        RecordField::At20c8 => Some(0x754f8c),
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
        SlotField::Reference => Some(0x752a40),
        SlotField::Offset => Some(0x752af0),
        SlotField::Rate => Some(0x754040),
        SlotField::SecondLimit => Some(0x752ba0),
        SlotField::MeasuresAreas => Some(0x7541a0),
        SlotField::Distance => Some(0x754300),
        SlotField::Trim => Some(0x752c50),
        SlotField::ProfileSelector => Some(0x7535f0),
        SlotField::Residual => Some(0x752db0),
        SlotField::Step => Some(0x752e60),
        SlotField::At017a => Some(0x752d00),
        SlotField::At0184 => Some(0x7531d0),
        SlotField::At0186 => Some(0x753120),
        SlotField::At0188 => Some(0x753330),
        SlotField::At0198 => Some(0x753280),
        SlotField::At0208 => Some(0x754460),
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
        SlotField::MeasuredValue => Some(0x753bc8),
        SlotField::EvaluationWord => Some(0x753dd8),
        SlotField::EvaluationOffset => Some(0x753d28),
        SlotField::Trim => Some(0x752ca8),
        SlotField::Distance => Some(0x754358),
        SlotField::At0184 => Some(0x753228),
        SlotField::At0186 => Some(0x753178),
        SlotField::At01b8 => Some(0x753b18),
        SlotField::At0204 => Some(0x754408),
        SlotField::SecondaryFirst
        | SlotField::SecondarySecond
        | SlotField::MeasuresAreas
        | SlotField::ProfileSelector
        | SlotField::At017a
        | SlotField::At0188
        | SlotField::At0198
        | SlotField::At0208 => None,
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

/// Slot-entry fields, as 0xf2a7e8..0xf2a820, 0xf2a790, 0xf1dd14 and 0xf2d398..0xf2d3d0 read and
/// write them.
impl SlotEntry {
    pub fn reference(&self) -> u32 { word(&self.0, 0x4).unwrap_or(0) }
    pub fn offset(&self) -> i16 { entry_half(self, 0x8) }
    pub fn second_limit(&self) -> i16 { entry_half(self, 0xa) }
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
/// (0xf2f2e8..0xf2f304, the capture's 0xf2d4a4..0xf2d4c0, the finish's 0xf17d7c..0xf17d94).
pub const fn absolute_move_arguments(profile: MoveProfile) -> [u32; 4] {
    match profile {
        MoveProfile::LensReturn | MoveProfile::Capture => [1, 1, 0, 4],
        MoveProfile::Finish { speed } => [1, 1, 0, speed],
    }
}

/// The words after the callback, delta and speed of the relative move 0x76e1ac (0xf2f3d0..0xf2f3f0,
/// the capture's 0xf2d64c..0xf2d66c, the finish's 0xf1856c..0xf1858c).
pub const fn relative_move_arguments(profile: MoveProfile) -> [u32; 4] {
    match profile {
        MoveProfile::LensReturn | MoveProfile::Capture => [1, 0, 5, 4],
        MoveProfile::Finish { speed } => [1, 0, speed, 4],
    }
}

/// Words passed to the trace entry 0x72872c (the capture moves' at 0xf2d48c and 0xf2d630).
pub const fn trace_word(trace: CaptureTrace) -> u32 {
    match trace {
        CaptureTrace::ReturnAbsolute | CaptureTrace::CaptureAbsolute => 0x0303_0100,
        CaptureTrace::ReturnRelative | CaptureTrace::CaptureRelative => 0x0303_0000,
        CaptureTrace::PrepareJob => 0x0300_0000,
        CaptureTrace::Released => 0x1300_0000,
    }
}

/// Words of the statistics setup request 0x1039dd4 after the enable flag (0xf2917c..0xf29180).
pub const STATISTICS_SETUP_SECOND_WORD: u32 = 1;

/// The secondary job's source descriptor word after the frame (0xf2bf20 reads +0x54).
pub const SOURCE_FRAME_DETAIL: u32 = 0x54;

/// The record's area lists: counts 0x755dac / 0x755f0c, entries 0x755e54 / 0x755fb4 (record,
/// index; 104 bytes each), detail words 0x75610c / 0x7561ac; 0x75606c prefers the second list.
pub const fn area_list_entries(list: AreaList) -> [u32; 3] {
    match list {
        AreaList::First => [0x755dac, 0x755e54, 0x75610c],
        AreaList::Second => [0x755f0c, 0x755fb4, 0x7561ac],
    }
}
pub const PREFERS_SECOND_LIST: u32 = 0x75606c;
/// 16-byte stores into the first slot (record, 0, bytes): profile extent 0x758460, cells 0x758520.
pub const STORE_SLOT_EXTENT: u32 = 0x758460;
pub const STORE_SLOT_CELLS: u32 = 0x758520;

/// Configuration the secondary job reads: bytes through 0x69f8fc, `MeasuredDefault` 24 bits
/// through 0x69fef0, the other defaults halfwords through 0x69fec8.
pub const fn measurement_setting_id(setting: MeasurementSetting) -> u16 {
    match setting {
        MeasurementSetting::Measure => 0x12ff,
        MeasurementSetting::SecondListAlone => 0x1300,
        MeasurementSetting::SearchAll => 0x1301,
        MeasurementSetting::CarriedFirst => 0x1302,
        MeasurementSetting::CarriedSecond => 0x1303,
        MeasurementSetting::ShiftLimit => 0x1359,
        MeasurementSetting::RequiresArea => 0x135a,
        MeasurementSetting::MeasuredDefault => 0x1390,
        MeasurementSetting::TransferDefault => 0x1395,
        MeasurementSetting::PrimaryLimitDefault => 0x1373,
    }
}

/// Readers of the capture pipeline's words 0x16f0f5c + 0x94..0xa4.
pub const fn measurement_word_reader(word: MeasurementWord) -> u32 {
    match word {
        MeasurementWord::FirstList => 0xfedab8,
        MeasurementWord::SecondList => 0xfedad8,
        MeasurementWord::DumpFirst => 0xfeda78,
        MeasurementWord::DumpSecond => 0xfeda98,
        MeasurementWord::DumpThird => 0xfedaf8,
    }
}

/// 0xfeaba8 hands the source, the store and the processing to 0xffd04c, which fills both.
pub const PREPARE_STATISTICS: u32 = 0xfeaba8;
pub const fn statistics_store(store: StatisticsStore) -> u32 {
    match store {
        StatisticsStore::First => 0x5ffb_6120,
        StatisticsStore::Later => 0x5ffb_71d0,
    }
}
/// Imgdc 0x3f46 through 0xfeab6c (callbacks 0xf2ad34 for 0x5b, 0xf2ad08 for 0x5c), imgdc 0x3f47
/// through 0xfeabb8 (callback 0xf2ac70 for 0x5d), the release 0xfeabf0.
pub const MEASURE_AREAS: u32 = 0xfeab6c;
pub const TRANSFER: u32 = 0xfeabb8;
pub const RELEASE_MEASUREMENT: u32 = 0xfeabf0;

/// The lens reference table index the first-pass processing keeps at +0x7c (0xf2bf78).
pub fn processing_reference_index(processing: &Processing) -> i32 { word(&processing.0, 0x7c).unwrap_or(0) as i32 }

/// The capture pipeline's words the secondary job copies into the frame's dump (0xf2c188).
pub const MEASUREMENT_WORDS: [(FrameOffset, MeasurementWord); 3] = [
    (FrameOffset(0xa_3238), MeasurementWord::DumpFirst),
    (FrameOffset(0xa_323c), MeasurementWord::DumpSecond),
    (FrameOffset(0xa_3240), MeasurementWord::DumpThird),
];

/// Each area list's part of the frame's dump (0xf2bbcc..0xf2bc98): the count, a pair of words per
/// area, an accepted word per area, then four words per area.
const DUMP_LISTS: [u32; 2] = [0xa_30c4, 0xa_311c];
pub const DUMP_SELECTED: FrameOffset = FrameOffset(0xa_324c);

pub const fn dump_list_count(list: usize) -> FrameOffset { FrameOffset(DUMP_LISTS[list]) }

pub const fn dump_list_pair_word(list: usize, word: u8) -> FrameOffset { FrameOffset(DUMP_LISTS[list] + 4 + 4 * word as u32) }

pub const fn dump_list_accepted(list: usize, index: u8) -> FrameOffset { FrameOffset(DUMP_LISTS[list] + 0x1c + 4 * index as u32) }

pub const fn dump_list_area(list: usize, index: u8, word: usize) -> FrameOffset { FrameOffset(DUMP_LISTS[list] + 0x28 + 16 * index as u32 + 4 * word as u32) }

/// Calc-library entries of `MeasurementCalc`: `capture_selection` 0xf4c958 (record pointer, 12-byte
/// out), `statistics_key` 0xf4ca3c (processing, two-halfword out), `scale_step` 0xf4ce2c,
/// `capture_offset` 0xf51b94, `curve_within` 0xf4d2dc, `request_evaluation` 0xf4ca60 (callbacks
/// 0xf2acd0 for 0x94 and 0xf2ac98 for 0x95; the words record, processing, frame, frame detail; the
/// geometry block).
pub const MEASUREMENT_CALC: [u32; 6] = [0xf4c958, 0xf4ca3c, 0xf4ce2c, 0xf51b94, 0xf4d2dc, 0xf4ca60];
/// Profile-geometry entries of `MeasurementProfiles`: `profile_geometry` 0xeda668 (mode 2, size
/// halfwords 0x50, 0x50, 0x10), `crop_geometry` 0xef6030, `flagged_scale_pair` 0xef5b5c,
/// `profile_extent` 0xef5d44.
pub const MEASUREMENT_PROFILES: [u32; 4] = [0xeda668, 0xef6030, 0xef5b5c, 0xef5d44];
/// Lens-layer entries of `MeasurementLens` beyond the finish's: `reference_value` 0x747d48.
pub const REFERENCE_VALUE: u32 = 0x747d48;

/// The 32 bytes 0xef6030 writes: `origin` at +0 (0xef5d44's left and top), `frame` at +8,
/// `window` at +0x10 (0xef6150..0xef6170).
pub const CROP_GEOMETRY_BYTES: usize = 32;

/// Processing byte +0x8c, compared with the outputs the processing's profile needs (0xf2cbe8).
pub fn processing_outputs(processing: &Processing) -> u8 { processing.0[0x8c] }
/// 0xef6844 and 0xef685c answer 1 for every profile in 1.50.
pub const OUTPUTS_REQUIRED: u8 = 1;

/// Statuses of the client callback 0xefab18 (0xf2e34c, 0xf2d34c, 0xf2ddc0, 0xf2d89c).
pub const fn capture_status(status: CaptureStatus) -> u32 {
    match status {
        CaptureStatus::Progress => 9,
        CaptureStatus::Finished => 10,
        CaptureStatus::Accounted => 11,
        CaptureStatus::Displayed => 12,
    }
}

/// The primary job's part of the frame's dump (0xf2e360, 0xf2e410): cleared, marked at both
/// ends, the second delivered list's count and areas, and a 72-byte table.
pub const PRIMARY_DUMP: FrameOffset = FrameOffset(0xa_5ae0);
pub const PRIMARY_DUMP_BYTES: u32 = 0x4ec;
pub const PRIMARY_DUMP_END: FrameOffset = FrameOffset(0xa_5ae0 + 0x4e8);
pub const PRIMARY_DUMP_MARKS: (u32, u32) = (0x5354_3246, 0x4c41_3246);
pub const PRIMARY_DUMP_COUNT: FrameOffset = FrameOffset(0xa_5ae0 + 0x8);
/// The words 0xef5d44 and then the table overwrite: +0x10 and +0xc.
pub const PRIMARY_DUMP_EXTENT: [FrameOffset; 2] = [FrameOffset(0xa_5ae0 + 0x10), FrameOffset(0xa_5ae0 + 0xc)];

/// A dumped area's nine words: the area's words +0x10, +0x14, +0, +4, +0xc, +8, +0x1c, +0x18 and
/// halfword +0x20, at +0x20 + 0x6c * index.
pub const fn primary_dump_area(index: u8, word: usize) -> FrameOffset { FrameOffset(0xa_5ae0 + 0x20 + 0x6c * index as u32 + 4 * word as u32) }
pub const DUMPED_AREA_WORDS: [usize; 8] = [0x10, 0x14, 0x0, 0x4, 0xc, 0x8, 0x1c, 0x18];
pub const DUMPED_AREA_HALF: usize = 0x20;

/// Where each word of the 72-byte table 0xeed630 copies from 0x169b8f0 goes; words 14, 15 and
/// 17 of it are not dumped.
pub fn primary_dump_table(index: usize) -> Option<FrameOffset> {
    match index {
        0 => Some(FrameOffset(0xa_5ae0 + 0xc)),
        1 => Some(FrameOffset(0xa_5ae0 + 0x10)),
        2..=13 => Some(FrameOffset(0xa_5ae0 + 0x4b0 + 4 * (index as u32 - 2))),
        16 => Some(FrameOffset(0xa_5ae0 + 0x4e0)),
        17 => Some(FrameOffset(0xa_5ae0 + 0x4e4)),
        _ => None,
    }
}

/// Primary-job entries. Client: source words +0, +0xc, +0x14, +0x20, byte +0x3c, word +0x4c
/// (0xf2db54..0xf2dc30), the output at source +0x42/+0x44 and selection +8, +0xc, +0x10, +0x12,
/// +0x14, +0x1c, +0x20, +0x24, +0x40, +0x44 (0xf2d268..0xf2d328), the capture end at selection
/// +0x18, +0x1c, +0x24 (0xf2d6b0..0xf2d6d0), the selection byte +4 and readout word +0x18,
/// calibration block words +0 and +4, halfword +8, byte +0x16, window 16 bytes, selector word +4,
/// delivered lists of the area buffer at +0, +0x828 and +0x1050 (count, then 104-byte areas).
/// Records: calibration 0x756e24 (+6, six bytes), statistics option 0x756e90 (+4, byte +0xc),
/// slot positions 0x758940 (count byte, entries at +4 of position halfword and speed word),
/// setting result 0x758578, delivered lists 0x7558c8/0x755968, 0x755a08/0x755ab0 (read
/// 0x7559bc/0x755a5c), 0x755ca8/0x755d50. Pipeline: imgdc 0x3f49 0xfeac00 (callbacks 0xf2c9c4
/// 0x5e, 0xf2ca78 0x5f/0x60, 0xf2ca34 0x61, 0xf2ca04 0x64, 0xf2c990 0x62, 0xf2c918 the reply),
/// 0x3f4a 0xfeac54 (0xefb004 with command-0xc words +4, +8), 0x3f4f 0xfeac84 (0xefb04c), 0x3f4e
/// 0xfead28, readout processing 0xfeacd4, template 0xfead5c, statistics ready 0xfeacfc; the
/// statistics request 0x1039b48 with 0xf2cad8 for 0x7d. Calibration: 0xef4518 (mode 2),
/// 0xef4b1c, 0xef4d18, 0xef4bf4; default calibration 0xf4d4cc; output scale 0x749be4, divided
/// into 0xffffff. Geometry: area selector 0xef617c, display layout 0xef6324, paired 0xfead0c,
/// sensor window 0xeed9c4, generated areas 0xeed3f0 (mode 2 or 6), frame areas 0xeee0f4,
/// display extent 0xedb0bc, table 0xeed630.
pub const PRIMARY_JOB_ENTRIES: [u32; 12] = [0xfeac00, 0xfeac54, 0xfeac84, 0xfead28, 0xfeacd4, 0xfead5c, 0xfeacfc, 0x1039b48, 0xf4d4cc, 0x749be4, 0xef617c, 0xef6324];
pub const AREA_BUFFER_LISTS: [u32; 3] = [0, 0x828, 0x1050];
pub const fn area_mode(mode: AreaMode) -> u32 {
    match mode {
        AreaMode::Second => 2,
        AreaMode::Third => 6,
    }
}
