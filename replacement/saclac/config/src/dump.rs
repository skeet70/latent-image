#![forbid(unsafe_code)]

//! The configuration section of a shot's dump (0x748928): a channel's step table and the
//! adjustment words, limits and selection words the shot was configured with.

use xm1_saclac_shared::settings::Channel;

use crate::adjustment::{byte, half, signed_half, Adjustments};
use crate::limits::{limit, shifted_floor, shifted_second_floor, Kind, Limit, Side};
use crate::ports::Derivations;
use crate::table::{bank_size, rounded_word, value, Row, Step};

/// Stock derivations only the dump reads, which stay stock.
pub trait DumpDerivations {
    /// 0x7468b8.
    fn transformed_value(&mut self, step: Step, row: Row) -> u16;
    /// 0x74695c.
    fn transformed_word(&mut self, step: Step, row: Row) -> u16;
    /// 0x76f340.
    fn lens_position_flags(&mut self) -> u32;
    /// 0x748398.
    fn selection_word(&mut self, channel: Channel, mode: u32, flags: u32, word: i16) -> u32;
}


/// How an adjustment word of the dump reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Read {
    Byte,
    Half,
    SignedHalf,
}

/// An adjustment word of the dump: where it reads and, for per-channel words, the stride.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdjustmentWord {
    pub at: u32,
    pub stride: u32,
    pub read: Read,
}

const fn per_channel(at: u32, read: Read) -> AdjustmentWord { AdjustmentWord { at, stride: 2, read } }

const fn fixed(at: u32, read: Read) -> AdjustmentWord { AdjustmentWord { at, stride: 0, read } }

/// The first word of the channel's 22-byte record.
pub const RECORD_WORD: AdjustmentWord = AdjustmentWord { at: 0x3f_3d14, stride: 22, read: Read::SignedHalf };

/// The adjustment words of every channel, in section order.
pub const WORDS: [AdjustmentWord; 15] = [
    fixed(0x3e_0240, Read::Half),
    fixed(0x3e_0243, Read::Half),
    fixed(0x3e_0246, Read::Byte),
    per_channel(0x3e_0317, Read::SignedHalf),
    per_channel(0x3e_03e0, Read::SignedHalf),
    per_channel(0x3e_04a9, Read::SignedHalf),
    per_channel(0x3e_0572, Read::Half),
    per_channel(0x3e_063b, Read::Half),
    per_channel(0x3e_0704, Read::Half),
    per_channel(0x3e_07f7, Read::Half),
    per_channel(0x3e_08c0, Read::Half),
    per_channel(0x3e_09a2, Read::SignedHalf),
    per_channel(0x3e_0a6b, Read::SignedHalf),
    fixed(0x3e_2640, Read::Half),
    fixed(0x3e_0fdd, Read::Half),
];

/// The adjustment words only channels 0..=9 have.
pub const REFERENCE_WORDS: [AdjustmentWord; 3] =
    [per_channel(0x3e_07cd, Read::Half), per_channel(0x3e_07e2, Read::Half), per_channel(0x3e_0989, Read::Half)];

/// The adjustment words after the floors, in section order.
pub const LATE_WORDS: [AdjustmentWord; 4] = [
    per_channel(0x3e_0b40, Read::Half),
    per_channel(0x3e_0c09, Read::Half),
    per_channel(0x3e_0cd2, Read::Half),
    per_channel(0x3e_0d9b, Read::Half),
];

/// The limit tables in section order, each read for `LIMIT_QUERIES`.
pub const LIMITS: [Limit; 5] = [Limit::OptionalWord, Limit::OptionalShiftedHalf, Limit::OptionalShiftedNextHalf, Limit::Word, Limit::ShiftedHalf];

pub const LIMIT_QUERIES: [(Kind, Side, u32); 10] = [
    (Kind::First, Side::Other, 0),
    (Kind::Second, Side::Other, 0),
    (Kind::First, Side::One, 9),
    (Kind::First, Side::Other, 9),
    (Kind::Second, Side::One, 9),
    (Kind::Second, Side::Other, 9),
    (Kind::First, Side::One, 11),
    (Kind::First, Side::Other, 11),
    (Kind::Second, Side::One, 11),
    (Kind::Second, Side::Other, 11),
];


fn read(area: &impl Adjustments, word: AdjustmentWord, channel: Channel) -> u32 {
    let at = word.at.wrapping_add(word.stride.wrapping_mul(channel.0 as u32));
    match word.read {
        Read::Byte => u32::from(byte(area, at)),
        Read::Half => u32::from(half(area, at)),
        Read::SignedHalf => i32::from(signed_half(area, at)) as u32,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DumpOffset(pub u32);

pub trait DumpBuffer {
    fn read_word(&self, at: DumpOffset) -> u32;
    fn write_word(&self, at: DumpOffset, value: u32);
    fn write_half(&self, at: DumpOffset, value: u16);
}

/// The configuration section's live reads and publications (0x748928).
pub fn export_configuration(
    area: &impl Adjustments,
    derivations: &mut impl Derivations,
    sources: &mut impl DumpDerivations,
    channel: Channel,
    output: &impl DumpBuffer,
) {
    let word = |at, value| output.write_word(DumpOffset(at), value);
    let half = |at, value| output.write_half(DumpOffset(at), value);
    word(0, 0x5354_4c45);
    word(0x2a4, 0x4c41_4c45);
    let row = Row(channel.0 as u32);
    let mut index = 0;
    while index < bank_size(area) {
        let step = Step(index);
        half(4 + 2 * index, rounded_word(area, step, row) as u16);
        half(4 + 2 * (32 + index), value(area, step, row));
        half(4 + 2 * (68 + index), sources.transformed_value(step, row));
        half(4 + 2 * (100 + index), sources.transformed_word(step, row));
        half(2 * (184 + index), derivations.table_scale(index, channel));
        index += 1;
    }
    for first in [0, 32, 68, 100] { half(4 + 2 * (first + index), 0xfdfd); }
    half(2 * (184 + index), 0xfdfd);
    for (at, source) in [0x130, 0x134, 0x140].into_iter().zip(REFERENCE_WORDS) {
        word(at, if channel.0 <= 9 { read(area, source, channel) } else { 0 });
    }
    word(0x84, read(area, RECORD_WORD, channel));
    word(0x88, read(area, RECORD_WORD, channel));
    let offsets = [0x10c, 0x110, 0x114, 0x118, 0x11c, 0x120, 0x124, 0x128, 0x12c, 0x138, 0x13c, 0x144, 0x148, 0x154, 0x158];
    for (at, source) in offsets[..13].iter().copied().zip(&WORDS[..13]) { word(at, read(area, *source, channel)); }
    word(0x14c, shifted_floor(area));
    word(0x150, shifted_second_floor(area));
    for (at, source) in offsets[13..].iter().copied().zip(&WORDS[13..]) { word(at, read(area, *source, channel)); }
    for (at, source) in [0x15c, 0x160, 0x164, 0x168].into_iter().zip(LATE_WORDS) { word(at, read(area, source, channel)); }
    word(0x16c, u32::from(derivations.channel_scale(channel)));
    let mut at = 0x1b0;
    for which in LIMITS {
        for (kind, side, mode) in LIMIT_QUERIES {
            word(at, limit(area, which, kind, side, mode));
            at += 4;
        }
    }
    let flags = sources.lens_position_flags();
    let base = output.read_word(DumpOffset(0x118)) as i16;
    word(0x278, sources.selection_word(channel, 2, flags, base));
    let primary = output.read_word(DumpOffset(0x11c)) as i16;
    word(0x27c, sources.selection_word(channel, 1, flags, primary));
    for (at, addition) in [(0x280, 0x120), (0x284, 0x124), (0x288, 0x128), (0x28c, 0x12c), (0x298, 0x138), (0x29c, 0x13c)] {
        let added = output.read_word(DumpOffset(addition));
        let base = output.read_word(DumpOffset(0x118));
        word(at, sources.selection_word(channel, 2, flags, base.wrapping_add(added) as i16));
    }
    for (at, addition) in [(0x290, 0x130), (0x294, 0x134), (0x2a0, 0x140)] {
        let value = if channel.0 <= 9 {
            let added = output.read_word(DumpOffset(addition));
            let base = output.read_word(DumpOffset(0x118));
            sources.selection_word(channel, 2, flags, base.wrapping_add(added) as i16)
        } else { 0 };
        word(at, value);
    }
}
