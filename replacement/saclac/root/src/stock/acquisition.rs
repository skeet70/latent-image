#![forbid(unsafe_code)]

//! X-M1 1.50 layouts of the shared-block records the measurement round's acquisition reads and
//! writes, and the configuration keys it reads.

use xm1_focus_measurement_round::acquisition::ports::{
    AcquisitionBlock, ExposureRecordUpdate, ProgramChoice, RateRecord, ReleaseSettings, SelectionRecord, StatisticsBuffer,
};
use xm1_focus_measurement_round::auxiliary::ResourceIndex;
use xm1_focus_measurement_round::pipeline::{PipelineWord, TimingWord};

/// A value at a byte offset of a record, little-endian.
trait Packed: Sized {
    fn get(bytes: &[u8], at: usize) -> Self;
    fn put(&self, bytes: &mut [u8], at: usize);
}

impl Packed for u8 {
    fn get(bytes: &[u8], at: usize) -> Self { bytes[at] }
    fn put(&self, bytes: &mut [u8], at: usize) { bytes[at] = *self }
}

impl Packed for u16 {
    fn get(bytes: &[u8], at: usize) -> Self { u16::from_le_bytes([bytes[at], bytes[at + 1]]) }
    fn put(&self, bytes: &mut [u8], at: usize) { bytes[at..at + 2].copy_from_slice(&self.to_le_bytes()) }
}

impl Packed for u32 {
    fn get(bytes: &[u8], at: usize) -> Self { u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) }
    fn put(&self, bytes: &mut [u8], at: usize) { bytes[at..at + 4].copy_from_slice(&self.to_le_bytes()) }
}

/// A flag kept as a word.
impl Packed for bool {
    fn get(bytes: &[u8], at: usize) -> Self { u32::get(bytes, at) != 0 }
    fn put(&self, bytes: &mut [u8], at: usize) { u32::from(*self).put(bytes, at) }
}

macro_rules! packed_word {
    ($($type:ident),*) => {$(
        impl Packed for $type {
            fn get(bytes: &[u8], at: usize) -> Self { Self(u32::get(bytes, at)) }
            fn put(&self, bytes: &mut [u8], at: usize) { self.0.put(bytes, at) }
        }
    )*};
}

packed_word!(ResourceIndex, PipelineWord, StatisticsBuffer, TimingWord);

/// Where a field sits: one offset, or one offset per element.
trait At<T> {
    fn get(self, bytes: &[u8]) -> T;
    fn put(self, bytes: &mut [u8], value: &T);
}

impl<T: Packed> At<T> for usize {
    fn get(self, bytes: &[u8]) -> T { T::get(bytes, self) }
    fn put(self, bytes: &mut [u8], value: &T) { value.put(bytes, self) }
}

impl<T: Packed, const N: usize> At<[T; N]> for [usize; N] {
    fn get(self, bytes: &[u8]) -> [T; N] { self.map(|at| T::get(bytes, at)) }
    fn put(self, bytes: &mut [u8], value: &[T; N]) {
        for (at, element) in self.into_iter().zip(value) {
            element.put(bytes, at);
        }
    }
}

/// A record's decoder and its encoder over the same field offsets; the encoder leaves the
/// other bytes as they are.
macro_rules! record {
    ($(#[$meta:meta])* $decode:ident, $encode:ident, $record:ident, $bytes:literal, { $($field:ident: $at:expr),* $(,)? }) => {
        $(#[$meta])*
        pub fn $decode(bytes: &[u8; $bytes]) -> $record { $record { $($field: At::get($at, bytes),)* } }

        pub fn $encode(bytes: &mut [u8; $bytes], record: &$record) { $(At::put($at, bytes, &record.$field);)* }
    };
}

record!(
    /// The focus result 0x756734 copies out and 0x756754 back. Offsets from 0xf3382c, 0xf34c98,
    /// 0xf3515c, 0xf3557c, 0xf363f8 and 0xf2457c.
    acquisition_block, put_acquisition_block, AcquisitionBlock, 132, {
    lens: 0x00usize,
    collected: 0x04usize,
    exposure: 0x08usize,
    level: 0x0cusize,
    gain_offset: 0x0eusize,
    offset: 0x10usize,
    aspect: 0x12usize,
    reference: 0x14usize,
    halfword: 0x16usize,
    levels: [0x20, 0x22, 0x24, 0x26],
    reused: 0x28usize,
    offsets: 0x2cusize,
    extra: 0x30usize,
    release_halfword: 0x34usize,
    classes: 0x38usize,
    weights: [0x3c, 0x3e, 0x40, 0x42],
    release_offset: 0x44usize,
    copy: 0x48usize,
    selected: 0x60usize,
    selection_reference: 0x64usize,
    selection_byte: 0x66usize,
    eligible: 0x68usize,
    setting_ready: 0x6cusize,
    setting_position: 0x70usize,
    setting_halfword: 0x74usize,
    grid_word: 0x78usize,
    shot_span: 0x7cusize,
    setting_shot_word: 0x80usize,
});

record!(
    /// The record 0x75a4ec reads and 0x75a52c writes; 0xf2476c fills it from a selection result.
    selection_record, put_selection_record, SelectionRecord, 44, {
    position: 0x00usize,
    word: 0x04usize,
    level: 0x08usize,
    levels: [0x0a, 0x0e, 0x10, 0x12],
    target: 0x0cusize,
    limit_level: 0x14usize,
    reference: 0x16usize,
    flags: [0x18, 0x1c, 0x20, 0x24],
    limit: 0x28usize,
    tracked: 0x29usize,
});

/// 0xf36324..0xf36354 over the 24-byte record 0x758068 reads: the flag byte at +4 set, the
/// word before it kept.
pub fn put_program_choice(bytes: &mut [u8; 24], choice: &ProgramChoice) {
    1u8.put(bytes, 0x04);
    choice.word.put(bytes, 0x08);
    choice.level.put(bytes, 0x0c);
    choice.timing.put(bytes, 0x10);
    choice.code.put(bytes, 0x14);
    choice.selected.put(bytes, 0x16);
}

/// The 40-byte rate record 0x7582c0 reads: the halfword and word 0xf521d8 takes (0xf35340).
pub fn rate_record(bytes: &[u8; 40]) -> RateRecord { RateRecord { word: u32::get(bytes, 0x10), level: u16::get(bytes, 0x14) } }

/// 0xf35294 and 0xf2457c set the record's word at +0x24 and write it back with 0x758300.
pub fn put_rate_selection(bytes: &mut [u8; 40], selected: u32) { selected.put(bytes, 0x24) }

/// The 76-byte exposure record 0x7588e8 reads and 0x75890c writes: the gain step (0xf332b8),
/// the lens and its column (0xf34548..0xf34554), the primary pair (0xf3380c..0xf33814) and
/// the selection tail with its last halfword (0xf3369c..0xf336b4).
pub fn update_exposure_record(bytes: &mut [u8; 76], update: ExposureRecordUpdate) {
    match update {
        ExposureRecordUpdate::GainStep(step) => step.0.put(bytes, 0x34),
        ExposureRecordUpdate::Context { lens, index } => {
            lens.put(bytes, 0x38);
            index.put(bytes, 0x3c);
        }
        ExposureRecordUpdate::PrimaryPair(pair) => [0x28, 0x2a].put(bytes, &pair),
        ExposureRecordUpdate::SelectionTail { tail, last } => {
            [0x2c, 0x2e, 0x30].put(bytes, &tail);
            last.put(bytes, 0x4a);
        }
    }
}

/// The lens snapshot 0x756840 reads, 48 bytes: the reference 0xf36760 subtracts at +0x28 and
/// the word 0xf33ef4 takes at +0x2c.
pub fn lens_snapshot_reference(bytes: &[u8; 48]) -> (u16, u32) { (u16::get(bytes, 0x28), u32::get(bytes, 0x2c)) }

/// Words of a record field, in order.
pub fn words<const N: usize, const BYTES: usize>(bytes: &[u8; BYTES]) -> [u32; N] { core::array::from_fn(|index| u32::get(bytes, 4 * index)) }

pub fn put_words<const N: usize, const BYTES: usize>(bytes: &mut [u8; BYTES], words: [u32; N]) {
    for (index, word) in words.into_iter().enumerate() {
        word.put(bytes, 4 * index);
    }
}

/// Configuration keys the acquisition reads through 0x69f8fc (0xf34c98, 0xf3515c, 0xf2457c,
/// 0xf332d0, 0xf367a4, 0xf3579c).
pub mod keys {
    /// Grid copy and extra statistics (0xf35b64, 0xf350c8).
    pub const SNAPSHOT_GRID: u32 = 0x4a07;
    pub const RATE: u32 = 0x5256;
    pub const S1_RATE: u32 = 0x5257;
    /// Bit 0 draws the rate status.
    pub const RATE_STATUS: u32 = 0x5258;
    /// Bit 7 draws the rate.
    pub const DRAWS_RATE: u32 = 0xa8;
    /// Bit 0 opens the selection gate (0xf332d0).
    pub const GATE: u32 = 0x65a6;
    pub const RATE_DIVISOR: u32 = 0x657d;
    /// Bit 3 of the halfword suppresses the eligible word (0xf353a0).
    pub const SUPPRESSES_ELIGIBLE: u32 = 0xe8;
    pub const TOP_STEPS: u32 = 0x24f;
    /// 1 leaves trace marks.
    pub const TRACES: u32 = 0x24e;
}

/// The release settings' keys (0xf34c98): signed tens of the bounds, the class weights and the
/// signed offsets.
pub const RELEASE_BOUNDS: [u32; 4] = [0x6484, 0x6485, 0x6486, 0x6487];
pub const RELEASE_WEIGHTS: [[u32; 2]; 4] = [[0x648c, 0x648d], [0x6488, 0x6489], [0x6494, 0x6495], [0x6492, 0x6493]];
pub const RELEASE_OFFSETS: [u32; 3] = [0x64a0, 0x64a1, 0x64a2];

/// The release settings from the configuration bytes `byte` reads.
pub fn release_settings(byte: impl Fn(u32) -> u8) -> ReleaseSettings {
    ReleaseSettings {
        bounds: RELEASE_BOUNDS.map(|key| byte(key) as i8),
        weights: RELEASE_WEIGHTS.map(|pair| pair.map(&byte)),
        offsets: RELEASE_OFFSETS.map(|key| byte(key) as i8),
    }
}
