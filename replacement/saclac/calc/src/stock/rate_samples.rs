//! The rate sample recorder's state at 0x16d6cbc, shared with calc's rate functions, and the
//! sample buffers its words point into.

use xm1_saclac_interface::calc::SharedAddress;

const STATE: u32 = 0x16d_6cbc;

const fn at(offset: u32) -> SharedAddress { SharedAddress(STATE + offset) }

/// Set once a recording is made; cleared by setting up.
pub const MARK: SharedAddress = at(0x0);
/// Columns recorded so far, and the fewest seen.
pub const RECORDED: SharedAddress = at(0x4);
pub const FEWEST: SharedAddress = at(0x5);
pub const AT_06: SharedAddress = at(0x6);
/// The first flagged channel's group of eight, and the count of leading channels.
pub const FIRST_GROUP: SharedAddress = at(0x7);
pub const LEADING: SharedAddress = at(0x8);
/// 0xc4 bytes set up clears.
pub const CLEARED: (SharedAddress, u32) = (at(0x9), 0xc4);
pub const AT_130: SharedAddress = at(0x130);
/// Where each configuration table is kept, and its size.
pub const TABLES: [(SharedAddress, u32); 7] =
    [(at(0x134), 0x98), (at(0x1cc), 0xc4), (at(0x290), 0x6c), (at(0x2fc), 0x18), (at(0x314), 0x4), (at(0x318), 0x90), (at(0x3a8), 0x218)];
/// The channel table: one flag byte every eight bytes, then the extra sample count and the
/// marker word.
pub const CHANNELS: SharedAddress = at(0x134);
pub const CHANNEL_STRIDE: u32 = 8;
pub const CHANNEL_FLAGS: usize = 64;
pub const MARKER: SharedAddress = at(0x178);
pub const EXTRA_SAMPLES: SharedAddress = at(0x17c);
/// The buffer the setup names.
pub const BUFFER: SharedAddress = at(0x5c0);
/// The results area, rounded up to a word.
pub const RESULTS: SharedAddress = at(0x6cc);

/// The buffer words of the sample arrays, and their offsets in the buffer.
pub const ARRAYS: [(SharedAddress, u32); 15] = [
    (at(0x690), 0x4),
    (at(0x694), 0x1d0a4),
    (at(0x698), 0x21f64),
    (at(0x69c), 0x181e4),
    (at(0x6a0), 0x336f4),
    (at(0x6a4), 0x346b4),
    (at(0x6a8), 0x35984),
    (at(0x6b4), 0x406d4),
    (at(0x6bc), 0x26e24),
    (at(0x6c0), 0x29194),
    (at(0x6c4), 0x2d874),
    (at(0x6c8), 0x2ea2c),
    (at(0x6b8), 0x2ec24),
    (at(0x6d0), 0x44c84),
    (at(0x6ac), 0x491a0),
];

/// One array per word of `ARRAYS`, in order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Array {
    Primary,
    PairFirst,
    PairSecond,
    Wide,
    StripFirst,
    StripSecond,
    Grid,
    Ring,
    HalfFirst,
    HalfSecond,
    HalfThird,
    HalfFourth,
    Records,
    Cleared,
    Tracks,
}

pub const fn array_word(array: Array) -> SharedAddress { ARRAYS[array as usize].0 }

/// One sample column: 252 words.
pub const COLUMN: u32 = 0x3f0;
/// The word every column starts with until it is filled.
pub const UNFILLED: u32 = 0xfdfd_fdfd;
pub const UNFILLED_HALF: u16 = 0xfdfd;
pub const UNFILLED_BYTE: u8 = 0xfd;

/// Strides of the arrays' channels and groups.
pub const PRIMARY_CHANNEL: u32 = 0xc0f0;
pub const PAIR_CHANNEL: u32 = 0x2760;
pub const PAIR_GROUP: u32 = 0x13b0;
pub const GRID_ROW: u32 = 0xfc0;
pub const HALF_COLUMN: u32 = 0x1f8;
pub const HALF_FIRST_CHANNEL: u32 = 0x11b8;
pub const HALF_SECOND_CHANNEL: u32 = 0x2370;
pub const TRACK_CHANNEL: u32 = 0x3720;
/// The records array's second run of columns and its last word.
pub const RECORD_EXTRAS: u32 = 0x1f80;
pub const RECORD_MARKER: u32 = 0x46e0;

/// Buffer offsets set up writes directly.
pub const BUFFER_MARKS: [u32; 4] = [0x388c4, 0x389c4, 0x38ac4, 0x38bc4];
pub const TRACK_FLAGS: u32 = 0x4fff0;
pub const TRACK_END: u32 = 0x4fffe;
pub const BUFFER_WORD: u32 = 0x50010;
pub const BUFFER_CLEARED: [(u32, u32); 2] = [(0x459e4, 0x3cc), (0x48d3c, 0x2dc)];

/// Results-area ranges set up fills, by value.
pub const RESULT_FILLS: [(u32, u8, u32); 9] = [
    (0x0, 0, 0x2a8),
    (0x6a4, 0, 0x2a8),
    (0x2a8, 0xff, 0x2a8),
    (0x94c, 0xff, 0x2a8),
    (0x550, 0, 0xaa),
    (0x5fa, 0, 0xaa),
    (0xbf4, 0, 0xaa),
    (0xc9e, 0, 0xaa),
    (0x1088, 0, 0x4cc),
];
/// Nine default rate records of six words.
pub const RATE_RECORDS: u32 = 0x1554;
pub const RATE_RECORD_COUNT: u32 = 9;
pub const DEFAULT_RATE_RECORD: [u32; 6] = [2, 0, 0x61, 4, 3, 0];
/// Three bands of four bytes and four words.
pub const BAND_BYTES: u32 = 0x162c;
pub const BAND_PATTERN: [u8; 4] = [1, 2, 1, 2];
pub const BAND_WORDS: u32 = 0x1638;

/// One array the extremes scan covers: its columns by (stride, count) from the outermost level,
/// and in the results area the threshold-and-index bytes and value words of its maxima and
/// minima, one per column in that order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExtremesLayout {
    pub array: Array,
    pub levels: [(u32, u32); 3],
    pub maxima: (u32, u32),
    pub minima: (u32, u32),
}

pub const EXTREMES: [ExtremesLayout; 3] = [
    ExtremesLayout { array: Array::Primary, levels: [(0, 1), (0xc0f0, 2), (COLUMN, 49)], maxima: (0x550, 0x0), minima: (0x5fa, 0x2a8) },
    ExtremesLayout { array: Array::PairFirst, levels: [(0x2760, 2), (0x13b0, 2), (COLUMN, 5)], maxima: (0x5b2, 0x188), minima: (0x65c, 0x430) },
    ExtremesLayout { array: Array::Wide, levels: [(0, 1), (0x2760, 2), (COLUMN, 10)], maxima: (0x5ca, 0x1e8), minima: (0x674, 0x490) },
];
