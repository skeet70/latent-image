#![forbid(unsafe_code)]

//! The statistics summary a shot record takes from the image pipeline's statistics buffers
//! (0xedb05c).

use super::{ProfileGeometry, Tables};
use crate::memory::Memory;
use crate::ports::{ConfigOffset, Ports, StatisticsBuffer, StatisticsRegion};
use crate::stock::Parameters;

const QUADRANTS: StatisticsRegion = StatisticsRegion(29);
const BLOCKS: StatisticsRegion = StatisticsRegion(13);
const LEVELS: StatisticsRegion = StatisticsRegion(3);
const LEVEL_CAP: ConfigOffset = ConfigOffset(0x258c);
const SUMMARIZED_POLICY: u32 = 2;
/// 0xef640c.
const FIXED_MULTIPLIER: i32 = 4;
const BLOCK_STRIDE: u32 = 0x40;
const LEVEL_WORDS: u32 = 0x400;

/// Which half of the shot record a profile's summary goes to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordHalf {
    First,
    Second,
}

/// What the summary is asked for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SummaryRequest {
    pub tag: u8,
    pub echoed: [u32; 3],
    /// Summarize the measured blocks rather than the level average.
    pub measured: bool,
    /// The second buffer the quadrant average reads, by record half.
    pub references: [StatisticsBuffer; 2],
}

/// The areas each average divides by, as factor pairs.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SummaryAreas {
    pub minimum: [u32; 2],
    pub level: [u32; 2],
    pub quadrant: [u32; 2],
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MeasuredSummary {
    pub tag: u8,
    pub echoed: [u32; 3],
    /// From the summarized buffer, then from the reference buffer.
    pub quadrant_averages: Option<[u32; 2]>,
    pub block_means: Option<[u16; 8]>,
    pub minimum: Option<u16>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Summary {
    Measured(MeasuredSummary),
    /// The level average, capped, which the record keeps four times.
    Level(u32),
}

/// A summary for one record half; `None` for nothing written.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StatisticsSummary {
    pub half: RecordHalf,
    pub summary: Option<Summary>,
}

impl<M: Memory, P: Ports, T: Tables> ProfileGeometry<M, P, T> {
    /// The summary of `buffer` for the record half `parameters`' profile selects; `None` for a
    /// profile with neither half (0xedb05c).
    pub fn statistics_summary(
        &mut self,
        parameters: &Parameters,
        buffer: Option<StatisticsBuffer>,
        areas: Option<&SummaryAreas>,
        request: &SummaryRequest,
    ) -> Option<StatisticsSummary> {
        let profile = parameters.profile();
        let (half, reference) = match self.profiles.flags_low(profile) {
            0 => (RecordHalf::First, request.references[0]),
            1 => (RecordHalf::Second, request.references[1]),
            _ => return None,
        };
        let summarized = self.profiles.policy(profile) == SUMMARIZED_POLICY;
        let summary = if request.measured {
            let mut measured = MeasuredSummary { tag: request.tag, echoed: request.echoed, ..MeasuredSummary::default() };
            if let (Some(buffer), true) = (buffer, summarized) {
                let count = areas.map_or(0, |areas| areas.quadrant[0].wrapping_mul(areas.quadrant[1]));
                let count = if count == 0 { 1 } else { count };
                let offset = self.ports.statistics_offset(QUADRANTS);
                let own = self.quadrant_sum(buffer, offset);
                let other = self.quadrant_sum(reference, offset);
                measured.quadrant_averages = Some([own / count, other / count]);
            }
            if let (Some(buffer), Some(areas), true) = (buffer, areas, summarized) {
                let blocks = self.ports.statistics_offset(BLOCKS);
                measured.block_means = Some(self.block_means(buffer, blocks, scaled_area(areas.quadrant)));
                let levels = self.ports.statistics_offset(LEVELS);
                let divisor = areas.minimum[0].wrapping_mul(areas.minimum[1]) / FIXED_MULTIPLIER as u32;
                let divisor = (divisor as u16).max(1) as u32;
                let minimum = (0..LEVEL_WORDS)
                    .map(|index| self.ports.statistics_word(buffer, levels + 4 * index))
                    .min()
                    .unwrap_or(u32::MAX);
                measured.minimum = Some((minimum / divisor) as u16);
            }
            Summary::Measured(measured)
        } else {
            let (Some(buffer), Some(areas), true) = (buffer, areas, summarized) else {
                return Some(StatisticsSummary { half, summary: None });
            };
            let blocks = self.ports.statistics_offset(BLOCKS);
            let divisor = scaled_area([areas.level[1], areas.level[0]]).max(1) as u64;
            let sum: u64 = (0..4).map(|index| self.ports.statistics_word(buffer, blocks + 4 * index) as u64).sum();
            let level = ((sum >> 2) / divisor) as u32;
            Summary::Level(level.min(self.ports.halfword(LEVEL_CAP) as u32))
        };
        Some(StatisticsSummary { half, summary: Some(summary) })
    }

    fn quadrant_sum(&mut self, buffer: StatisticsBuffer, offset: u32) -> u32 {
        (0..4).fold(0u32, |sum, block| sum.wrapping_add(self.ports.statistics_word(buffer, offset + BLOCK_STRIDE * block)))
    }

    /// The means of each block's first and last eight words, each word divided by `count` before
    /// it is summed. Stock reads the first eight from the region offset as an address and the
    /// last eight from address 0x20 on (0xee0b70, 0xee0c34); both are read from the buffer here.
    fn block_means(&mut self, buffer: StatisticsBuffer, offset: u32, count: u16) -> [u16; 8] {
        let count = count.max(1) as u32;
        let mut means = [0; 8];
        for (half, start) in [0u32, 0x20].into_iter().enumerate() {
            for block in 0..4 {
                let sum = (0..8).fold(0u32, |sum, word| {
                    let at = offset + start + BLOCK_STRIDE * block + 4 * word;
                    sum.wrapping_add(self.ports.statistics_word(buffer, at) / count)
                });
                means[half * 4 + block as usize] = (sum >> 3) as u16;
            }
        }
        means
    }
}

/// The product of an area's factors, as halfwords, over the fixed multiplier.
fn scaled_area(factors: [u32; 2]) -> u16 {
    let product = (factors[1] as u16 as u32).wrapping_mul(factors[0] as u16 as u32) as i32;
    (product / FIXED_MULTIPLIER) as u16
}
