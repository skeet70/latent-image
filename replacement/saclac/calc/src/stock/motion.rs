//! The focus section of the client's dump frame the motion layer traces into, and the
//! configuration ids it reads.

use xm1_saclac_interface::calc::SharedAddress;

use crate::exposure::ports::{Property, StockMemory};

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
