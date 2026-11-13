//! Offsets from the client's dump frame base of the exposure context's trace (0xf6baa8), and the
//! work-memory table it copies.

use xm1_saclac_interface::calc::SharedAddress;

/// The last evaluation request's measured halfword pairs 0 and 2, first halfword high.
pub const MEASURED: [u32; 2] = [0x4bbc, 0x4bc0];

pub const STEP_WORDS: usize = 9;
pub const fn step(index: usize) -> u32 { 0x4a78 + 0x24 * index as u32 }

/// The snapshot's focus result.
pub const FOCUS_WORDS: usize = 12;
pub const FOCUS: u32 = 0x35dc;

/// 64 signed halfwords in the work memory.
pub const WORK_TABLE: SharedAddress = SharedAddress(0x5ffc_0140);
pub const WORK_TABLE_HALVES: usize = 64;
pub const WORK_TABLE_COPY: u32 = 0xde54;

pub const SUBJECT_COUNT: u32 = 0x1_1118;
/// Four words per subject, ten subjects.
pub const fn subject(index: usize) -> u32 { 0x1_111c + 0x10 * index as u32 }
/// Ten words after the subjects: the snapshot's limits and bounds.
pub const LIMITS: u32 = 0x1_11bc;
pub const LIMIT_WORDS: usize = 10;

/// The adjustment code, or 7 while the evaluation path decides.
pub const PATH: u32 = 0x4a70;
pub const SPAN_RANGE: u32 = 0x4a74;
/// Six words: the first step, the last applied candidate, start, end, first step size and step.
pub const SPAN: u32 = 0x4b98;
pub const SPAN_WORDS: usize = 6;

/// One slot per traced buffer: the row buffer, its tail and each region's four channel levels.
pub const SLOTS: usize = 8;
pub const SLOT_BUFFER_BYTES: usize = 0x1000;
pub const SLOT_TAIL_BYTES: usize = 0x10;
pub const fn slot_buffer(slot: usize) -> u32 { 0x4bc4 + 0x1000 * slot as u32 }
pub const fn slot_tail(slot: usize) -> u32 { 0xcbc4 + 0x10 * slot as u32 }
pub const CHANNEL_LEVEL_BYTES: usize = 0xa0;
pub const fn channel_level(slot: usize, region: usize, channel: usize) -> u32 { 0xe164 + 0xa0 * slot as u32 + 0x10 * region as u32 + 4 * channel as u32 }

/// Each row's region sizes, ten words a row; the clearing of unused slots indexes them by slot.
pub const REGION_BYTES: usize = 0x28;
pub const fn region(row: usize, index: usize) -> u32 { 0xe024 + 0x28 * row as u32 + 4 * index as u32 }

/// The last evaluation request's four statistics halfwords, one word each.
pub const STATISTICS: u32 = 0xcc44;

/// The statistics block a focus result names: the row buffer, its tail, then four 64-bit sums per
/// region.
pub const STATISTICS_TAIL: u32 = 0x1000;
pub const STATISTICS_SUMS: u32 = 0x1010;
/// A column buffer's four 64-bit sums per region.
pub const SUM_BYTES: u32 = 0x20;
