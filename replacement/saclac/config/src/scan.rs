#![forbid(unsafe_code)]

//! The scan configuration saclac builds for the scan library other tasks share (image 5,
//! 0xd5dfc4..0xd79364): the parts applied from a request block, each marked in the
//! configuration's applied-parts word, the window the window part copies, and the tables the
//! library reads back from the configuration.

use crate::compiled::Optioned;

/// The configuration's applied-parts word, a bit from 16 up for each part applied.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AppliedParts(pub u32);

/// The configuration and request blocks of one scan, in the layout the scan library reads.
pub trait ScanBlocks {
    fn clear_configuration(&mut self);
    /// Request byte `index` of the part enables.
    fn request_flag(&self, index: u8) -> u8;
    fn applied_parts(&self) -> AppliedParts;
    fn mark_applied(&mut self, parts: AppliedParts);
    /// 0xd5e1b8, which 0xedc61c enters on the applied-parts word after restoring the groups.
    fn clear_applied_parts(&mut self);
    /// The window limits the window part copies into the request (configuration +0x1c8..+0x1cc).
    fn configured_window(&self) -> [u16; 3];
    fn set_configured_window(&mut self, limits: [u16; 3]);
    /// The request's window limits (bytes 0x3e, 0x40, 0x42).
    fn set_request_window(&mut self, limits: [u16; 3]);
}

/// The window limits the window part copies into the request (0xd7914c).
pub fn set_window(blocks: &mut impl ScanBlocks, limits: [u16; 3]) { blocks.set_configured_window(limits) }

/// 0xd77abc.
pub fn clear(blocks: &mut impl ScanBlocks) { blocks.clear_configuration() }

/// The scan library's entries the configuration parts, defaults and reads call.
pub trait ScanLibrary<B: ScanBlocks> {
    /// A caller's block the library fills, in the library's layout.
    type Output;

    /// The stock entries of `part`, in order, entered with the configuration and request.
    fn apply(&mut self, part: StockPart, blocks: &mut B);
    /// 0xd79178 then 0xd79194, which finish the defaults.
    fn finish_defaults(&mut self, blocks: &mut B);
    /// 0xd64840.
    fn fill_settings(&mut self, blocks: &B, output: &mut Self::Output);
    /// 0xd649e8, whose two scratch outputs are dropped.
    fn fill_geometry(&mut self, blocks: &B, output: &mut Self::Output);
    /// 0xd64940.
    fn nine(&mut self, blocks: &B) -> Optioned<9>;
    /// 0xd64e2c.
    fn four(&mut self, blocks: &B) -> Optioned<4>;
}

/// 0xd5dfec.
pub fn nine_values<B: ScanBlocks>(library: &mut impl ScanLibrary<B>, blocks: &B) -> [u16; 9] { library.nine(blocks).values }

/// 0xd5e120.
pub fn four_values<B: ScanBlocks>(library: &mut impl ScanLibrary<B>, blocks: &B) -> [u16; 4] { library.four(blocks).values }

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
            let window = blocks.configured_window();
            blocks.set_request_window(window);
        }
    }
    blocks.mark_applied(AppliedParts(bit));
    true
}
