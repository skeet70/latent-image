#![forbid(unsafe_code)]

//! The primary statistics the live acquisition holds between collections, and their release:
//! each value classed against the acquisition level and all four buffers moved by one offset.

use crate::pipeline::PipelineWord;

use super::ports::{ReleaseSettings, StatisticsBuffer};

/// Statistics values per buffer.
pub const VALUES: usize = 256;

/// The four statistics buffers and the exposure they were measured at.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StatisticsSet {
    pub buffers: [StatisticsBuffer; 4],
    pub exposure: PipelineWord,
    pub level: u16,
    /// The offset and second level together.
    pub offset: u16,
    pub aspect: u16,
    pub halfword: u16,
    pub gain_offset: u16,
    pub correction: u16,
    /// Its low halfword counts the extra statistics.
    pub count: u32,
    /// The selection result's selected word.
    pub selected: u32,
}

/// Where a statistics value falls about the acquisition level.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Class { Within = 0, Above = 1, Below = 2, High = 3, Low = 4 }

/// A release: each primary value's class, the class counts weighted, and the offset all four
/// buffers move by.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Classification {
    pub classes: [u8; VALUES],
    /// Weighted sums of the classes above and below the band, two of each.
    pub weights: [u16; 4],
}

/// Adjustment modes whose release takes the first configured offset.
const FIRST_OFFSET_MODES: u32 = 0x03c1_8070;

/// The configured release offset for an adjustment mode and class profile.
pub fn release_offset(settings: &ReleaseSettings, adjustment: u32, class_profile: u32) -> u16 {
    let index = if adjustment <= 25 && FIRST_OFFSET_MODES >> adjustment & 1 != 0 {
        0
    } else if class_profile == 1 {
        2
    } else {
        1
    };
    settings.offsets[index] as i16 as u16
}

/// Classes each value's distance from `level` against the tens the settings bound it by.
pub fn classify(values: &[u16; VALUES], level: u16, settings: &ReleaseSettings) -> Classification {
    let bound = |index: usize| (i16::from(settings.bounds[index])).wrapping_mul(10);
    let (lower, lowest, upper, highest) = (bound(0), bound(1), bound(2), bound(3));
    let mut classes = [0; VALUES];
    let mut counts = [0u16; 5];
    for (class, &value) in classes.iter_mut().zip(values) {
        let distance = value.wrapping_sub(level) as i16;
        let found = if lower <= distance && distance <= upper {
            Class::Within
        } else if upper < distance && distance <= highest {
            Class::Above
        } else if highest < distance {
            Class::High
        } else if lowest <= distance && distance < lower {
            Class::Below
        } else {
            Class::Low
        };
        *class = found as u8;
        counts[found as usize] = counts[found as usize].wrapping_add(1);
    }
    let weigh = |pair: [u8; 2], first: u16, second: u16| {
        first.wrapping_mul(u16::from(pair[0])).wrapping_add(second.wrapping_mul(u16::from(pair[1])))
    };
    let [_, above, below, high, low] = counts;
    let weights = [
        weigh(settings.weights[0], above, high),
        weigh(settings.weights[1], below, low),
        weigh(settings.weights[2], above, high),
        weigh(settings.weights[3], below, low),
    ];
    Classification { classes, weights }
}

/// `values` moved by `offset`, wrapping.
pub fn offset_values(values: &[u16; VALUES], offset: u16) -> [u16; VALUES] {
    values.map(|value| value.wrapping_add(offset))
}
