#![forbid(unsafe_code)]

//! X-M1 1.50 boundary values of the shot module.

pub mod calculation;
pub mod finish;

use crate::boundary::{ClientRef, FocusConfirmation, FocusHistory, FocusResult, LensPoint, LensReference, LensSnapshot};
use crate::ports::{AbortStatus, HalfPressMode, HalfPressNotifications};
use crate::preparation::{FrameHeader, HalfPressMask, HalfPressParameters};
use crate::subjects::{Subject, SubjectId, SubjectList, CAPACITY, DETAIL_BYTES};
use crate::summary::{AreaEntry, AreaStatistics, FrameSummary, CELLS, ENTRIES, FIELDS, GRID, REGIONS};
use crate::Outcome;
use xm1_saclac_controllers::children::ShotControl;
use xm1_saclac_interface::CompletionStatus;

pub mod exposure;
pub mod statistics_dump;

/// Codes retained parents pass to the row functions 0xefdab8 (0xeb, 0xec), 0xeffcfc (0x103)
/// and 0xf005b4 (0xac).
pub const fn control_code(control: ShotControl) -> u16 {
    match control {
        ShotControl::StartS1 => 0xeb,
        ShotControl::AdvanceS1 => 0xec,
        ShotControl::InitializeCapture => 0x103,
        ShotControl::StartLiveSequence => 0xac,
    }
}

pub const fn control(code: u16) -> Option<ShotControl> {
    match code {
        0xeb => Some(ShotControl::StartS1),
        0xec => Some(ShotControl::AdvanceS1),
        0x103 => Some(ShotControl::InitializeCapture),
        0xac => Some(ShotControl::StartLiveSequence),
        _ => None,
    }
}

/// Retained parents test 1 and 2; 21 is the finish's relative-done word (0xf043d4, 0xf043f8),
/// which only the half-press pipeline distinguishes (0xf21de4).
pub const fn outcome_word(outcome: Outcome) -> i32 {
    match outcome {
        Outcome::Complete => 1,
        Outcome::Pending => 2,
        Outcome::Unhandled => 3,
        Outcome::Progress => 21,
        Outcome::Fault | Outcome::Aborted => 0,
    }
}

fn word(block: &[u8], offset: usize) -> Option<u32> {
    let bytes: [u8; 4] = block.get(offset..offset + 4)?.try_into().ok()?;
    Some(u32::from_le_bytes(bytes))
}

fn half(block: &[u8], offset: usize) -> u16 { u16::from_le_bytes([block[offset], block[offset + 1]]) }

fn put_word(block: &mut [u8], offset: usize, value: u32) { block[offset..offset + 4].copy_from_slice(&value.to_le_bytes()); }

fn client(word: u32) -> Option<ClientRef> { (word != 0).then_some(ClientRef(word)) }

pub const HALF_PRESS_BLOCK_BYTES: usize = 64;

/// The command-6 block (0x169d05c), as 0xf16ffc, 0xf15708, 0xf17a4c, 0xf1ee48 and 0xf1f2b0 read it.
pub fn half_press_parameters(block: &[u8]) -> Option<HalfPressParameters> {
    let block = block.get(..HALF_PRESS_BLOCK_BYTES)?;
    let at = |offset| word(block, offset).unwrap_or(0);
    Some(HalfPressParameters {
        mode: HalfPressMode(at(0x4)),
        option: at(0xc),
        mask: HalfPressMask(at(0x10)),
        area_count: client(at(0x18)),
        area_selection: at(0x1c),
        areas: client(at(0x20)),
        lens_target: client(at(0x24)),
        frame_slot: client(at(0x2c)),
        lens_reference: LensReference(at(0x30)),
    })
}

/// Word +0x14 of the command-2 block (0x169d004), read at 0xf17064 for the live branch.
pub fn live_lens_reference(block: &[u8]) -> Option<LensReference> { word(block, 0x14).map(LensReference) }

pub fn half_press_mode(block: &[u8]) -> Option<HalfPressMode> { half_press_parameters(block).map(|parameters| parameters.mode) }

pub fn half_press_notifications(block: &[u8]) -> Option<HalfPressNotifications> {
    half_press_parameters(block).map(|parameters| parameters.mask.notifications())
}

/// Word +0x10 of 0x169d370 (0xf2e65c); 13 and 14 abort a capture (0xf0c420..0xf0c43c).
pub const fn abort_status(word: u32) -> Option<AbortStatus> {
    match word {
        13 | 14 => Some(AbortStatus(CompletionStatus(word as i32))),
        _ => None,
    }
}

fn lens_point(bytes: &[u8]) -> LensPoint {
    LensPoint {
        position: half(bytes, 0) as i16,
        detail: half(bytes, 2),
        reference: LensReference(word(bytes, 4).unwrap_or(0)),
        state: word(bytes, 8).unwrap_or(0),
        kind: word(bytes, 12).unwrap_or(0),
    }
}

fn put_lens_point(bytes: &mut [u8], point: LensPoint) {
    bytes[0..2].copy_from_slice(&point.position.to_le_bytes());
    bytes[2..4].copy_from_slice(&point.detail.to_le_bytes());
    put_word(bytes, 4, point.reference.0);
    put_word(bytes, 8, point.state);
    put_word(bytes, 12, point.kind);
}

/// Record slot 17 (0x756840, 0x756878), 48 bytes.
pub fn lens_snapshot(bytes: &[u8; 48]) -> LensSnapshot {
    let mut tail = [0; 12];
    tail.copy_from_slice(&bytes[36..48]);
    LensSnapshot { current: lens_point(&bytes[0..16]), held: lens_point(&bytes[16..32]), marker: word(bytes, 32).unwrap_or(0), tail }
}

pub fn lens_snapshot_bytes(snapshot: &LensSnapshot) -> [u8; 48] {
    let mut bytes = [0; 48];
    put_lens_point(&mut bytes[0..16], snapshot.current);
    put_lens_point(&mut bytes[16..32], snapshot.held);
    put_word(&mut bytes, 32, snapshot.marker);
    bytes[36..48].copy_from_slice(&snapshot.tail);
    bytes
}

/// The 28-byte lens designation at 0x91988c (0x758c68): a first word of 1 designates the point at +12.
pub fn designated_point(bytes: &[u8; 28]) -> Option<LensPoint> { (word(bytes, 0) == Some(1)).then(|| lens_point(&bytes[12..28])) }

/// Record slot 14 (0x756734, 0x756754), 132 bytes; fields read and cleared at 0xf172c0..0xf17300,
/// 0xf174d8..0xf174e0 and 0xf1752c..0xf17568.
pub fn focus_result(bytes: &[u8; 132]) -> FocusResult {
    let at = |offset| word(bytes, offset).unwrap_or(0);
    FocusResult { reference: LensReference(at(0)), valid: at(4), kept: at(0x60), refined: at(0x68), extent: at(0x80), bytes: *bytes }
}

pub fn focus_result_bytes(result: &FocusResult) -> [u8; 132] {
    let mut bytes = result.bytes;
    put_word(&mut bytes, 0, result.reference.0);
    put_word(&mut bytes, 4, result.valid);
    put_word(&mut bytes, 0x60, result.kept);
    put_word(&mut bytes, 0x68, result.refined);
    put_word(&mut bytes, 0x80, result.extent);
    bytes
}

/// Record slot 15 (0x756788, 0x7567b8), 32 bytes.
pub fn focus_confirmation(bytes: &[u8; 32]) -> FocusConfirmation {
    FocusConfirmation { active: word(bytes, 0).unwrap_or(0), reference: LensReference(word(bytes, 12).unwrap_or(0)), bytes: *bytes }
}

pub fn focus_confirmation_bytes(confirmation: &FocusConfirmation) -> [u8; 32] {
    let mut bytes = confirmation.bytes;
    put_word(&mut bytes, 0, confirmation.active);
    put_word(&mut bytes, 12, confirmation.reference.0);
    bytes
}

/// Record slot 25 (0x756f3c, 0x756f60), 80 bytes.
pub fn focus_history(bytes: &[u8; 80]) -> FocusHistory { FocusHistory { valid: word(bytes, 0).unwrap_or(0), bytes: *bytes } }

pub fn focus_history_bytes(history: &FocusHistory) -> [u8; 80] {
    let mut bytes = history.bytes;
    put_word(&mut bytes, 0, history.valid);
    bytes
}

pub const SUBJECT_BYTES: usize = 104;
const SUBJECT_ID: usize = 0x5c;
pub const SUBJECT_LIST_BYTES: usize = 0x414;

/// A subject entry; its identity is the word at +0x5c (0xf15a60, 0xf15af8, 0xf15c64).
pub fn subject(bytes: &[u8; SUBJECT_BYTES]) -> Subject {
    let mut detail = [0; DETAIL_BYTES];
    detail[..SUBJECT_ID].copy_from_slice(&bytes[..SUBJECT_ID]);
    detail[SUBJECT_ID..].copy_from_slice(&bytes[SUBJECT_ID + 4..]);
    Subject { id: SubjectId(word(bytes, SUBJECT_ID).unwrap_or(0)), detail }
}

pub fn subject_bytes(subject: &Subject) -> [u8; SUBJECT_BYTES] {
    let mut bytes = [0; SUBJECT_BYTES];
    bytes[..SUBJECT_ID].copy_from_slice(&subject.detail[..SUBJECT_ID]);
    put_word(&mut bytes, SUBJECT_ID, subject.id.0);
    bytes[SUBJECT_ID + 4..].copy_from_slice(&subject.detail[SUBJECT_ID..]);
    bytes
}

/// A candidate list of the shot context (0x169e580, +0x414, +0x828, +0xc3c, +0x1878): a count
/// word and ten entries.
pub fn subject_list(bytes: &[u8; SUBJECT_LIST_BYTES]) -> SubjectList {
    let mut list = SubjectList { count: word(bytes, 0).unwrap_or(0), ..SubjectList::default() };
    for (index, entry) in list.entries.iter_mut().enumerate() {
        let start = 4 + index * SUBJECT_BYTES;
        let mut raw = [0; SUBJECT_BYTES];
        raw.copy_from_slice(&bytes[start..start + SUBJECT_BYTES]);
        *entry = subject(&raw);
    }
    list
}

pub fn subject_list_bytes(list: &SubjectList) -> [u8; SUBJECT_LIST_BYTES] {
    let mut bytes = [0; SUBJECT_LIST_BYTES];
    put_word(&mut bytes, 0, list.count);
    for (index, entry) in list.entries.iter().enumerate().take(CAPACITY) {
        let start = 4 + index * SUBJECT_BYTES;
        bytes[start..start + SUBJECT_BYTES].copy_from_slice(&subject_bytes(entry));
    }
    bytes
}

pub const AREA_ENTRY_BYTES: usize = 552;
/// Entry words copied into the summary, in summary order (0xf16ba4..0xf16c10).
const AREA_FIELDS: [(usize, FieldKind); FIELDS] = [
    (0x224, FieldKind::Word), (0x204, FieldKind::Half), (0x206, FieldKind::Half), (0x208, FieldKind::SignedHalf), (0x20a, FieldKind::SignedHalf),
    (0x210, FieldKind::Word), (0x20c, FieldKind::Word), (0x218, FieldKind::Word), (0x214, FieldKind::Word), (0x220, FieldKind::Word),
    (0x21c, FieldKind::Word),
];

#[derive(Clone, Copy)]
enum FieldKind { Word, Half, SignedHalf }

/// The 12-byte statistics descriptor of 0x7567ec (0x9190f0) and the five 552-byte entries its third
/// word names.
pub fn area_statistics(descriptor: &[u8; 12], entries: &[u8; ENTRIES * AREA_ENTRY_BYTES]) -> AreaStatistics {
    let mut statistics = AreaStatistics { present: word(descriptor, 0).unwrap_or(0), layout: descriptor[4], ..AreaStatistics::default() };
    for (index, entry) in statistics.entries.iter_mut().enumerate() {
        let bytes = &entries[index * AREA_ENTRY_BYTES..(index + 1) * AREA_ENTRY_BYTES];
        let at = |offset| word(bytes, offset).unwrap_or(0);
        let mut parsed = AreaEntry { id: at(0), ..AreaEntry::default() };
        for row in 0..GRID {
            for column in 0..GRID {
                let cell = row * GRID + column;
                parsed.values[row][column] = at(4 + cell * 4);
                parsed.weights[row][column] = at(0x104 + cell * 4);
            }
        }
        for (field, (offset, kind)) in parsed.fields.iter_mut().zip(AREA_FIELDS) {
            *field = match kind {
                FieldKind::Word => at(offset),
                FieldKind::Half => u32::from(half(bytes, offset)),
                FieldKind::SignedHalf => half(bytes, offset) as i16 as i32 as u32,
            };
        }
        *entry = parsed;
    }
    statistics
}

/// The summary is written from frame +0x6c3c0 (0xf16b0c); `summary_words` offsets are relative to it.
pub const FRAME_SUMMARY: u32 = 0x6c3c0;
const SUMMARY_TAG: u32 = 0x5354_5253;
const SUMMARY_END_TAG: u32 = 0x4c41_5253;

/// Every word 0xf16ae0 stores for `summary`, as (offset from `FRAME_SUMMARY`, value), in no order.
pub fn summary_words(summary: &FrameSummary, mut store: impl FnMut(u32, u32)) {
    store(0, SUMMARY_TAG);
    store(0x99c, SUMMARY_END_TAG);
    store(4, summary.present);
    let Some(detail) = summary.detail else { return };
    store(8, u32::from(detail.layout));
    for (index, (id, entry)) in detail.ids.iter().zip(detail.entries.iter()).enumerate() {
        let index = index as u32;
        store(0xc + 4 * index, *id);
        let Some(entry) = entry else { continue };
        for (field, value) in entry.fields.iter().enumerate() {
            store(0x890 + 4 * index + 0x14 * field as u32, *value);
        }
        for cell in 0..CELLS {
            let slot = (5 * cell as u32 + index) * 4;
            store(0x20 + slot, entry.values[cell]);
            store(0x3f4 + slot, entry.weights[cell]);
        }
        for region in 0..REGIONS {
            let slot = 0x7c8 + 4 * index + 0x14 * region as u32;
            store(slot, entry.means[region].value);
            store(slot + 0x64, entry.means[region].weight);
        }
    }
    for offset in (0x96c..0x99c).step_by(4) { store(offset, 0); }
}

/// Frame words of the header 0xf176ac..0xf176cc writes; the calculation task reads the header
/// at +0x35c8 through word +0x14 of 0x16cd228.
pub const FRAME_HEADER: u32 = 0x35c8;

pub fn frame_header_words(header: FrameHeader) -> [(u32, u32); 4] {
    [(0x35cc, header.mode.0), (0x35d0, header.mask.0), (0x35d4, header.option), (0x35d8, header.lens_reference.0)]
}

/// Frame word cleared by a half press outside mode 6 (0xf175a8).
pub const FRAME_PROGRESS: u32 = 0x63314;

/// Client lens-target words 0xf171e4..0xf17274 store: the point at +0xc and +0x1c, the step over
/// the halfword at +0x1c, the distance at +4.
pub fn lens_target_words(point: LensPoint, step: u16, distance: u32, mut store: impl FnMut(u32, &[u8])) {
    let mut bytes = [0; 16];
    put_lens_point(&mut bytes, point);
    store(0xc, &bytes);
    store(0x1c, &bytes);
    store(0x1c, &step.to_le_bytes());
    store(4, &distance.to_le_bytes());
}
