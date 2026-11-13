#![forbid(unsafe_code)]

//! X-M1 1.50 frame layout of the statistics dump 0xf48fd8 writes (entered from 0xf16a80); the
//! armed flag is the word at 0x16c2220, which 0xf49138 shares.

pub const DUMP: u32 = 0x84f74;
pub const DUMP_BYTES: u32 = 0x17f5c;
/// "DMTS" at the dump's start, "DMAL" at +0x17f58 (0xf49024..0xf49044).
pub const START_TAG: u32 = 0x5354_4d44;
pub const END_TAG_OFFSET: u32 = DUMP + 0x17f58;
pub const END_TAG: u32 = 0x4c41_4d44;
/// Copied from the statistics source 0x5ffdb9ec (0xf490cc..0xf490e0).
pub const STATISTICS: u32 = DUMP + 0x288;
pub const STATISTICS_BYTES: u32 = 0xbe68;
/// Without a statistics source (config 0x5256 zero) the statistics area is cleared, then three
/// ranges are set to 0xff (0xf49068..0xf490bc).
pub const UNSOURCED_FILLS: [(u32, u32, u8); 4] =
    [(STATISTICS, STATISTICS_BYTES, 0), (STATISTICS, 0x2c00, 0xff), (DUMP + 0x2ea0, 0x8400, 0xff), (DUMP + 0xb2a0, 0x168, 0xff)];
