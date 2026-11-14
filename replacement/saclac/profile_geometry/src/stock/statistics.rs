#![forbid(unsafe_code)]

//! X-M1 1.50 layouts of the statistics summary in the shot record 0xedb05c writes, and of the
//! destination record the image pipeline's 0xda035c reads.

use crate::controller::statistics::{RecordHalf, StatisticsSummary, Summary};
use crate::ports::StatisticsDestinations;

pub const HALF_BYTES: usize = 712;
/// Where the record keeps a copy of its first half.
pub const COPY_OFFSET: usize = 0x858;
pub const RECORD_BYTES: usize = COPY_OFFSET + HALF_BYTES;

fn put(record: &mut [u8], at: usize, bytes: &[u8]) { record[at..at + bytes.len()].copy_from_slice(bytes) }

/// Writes `summary` into its half of `record`, then copies the first half when `keep_copy`, as
/// 0xee0928 does whether or not a half was written.
pub fn write_summary(record: &mut [u8; RECORD_BYTES], summary: Option<&StatisticsSummary>, keep_copy: bool) {
    if let Some(StatisticsSummary { half, summary: Some(summary) }) = summary {
        let base = match half {
            RecordHalf::First => 0,
            RecordHalf::Second => HALF_BYTES,
        };
        let half = &mut record[base..base + HALF_BYTES];
        match summary {
            Summary::Measured(measured) => {
                half[0x284] = measured.tag;
                put(half, 0x288, &[0; 8]);
                for (index, word) in measured.echoed.iter().enumerate() {
                    put(half, 0x2a0 + 4 * index, &word.to_le_bytes());
                }
                if let Some([own, other]) = measured.quadrant_averages {
                    put(half, 0x2bc, &own.to_le_bytes());
                    put(half, 0x2c0, &[0; 4]);
                    put(half, 0x2c4, &other.to_le_bytes());
                }
                if let Some(means) = measured.block_means {
                    for (index, mean) in means.iter().enumerate() {
                        put(half, 0x290 + 2 * index, &mean.to_le_bytes());
                    }
                }
                if let Some(minimum) = measured.minimum {
                    put(half, 0x2ac, &minimum.to_le_bytes());
                }
            }
            Summary::Level(level) => {
                for index in 0..4 {
                    put(half, 0x254 + 4 * index, &level.to_le_bytes());
                }
            }
        }
    }
    if keep_copy {
        record.copy_within(..HALF_BYTES, COPY_OFFSET);
    }
}

/// The record 0xda035c takes; it reads the two leading words only for stage bits from 0x80.
pub fn destination_record(destinations: &StatisticsDestinations) -> [u32; 12] {
    let [s01, s02, s04, s08, s10, s20, s40] = destinations.0.map(|stage| stage.map(|address| address.0));
    [0, 0, s01[0], s04[0], s10[0], s10[1], s20[0], s20[1], s20[2], s02[0], s08[0], s40[0]]
}
