//! X-M1 1.50 (id5) layouts of the library's diagnostics: the adjustment layer's joint mapping
//! tables and sample history it exports, the calc task's dispatch state, and the client's dump
//! frame sections the exports and clears write.

use xm1_saclac_interface::calc::SharedAddress;

use crate::exposure::ports::Property;

/// Bit 7 of property 0xa8 enables the dump frame's diagnostics (0xf60378, 0xf604c0).
pub const DUMP_ENABLED: u32 = 0x80;

/// The word calc's task passes its dispatcher with every message (0xf49558, 0xf49588).
pub const CALC_DISPATCH_STATE: SharedAddress = SharedAddress(0x016c_2370);

/// How a source field widens to the dump's word.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Width {
    Byte,
    SignedByte,
    Half,
    SignedHalf,
    Word,
}

impl Width {
    pub const fn bytes(self) -> u32 {
        match self {
            Self::Byte | Self::SignedByte => 1,
            Self::Half | Self::SignedHalf => 2,
            Self::Word => 4,
        }
    }
}

/// The joint mapping tables the adjustment layer keeps (0xfb2438..0xfb35d8), two blocks.
pub const JOINT_TABLES: SharedAddress = SharedAddress(0x016c_73f0);
pub const JOINT_BLOCK_BYTES: u32 = 0x1d74;
pub const JOINT_SCALARS: u32 = 0;
pub const JOINT_SCALAR_COUNT: u32 = 300;
pub const JOINT_MAPPINGS: u32 = 0x12c;
pub const JOINT_MAPPING_BYTES: u32 = 24;
pub const JOINT_ENTRIES: u32 = 256;
pub const JOINT_TRIPLES: u32 = 0x192c;
pub const JOINT_EXTRAS: u32 = 0x1c2c;
pub const JOINT_EXTRA_COUNT: u32 = 300;
pub const JOINT_WORDS: u32 = 0x1d58;
pub const JOINT_WORD_COUNT: u32 = 7;

/// A mapping's fields in their export order; byte 7 is not exported (0xf4eda0..0xf4ee1c).
pub const JOINT_MAPPING_FIELDS: [(u32, Width); 14] = [
    (0, Width::Byte),
    (1, Width::Byte),
    (2, Width::Half),
    (4, Width::Byte),
    (5, Width::SignedByte),
    (6, Width::SignedByte),
    (8, Width::SignedHalf),
    (10, Width::Half),
    (12, Width::Half),
    (14, Width::Half),
    (16, Width::Half),
    (18, Width::Half),
    (20, Width::Half),
    (22, Width::Half),
];

/// The first block's extras as saclac's start leaves them (0xf4c39c).
pub const JOINT_EXTRA_UNSET: u8 = 0xe0;

/// The joint tables' place in the dump frame: a mark, every exported field as a word, block by
/// block, and an end mark (0xf4ed28).
pub const JOINT_EXPORT: u32 = 0xa_7a20;
pub const JOINT_EXPORT_BYTES: usize = 0x9b00;
pub const JOINT_MARK: u32 = 0x5354_4f4a;
pub const JOINT_END: u32 = JOINT_EXPORT + 0x9afc;
pub const JOINT_END_MARK: u32 = 0x4c41_4f4a;

/// The sample history the adjustment layer keeps, read after invalidating its cache lines
/// (0xf60504..0xf60514): a byte the dump copies, the slot count, three 20-entry halfword tables
/// and two 20-entry tables of 76-byte samples.
pub const SAMPLE_HISTORY: SharedAddress = SharedAddress(0x016d_47a0);
pub const SAMPLE_HISTORY_BYTES: usize = 0xc5c;
pub const SAMPLE_SLOTS: u8 = 20;
pub const SAMPLE_COUNT: u32 = 1;
pub const SAMPLE_REFERENCES: u32 = 2;
pub const SAMPLE_LEVELS: [u32; 2] = [42, 82];
pub const SAMPLES: [u32; 2] = [0x7c, 0x66c];
pub const SAMPLE_BYTES: u32 = 76;

/// A sample's fields in their export order, each with its offset in the 0x7c-byte record
/// (0xf605fc..0xf606f4).
pub const SAMPLE_FIELDS: [(u32, u32, Width); 28] = [
    (0x08, 0, Width::SignedHalf),
    (0x0c, 2, Width::SignedHalf),
    (0x10, 4, Width::SignedHalf),
    (0x14, 6, Width::SignedHalf),
    (0x18, 8, Width::SignedHalf),
    (0x1c, 10, Width::SignedHalf),
    (0x20, 12, Width::SignedHalf),
    (0x24, 14, Width::SignedHalf),
    (0x28, 16, Width::SignedHalf),
    (0x2c, 18, Width::Half),
    (0x30, 20, Width::SignedHalf),
    (0x34, 22, Width::SignedHalf),
    (0x38, 24, Width::SignedHalf),
    (0x3c, 26, Width::SignedHalf),
    (0x40, 30, Width::Half),
    (0x44, 32, Width::Half),
    (0x48, 34, Width::Half),
    (0x4c, 36, Width::SignedHalf),
    (0x50, 38, Width::SignedHalf),
    (0x54, 40, Width::SignedHalf),
    (0x58, 44, Width::Word),
    (0x5c, 48, Width::SignedHalf),
    (0x64, 56, Width::Half),
    (0x68, 58, Width::Half),
    (0x6c, 60, Width::Word),
    (0x70, 64, Width::Word),
    (0x74, 68, Width::SignedHalf),
    (0x78, 72, Width::Word),
];

/// The dump frame's sample records: the copied byte, then one area of 20 records of 0x7c bytes
/// per pass: the pass's level and the reference at +0 and +4, the setting at +0x60, the sample's
/// fields (0xf60518..0xf606f4).
pub const SAMPLE_MARK: u32 = 0xfdb4;
pub const SAMPLE_AREAS: [u32; 2] = [0xfdb8, 0x1_0768];
pub const SAMPLE_RECORD_BYTES: u32 = 0x7c;
pub const SAMPLE_AREA_BYTES: usize = 0x9b0;
pub const SAMPLE_LEVEL: u32 = 0;
pub const SAMPLE_REFERENCE: u32 = 4;
pub const SAMPLE_SETTING_AT: u32 = 0x60;
pub const SAMPLE_SETTING: Property = Property(0x6556);

/// What a correction export writes for one correction value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Term {
    Signed(usize),
    Unsigned(usize),
    /// The half moved from the correction's base level to 500.
    Relative(usize),
    /// The word whose low half is the index.
    Word(usize),
    /// 0x69fffc.
    Level(Property),
    /// 0x69ff68.
    SignedByte(Property),
    Setting(Property),
}

/// The correction's base level, which the dump keeps as is (0xf60770..0xf60784).
pub const CORRECTION_BASE: (u32, usize) = (0xfd78, 2);

/// The correction export's frame words before its rate, in store order (0xf6078c..0xf608f8).
pub const CORRECTION_TERMS: [(u32, Term); 33] = [
    (0xfd7c, Term::Relative(3)),
    (0xfd80, Term::Relative(4)),
    (0xfd84, Term::Relative(5)),
    (0xfd88, Term::Relative(6)),
    (0xfd8c, Term::Relative(7)),
    (0xfd90, Term::Relative(8)),
    (0xfd94, Term::Relative(9)),
    (0xfd98, Term::Relative(10)),
    (0xfd9c, Term::Relative(11)),
    (0xfda0, Term::Relative(12)),
    (0xfda4, Term::Relative(13)),
    (0xfda8, Term::Relative(14)),
    (0xfdac, Term::Relative(16)),
    (0xfdb0, Term::Relative(17)),
    (0x1_15f8, Term::Signed(50)),
    (0x1_15fc, Term::Signed(51)),
    (0x1_1600, Term::Signed(52)),
    (0x1_1604, Term::Unsigned(53)),
    (0x1_1608, Term::Signed(54)),
    (0x1_160c, Term::Signed(55)),
    (0x1_1610, Term::Signed(56)),
    (0x1_1614, Term::Word(18)),
    (0x1_1618, Term::Word(20)),
    (0x1_161c, Term::Relative(23)),
    (0x1_1620, Term::Relative(34)),
    (0x1_1624, Term::Relative(35)),
    (0x1_1628, Term::Signed(22)),
    (0x1_162c, Term::Signed(24)),
    (0x1_1630, Term::Relative(25)),
    (0x1_1638, Term::Level(Property(0x6585))),
    (0x1_163c, Term::Level(Property(0x4a00))),
    (0x1_1640, Term::Unsigned(26)),
    (0x1_1644, Term::Unsigned(27)),
];

/// Where the rate setting and the rate go, and the setting without a rate (0xf608fc..0xf60944).
pub const RATE_SETTING_AT: u32 = 0x1_1648;
pub const RATE_AT: u32 = 0x1_1634;
pub const RATED_SETTING: Property = Property(0x746b);
pub const UNRATED_SETTING: Property = Property(0x6584);
pub const RATE_HALF: usize = 22;
pub const NO_RATE: u32 = u32::MAX;

/// The correction export's frame words after its rate (0xf60948..0xf6095c).
pub const CORRECTION_TAIL: [(u32, Term); 2] = [(0x1_164c, Term::Relative(30)), (0x1_1650, Term::Signed(28))];

/// Two relative halves and their difference (0xf60958..0xf6097c).
pub const CORRECTION_SPAN: [(u32, usize); 2] = [(0x1_1654, 29), (0x1_1658, 33)];
pub const CORRECTION_SPAN_WIDTH: u32 = 0x1_165c;

/// The configuration the export copies (0xf60980..0xf60a34).
pub const CORRECTION_SETTINGS: [(u32, Term); 11] = [
    (0xf890, Term::Setting(Property(0x659a))),
    (0xf894, Term::SignedByte(Property(0x659d))),
    (0xf898, Term::SignedByte(Property(0x659e))),
    (0xf89c, Term::SignedByte(Property(0x659f))),
    (0xf8a0, Term::SignedByte(Property(0x65a0))),
    (0xf8a4, Term::SignedByte(Property(0x65a1))),
    (0xf8a8, Term::SignedByte(Property(0x65a2))),
    (0xf8ac, Term::Setting(Property(0x4a02))),
    (0xf8b0, Term::Setting(Property(0x4a03))),
    (0xf8b4, Term::SignedByte(Property(0x4a04))),
    (0xf8b8, Term::SignedByte(Property(0x4a05))),
];

/// Ten subject levels, relative halves from half 38, zero past the subject count
/// (0xf60a38..0xf60ab0).
pub const SUBJECT_LEVELS: u32 = 0xfd40;
pub const SUBJECT_LEVEL_HALVES: usize = 38;
pub const SUBJECT_LEVEL_COUNT: usize = 10;

/// The level the relative halves are moved to.
pub const RELATIVE_BASE: i32 = 500;

/// The sections a primary exposure's clear zeroes (0xf60388..0xf603e8).
pub const PRIMARY_SELECTION: [(u32, usize); 5] = [(0x1_0768, 0x9b0), (0xfd78, 0x3c), (0x1_15f8, 0x1c), (0x1_1614, 0x4c), (0xfd40, 0x28)];
/// The sections another exposure's clear zeroes (0xf603f0..0xf60494).
pub const SECONDARY_SELECTION: [(u32, usize); 8] =
    [(0xdfa8, 0x7c), (0xdf54, 0x3c), (0xf824, 0x1c), (0xf840, 0x4c), (0xe664, 0x28), (0xe68c, 0x28), (0xe6b4, 0x28), (0xe6dc, 0x28)];

/// The rate status the debug overlay shows (0xf521d8).
pub const STATUS_STILL: &[u8] = b"STIL  ";
pub const STATUS_ACTIVE: &[u8] = b"ACT   ";
pub const STATUS_NO_FOCUS: &[u8] = b"NO F  ";
pub const STATUS_FAILED: &[u8] = b"FAIL  ";
pub const STATUS_BLANK: &[u8] = b"----- ";
pub const COLOUR_STILL: u8 = 7;
pub const COLOUR_ACTIVE: u8 = 10;
pub const COLOUR_ALERT: u8 = 4;
pub const COLOUR_BLANK: u8 = 7;
pub const STATUS_FOREGROUND: u8 = 1;
pub const STATUS_COLUMNS: [u8; 2] = [2, 8];
pub const RATE_ROW: u8 = 5;
pub const STATUS_ROW: u8 = 6;
pub const RATE_WIDTH: usize = 6;
