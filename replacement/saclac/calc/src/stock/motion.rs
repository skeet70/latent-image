//! The focus section of the client's dump frame the motion layer traces into, and the
//! configuration ids it reads.

use xm1_saclac_interface::calc::SharedAddress;

use crate::adjust::motion::measured::MeasuredLevels;
use crate::adjust::motion::ports::{AreaClass, AreaGrid, CropGeometry, GridRequest, SearchBounds};
use crate::exposure::ports::{Properties, Property, StockMemory};

/// The section starts 0x116cc into the frame and is addressed through its alias with bit 30
/// clear (0xfa3dd8, 0x740e0c).
pub const fn section(frame: SharedAddress) -> SharedAddress { SharedAddress((frame.0 + 0x116cc) & !0x4000_0000) }

/// Closes each traced list.
pub const TERMINATOR: u32 = 0xfdfd_fdfd;
const HALF_TERMINATOR: u16 = 0xfdfd;
const BYTE_TERMINATOR: u8 = 0xfd;

const fn at(section: SharedAddress, offset: u32) -> SharedAddress { SharedAddress(section.0 + offset) }

/// The section's first word and the word 0x51c4c into it, "OFTS" and "OFAL".
pub const OPENING_MARKER: u32 = 0x5354_464f;
pub const CLOSING_MARKER: u32 = 0x4c41_464f;
const CLOSING_MARKER_AT: u32 = 0x51c4c;

/// The records one channel's half of the section clears, at its own offsets; the first
/// terminated word falls between the fourth and the fifth span.
struct ChannelRecords {
    zeroed_words: &'static [u32],
    /// Offset and length of each zeroed span.
    spans: [(u32, usize); 6],
    terminated: &'static [u32],
    /// The first of six zeroed three-word records.
    triples: u32,
}

const FIRST_CHANNEL: ChannelRecords = ChannelRecords {
    zeroed_words: &[
        0x44534, 0x44538, 0x4453c, 0x44c78, 0x44c7c, 0x44c80, 0x44544, 0x44550, 0x4455c, 0x44568, 0x44548, 0x44554, 0x44560, 0x44570, 0x4454c,
        0x44558, 0x44564, 0x44578,
    ],
    spans: [(0x44580, 0x1c), (0x4459c, 0x1c), (0x445b8, 0x28), (0x445e0, 0x28), (0x449f8, 0x30), (0x44a28, 0x30)],
    terminated: &[0x44608, 0x44a58, 0x44aac],
    triples: 0x44b48,
};

const SECOND_CHANNEL: ChannelRecords = ChannelRecords {
    zeroed_words: &[
        0x45db0, 0x45db4, 0x45db8, 0x45dbc, 0x45dc0, 0x45dc4, 0x45dc8, 0x45dcc, 0x45dd0, 0x45dd4, 0x45dd8, 0x45ddc, 0x45de0, 0x45de4, 0x45de8,
        0x45dec, 0x46110, 0x46114,
    ],
    spans: [(0x45e28, 0x1c), (0x45e44, 0x1c), (0x45e60, 0x28), (0x45e88, 0x28), (0x45fb0, 0x30), (0x45fe0, 0x30)],
    terminated: &[0x45eb0, 0x46010, 0x46064],
    triples: 0x460b8,
};

fn clear_channel(memory: &mut impl StockMemory, section: SharedAddress, records: &ChannelRecords, between: impl FnOnce(&mut dyn FnMut(u32, u32))) {
    let mut word = |offset, value| memory.write_u32(at(section, offset), value);
    records.zeroed_words.iter().for_each(|&offset| word(offset, 0));
    between(&mut word);
    let [first, second, third, fourth, fifth, sixth] = records.spans;
    for (offset, bytes) in [first, second, third, fourth] {
        memory.fill(at(section, offset), bytes, 0);
    }
    memory.write_u32(at(section, records.terminated[0]), TERMINATOR);
    for (offset, bytes) in [fifth, sixth] {
        memory.fill(at(section, offset), bytes, 0);
    }
    for &offset in &records.terminated[1..] {
        memory.write_u32(at(section, offset), TERMINATOR);
    }
    for triple in 0..6 {
        memory.fill(at(section, records.triples + 0xc * triple), 0xc, 0);
    }
}

/// A zeroed 0x34-byte record whose three marks, 0x10 apart, read 0xfd.
fn clear_marks(memory: &mut impl StockMemory, section: SharedAddress, offset: u32) {
    memory.fill(at(section, offset), 0x34, 0);
    for mark in 0..3 {
        memory.write_u8(at(section, offset + 0x10 * mark), BYTE_TERMINATOR);
    }
}

fn terminate(memory: &mut impl StockMemory, section: SharedAddress, offsets: impl Iterator<Item = u32>) {
    offsets.for_each(|offset| memory.write_u32(at(section, offset), TERMINATOR));
}

/// Writes the markers, terminates the section's lists and clears its records, as the
/// preparation does before a run (0xfa44ec, 0xfa4654, 0xfa4674..0xfa51dc).
pub fn clear_section(section: SharedAddress, memory: &mut impl StockMemory) {
    memory.fill(at(section, 0x4693c), 0x28c, 0);
    memory.fill(at(section, 0x46bc8), 0x2174, 0);
    memory.write_u32(section, OPENING_MARKER);
    memory.write_u32(at(section, CLOSING_MARKER_AT), CLOSING_MARKER);
    for offset in [0x35674, 0x35738, 0x357fc, 0x358c0] {
        memory.fill(at(section, offset), 0xc4, 0);
    }
    terminate(memory, section, (0..18).map(|list| 0x2ec24 + 0x3f0 * list));
    terminate(memory, section, core::iter::once(0x33304));
    clear_channel(memory, section, &FIRST_CHANNEL, |_| {});
    for (offset, value) in [(0x44b98, 0), (0x44b9c, 0), (0x44ba0, 0), (0x44ba4, 0), (0x44ba8, 0), (0x44bac, 0), (0x44bb0, 0x61), (0x44bb4, 0), (0x44bb8, 0), (0x44bcc, 3), (0x44bfc, 0), (0x44c00, 0), (0x456a0, 0), (0x456a4, 0)] {
        memory.write_u32(at(section, offset), value);
    }
    memory.fill(at(section, 0x456a8), 0x33c, 0);
    memory.fill(at(section, 0x459e4), 0x3cc, 0);
    memory.write_u32(at(section, 0x45694), 0);
    memory.write_u32(at(section, 0x45698), 0);
    memory.fill(at(section, 0x44c84), 0xa04, 0);
    memory.fill(at(section, 0x49018), 0x188, 0);
    memory.write_u32(at(section, 0x46934), 0);
    memory.write_u32(at(section, 0x46938), 2);
    clear_marks(memory, section, 0x4ffe0);
    terminate(memory, section, (0..5).flat_map(|entry| (0..8).map(move |list| 0x3a0c4 + 0x500 * list + 0x100 * entry)));
    terminate(memory, section, (0..14).flat_map(|entry| [0x50014, 0x50e14].map(|list| list + 0x100 * entry)));
    for entry in 0..9 {
        memory.write_u16(at(section, 0x3c8c4 + 0x80 * entry), HALF_TERMINATOR);
        memory.write_u16(at(section, 0x3cd44 + 0x80 * entry), HALF_TERMINATOR);
    }
    for entry in 0..9 {
        memory.write_u32(at(section, 0x3d1c4 + 0x100 * entry), TERMINATOR);
        memory.write_u32(at(section, 0x3dac4 + 0x100 * entry), TERMINATOR);
        memory.write_u16(at(section, 0x3e3c4 + 0x80 * entry), HALF_TERMINATOR);
    }
    memory.write_u16(at(section, 0x3e844), HALF_TERMINATOR);
    terminate(memory, section, (0..10).flat_map(|entry| [0x38cc4, 0x396c4].map(|list| list + 0x100 * entry)));
    terminate(memory, section, (0..5).flat_map(|entry| [0x42e34, 0x43334].map(|list| list + 0x100 * entry)));
    terminate(memory, section, (0..12).map(|entry| 0x43834 + 0x100 * entry));
    for entry in 0..4 {
        memory.write_u8(at(section, 0x44434 + 0x40 * entry), BYTE_TERMINATOR);
    }
    for offset in [0x3e8c4, 0x3e988, 0x3ea4c, 0x3eb10] {
        memory.fill(at(section, offset), 0xc4, 0);
    }
    terminate(memory, section, (0..27).map(|entry| 0x3ebd4 + 0x100 * entry));
    clear_channel(memory, section, &SECOND_CHANNEL, |word| {
        [(0x46118, 3), (0x4611c, 2), (0x46120, 2), (0x46124, 2), (0x46128, 2), (0x4612c, 2), (0x46130, 2)].into_iter().for_each(|(offset, value)| word(offset, value));
        (0..3).flat_map(|column| (0..4).map(move |row| 0x45df0 + 4 * column + 0xc * row)).chain([0x45e20]).for_each(|offset| word(offset, 0));
    });
    memory.write_u32(at(section, 0x46108), 0);
    memory.write_u32(at(section, 0x4610c), 0);
    memory.fill(at(section, 0x46134), 0x33c, 0);
    memory.fill(at(section, 0x46470), 0x3cc, 0);
    clear_marks(memory, section, 0x51c14);
}

/// The lens position a sample's targets were taken around (0xfa3068).
pub const fn sample_position(section: SharedAddress, index: u32) -> SharedAddress { at(section, 0x2ec24 + 4 * index) }

/// The drive time of that position (0xfa309c).
pub const fn sample_time(section: SharedAddress, index: u32) -> SharedAddress { at(section, 0x44608 + 4 * index) }

/// The drive planning record: the start time, then the eleven planning words 0xf969a8 stores.
pub const fn drive_planning(section: SharedAddress, word: u32) -> SharedAddress { at(section, 0x44a28 + 4 * word) }

/// The speed schedule a replanning reads: frames, then speeds (0xfa34c4, 0xfa34cc).
pub const fn schedule_frame(section: SharedAddress, index: u32) -> SharedAddress { at(section, 0x44a58 + 4 * index) }

pub const fn schedule_speed(section: SharedAddress, index: u32) -> SharedAddress { at(section, 0x44aac + 4 * index) }

/// Percent of the sampled midpoint's distance from the target that corrects a sample target
/// (0xf96f84).
pub const SAMPLE_CORRECTION_SCALE: Property = Property(0x9276);
/// Signed bytes: the bias the correction adds in the drive's direction, and the base every
/// correction starts from (0xf96f90, 0xf96ff4).
pub const SAMPLE_CORRECTION_BIAS: Property = Property(0x9274);
pub const SAMPLE_CORRECTION_BASE: Property = Property(0x9275);

/// The fewest frames a measured run must span before its speed replaces the scheduled one
/// (0xfa337c).
pub const MEASURED_RUN_FRAMES: Property = Property(0x9280);

/// Set by the measurement pipeline and read back by the motion layer (0xf9d3f4).
pub const fn pipeline_flag(section: SharedAddress) -> SharedAddress { at(section, 0x45dac) }

/// Ten times the ticks since preparation, when the pipeline asks (0xf9cdcc).
pub const fn elapsed(section: SharedAddress) -> SharedAddress { at(section, 0x44c10) }

/// The contrast results 0x6c3c0 into the frame, addressed without the alias (0xfa3dc4).
pub const fn results(frame: SharedAddress) -> SharedAddress { SharedAddress(frame.0 + 0x6c3c0) }

pub const RESULT_AREAS: u32 = 5;

/// Non-zero once the results hold a measurement.
pub const fn results_present(results: SharedAddress) -> SharedAddress { at(results, 0x4) }

/// Where the peak check records the threshold it compared with (0xf9623c).
pub const fn results_threshold(results: SharedAddress) -> SharedAddress { at(results, 0x970) }

/// 1 for an area the results measured.
pub const fn result_measured(results: SharedAddress, area: u32) -> SharedAddress { at(results, 0xc + 4 * area) }

/// The area's two levels the peak check compares (0xf96258, 0xf96264).
pub const fn result_levels(results: SharedAddress, area: u32) -> [SharedAddress; 2] {
    [at(results, 0x8a4 + 4 * area), at(results, 0x8b8 + 4 * area)]
}

/// Whether the peak check runs at all (0xf9ed5c).
pub const PEAK_CHECK: Property = Property(0x92ca);
/// Two bytes: the level an area must reach (0xf96230).
pub const PEAK_THRESHOLD: Property = Property(0x944f);

/// Two bytes each: the bounds a step offset's scale is clamped to (0xfa3598, 0xfa35a8).
pub const STEP_SCALE_HIGH: Property = Property(0x3e_270d);
pub const STEP_SCALE_LOW: Property = Property(0x3e_270f);
/// The scale selector for kinds outside 5..=18 (0xfa35e8).
pub const STEP_SCALE_SELECTOR: Property = Property(0x9301);

/// Two bytes: the scale of a step offset for `kind` (0xfa35d0, 0xfa35f0).
pub const fn step_scale(kind: u32, selector: u32) -> Property {
    if kind.wrapping_sub(5) <= 13 { Property(((kind - 5) + 0x4ee2) * 2) } else { Property(0x9305 + 2 * selector) }
}

/// Low nibble for channel 0, the next byte's nibble otherwise: which record level a search
/// starts from (0xf970b0).
pub const LEVEL_SELECTION: Property = Property(0x94c0);

/// Bits 0 to 2 enable the lens's own step range, the speed bound and the per-lens table
/// (0xfa3bc8).
pub const STEP_LIMITS: Property = Property(0x924b);
/// Tenths of depth the drive must cover within one exposure (0xfa3c60).
pub const FASTEST_DEPTH: Property = Property(0x924c);
/// The first of the five per-lens table values (0xfa3cbc).
pub const STEP_LIMIT_TABLE: Property = Property(0x924e);
pub const fn step_limit_floor(entry: u32) -> Property { Property(0x9252 + entry) }
pub const fn step_limit_ceiling(entry: u32) -> Property { Property(0x9257 + entry) }
/// A cap on the highest step applies when both the low nibble and bit 4 are set (0xfa3d18).
pub const STEP_CAP: Property = Property(0x3e_272b);
/// Zero caps at the last step (0xfa3d34).
pub const STEP_CAP_VALUE: Property = Property(0x3e_272c);
/// Whether the run computes its reach (0xfa3d84).
pub const REACH: Property = Property(0x925f);
pub const REACH_DEPTH: Property = Property(0x925d);
/// Tenths of the level step's depth the reach may span (0xfa3e74).
pub const REACH_SHARE: Property = Property(0x925e);

/// The capture profile the motion layer's exposures are timed in.
pub const SEARCH_PROFILE: u32 = 0x22;

/// The capture profiles of the program's four capture modes (0xfa653c).
pub const MODE_PROFILES: [u32; 4] = [0x22, 0x21, 0x1f, 0x1e];

/// 1 replaces the exposure program with the configured fixed exposure (0xf971bc).
pub const FIXED_EXPOSURE: Property = Property(0x9602);
pub const FIXED_STEP: Property = Property(0x9603);
/// Two bytes, signed (0xf971d8).
pub const FIXED_LEVEL: Property = Property(0x9604);
/// Indexes the level steps 1 to 9 and 0; larger values take step 2 (0xf971e8, 0xfa6408).
pub const FIXED_GAIN_STEP: Property = Property(0x9606);

/// 1, 2 and 3 force the first, second and third program (0xf97354).
pub const PROGRAM: Property = Property(0x9000);
pub const PROGRAM_LEVEL_GATE: Property = Property(0x9002);
pub const PROGRAM_LEVEL_LIMIT: Property = Property(0x9003);

/// Which program steps add a configured first and second range (0xfa6504).
pub const PROGRAM_INCREASES: [[bool; 18]; 3] = {
    const O: bool = false;
    const I: bool = true;
    [
        [O, O, I, O, I, O, I, O, I, O, O, O, I, O, I, O, O, O],
        [O, O, I, O, I, O, I, O, I, O, O, O, I, O, I, O, O, O],
        [O, O, I, O, I, O, I, O, I, O, I, O, I, O, I, O, O, O],
    ]
};

/// Which program steps take their own floor and gain rather than the next step's (0xfa654c).
pub const PROGRAM_REFINES: [[bool; 18]; 3] = {
    const O: bool = false;
    const I: bool = true;
    [
        [I, I, I, I, I, I, I, I, I, I, O, O, I, I, I, I, I, I],
        [I, I, I, I, I, I, I, I, I, I, O, O, I, I, I, I, I, I],
        [I; 18],
    ]
};

/// Each program step's gain before the exposure times choose one (0xfa642c).
pub const PROGRAM_GAINS: [[u32; 18]; 3] = [
    [0; 18],
    [4, 4, 4, 4, 6, 6, 8, 8, 10, 10, 12, 12, 12, 12, 12, 12, 12, 12],
    [4, 4, 4, 4, 5, 5, 7, 7, 9, 9, 11, 11, 13, 13, 13, 13, 13, 13],
];

/// The configured ranges of a program, taken from its last value down as the steps from the
/// top add them (0xfa63fc, 0xfa6584).
pub const fn first_range(program: usize, used: u32) -> Property { Property([0x901e, 0x9024, 0x902b][program] - used) }
pub const fn second_range(program: usize, used: u32) -> Property { Property([0x9032, 0x9038, 0x903f][program] - used) }

/// A program step's capture mode and signed level offset (0xf9771c..0xf9774c).
pub const fn program_mode(program: usize, step: usize) -> Property { Property([0x919e, 0x91b0, 0x91c2][program] + step as u32) }
pub const fn program_offset(program: usize, step: usize) -> Property { Property([0x91d5, 0x91e7, 0x91f9][program] + step as u32) }

/// A program step's configured floor; step 0's is the floor every step keeps.
pub const fn level_floor(step: usize) -> Property { Property(0x9005 + step as u32) }
/// The last program step's ceiling.
pub const LEVEL_CEILING: Property = Property(0x9017);
/// The level of a profile's longest exposure, the gains' lower bound (0xf97aac).
pub const LONGEST_LEVEL: i16 = 0x69a;

/// 1 counts frames by the exposure's excess over the level, else by the change (0xf98168).
pub const FRAMES_BY_EXCESS: Property = Property(0x96e9);
pub const FRAMES_FACTOR: Property = Property(0x9265);
pub const FRAMES_LIMIT: Property = Property(0x9264);

/// How the previous focus confirmation is reused, 1 to 3 (0xf9c75c).
pub const PREVIOUS_FOCUS_MODE: Property = Property(0x94b8);
/// The lowest record level at which a confirmation is admitted, in tenths (0xf9c828).
pub const PREVIOUS_FOCUS_LEVEL: Property = Property(0x8808);
/// The record classes that admit a confirmation, for channel 0 and for the others (0xf9c850).
pub const PREVIOUS_FOCUS_CLASSES: Property = Property(0x94b9);
pub const PREVIOUS_FOCUS_CHANNEL_CLASSES: Property = Property(0x94b7);

/// The capture mode the selection lists' areas are laid out in (0xf991d4).
pub const SELECTION_MODE: u32 = 0x1e;

/// 1 starts every run from a result it already has (0xf96298).
pub const NO_FULL_SEARCH: Property = Property(0x3e_0009);
/// The scene kind that never searches in full (0xfa45d8).
pub const SCENE_WITHOUT_SEARCH: u8 = 24;

/// The image-pipeline buffers the search hands the pipeline at the preparation's output
/// +0x18, +0x14, +0x1c, +0x20, +0x24 and +0x28 (0xf9e8e4..0xf9e940).
pub const IMAGE_BUFFERS: [SharedAddress; 6] =
    [SharedAddress(0x5ffe_be8c), SharedAddress(0x5ffe_c0b4), SharedAddress(0x5ffe_c08c), SharedAddress(0x5ffe_c4b4), SharedAddress(0x5ffe_c6b4), SharedAddress(0x5ffe_c8b4)];

/// The statistics buffers (0xf99a50..0xf99a9c).
pub const STATISTICS_BUFFERS: [SharedAddress; 5] =
    [SharedAddress(0x5ffe_e12c), SharedAddress(0x5ffe_e32c), SharedAddress(0x5ffe_e3ac), SharedAddress(0x5ffe_e5ac), SharedAddress(0x5ffe_e7ac)];

/// The section's statistics record (0xf99a3c).
pub const fn statistics_trace(section: SharedAddress) -> SharedAddress { at(section, 0x46bc8) }

/// Bit 0 makes every search plain and every run single-channel (0xf9e238, 0xf9e3ac).
pub const PLAIN_SEARCH: Property = Property(0xde);
/// Under the load test, classes 3 and 6 search plainly when this is set (0xf9680c), else with
/// their area when 0xa6 is 1.
pub const LOAD_TEST_PLAIN: Property = Property(0xa7);
pub const LOAD_TEST_AREA: Property = Property(0xa6);
/// Bit 1 for channels 1 and 2, bit 0 for channel 0 (0xf9d3b4).
pub const CHANNEL_SEARCH: Property = Property(0x928e);

/// 0 keeps every run's channel class off (0xf9e334).
pub const CHANNEL_CLASSES: Property = Property(0x946d);
/// Lenses above this less one keep to one channel (0xf9e3cc).
pub const LENS_CHANNEL_LIMIT: Property = Property(0x3e_1213);
/// The armed scene bits that widen the run to channel 1 (0xf9e484).
pub const ARMED_SCENE_CHANNELS: Property = Property(0x9474);
/// Which record traits leave the second channel open (0xf9e4e4).
pub const SECOND_CHANNEL_MASK: Property = Property(0x9473);
/// The scene whose second channel needs mask bit 7 (0xf9e584).
pub const MASKED_SCENE: u8 = 50;

/// A scene's entry in the channel table 0xfa5ec8.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SceneChannels {
    /// An unarmed record of the scene may take the second channel.
    pub second_channel: bool,
    /// The bit of `ARMED_SCENE_CHANNELS` an armed record of the scene tests.
    pub armed_bit: Option<u32>,
    /// A channel-0 run starts from channel 0's table position, not the configured start.
    pub starts_at_channel_position: bool,
}

const SCENE_CHANNEL_TABLE: [(u8, u8); 58] = [
    (1, 5), (0, 0), (0, 7), (0, 7), (1, 1), (0, 2), (0, 2), (0, 2), (0, 0), (0, 1), (0, 7), (0, 3), (0, 3), (0, 3), (1, 6), (1, 6), (1, 7), (1, 7), (0, 4), (0, 0),
    (0, 1), (0, 4), (0, 0), (0, 1), (0, 7), (0, 4), (0, 0), (0, 1), (1, 7), (1, 7), (0, 7), (1, 7), (1, 7), (1, 7), (0, 2), (0, 0), (0, 1), (0, 2), (0, 0), (0, 1),
    (0, 2), (0, 0), (0, 1), (1, 7), (1, 7), (1, 7), (1, 7), (1, 7), (1, 7), (1, 7), (1, 7), (1, 7), (1, 7), (1, 7), (1, 7), (1, 7), (1, 7), (1, 7),
];

/// A scene above 57 reads past the table in stock (0xf9e428, 0xf9e4d4, 0xf99fe8); it takes
/// neither the second channel, an armed bit nor the channel position start.
pub const fn scene_channels(scene: u8) -> SceneChannels {
    if scene as usize >= SCENE_CHANNEL_TABLE.len() {
        return SceneChannels { second_channel: false, armed_bit: None, starts_at_channel_position: false };
    }
    let (second_channel, armed) = SCENE_CHANNEL_TABLE[scene as usize];
    let armed_bit = match armed {
        0 => Some(2),
        1 => Some(0x10),
        2 => Some(4),
        3 => Some(8),
        5 => Some(1),
        6 => Some(0x20),
        _ => None,
    };
    SceneChannels { second_channel: second_channel != 0, armed_bit, starts_at_channel_position: armed == 2 }
}

/// Signed: a record level below ten times this sets the channel's status bit (0xf9e6a0).
pub const CHANNEL_LEVEL_LIMIT: Property = Property(0x94bf);
pub const CHANNEL_STATUS_MASK: Property = Property(0x94be);
/// The channel bits whose status the pipeline is given (0xf9e790).
pub const CHANNEL_STATUS_ENABLE: Property = Property(0x4b00);
pub const FIRST_HELD_SCENE: u8 = 0x33;
pub const SECOND_HELD_SCENE: u8 = 0x34;
/// The drive rate a treated run must reach (0xf9e82c).
pub const RATE_THRESHOLD: Property = Property(0x29e);
/// The scene whose frame delay is 8 (0xf9e854).
pub const DELAYED_SCENE: u8 = 28;

/// Whether the run checks its contrast ratio, and the per-mille scale (two bytes) it checks
/// against (0xf9e944, 0xf9e97c).
pub const RATIO_CHECK: Property = Property(0x92d5);
pub const RATIO_CHECK_SCALE: Property = Property(0x92c2);
/// 1 reads the ratio check's contrast from the alternate tables (0xf9d57c).
pub const RESULT_TABLES: Property = Property(0x92c5);

/// The results' frame byte (0xf9d578).
pub const fn results_frame(results: SharedAddress) -> SharedAddress { at(results, 0x8) }

/// The contrast of a cell (grid area times five plus frame), plain or alternate.
pub const fn result_cell_contrast(results: SharedAddress, cell: u32, alternate: bool) -> SharedAddress {
    at(results, if alternate { 0x3f4 } else { 0x20 } + 4 * cell)
}

/// The contrast of a frame over every area, plain or alternate.
pub const fn result_frame_contrast(results: SharedAddress, frame: u32, alternate: bool) -> SharedAddress {
    at(results, if alternate { 0x82c } else { 0x7c8 } + 4 * frame)
}

/// A frame's measured area (two sides), its reference area (two sides) and two signed level
/// offsets, in the order 0xf9d4bc takes them.
pub const fn result_ratio_words(results: SharedAddress, frame: u32) -> [SharedAddress; 6] {
    let base = SharedAddress(results.0 + 4 * frame);
    [at(base, 0x908), at(base, 0x8f4), at(base, 0x930), at(base, 0x91c), at(base, 0x8cc), at(base, 0x8e0)]
}

/// The level ceiling of an unclassed or unmeasured run (0xf9e9e4) and the table its contrast is
/// read from (0xf9ea1c).
pub const LEVEL_CEILING_DEFAULT: Property = Property(0x923c);
pub const LEVEL_CEILING_TABLE: Property = Property(0x923b);
/// Two-byte contrast bounds and the levels at them, after a peak and otherwise (0xf9ea58,
/// 0xf9ea9c).
pub const PEAKED_CEILING: [Property; 4] = [Property(0x9246), Property(0x9248), Property(0x9244), Property(0x9245)];
pub const LEVEL_CEILING_BAND: [Property; 4] = [Property(0x923f), Property(0x9241), Property(0x923d), Property(0x923e)];

/// A listed area's class from its kind word (0xf95df0).
pub const fn area_class(kind: u32) -> AreaClass {
    match kind {
        8..=10 => AreaClass::ColumnsAlternate,
        5..=7 => AreaClass::RowsUpward,
        2..=4 => AreaClass::Columns,
        _ => AreaClass::RowsDownward,
    }
}

/// What the search frames are laid out with (0xf9eb8c..0xf9ebc0).
pub const SEARCH_FRAME_SETTINGS: [Property; 4] = [Property(0x920c), Property(0x920d), Property(0x9232), Property(0x9233)];

/// Which record level each search mode exposes at, two bits per mode (0xfa3f40).
pub const FRAME_LEVEL_SELECTION: Property = Property(0x95f1);
/// Two-byte factors of the first frame's change, the scene-28 offset and the limit in tenths
/// (0xfa4030, 0xfa4118, 0xfa4128).
pub const CHANGE_FACTORS: [Property; 2] = [Property(0x4b02), Property(0x4b04)];
pub const CHANGE_SCENE_OFFSET: Property = Property(0x4b0f);
pub const CHANGE_LIMIT: Property = Property(0x4b08);
/// The second frame's change in percent and its two-byte limit (0xfa4234, 0xfa425c).
pub const SECOND_CHANGE_SCALE: Property = Property(0x9261);
pub const SECOND_CHANGE_LIMIT: Property = Property(0x9262);

/// The second frame exposure's trace (0xfa42b4..0xfa43c4).
pub const fn exposure_trace(section: SharedAddress, word: u32) -> SharedAddress { at(section, 0x44b04 + 4 * word) }

/// Plan header bytes 2 to 5 of a treated run (0xfa4498..0xfa44d0).
pub const TREATED_PLAN_SETTINGS: [Property; 4] = [Property(0xa48e), Property(0xa48f), Property(0xa48c), Property(0xa4d6)];

/// The sample grid's height and width in percent of the crop (0xf99af0, 0xf99afc).
pub const SAMPLE_GRID_PERCENT: [Property; 2] = [Property(0x920c), Property(0x920d)];
/// The sample grid's row and column percent for a program step (0xf99b0c, 0xf99b18).
pub const fn sample_row_percent(step: u8) -> Property { Property(0x920e + step as u32) }
pub const fn sample_column_percent(step: u8) -> Property { Property(0x9220 + step as u32) }
pub const SAMPLE_GRID_BANDS: u8 = 7;

/// A program step's setting the pipeline reads, per program (0xf99bc0..0xf99be4).
pub const fn program_setting(program: usize, step: u8) -> Property { Property([0x9167, 0x9179, 0x918b][program] + step as u32) }
pub const PROGRAM_SETTING_LIMIT: u32 = 5;

/// The cases in which a treated run starts at its window position: bit 4 held, bit 2 scene 51,
/// bit 3 scene 52, bit 1 a listed area with word 0x2c set, bit 5 one with byte 0x26 at 1, bit 0
/// any other (0xf95e78).
pub const TREATED_WINDOW_START: Property = Property(0xa490);

/// 0 takes a positive second change off the exposure ratio's level (0xf9a228).
pub const RATIO_WITHOUT_CHANGE: Property = Property(0x9269);
/// Tenths: the exposure ratio's ceiling and floor, and the level whose ratio less the second
/// exposure's word 0xc also caps it (0xf9a310, 0xf9a368, 0xf9a344).
pub const RATIO_CEILING: Property = Property(0x9268);
pub const RATIO_FLOOR: Property = Property(0x9267);
pub const RATIO_LEVEL: Property = Property(0x9266);

/// Each sampled area's exposure start and readout in the section, primary areas first, then the
/// line offset the readouts count from (0xf99e2c..0xf99ef0).
pub fn trace_schedule(section: SharedAddress, primary: &[(u32, u32)], secondary: &[(u32, u32)], line_offset: u32, memory: &mut impl StockMemory) {
    for (start_list, readout_list, entries) in [(0x44580, 0x4459c, primary), (0x445b8, 0x445e0, secondary)] {
        for (index, &(start, readout)) in (0..).zip(entries) {
            memory.write_u32(at(section, start_list + 4 * index), start);
            memory.write_u32(at(section, readout_list + 4 * index), readout);
        }
    }
    memory.write_u32(at(section, 0x4457c), line_offset);
}

/// What a treated run's window position is computed from.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TreatedWindowTrace {
    pub amount: u32,
    pub share: u32,
    pub rate: u32,
    pub base_value: u32,
    pub lens: u32,
    pub lens_value: u32,
    pub scaled_rate: u32,
    pub period: u16,
    pub position: i16,
}

/// 0xf997a8..0xf9982c.
pub fn trace_treated_window(section: SharedAddress, trace: &TreatedWindowTrace, memory: &mut impl StockMemory) {
    let words = [
        trace.amount,
        trace.share,
        trace.rate,
        trace.base_value,
        trace.lens,
        trace.lens_value,
        trace.scaled_rate,
        u32::from(trace.period),
        i32::from(trace.position) as u32,
    ];
    for (index, value) in (0..).zip(words) {
        memory.write_u32(at(section, 0x469e0 + 4 * index), value);
    }
}

/// What the layout traces after its schedule.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LayoutTrace {
    pub grid: GridRequest,
    pub sample_grids: [AreaGrid; 2],
    pub search_grids: [AreaGrid; 2],
    /// A treated run's crop geometry.
    pub treated: Option<CropGeometry>,
    pub bounds: SearchBounds,
    pub channel_bounds: SearchBounds,
}

const fn halves(high: u32, low: u32) -> u32 { low | high << 16 }

const fn bound_halves(bounds: SearchBounds) -> u32 { halves(bounds.low as u16 as u32, bounds.high as u16 as u32) }

/// The grid's position, cell size, gaps and counts, each as two halves.
const fn grid_words(grid: &AreaGrid) -> [u32; 4] {
    [
        halves(grid.left, grid.top),
        halves(grid.cell_width, grid.cell_height),
        halves(grid.gap_horizontal as u32, grid.gap_vertical as u32),
        halves(grid.columns as u32, grid.rows as u32),
    ]
}

/// 0xf9a3d0..0xf9a670.
pub fn trace_layout(section: SharedAddress, trace: &LayoutTrace, memory: &mut impl StockMemory) {
    let mut store = |offset: u32, value: u32| memory.write_u32(at(section, offset), value);
    let grid = &trace.grid;
    for (index, value) in (0..).zip([grid.height_percent, grid.width_percent, grid.row_percent, grid.column_percent]) {
        store(0x44c28 + 4 * index, u32::from(value));
    }
    let [first, second] = trace.sample_grids.map(|grid| grid_words(&grid));
    let [third, fourth] = trace.search_grids.map(|grid| grid_words(&grid));
    let words = [first[0], first[1], first[2], first[3], second[0], second[1], second[3], third[0], third[1], third[2], third[3], fourth[0], fourth[1], fourth[3]];
    for (index, value) in (0..).zip(words) {
        store(0x44c40 + 4 * index, value);
    }
    if let Some(CropGeometry { window, crop }) = trace.treated {
        let tracked = &trace.sample_grids[1];
        let words = [
            window.width,
            window.height,
            crop.left.wrapping_sub(window.left),
            crop.top.wrapping_sub(window.top),
            tracked.left.wrapping_sub(window.left),
            tracked.top.wrapping_sub(window.top),
            tracked.cell_width,
            tracked.cell_height,
            u32::from(tracked.columns),
            u32::from(tracked.rows),
        ];
        for (index, value) in (0..).zip(words) {
            store(0x469b4 + 4 * index, value);
        }
    }
    store(0x44bfc, bound_halves(trace.bounds));
    store(0x44c00, bound_halves(trace.channel_bounds));
}

/// One listed area as the preparation traces it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AreaTrace {
    pub position: i32,
    pub centre_x: u32,
    pub centre_y: u32,
    pub at_22: u16,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub at_20: u16,
    pub at_18: u32,
    pub at_1c: u32,
}

/// What the preparation traces besides its configuration.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PreparationTrace {
    /// Confirmation words 1, 2 (its low half, signed) and 7.
    pub confirmation: [i32; 3],
    pub record_level: i32,
    pub snapshot_position: i16,
    pub reference: u32,
    pub drive_kind: u32,
    pub configured_level: i16,
    pub area_count: u32,
    pub areas: [AreaTrace; 10],
}

/// 1 for 1, 3 for 2, else 0 (0xf95ef4).
const fn mapped_selection(value: u32) -> u32 {
    match value {
        1 => 1,
        2 => 3,
        _ => 0,
    }
}

/// The preparation's last traces, in stock order (0xfa541c..0xfa5be0).
pub fn trace_preparation<P: Properties + StockMemory>(section: SharedAddress, trace: &PreparationTrace, ports: &mut P) {
    let byte = |ports: &mut P, id: u32| ports.property(Property(id));
    let word = |ports: &mut P, id: u32| u32::from(crate::exposure::settings::word(ports, Property(id)));
    let store = |ports: &mut P, offset: u32, value: u32| ports.write_u32(at(section, offset), value);
    let value = byte(ports, 0x9232);
    store(ports, 0x44c38, value);
    let value = byte(ports, 0x9233);
    store(ports, 0x44c3c, value);
    for (offset, value) in [0x45688, 0x4568c, 0x45690].into_iter().zip(trace.confirmation) {
        store(ports, offset, value as u32);
    }
    store(ports, 0x44c14, trace.record_level as u32);
    store(ports, 0x44c04, i32::from(trace.snapshot_position) as u32);
    let value = i32::from(byte(ports, 0x9275) as u8 as i8) as u32;
    store(ports, 0x44b00, value);
    store(ports, 0x44c08, trace.reference);
    store(ports, 0x44c0c, trace.drive_kind);
    for (offset, value) in [(0x44b44, 0), (0x44bf8, 1), (0x44bf4, 0), (0x44bf0, 0)] {
        store(ports, offset, value);
    }
    for pair in 0..6 {
        let value = byte(ports, 0x96d4 + 2 * pair);
        store(ports, 0x468a8 + 4 * pair, value);
        let value = byte(ports, 0x96d5 + 2 * pair);
        store(ports, 0x468c0 + 4 * pair, value);
    }
    for (offset, id) in [(0x468d8, 0x92b8), (0x468dc, 0x92bf), (0x468e0, 0x92c0), (0x468e4, 0x92b9)] {
        let value = byte(ports, id);
        store(ports, offset, value);
    }
    let value = mapped_selection(byte(ports, 0x92bc));
    store(ports, 0x468e8, value);
    let value = mapped_selection(byte(ports, 0x92bb));
    store(ports, 0x468ec, value);
    let value = u32::from(byte(ports, 0x92ba) != 0);
    store(ports, 0x468f0, value);
    for index in 0..11 {
        let value = byte(ports, 0x94d2 + index);
        store(ports, 0x468f4 + 4 * index, value);
    }
    store(ports, 0x46920, i32::from(trace.configured_level) as u32);
    for (offset, id) in [(0x46924, 0x94bf), (0x46928, 0x29e), (0x456a4, 0x2b4), (0x4692c, 0x9293)] {
        let value = byte(ports, id);
        store(ports, offset, value);
    }
    let value = word(ports, 0x9294);
    store(ports, 0x46930, value);
    let value = byte(ports, 0x9276);
    store(ports, 0x44574, value);
    for index in 0..4 {
        let value = byte(ports, 0x961c + index);
        store(ports, 0x44c18 + 4 * index, (value & 0xf) | (value & 0xf0) << 12);
    }
    const SEARCH_SETTINGS: [Option<u32>; 30] = [
        Some(0xa480), Some(0xa481), Some(0xa482), Some(0xa483), Some(0xa484), Some(0xa485), Some(0xa486), Some(0xa487), Some(0xa488), Some(0xa489),
        Some(0xa48a), Some(0xa48b), Some(0xa48c), Some(0xa4d6), Some(0xa48d), Some(0xa48e), Some(0xa48f), Some(0xa490), Some(0xa491), None,
        None, Some(0xa4df), Some(0xa492), Some(0xa493), Some(0xa495), Some(0xa4d4), Some(0xa497), Some(0xa498), Some(0xa499), None,
    ];
    for (index, id) in (0..).zip(SEARCH_SETTINGS) {
        let value = match id {
            Some(0xa493) => word(ports, 0xa493),
            Some(id) => byte(ports, id),
            None => 0,
        };
        store(ports, 0x4693c + 4 * index, value);
    }
    store(ports, 0x46a08, 0);
    store(ports, 0x46a0c, trace.area_count);
    for (index, area) in (0..).zip(&trace.areas) {
        //fa5b24..fa5bcc: r3=section+0x8d278+44*i; the signed offsets start at -0x46868.
        let words = [
            area.position as u32,
            area.centre_y,
            area.centre_x,
            u32::from(area.at_22),
            area.y,
            area.x,
            area.height,
            area.width,
            u32::from(area.at_20),
            area.at_18,
            area.at_1c,
        ];
        for (word, value) in (0..).zip(words) {
            store(ports, 0x46a10 + 0x2c * index + 4 * word, value);
        }
    }
}

/// The bytes of calc 0x3c03's result, which the pipeline's callback 0xf20380 copies to block
/// +0x1164.
pub const MEASURED_LEVELS_BYTES: usize = 20;

/// The halves the level check reads (0xf9cdfc): the record levels at +0xc, +4, +6, +0xe and
/// +0x10, the curve level at +8 and its curve point at +0xa.
pub fn measured_levels(result: &[u8; MEASURED_LEVELS_BYTES]) -> MeasuredLevels {
    let half = |offset: usize| u16::from_le_bytes([result[offset], result[offset + 1]]);
    MeasuredLevels { levels: [0xc, 4, 6, 0xe, 0x10].map(|offset| half(offset) as i16), curve_level: half(8) as i16, curve_point: half(0xa) }
}

/// Which record level the level check compares, low nibble for channel 0 and the next byte
/// otherwise, and the margin its excess must pass (0xf9ce64, 0xf9cf50).
pub const MEASURED_LEVEL_SELECTION: Property = Property(0x4b0a);
pub const MEASURED_LEVEL_MARGIN: Property = Property(0x4b0b);

/// 1 when the level check exposed the frames again, else 0 (0xf9cf80, 0xf9d178).
pub const fn reexposure_flag(section: SharedAddress) -> SharedAddress { at(section, 0x48d38) }

/// 0x10443f4's value, constantly 0, which a re-exposure stores (0xf9d15c).
pub const fn reexposure_word(section: SharedAddress) -> SharedAddress { at(section, 0x456a0) }

/// A frame exposure as the level check traces it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExposureSummary {
    pub at_06: i16,
    pub lens_step: u32,
    pub step_level: i16,
    pub exposure_level: i16,
    pub first_level: i16,
    /// 0xf95d14's index of the exposure's profile.
    pub mode: u32,
    pub change: i16,
    pub step: u8,
}

impl ExposureSummary {
    const fn words(&self) -> [u32; 8] {
        [
            self.at_06 as i32 as u32,
            self.lens_step,
            self.step_level as i32 as u32,
            self.exposure_level as i32 as u32,
            self.first_level as i32 as u32,
            self.mode,
            self.change as i32 as u32,
            self.step as u32,
        ]
    }
}

/// What the level check traces after its decision.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LevelCheckTrace {
    /// Record levels 0xa8, 0x94, 0x96 and 0x92 and the record's curve level, each raised by level
    /// step 2.
    pub record_levels: [i32; 5],
    /// The measured levels in the same order.
    pub measured_levels: [i16; 5],
    /// The second exposure as the preparation computed it, and the first.
    pub second: ExposureSummary,
    pub first: ExposureSummary,
    pub selection: u32,
    pub curve_point: u16,
}

/// 0xf9d18c..0xf9d39c.
pub fn trace_level_check(section: SharedAddress, trace: &LevelCheckTrace, memory: &mut impl StockMemory) {
    const LEVEL_ORDER: [usize; 5] = [1, 0, 3, 2, 4];
    for index in LEVEL_ORDER {
        memory.write_u32(at(section, [0x48d10, 0x48d08, 0x48d0c, 0x48d14, 0x48d18][index]), trace.record_levels[index] as u32);
    }
    for index in LEVEL_ORDER {
        memory.write_u32(at(section, [0x48d24, 0x48d1c, 0x48d20, 0x48d28, 0x48d2c][index]), i32::from(trace.measured_levels[index]) as u32);
    }
    for (base, summary) in [(0x48cc8, &trace.second), (0x48ce8, &trace.first)] {
        for (word, value) in (0..).zip(summary.words()) {
            memory.write_u32(at(section, base + 4 * word), value);
        }
    }
    memory.write_u32(at(section, 0x48d30), trace.selection);
    memory.write_u32(at(section, 0x48d34), u32::from(trace.curve_point));
}
