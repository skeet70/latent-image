//! X-M1 1.50 (id5) layouts of SR-AUTO scene recognition: the scenes its history codes name, the
//! work-memory log the live vote keeps, the capture dump section 0xf525ec writes, and its state as
//! the capture dump 0xf29318 reads it through the library's getters.

use xm1_saclac_interface::calc::SharedAddress;

use crate::adjust::configuration::{History, HISTORY};
use crate::exposure::ports::Property;
use crate::scene_recognition::history::{Code, ComparisonRecord, FrameNote, IndexRecord, KeyRecord};
use crate::scene_recognition::{CaptureTrace, Evaluation, Features, Grid, LensDetectors, Recognition, Scene, SceneRecognition, Trace};

/// The scenes a history code names, by code modulo 29 (0xf53404).
pub const CODED_SCENES: [u8; 29] = [0, 1, 4, 5, 8, 9, 11, 12, 14, 15, 18, 19, 20, 21, 22, 23, 25, 26, 27, 34, 35, 36, 37, 38, 39, 40, 41, 42, 48];

/// The byte of an empty history entry (0xf4dbcc, 0xf4d890).
pub const EMPTY_CODE: u8 = 100;

/// The live vote's work-memory log while property 0xa8 bit 7 is set: 60-byte entries, the next
/// index wrapping past 100 (0xf50dc4..0xf50ea4).
pub const LOG: SharedAddress = SharedAddress(0x5ffe_7854);
pub const LOG_ENTRIES: u16 = 101;
pub const LOG_ENTRY_BYTES: u32 = 60;
pub const LOG_BYTES: u32 = LOG_ENTRIES as u32 * LOG_ENTRY_BYTES;

pub const fn log_entry(index: u16) -> SharedAddress { SharedAddress(LOG.0 + LOG_ENTRY_BYTES * index as u32) }

/// Frame offsets of the capture dump's scene-recognition section (0xf525ec).
pub const DUMP_SECTION: u32 = 0x1e00;
pub const DUMP_MARK: u32 = 0x5354_534d;
pub const DUMP_SETTINGS_AT: u32 = DUMP_SECTION + 4;
pub const DUMP_LOG: u32 = DUMP_SECTION + 0x18;
pub const DUMP_END: u32 = DUMP_SECTION + 0x17c4;
pub const DUMP_END_MARK: u32 = 0x4c41_534d;

/// The settings the section carries in tenths, in order (0xf52608..0xf5265c).
pub const DUMP_SETTINGS: [Property; 5] = [Property(0x75ff), Property(0x7600), Property(0x7601), Property(0x7602), Property(0x7603)];

/// Names the scene group the rate filter tests whatever the capture pipeline asked; 255 names
/// none (0xf4e9c8).
pub const FORCED_GROUP: Property = Property(0x4a14);

/// The group property 0x4a14 forces: one bit per group (0xf4e9e8 table).
pub const fn forced_group(value: u32) -> Option<Scene> {
    match value {
        1 => Some(Scene(18)),
        2 => Some(Scene(21)),
        4 => Some(Scene(25)),
        8 => Some(Scene(34)),
        16 => Some(Scene(37)),
        32 => Some(Scene(40)),
        _ => None,
    }
}

/// The state the capture dump reads, as the library's getters answer it: the saved evaluation
/// (0xf4dfe8), the saved capture features (0xf4e004), the live and capture traces (0xf4e020,
/// 0xf4e03c), the code lists (0xf4d774, 0xf4d8a0, 0xf4dbfc), the reference history (0xf4d94c),
/// the stationary history (0xf4dc34) and the comparison history (0xf4de88).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Workspace {
    SavedEvaluation,
    SavedFeatures,
    LiveTrace,
    CaptureTrace,
    PrimaryCodes,
    SecondaryCodes,
    TertiaryCodes,
    References,
    Stationary,
    Comparisons,
}

/// How a workspace field widens to the dump's word.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Width {
    Byte,
    Half,
    SignedHalf,
    Word,
}

fn put(bytes: &mut [u8], at: usize, value: &[u8]) { bytes[at..at + value.len()].copy_from_slice(value) }

fn put_word(bytes: &mut [u8], at: usize, value: u32) { put(bytes, at, &value.to_le_bytes()) }

fn put_half(bytes: &mut [u8], at: usize, value: u16) { put(bytes, at, &value.to_le_bytes()) }

fn put_grid(bytes: &mut [u8], at: usize, grid: &Grid) {
    for (index, cell) in grid.iter().enumerate() {
        put_half(bytes, at + 2 * index, *cell as u16);
    }
}

fn code_byte(code: Option<Code>) -> u8 { code.map_or(EMPTY_CODE, Code::byte) }

const KEY_RECORD_BYTES: usize = 0x214;
const INDEX_RECORD_BYTES: usize = 0x24;
const COMPARISON_RECORD_BYTES: usize = 0x450;
const FEATURES_BYTES: usize = 0x80;
const EVALUATION_BYTES: usize = 0x974;
const LIVE_TRACE_BYTES: usize = 0x5c;
const CAPTURE_TRACE_BYTES: usize = 0x40;

/// The note's grid, position and words; the snapshot's words follow a two-byte pad.
fn note(bytes: &mut [u8], at: usize, note: &FrameNote, words_at: usize) {
    put_grid(bytes, at, &note.grid);
    put_half(bytes, at + 0x200, note.position as u16);
    for (index, word) in [u32::from(note.subject), note.reference, note.motion, u32::from(note.stationary)].into_iter().enumerate() {
        put_word(bytes, at + words_at + 4 * index, word);
    }
}

fn key_record(bytes: &mut [u8], at: usize, record: Option<&KeyRecord>) {
    match record {
        Some(record) => {
            bytes[at] = record.code.byte();
            note(bytes, at + 2, &record.note, 0x202);
        }
        None => bytes[at] = EMPTY_CODE,
    }
}

fn index_record(bytes: &mut [u8], at: usize, record: &IndexRecord) {
    put_word(bytes, at, u32::from(record.stationary));
    put_word(bytes, at + 4, u32::from(record.steady));
    bytes[at + 8] = record.count;
}

fn comparison_record(bytes: &mut [u8], at: usize, record: &ComparisonRecord) {
    key_record(bytes, at, Some(&record.reference));
    put_word(bytes, at + 0x214, record.source.map_or(0, |source| source.0));
    put_grid(bytes, at + 0x218, &record.compared);
    put_half(bytes, at + 0x418, record.mean);
    put_half(bytes, at + 0x41c, record.position as u16);
    put_half(bytes, at + 0x41e, record.previous_position as u16);
    put_half(bytes, at + 0x420, record.position_change);
    put_word(bytes, at + 0x424, u32::from(record.subject_present));
    put_word(bytes, at + 0x428, record.motion);
    index_record(bytes, at + 0x42c, &record.stationary);
}

fn recognition(bytes: &mut [u8], at: usize, recognition: Recognition) {
    put_word(bytes, at, recognition.scene.0);
    put_half(bytes, at + 4, recognition.flags.0);
}

fn features(bytes: &mut [u8], features: &Features) {
    recognition(bytes, 0x00, features.selection);
    put(bytes, 0x06, &features.tried);
    put_word(bytes, 0x0c, features.at_005d);
    put_word(bytes, 0x10, features.subjects);
    let levels = [features.level, features.primary_level, features.alternate_level, features.third_level, features.upper_level, features.lower_level];
    for (index, level) in levels.into_iter().enumerate() {
        put_half(bytes, 0x14 + 2 * index, level as u16);
    }
    put_word(bytes, 0x20, features.difference_answer);
    put_half(bytes, 0x24, features.at_38 as u16);
    put_word(bytes, 0x28, features.at_3c);
    put_word(bytes, 0x2c, features.status);
    for (index, half) in [features.position, features.lens_step, features.distance, features.response].into_iter().enumerate() {
        put_half(bytes, 0x30 + 2 * index, half as u16);
    }
    put_word(bytes, 0x38, features.at_4c);
    put_word(bytes, 0x3c, features.reference);
    put_word(bytes, 0x40, features.state);
    put_word(bytes, 0x44, features.selected);
    for (index, bound) in features.bounds.into_iter().enumerate() {
        put_half(bytes, 0x48 + 2 * index, bound as u16);
    }
    put(bytes, 0x50, &features.motion.encode());
    put_word(bytes, 0x78, u32::from(features.stationary));
    put_word(bytes, 0x7c, u32::from(features.index_recorded));
}

/// The evaluation as stock keeps it at state +0x14 and its saved copy at +0xfa8.
fn evaluation(bytes: &mut [u8], evaluation: &Evaluation) {
    features(bytes, &evaluation.features);
    let choices = &evaluation.choices;
    recognition(bytes, 0x80, choices.history);
    recognition(bytes, 0x88, choices.majority);
    put_word(bytes, 0x90, u32::from(choices.holding));
    put_word(bytes, 0x94, u32::from(choices.recording));
    bytes[0x98] = choices.recorded;
    bytes[0x9c] = choices.held;
    put_word(bytes, 0xa0, u32::from(choices.released));
    put_word(bytes, 0xa4, u32::from(choices.forced));
    let tally = &choices.tally;
    put(bytes, 0xa8, &tally.detectors);
    put(bytes, 0xae, &tally.weights);
    for (index, answer) in tally.answers.into_iter().enumerate() {
        put_word(bytes, 0xb4 + 4 * index, answer);
    }
    bytes[0xcc] = tally.weight;
    put_word(bytes, 0xd0, tally.decision);
    comparison_record(bytes, 0xd4, &evaluation.against_reference);
    note(bytes, 0x524, &evaluation.snapshot, 0x204);
    put_grid(bytes, 0x73c, &evaluation.against_snapshot.compared);
    put_half(bytes, 0x93c, evaluation.against_snapshot.mean);
    put_half(bytes, 0x942, evaluation.against_snapshot.previous_position as u16);
    put_half(bytes, 0x944, evaluation.against_snapshot.position_change);
}

fn lens_detectors(bytes: &mut [u8], at: usize, detectors: &LensDetectors) {
    let words = [u32::from(detectors.matched), detectors.match_direction, u32::from(detectors.lens_moved), detectors.lens_direction, u32::from(detectors.lens_changed)];
    for (index, word) in words.into_iter().enumerate() {
        put_word(bytes, at + 4 * index, word);
    }
}

fn codes(bytes: &mut [u8], at: usize, codes: &History<Code>) {
    for (index, code) in codes.entries().iter().enumerate() {
        bytes[at + index] = code_byte(*code);
    }
}

fn live_trace(bytes: &mut [u8], trace: &Trace) {
    for (index, word) in [trace.query.0, trace.option, trace.candidate.0, trace.recorded].into_iter().enumerate() {
        put_word(bytes, 4 * index, word);
    }
    codes(bytes, 0x10, &trace.hints);
    for (index, word) in [trace.filtered.0, trace.earlier_candidate.0, trace.earlier_filtered.0, trace.selection.0].into_iter().enumerate() {
        put_word(bytes, 0x24 + 4 * index, word);
    }
    put_half(bytes, 0x34, trace.flags.0);
    put_word(bytes, 0x38, u32::from(trace.filter.enabled));
    put_word(bytes, 0x3c, u32::from(trace.filter.passed));
    lens_detectors(bytes, 0x40, &trace.detectors);
    recognition(bytes, 0x54, trace.promoted);
}

fn capture_trace(bytes: &mut [u8], trace: &CaptureTrace) {
    for (index, word) in [trace.query.0, trace.option, trace.candidate.0, trace.recorded].into_iter().enumerate() {
        put_word(bytes, 4 * index, word);
    }
    lens_detectors(bytes, 0x10, &trace.detectors);
    for (index, word) in [trace.group.0, trace.hint.0, trace.filtered.0].into_iter().enumerate() {
        put_word(bytes, 0x24 + 4 * index, word);
    }
    recognition(bytes, 0x30, trace.selection);
    recognition(bytes, 0x38, trace.promoted);
}

fn widen(bytes: &[u8], at: usize, width: Width) -> u32 {
    let byte = |offset: usize| bytes.get(at + offset).copied().unwrap_or(0);
    let half = u16::from_le_bytes([byte(0), byte(1)]);
    match width {
        Width::Byte => u32::from(byte(0)),
        Width::Half => u32::from(half),
        Width::SignedHalf => half as i16 as u32,
        Width::Word => u32::from_le_bytes([byte(0), byte(1), byte(2), byte(3)]),
    }
}

/// The field at `offset` of `workspace` as the capture dump reads it.
pub fn workspace_field(state: &SceneRecognition, workspace: Workspace, offset: u32, width: Width) -> u32 {
    let offset = offset as usize;
    let field = |bytes: &[u8], at: usize| widen(bytes, at, width);
    match workspace {
        Workspace::SavedEvaluation => {
            let mut bytes = [0; EVALUATION_BYTES];
            evaluation(&mut bytes, state.saved_evaluation());
            field(&bytes, offset)
        }
        Workspace::SavedFeatures => {
            let mut bytes = [0; FEATURES_BYTES + 8];
            let (saved, chosen) = state.saved_features();
            features(&mut bytes, saved);
            recognition(&mut bytes, FEATURES_BYTES, chosen);
            field(&bytes, offset)
        }
        Workspace::LiveTrace => {
            let mut bytes = [0; LIVE_TRACE_BYTES];
            live_trace(&mut bytes, state.live_trace());
            field(&bytes, offset)
        }
        Workspace::CaptureTrace => {
            let mut bytes = [0; CAPTURE_TRACE_BYTES];
            capture_trace(&mut bytes, state.capture_trace());
            field(&bytes, offset)
        }
        Workspace::PrimaryCodes | Workspace::SecondaryCodes | Workspace::TertiaryCodes => {
            let list = match workspace {
                Workspace::PrimaryCodes => state.primary_codes(),
                Workspace::SecondaryCodes => state.secondary_codes(),
                _ => state.tertiary_codes(),
            };
            let mut bytes = [0; HISTORY];
            codes(&mut bytes, 0, list);
            field(&bytes, offset)
        }
        Workspace::References => {
            let Some(entry) = state.references().entries().get(offset / KEY_RECORD_BYTES) else { return 0 };
            let mut bytes = [0; KEY_RECORD_BYTES];
            key_record(&mut bytes, 0, entry.as_ref());
            field(&bytes, offset % KEY_RECORD_BYTES)
        }
        Workspace::Stationary => {
            let Some(entry) = state.stationary_history().get(offset / INDEX_RECORD_BYTES) else { return 0 };
            let mut bytes = [0; INDEX_RECORD_BYTES];
            index_record(&mut bytes, 0, entry);
            field(&bytes, offset % INDEX_RECORD_BYTES)
        }
        Workspace::Comparisons => {
            let Some(entry) = state.comparisons().get(offset / COMPARISON_RECORD_BYTES) else { return 0 };
            let mut bytes = [0; COMPARISON_RECORD_BYTES];
            comparison_record(&mut bytes, 0, entry);
            field(&bytes, offset % COMPARISON_RECORD_BYTES)
        }
    }
}
