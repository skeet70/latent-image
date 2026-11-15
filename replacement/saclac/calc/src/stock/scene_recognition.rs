//! X-M1 1.50 (id5) layouts of SR-AUTO scene recognition: the scenes its history codes name, the
//! work-memory log the live vote keeps, the capture dump section 0xf525ec writes, and its state as
//! the capture dump 0xf29318 reads it through the library's getters.

use xm1_saclac_interface::calc::SharedAddress;

use crate::adjust::configuration::HISTORY;
use crate::exposure::ports::Property;
use crate::scene_recognition::history::{Code, ComparisonRecord, FrameNote, IndexRecord, KeyRecord};
use crate::scene_recognition::detectors::{CaptureFrame, FrameRecord, LiveFrame};
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

/// The frame record the frame query and the change detectors read (shared field group 33 at
/// +0x7a0, 0x7588e8): the capture measurement's words, then live view's.
pub fn frame_record(bytes: &[u8; 76]) -> FrameRecord {
    let half = |at: usize| i16::from_le_bytes([bytes[at], bytes[at + 1]]);
    let word = |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    FrameRecord {
        capture: CaptureFrame {
            level: half(0x00),
            primary_level: half(0x02),
            alternate_level: half(0x04),
            status: word(0x0c),
            position: half(0x10),
            reference: word(0x14),
            state: word(0x18),
            selected: word(0x20),
            difference: word(0x24),
        },
        live: LiveFrame {
            upper_level: half(0x28),
            lower_level: half(0x2a),
            primary_level: half(0x2c),
            alternate_level: half(0x2e),
            third_level: half(0x30),
            reference: word(0x38),
            state: word(0x3c),
            at_4c: word(0x40),
            at_3c: word(0x44),
            position: half(0x48),
            at_38: half(0x4a),
        },
    }
}

/// Configuration the detectors read (0x69f8fc unless noted).
pub const DIFFERENCE_TEST: Property = Property(0x75fa);
pub const BELOW_TEST: Property = Property(0x75fb);
/// Nonzero: the portrait detector tests the level before the level difference.
pub const BELOW_FIRST: Property = Property(0x75f9);
/// Each pair: the value for a record without the record measurement selection, then with it.
pub const DIFFERENCE_LIMIT: [Property; 2] = [Property(0x75fc), Property(0x75ff)];
pub const BELOW_LIMIT: [Property; 2] = [Property(0x75fd), Property(0x7600)];
/// Signed, in tenths (0x69ffe8).
pub const LANDSCAPE_LEVEL: [Property; 2] = [Property(0x75f0), Property(0x7601)];
pub const NIGHT_LEVEL: [Property; 2] = [Property(0x75f2), Property(0x7602)];
pub const CLOSEUP_LEVEL: [Property; 2] = [Property(0x75f7), Property(0x7603)];
/// Nonzero: the night detector checks the capture status and position, and gates its answer.
pub const NIGHT_GATED: Property = Property(0x75f5);
pub const NIGHT_SELECTED: Property = Property(0x75f3);
pub const NIGHT_LIVE: Property = Property(0x75f4);
/// Two bytes: the count below which the live gate reads `GATE_BELOW`, else `GATE_ABOVE`.
pub const GATE_SWITCH: Property = Property(0x75e9);
pub const GATE_BELOW: Property = Property(0x75ec);
pub const GATE_ABOVE: Property = Property(0x75eb);
pub const GRID_WEIGHTING: Property = Property(0x75c3);
pub const MATCH_THRESHOLD: Property = Property(0x75c4);
pub const SNAPSHOT_GRID_THRESHOLD: Property = Property(0x7626);
pub const REFERENCE_THRESHOLD: Property = Property(0x7618);
pub const SNAPSHOT_REFERENCE_THRESHOLD: Property = Property(0x7677);
pub const STATIONARY_LIMIT: Property = Property(0x76de);
/// Nonzero: the stationary decision writes its overlay line.
pub const OVERLAY: Property = Property(0xeb);
/// The overlay line's text before the steadiness digits (0xf52c84).
pub const STATIONARY_TEXT: &[u8] = b"GETSTATIC:";
pub const TRIPOD: Property = Property(0x76dd);
pub const SHAKE_REDUCTION: Property = Property(0x78d0);
/// Read only for a record without the record measurement selection (0xf4acd4, 0xf4ad54, 0xf4a554).
pub const KEEP_STATIONARY: Property = Property(0x76dc);
pub const SHAKE_CHECK: Property = Property(0x76da);
pub const PROMOTION: Property = Property(0x7684);
/// The landscape promotion's switch: live view's, then the shot's (0xf4a520).
pub const PROMOTION_SWITCH: [Property; 2] = [Property(0x7683), Property(0x76b0)];

/// Two bytes: the snapshot position detector's threshold for a lens reference (0xf4bf5c).
pub const fn snapshot_position_threshold(reference: u32) -> Property { Property(0x7627u32.wrapping_add(reference.wrapping_mul(2))) }

/// A cell's weight: its zone's configuration byte (0xf4a7a8).
pub const fn zone_weight(cell: usize) -> Property { Property(0x64bc + crate::stock::adjust::ZONES[cell] as u32) }

/// The centre four cells, ring 0 of table 0xf568c0.
pub const CENTRE: [usize; 4] = [119, 120, 135, 136];

/// Whether a scene may be published, for the scenes that have a switch (0xf4a57c).
pub const fn scene_enable(scene: Scene) -> Option<Property> {
    Some(Property(match scene.0 {
        0 => 0x7685,
        1 => 0x7687,
        4 => 0x7688,
        5 => 0x7686,
        8 => 0x7689,
        9 => 0x768a,
        11 => 0x768b,
        14 => 0x768c,
        48 => 0x768d,
        _ => return None,
    }))
}

/// The rate filter's switches and level bounds and the reference grid's threshold, by scene
/// group in `SceneGroup` order (0xf4a848, 0xf4ad7c).
pub const FILTER_ENABLE: [Property; 6] = [Property(0x7697), Property(0x7699), Property(0x7698), Property(0x769a), Property(0x769b), Property(0x769c)];
pub const FILTER_POSITION: [Property; 6] = [Property(0x769d), Property(0x769f), Property(0x769e), Property(0x76a0), Property(0x76a1), Property(0x76a2)];
pub const FILTER_LEVEL: [Property; 6] = [Property(0x76a3), Property(0x76a5), Property(0x76a4), Property(0x76a6), Property(0x76a7), Property(0x76a8)];
pub const FILTER_LOW: [Property; 6] = [Property(0x76a9), Property(0x76ac), Property(0x76aa), Property(0x76ad), Property(0x76ae), Property(0x76af)];
/// Only the sunset group has an upper bound.
pub const FILTER_SUNSET_HIGH: Property = Property(0x76ab);
pub const GROUP_GRID_THRESHOLD: [Property; 6] = [Property(0x7612), Property(0x7614), Property(0x7613), Property(0x7615), Property(0x7616), Property(0x7617)];

/// The calibrated level step live view's levels are measured at (0xfa7edc returns 3).
pub const LIVE_LEVEL_STEP: u8 = 3;

/// Nonzero: the frame query skips the majority's detector; by whether record byte 0x65 is set
/// (0xf4f584).
pub const QUERY_SKIP: [Property; 2] = [Property(0x75e7), Property(0x767d)];

/// The frame query's fallback detectors in order, by whether record byte 0x65 is set (0xf4f74c).
pub const fn fallback_detector(alternate: bool, index: u32) -> Property { Property(if alternate { 0x7678 } else { 0x75e1 } + index) }

pub const FALLBACK_DETECTORS: u32 = 5;

/// The majority's window: live view's, then the shot's (0xf4d7b0).
pub const MAJORITY_WINDOW: [Property; 2] = [Property(0x75b1), Property(0x75df)];

/// The response curve's two points, value and response in hundreds (0xf4b1ac..0xf4b1e0).
pub const RESPONSE_LOW: [Property; 2] = [Property(0x75c9), Property(0x75c7)];
pub const RESPONSE_HIGH: [Property; 2] = [Property(0x75c8), Property(0x75c6)];

/// The lens distance's band limits and factors, its lens spans and minimum (two bytes each, by
/// lens), and the minimum's scale (0xf4e7a4).
pub const fn distance_band_limit(band: u32) -> Property { Property(0x760b + band) }
pub const fn distance_factor(band: u32) -> Property { Property(0x7608 + band) }
pub const fn distance_spans(lens: u32) -> [Property; 2] { [Property(0x3e_0b40u32.wrapping_add(lens.wrapping_mul(2))), Property(0x3e_0c09u32.wrapping_add(lens.wrapping_mul(2)))] }
pub const fn distance_minimum(lens: u32) -> Property { Property(0x3e_0572u32.wrapping_add(lens.wrapping_mul(2))) }
pub const DISTANCE_SCALE: Property = Property(0x75cc);

/// The setting the level bounds read the lens's level at, and each level class's reference in
/// signed tenths (0xf4bfac, 0xf4bfc0).
pub const BOUNDS_SETTING: Property = Property(0x75d5);
pub const fn class_reference(class: u32) -> Property { Property(0x75d6u32.wrapping_add(class)) }

/// One bound's settings: the slope in hundredths, the offset, cap and floor in signed tenths, and
/// the two-byte margin.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoundSettings {
    pub slope: Property,
    pub offset: Property,
    pub cap: Property,
    pub floor: Property,
    pub margin: Property,
}

/// The upper and lower bound's settings of record kind 0, then kind 11 (0xf4bfdc..0xf4c11c).
pub const BOUNDS: [[BoundSettings; 2]; 2] = [
    [
        BoundSettings { slope: Property(0x78d9), offset: Property(0x78da), cap: Property(0x78e2), floor: Property(0x78e1), margin: Property(0x78d1) },
        BoundSettings { slope: Property(0x78db), offset: Property(0x78dc), cap: Property(0x78e4), floor: Property(0x78e3), margin: Property(0x78d5) },
    ],
    [
        BoundSettings { slope: Property(0x78dd), offset: Property(0x78de), cap: Property(0x78e6), floor: Property(0x78e5), margin: Property(0x78d3) },
        BoundSettings { slope: Property(0x78df), offset: Property(0x78e0), cap: Property(0x78e8), floor: Property(0x78e7), margin: Property(0x78d7) },
    ],
];

// The vote's configuration (0xf4f838).

/// Nonzero: the shot's vote asks the frame query; zero, it takes the majority.
pub const CAPTURE_QUERY: Property = Property(0x75e6);
/// The shot's change detectors by slot, their weights six ids on, and the weight that finds the
/// frame unchanged.
pub const CAPTURE_DETECTORS: [Property; 6] = detector_keys(0x76b3);
pub const CAPTURE_THRESHOLD: Property = Property(0x76bf);
/// Nonzero: a grid match toward the shifted grid, then a lens move toward the reference, leave the
/// frame unchanged.
pub const CAPTURE_MATCH_TOWARD: Property = Property(0x76c0);
pub const CAPTURE_LENS_TOWARD: Property = Property(0x76c1);
/// Nonzero: the shot records its recognition and takes the majority.
pub const CAPTURE_RECORD: Property = Property(0x75e0);
/// Zero: live view publishes the majority of its frame queries.
pub const LIVE_VOTE: Property = Property(0x75b0);
/// Nonzero: a held recognition is released after `HOLD_FRAMES` frames, not by its detectors.
pub const HOLD_BY_COUNT: Property = Property(0x75b2);
pub const HOLD_FRAMES: Property = Property(0x75b5);
pub const REFERENCE_DETECTORS: [Property; 6] = detector_keys(0x75b6);
pub const REFERENCE_VOTE_THRESHOLD: Property = Property(0x75c2);
/// The window of change decisions and the changes in it that release a held recognition.
pub const RELEASE_WINDOW: Property = Property(0x75b3);
pub const RELEASE_CHANGES: Property = Property(0x75b4);
/// Nonzero: live view compares each frame with the snapshot.
pub const SNAPSHOT_VOTE: Property = Property(0x7610);
pub const SNAPSHOT_DETECTORS: [Property; 6] = detector_keys(0x7619);
pub const SNAPSHOT_VOTE_THRESHOLD: Property = Property(0x7625);
/// Nonzero: only a frame unchanged from the snapshot is recognised, and only such a frame is
/// filtered.
pub const UNCHANGED_ONLY: Property = Property(0x7611);
pub const FILTER_UNCHANGED_ONLY: Property = Property(0x768f);
/// Nonzero: a changed frame keeps a group scene only when the group's switch allows it, by
/// scene group in `SceneGroup` order.
pub const GROUP_SWITCHES: Property = Property(0x7690);
pub const GROUP_SWITCH: [Property; 6] = [Property(0x7691), Property(0x7693), Property(0x7692), Property(0x7694), Property(0x7695), Property(0x7696)];
/// The scene and flag overrides, 255 for none, live view's and then the shot's, with the bit of
/// property 0x65a8 that holds their flags (0xf4fd98, 0xf50bc0).
pub const SCENE_OVERRIDE: [Property; 2] = [Property(0x4a12), Property(0x4a13)];
pub const FLAG_OVERRIDES: Property = Property(0x65a8);
pub const FLAG_OVERRIDE_SHIFT: [u32; 2] = [3, 5];
/// Nonzero: a record with the record measurement selection publishes this kind (0xf4a0e4).
pub const KIND_OVERRIDE: Property = Property(0x7606);
/// Bit 7 of property 0xa8 keeps the work-memory log; bit 10 of the two-byte property 0xe8 writes
/// the trace lines.
pub const LOG_SWITCH: (Property, u32) = (Property(0xa8), 0x80);
pub const TRACE_SWITCH: (Property, u16) = (Property(0xe8), 0x400);
/// The log's last index; the one after it wraps to 0.
pub const LOG_LAST: u16 = LOG_ENTRIES - 1;

const fn detector_keys(first: u32) -> [Property; 6] { [Property(first), Property(first + 1), Property(first + 2), Property(first + 3), Property(first + 4), Property(first + 5)] }

/// A change detector's weight id: six on from its slot id.
pub const fn detector_weight(key: Property) -> Property { Property(key.0 + 6) }

/// The scene the configured kind names (0xf4a0e4).
pub const fn translated_kind(kind: u32) -> Scene {
    Scene(match kind {
        2 => 48,
        3 => 1,
        4 => 4,
        5 => 14,
        6 => 5,
        7 => 11,
        _ => 0,
    })
}

/// The on-screen text's scene line for each scene, AUTO's for the rest (0xf4e058).
pub fn scene_label(scene: Scene) -> &'static [u8] {
    match scene.0 {
        1 => b"PORTRAIT            ",
        4 => b"BACKLIGHT           ",
        5 => b"LANDSCAPE           ",
        8 => b"LANDSCAPE_PORTRAIT  ",
        9 => b"LANDSCAPE_BACKLIGHT ",
        11 => b"NIGHTSCAPE_HAND     ",
        12 => b"NIGHTSCAPE          ",
        14 => b"NIGHTSCAPE_PORTRAIT ",
        15 => b"NIGHT_TRIPOD_POR    ",
        18 => b"BEACH               ",
        19 => b"BEACH_PORTRAIT      ",
        20 => b"BEACH_BACKLIGHT     ",
        21 => b"SNOW                ",
        22 => b"SNOW_PORTRAIT       ",
        23 => b"SNOW_BACKLIGHT      ",
        25 => b"SUNSET              ",
        26 => b"SUNSET_PORTRAIT     ",
        27 => b"SUNSET_BACKLIGHT    ",
        34 => b"SKY                 ",
        35 => b"SKY_PORTRAIT        ",
        36 => b"SKY_BACKLIGHT       ",
        37 => b"LEAF              ",
        38 => b"LEAF_PORTRAIT     ",
        39 => b"LEAF_BACKLIGHT    ",
        40 => b"SKY_LEAF          ",
        41 => b"SKY_LEAF_PORTRAIT ",
        42 => b"SKY_LEAF_BACKLIGHT",
        48 => b"MACRO               ",
        _ => b"AUTO                ",
    }
}

/// The on-screen text's flag words (0xf52f30, 0xf52f38, 0xf52f40).
pub const MOVING_TEXT: &[u8] = b"&MOTION";
pub const SHAKE_REDUCTION_TEXT: &[u8] = b"&ADIS";
pub const NO_FLAGS_TEXT: &[u8] = b"NO_EXT_SCENE";

/// Where the trace's scene names lie, by scene 1..=57, AUTO's for the rest (0xf50fa0 table).
pub const fn trace_scene_name(scene: Scene) -> SharedAddress {
    const NAMES: [u32; 57] = [
        0xf5_2f58, 0xf5_2f68, 0xf5_2f84, 0xf5_2f90, 0xf5_2fb0, 0xf5_2fc0, 0xf5_2fdc, 0xf5_2ffc, 0xf5_3008, 0xf5_3018, 0xf5_302c, 0xf5_3048, 0xf5_3060, 0xf5_3080, 0xf5_30a0,
        0xf5_30c4, 0xf5_30d8, 0xf5_30f0, 0xf5_3100, 0xf5_3110, 0xf5_3124, 0xf5_3134, 0xf5_3144, 0xf5_3158, 0xf5_3168, 0xf5_3178, 0xf5_3188, 0xf5_319c, 0xf5_31b0, 0xf5_31c4,
        0xf5_31d4, 0xf5_31e4, 0xf5_31f4, 0xf5_3204, 0xf5_320c, 0xf5_3218, 0xf5_3228, 0xf5_322c, 0xf5_3238, 0xf5_3248, 0xf5_3250, 0xf5_3260, 0xf5_3274, 0xf5_3294, 0xf5_32a4,
        0xf5_32b8, 0xf5_32c4, 0xf5_32d4, 0xf5_32e4, 0xf5_32f4, 0xf5_3300, 0xf5_330c, 0xf5_3318, 0xf5_332c, 0xf5_3340, 0xf5_3354, 0xf5_3368,
    ];
    SharedAddress(match scene.0 {
        1..=57 => NAMES[scene.0 as usize - 1],
        _ => 0xf5_2f50,
    })
}

/// The trace's flag words: moving, shake reduction, neither (0xf53378, 0xf52f38, 0xf53380), and
/// the measurements line's format (0xf53390). The trace writes to log 0xffb104 through 0x64744c.
pub const TRACE_MOVING: SharedAddress = SharedAddress(0xf5_3378);
pub const TRACE_SHAKE_REDUCTION: SharedAddress = SharedAddress(0xf5_2f38);
pub const TRACE_NO_FLAGS: SharedAddress = SharedAddress(0xf5_3380);
pub const TRACE_MEASUREMENTS: SharedAddress = SharedAddress(0xf5_3390);
pub const TRACE_LOG: u32 = 0xff_b104;

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
    Changes,
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

const KEY_RECORD_BYTES: usize = 0x214;
const INDEX_RECORD_BYTES: usize = 0x24;
const COMPARISON_RECORD_BYTES: usize = 0x450;
const FEATURES_BYTES: usize = 0x80;
const EVALUATION_BYTES: usize = 0x974;
const LIVE_TRACE_BYTES: usize = 0x5c;
const CAPTURE_TRACE_BYTES: usize = 0x40;

fn word_byte(value: u32, at: usize) -> u8 { value.to_le_bytes()[at & 3] }
fn half_byte(value: u16, at: usize) -> u8 { value.to_le_bytes()[at & 1] }
fn grid_byte(grid: &Grid, at: usize) -> u8 { half_byte(grid[at / 2] as u16, at) }
fn recognition_byte(value: Recognition, at: usize) -> u8 {
    match at {
        0..=3 => word_byte(value.scene.0, at),
        4..=5 => half_byte(value.flags.0, at),
        _ => 0,
    }
}

fn note_byte(note: &FrameNote, at: usize, words_at: usize) -> u8 {
    match at {
        0..=0x1ff => return grid_byte(&note.grid, at),
        0x200..=0x201 => return half_byte(note.position as u16, at),
        _ => {}
    }
    let Some(at) = at.checked_sub(words_at) else { return 0 };
    let value = match at / 4 {
        0 => u32::from(note.subject),
        1 => note.reference,
        2 => note.motion,
        3 => u32::from(note.stationary),
        _ => return 0,
    };
    word_byte(value, at)
}

fn key_byte(record: Option<&KeyRecord>, at: usize) -> u8 {
    if at >= KEY_RECORD_BYTES { return 0; }
    match (record, at) {
        (Some(record), 0) => record.code.byte(),
        (Some(record), 2..) => note_byte(&record.note, at - 2, 0x202),
        (None, 0) => EMPTY_CODE,
        _ => 0,
    }
}

fn index_byte(record: &IndexRecord, at: usize) -> u8 {
    match at {
        0..=3 => word_byte(u32::from(record.stationary), at),
        4..=7 => word_byte(u32::from(record.steady), at),
        8 => record.count,
        _ => 0,
    }
}

fn comparison_byte(record: &ComparisonRecord, at: usize) -> u8 {
    match at {
        0..=0x213 => key_byte(Some(&record.reference), at),
        0x214..=0x217 => word_byte(record.source.map_or(0, |source| source.0), at),
        0x218..=0x417 => grid_byte(&record.compared, at - 0x218),
        0x418..=0x419 => half_byte(record.mean, at),
        0x41c..=0x41d => half_byte(record.position as u16, at),
        0x41e..=0x41f => half_byte(record.previous_position as u16, at),
        0x420..=0x421 => half_byte(record.position_change, at),
        0x424..=0x427 => word_byte(u32::from(record.subject_present), at),
        0x428..=0x42b => word_byte(record.motion, at),
        0x42c..=0x437 => index_byte(&record.stationary, at - 0x42c),
        _ => 0,
    }
}

fn motion_byte(record: &crate::adjust::configuration::MotionRecord, at: usize) -> u8 {
    match at {
        0..=3 => word_byte(record.found, at),
        4..=5 => half_byte(record.speed, at),
        8..=11 => word_byte(record.published_kind, at),
        12..=13 => half_byte(record.published_speed, at),
        16..=19 => word_byte(record.dominant, at),
        20..=21 => half_byte(record.average, at),
        22..=23 => half_byte(record.average_scale, at),
        24..=27 => word_byte(record.published_area, at),
        28..=31 => word_byte(record.published_exposure, at),
        36..=39 => word_byte(record.source, at),
        _ => record.bytes.get(at).copied().unwrap_or(0),
    }
}

fn features_byte(value: &Features, at: usize) -> u8 {
    match at {
        0..=5 => recognition_byte(value.selection, at),
        6..=11 => value.tried[at - 6],
        0x14..=0x1f => {
            let level = match (at - 0x14) / 2 {
                0 => value.level, 1 => value.primary_level, 2 => value.alternate_level,
                3 => value.third_level, 4 => value.upper_level, _ => value.lower_level,
            };
            half_byte(level as u16, at)
        }
        0x24..=0x25 => half_byte(value.at_38 as u16, at),
        0x30..=0x37 => {
            let half = match (at - 0x30) / 2 {
                0 => value.position, 1 => value.lens_step, 2 => value.distance, _ => value.response,
            };
            half_byte(half as u16, at)
        }
        0x48..=0x4f => {
            let bounds = &value.bounds[(at - 0x48) / 4];
            half_byte(if at & 2 == 0 { bounds.high } else { bounds.low } as u16, at)
        }
        0x50..=0x77 => motion_byte(&value.motion, at - 0x50),
        _ => {
            let word = match at & !3 {
                0x0c => value.at_005d, 0x10 => value.subjects, 0x20 => value.difference_answer,
                0x28 => value.at_3c, 0x2c => value.status, 0x38 => value.at_4c,
                0x3c => value.reference, 0x40 => value.state, 0x44 => value.selected,
                0x78 => u32::from(value.stationary), 0x7c => u32::from(value.index_recorded),
                _ => return 0,
            };
            word_byte(word, at)
        }
    }
}

fn evaluation_byte(value: &Evaluation, at: usize) -> u8 {
    let choices = &value.choices;
    let tally = &choices.tally;
    match at {
        0..=0x7f => features_byte(&value.features, at),
        0x80..=0x85 => recognition_byte(choices.history, at - 0x80),
        0x88..=0x8d => recognition_byte(choices.majority, at - 0x88),
        0x98 => choices.recorded,
        0x9c => choices.held,
        0xa8..=0xad => tally.detectors[at - 0xa8],
        0xae..=0xb3 => tally.weights[at - 0xae],
        0xb4..=0xcb => word_byte(tally.answers[(at - 0xb4) / 4], at),
        0xcc => tally.weight,
        0xd4..=0x523 => comparison_byte(&value.against_reference, at - 0xd4),
        0x524..=0x737 => note_byte(&value.snapshot, at - 0x524, 0x204),
        0x73c..=0x93b => grid_byte(&value.against_snapshot.compared, at - 0x73c),
        0x93c..=0x93d => half_byte(value.against_snapshot.mean, at),
        0x942..=0x943 => half_byte(value.against_snapshot.previous_position as u16, at),
        0x944..=0x945 => half_byte(value.against_snapshot.position_change, at),
        _ => {
            let word = match at & !3 {
                0x90 => u32::from(choices.holding), 0x94 => u32::from(choices.recording),
                0xa0 => u32::from(choices.released), 0xa4 => u32::from(choices.unchanged),
                0xd0 => tally.decision,
                _ => return 0,
            };
            word_byte(word, at)
        }
    }
}

fn detectors_byte(value: &LensDetectors, at: usize) -> u8 {
    let word = match at / 4 {
        0 => u32::from(value.matched), 1 => value.match_direction,
        2 => u32::from(value.lens_moved), 3 => value.lens_direction,
        4 => u32::from(value.lens_changed),
        _ => return 0,
    };
    word_byte(word, at)
}

fn live_trace_byte(value: &Trace, at: usize) -> u8 {
    match at {
        0x10..=0x23 => value.hints.entries()[at - 0x10].map_or(EMPTY_CODE, Code::byte),
        0x34..=0x35 => half_byte(value.flags.0, at),
        0x40..=0x53 => detectors_byte(&value.detectors, at - 0x40),
        0x54..=0x5b => recognition_byte(value.promoted, at - 0x54),
        _ => {
            let word = match at & !3 {
                0 => value.query.0, 4 => value.option, 8 => value.candidate.0, 12 => u32::from(value.ready),
                0x24 => value.filtered.0, 0x28 => value.earlier_candidate.0,
                0x2c => value.earlier_filtered.0, 0x30 => value.selection.0,
                0x38 => u32::from(value.filter.position), 0x3c => u32::from(value.filter.level),
                _ => return 0,
            };
            word_byte(word, at)
        }
    }
}

fn capture_trace_byte(value: &CaptureTrace, at: usize) -> u8 {
    match at {
        0x10..=0x23 => detectors_byte(&value.detectors, at - 0x10),
        0x30..=0x37 => recognition_byte(value.selection, at - 0x30),
        0x38..=0x3f => recognition_byte(value.promoted, at - 0x38),
        _ => {
            let word = match at & !3 {
                0 => value.query.0, 4 => value.option, 8 => value.candidate.0, 12 => u32::from(value.ready),
                0x24 => value.group.0, 0x28 => value.hint.0, 0x2c => value.filtered.0,
                _ => return 0,
            };
            word_byte(word, at)
        }
    }
}

/// The field at `offset` of `workspace` as the capture dump reads it.
pub fn workspace_field(state: &SceneRecognition, workspace: Workspace, offset: u32, width: Width) -> u32 {
    let offset = offset as usize;
    let (base, extent) = match workspace {
        Workspace::SavedEvaluation => (0, EVALUATION_BYTES),
        Workspace::SavedFeatures => (0, FEATURES_BYTES + 8),
        Workspace::LiveTrace => (0, LIVE_TRACE_BYTES),
        Workspace::CaptureTrace => (0, CAPTURE_TRACE_BYTES),
        Workspace::Changes | Workspace::PrimaryCodes | Workspace::SecondaryCodes => (0, HISTORY),
        Workspace::References => (offset / KEY_RECORD_BYTES * KEY_RECORD_BYTES, KEY_RECORD_BYTES),
        Workspace::Stationary => (offset / INDEX_RECORD_BYTES * INDEX_RECORD_BYTES, INDEX_RECORD_BYTES),
        Workspace::Comparisons => (offset / COMPARISON_RECORD_BYTES * COMPARISON_RECORD_BYTES, COMPARISON_RECORD_BYTES),
    };
    let byte = |index| {
        let at = offset - base + index;
        if at >= extent { return 0; }
        match workspace {
            Workspace::SavedEvaluation => evaluation_byte(state.saved_evaluation(), at),
            Workspace::SavedFeatures => {
                let (saved, chosen) = state.saved_features();
                if at < FEATURES_BYTES { features_byte(saved, at) } else { recognition_byte(chosen, at - FEATURES_BYTES) }
            }
            Workspace::LiveTrace => live_trace_byte(state.live_trace(), at),
            Workspace::CaptureTrace => capture_trace_byte(state.capture_trace(), at),
            Workspace::Changes => state.changes().entries()[at].map_or(EMPTY_CODE, u8::from),
            Workspace::PrimaryCodes => state.primary_codes().entries()[at].map_or(EMPTY_CODE, Code::byte),
            Workspace::SecondaryCodes => state.secondary_codes().entries()[at].map_or(EMPTY_CODE, Code::byte),
            Workspace::References => state.references().entries().get(base / KEY_RECORD_BYTES).map_or(0, |entry| key_byte(entry.as_ref(), at)),
            Workspace::Stationary => state.stationary_history().get(base / INDEX_RECORD_BYTES).map_or(0, |entry| index_byte(entry, at)),
            Workspace::Comparisons => state.comparisons().get(base / COMPARISON_RECORD_BYTES).map_or(0, |entry| comparison_byte(entry, at)),
        }
    };
    match width {
        Width::Byte => u32::from(byte(0)),
        Width::Half => u32::from(u16::from_le_bytes([byte(0), byte(1)])),
        Width::SignedHalf => i32::from(i16::from_le_bytes([byte(0), byte(1)])) as u32,
        Width::Word => u32::from_le_bytes([byte(0), byte(1), byte(2), byte(3)]),
    }
}
