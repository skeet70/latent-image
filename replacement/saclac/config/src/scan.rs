#![forbid(unsafe_code)]

//! The scan configuration saclac builds for the scan library other tasks share (image 5,
//! 0xd5dfc4..0xd79364): the configuration block's settings and defaults, and the parts applied
//! from a request block, each marked in the configuration's applied-parts word.

/// A configuration field by its stock offset; their meanings are not yet named by any caller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScanField {
    WordDc,
    WordE0,
    WordE4,
    WordE8,
    WordEc,
    WordF0,
    WordF4,
    WordF8,
    WordFc,
    Word100,
    Word104,
    Word108,
    Word10c,
    Word110,
    Word114,
    Half118,
    Byte11a,
    Byte11b,
    Word11c,
    Word124,
    Word128,
    Word13c,
    Word140,
    Word144,
    Word148,
    Word160,
    Word164,
    Half168,
    Half16a,
    Byte16c,
    Word17c,
    Word180,
    Word184,
    Word188,
    Word18c,
    Byte190,
    Word194,
    Half1a0,
    Byte1a2,
    Half1a4,
    Word1a8,
    Byte1ac,
    Byte1ad,
    Byte1ae,
    Byte1af,
    Word1b4,
    Word1bc,
    Half1c8,
    Half1ca,
    Half1cc,
    Word1d0,
    Word1d4,
    Word1d8,
    Word1dc,
    Word1e0,
    Word1e4,
    Half1e8,
    Half1ea,
}

/// The configuration and request blocks of one scan, in the layout the scan library reads.
pub trait ScanBlocks {
    fn clear_configuration(&mut self);
    fn set(&mut self, field: ScanField, value: u32);
    fn configuration(&self, field: ScanField) -> u32;
    /// Request byte `index` of the part enables.
    fn request_flag(&self, index: u8) -> u8;
    fn mark_applied(&mut self, bits: u32);
    /// The request's window limits (bytes 0x3e, 0x40, 0x42).
    fn set_request_window(&mut self, limits: [u16; 3]);
}

/// 0xd78400.
pub fn set_ec(blocks: &mut impl ScanBlocks, value: u32) { blocks.set(ScanField::WordEc, value) }

/// Values above 7 become 8 (0xd784ac).
pub fn set_140(blocks: &mut impl ScanBlocks, value: u32) { blocks.set(ScanField::Word140, value.min(8)) }

/// 0xd784cc.
pub fn set_144(blocks: &mut impl ScanBlocks, enabled: bool) { blocks.set(ScanField::Word144, u32::from(enabled)) }

/// 0xd78674.
pub fn set_11a(blocks: &mut impl ScanBlocks, value: u8) { blocks.set(ScanField::Byte11a, u32::from(value)) }

/// Values above 8 become 9 (0xd7868c).
pub fn set_13c(blocks: &mut impl ScanBlocks, value: u32) { blocks.set(ScanField::Word13c, value.min(9)) }

/// Values above 2 become 3 (0xd789f0).
pub fn set_148(blocks: &mut impl ScanBlocks, value: u32) { blocks.set(ScanField::Word148, value.min(3)) }

/// The window limits the window part copies into the request (0xd7914c).
pub fn set_window(blocks: &mut impl ScanBlocks, limits: [u16; 3]) {
    for (field, limit) in [ScanField::Half1c8, ScanField::Half1ca, ScanField::Half1cc].into_iter().zip(limits) {
        blocks.set(field, u32::from(limit));
    }
}

/// Values above 4 become 0 (0xd79364).
pub fn set_1dc(blocks: &mut impl ScanBlocks, value: u32) { blocks.set(ScanField::Word1dc, if value > 4 { 0 } else { value }) }

/// 0xd77abc.
pub fn clear(blocks: &mut impl ScanBlocks) { blocks.clear_configuration() }

/// The defaults 0xd791b0 stores, in its order.
const DEFAULTS: [(ScanField, u32); 54] = [
    (ScanField::WordEc, 0),
    (ScanField::WordE4, 0),
    (ScanField::WordDc, 1),
    (ScanField::WordE0, 0),
    (ScanField::WordF0, 0),
    (ScanField::WordF4, 2),
    (ScanField::WordF8, 2),
    (ScanField::WordFc, 2),
    (ScanField::Word100, 2),
    (ScanField::Word104, 0),
    (ScanField::Word108, 0),
    (ScanField::Word10c, 2),
    (ScanField::Word110, 0),
    (ScanField::Word114, 2),
    (ScanField::Byte11b, 1),
    (ScanField::Word11c, 600),
    (ScanField::Half118, 0),
    (ScanField::Byte11a, 1),
    (ScanField::Word124, 1),
    (ScanField::Word160, 0),
    (ScanField::Word164, 0),
    (ScanField::Half168, 0),
    (ScanField::Half16a, 0),
    (ScanField::Byte16c, 0),
    (ScanField::Word180, 2),
    (ScanField::Word184, 0),
    (ScanField::Word188, 0),
    (ScanField::Word17c, 1),
    (ScanField::Word18c, 0),
    (ScanField::Byte190, 0),
    (ScanField::Word194, 2),
    (ScanField::Word128, 0),
    (ScanField::Half1a0, 0x2ba),
    (ScanField::Byte1a2, 0),
    (ScanField::Half1a4, 0x7e),
    (ScanField::Byte1ac, 0),
    (ScanField::Word1a8, 0),
    (ScanField::Byte1ad, 0),
    (ScanField::Byte1ae, 0),
    (ScanField::Byte1af, 0),
    (ScanField::Word1bc, 1),
    (ScanField::Word1e0, 0),
    (ScanField::WordE8, 0),
    (ScanField::Half1c8, 0),
    (ScanField::Half1ca, 0),
    (ScanField::Half1cc, 0),
    (ScanField::Word1d0, 0),
    (ScanField::Word1d4, 0),
    (ScanField::Word1d8, 0),
    (ScanField::Word1b4, 0),
    (ScanField::Word1dc, 0),
    (ScanField::Word1e4, 0),
    (ScanField::Half1e8, 0),
    (ScanField::Half1ea, 0),
];

/// The scan library's entries the configuration parts and defaults call.
pub trait ScanLibrary<B: ScanBlocks> {
    /// The stock entries of `part`, in order, entered with the configuration and request.
    fn apply(&mut self, part: StockPart, blocks: &mut B);
    /// 0xd79178 then 0xd79194, which finish the defaults.
    fn finish_defaults(&mut self, blocks: &mut B);
}

/// Stores the defaults and lets the library finish them (0xd791b0).
pub fn set_defaults<B: ScanBlocks>(blocks: &mut B, library: &mut impl ScanLibrary<B>) {
    for (field, value) in DEFAULTS {
        blocks.set(field, value);
    }
    library.finish_defaults(blocks);
}

/// The scan library's part entries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StockPart {
    /// 0xd6415c with the request bytes from 6, 0xf and 0x13.
    Geometry,
    /// 0xd64424.
    Second,
    /// 0xd64800 then 0xd64914.
    Fourth,
    /// 0xd649b4.
    Fifth,
    /// 0xd64b5c.
    Sixth,
    /// 0xd656ac.
    Eighth,
    /// 0xd653e8.
    Ninth,
    /// 0xd6541c.
    Tenth,
    /// 0xd65524.
    Eleventh,
    /// 0xd655f4.
    Twelfth,
    /// 0xd6579c then 0xd65834.
    Pair,
    /// 0xd658a0.
    Sixteenth,
    /// 0xd659bc, 0xd65ce4, 0xd65e90, 0xd65ee8, 0xd65f94, 0xd660fc.
    Seventeenth,
    /// 0xd662a0.
    Eighteenth,
    /// 0xd663e0.
    Nineteenth,
}

/// A configuration part: the request byte that enables it, the stock entries that apply it and
/// the applied-parts bit it sets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Part {
    /// Always applied (0xd5e208).
    Geometry,
    /// 0xd5e258.
    Second,
    /// Copies the configured window into the request (0xd5e284).
    Window,
    /// 0xd5e2d4.
    Fourth,
    /// 0xd5e310.
    Fifth,
    /// 0xd5e33c.
    Sixth,
    /// 0xd5e368.
    Eighth,
    /// 0xd5e394.
    Ninth,
    /// 0xd5e3c0.
    Tenth,
    /// 0xd5e3ec.
    Eleventh,
    /// 0xd5e418.
    Twelfth,
    /// Always applied (0xd5e444).
    Pair,
    /// 0xd5e474.
    Sixteenth,
    /// Enabled by any nonzero byte (0xd5e4a0).
    Seventeenth,
    /// 0xd5e50c.
    Eighteenth,
    /// Enabled by any nonzero byte, and marks the same bit as `Eighteenth` (0xd5e538).
    Nineteenth,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Enable {
    Always,
    One(u8),
    Nonzero(u8),
}

impl Part {
    const fn plan(self) -> (Enable, Option<StockPart>, u32) {
        match self {
            Self::Geometry => (Enable::Always, Some(StockPart::Geometry), 1 << 16),
            Self::Second => (Enable::One(1), Some(StockPart::Second), 1 << 17),
            Self::Window => (Enable::One(2), None, 1 << 18),
            Self::Fourth => (Enable::One(3), Some(StockPart::Fourth), 1 << 20),
            Self::Fifth => (Enable::One(4), Some(StockPart::Fifth), 1 << 21),
            Self::Sixth => (Enable::One(5), Some(StockPart::Sixth), 1 << 22),
            Self::Eighth => (Enable::One(7), Some(StockPart::Eighth), 1 << 23),
            Self::Ninth => (Enable::One(8), Some(StockPart::Ninth), 1 << 24),
            Self::Tenth => (Enable::One(9), Some(StockPart::Tenth), 1 << 25),
            Self::Eleventh => (Enable::One(10), Some(StockPart::Eleventh), 1 << 26),
            Self::Twelfth => (Enable::One(11), Some(StockPart::Twelfth), 1 << 27),
            Self::Pair => (Enable::Always, Some(StockPart::Pair), 1 << 19),
            Self::Sixteenth => (Enable::One(15), Some(StockPart::Sixteenth), 1 << 28),
            Self::Seventeenth => (Enable::Nonzero(16), Some(StockPart::Seventeenth), 1 << 29),
            Self::Eighteenth => (Enable::One(17), Some(StockPart::Eighteenth), 1 << 30),
            Self::Nineteenth => (Enable::Nonzero(18), Some(StockPart::Nineteenth), 1 << 30),
        }
    }
}

/// Applies `part` when its request byte enables it and marks it applied; returns whether it was.
pub fn apply<B: ScanBlocks>(part: Part, blocks: &mut B, library: &mut impl ScanLibrary<B>) -> bool {
    let (enable, stock, bit) = part.plan();
    let enabled = match enable {
        Enable::Always => true,
        Enable::One(index) => blocks.request_flag(index) == 1,
        Enable::Nonzero(index) => blocks.request_flag(index) != 0,
    };
    if !enabled {
        return false;
    }
    match stock {
        Some(stock) => library.apply(stock, blocks),
        None => {
            let window = [ScanField::Half1c8, ScanField::Half1ca, ScanField::Half1cc].map(|field| blocks.configuration(field) as u16);
            blocks.set_request_window(window);
        }
    }
    blocks.mark_applied(bit);
    true
}
