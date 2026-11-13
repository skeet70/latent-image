#![forbid(unsafe_code)]

//! The configuration section of a shot's dump (0x748928): a channel's step table and the
//! adjustment words, limits and selection words the shot was configured with.

use crate::adjustment::{byte, half, signed_half, Adjustments};
use crate::channel::Channel;
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

/// One step of the channel's row.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StepDump {
    pub rounded: i16,
    pub value: u16,
    pub transformed_value: u16,
    pub transformed_word: u16,
    pub scale: u16,
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

const SELECTED: [usize; 6] = [5, 6, 7, 8, 9, 10];

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ConfigurationDump {
    /// The channel's steps; `count` of them are the bank's.
    pub steps: [StepDump; 31],
    pub count: usize,
    pub record_word: i16,
    /// `WORDS` as read, signed halfwords sign-extended.
    pub words: [u32; 15],
    pub references: Option<[u32; 3]>,
    pub floors: [u32; 2],
    pub late_words: [u32; 4],
    pub channel_scale: u16,
    pub limits: [[u32; 10]; 5],
    /// The base word under mode 2, `WORDS[4]` under mode 1, then the base plus each of
    /// `WORDS[5..=10]` under mode 2.
    pub selections: [u32; 8],
    /// The base plus each reference word under mode 2.
    pub reference_selections: Option<[u32; 3]>,
}

fn read(area: &impl Adjustments, word: AdjustmentWord, channel: Channel) -> u32 {
    let at = word.at.wrapping_add(word.stride.wrapping_mul(channel.0 as u32));
    match word.read {
        Read::Byte => u32::from(byte(area, at)),
        Read::Half => u32::from(half(area, at)),
        Read::SignedHalf => i32::from(signed_half(area, at)) as u32,
    }
}

/// The configuration dump of `channel` (0x748928).
pub fn configuration_dump(
    area: &impl Adjustments,
    derivations: &mut impl Derivations,
    sources: &mut impl DumpDerivations,
    channel: Channel,
) -> ConfigurationDump {
    let row = Row(channel.0 as u32);
    let count = bank_size(area) as usize;
    let mut steps = [StepDump::default(); 31];
    for (index, step) in steps.iter_mut().enumerate().take(count) {
        let at = Step(index as u32);
        *step = StepDump {
            rounded: rounded_word(area, at, row),
            value: value(area, at, row),
            transformed_value: sources.transformed_value(at, row),
            transformed_word: sources.transformed_word(at, row),
            scale: derivations.table_scale(at.0, channel),
        };
    }
    let has_references = channel.0 <= 9;
    let words = WORDS.map(|word| read(area, word, channel));
    let references = has_references.then(|| REFERENCE_WORDS.map(|word| read(area, word, channel)));
    let channel_scale = derivations.channel_scale(channel);
    let limits = LIMITS.map(|which| LIMIT_QUERIES.map(|(kind, side, mode)| limit(area, which, kind, side, mode)));
    let flags = sources.lens_position_flags();
    let base = words[3];
    let mut select = |mode: u32, word: u32| sources.selection_word(channel, mode, flags, word as i16);
    let mut selections = [select(2, base), select(1, words[4]), 0, 0, 0, 0, 0, 0];
    for (slot, index) in selections[2..].iter_mut().zip(SELECTED) {
        *slot = select(2, base.wrapping_add(words[index]));
    }
    let reference_selections = references.map(|references| references.map(|word| select(2, base.wrapping_add(word))));
    ConfigurationDump {
        steps,
        count,
        record_word: read(area, RECORD_WORD, channel) as i16,
        words,
        references,
        floors: [shifted_floor(area), shifted_second_floor(area)],
        late_words: LATE_WORDS.map(|word| read(area, word, channel)),
        channel_scale,
        limits,
        selections,
        reference_selections,
    }
}
