#![forbid(unsafe_code)]

//! The request saclac posts to calcsub (queue 46): the classification of one frame.

use crate::calc::SharedAddress;
use crate::stock::StockCallbackWord;

/// The frame's aspect ratio as calcsub reads it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AspectRatio {
    Other,
    ThreeTwo,
    SixteenNine,
    Square,
}

/// The input driver's reading (0x6a3a6c), remapped for calcsub.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Orientation {
    /// Reading 1, or no reading.
    Reading1,
    Reading2,
    Reading3,
    Reading4,
}

/// The buffers and tables calcsub reads for the frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Frame {
    /// The grid calcsub clusters (0xfb56d4).
    pub samples: SharedAddress,
    /// The image plane calcsub takes edge histograms of, rows 0x140 bytes apart (0xfb4500).
    pub plane: SharedAddress,
    /// 16 by 16 halfword levels, which calcsub averages over the rectangles that `columns` and
    /// `rows` bound (0xfb360c).
    pub levels: SharedAddress,
    /// Halfword bounds of the rectangles' columns and rows.
    pub columns: SharedAddress,
    pub rows: SharedAddress,
    /// Calcsub averages `levels` when this word is 1 (0xfb28dc..0xfb2930).
    pub average_levels: u32,
    /// The motion regions calcsub rejects candidates in (0xfb41c8): how many, and their list.
    pub motion_regions: u32,
    pub region_list: SharedAddress,
}

/// Calcsub 0x3e00: classify the frame, then enter `completion` with the result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClassificationRequest {
    pub frame: Frame,
    pub aspect: AspectRatio,
    pub orientation: Orientation,
    /// Whether calcsub clears its adaptation history first (0xfb3034..0xfb30dc).
    pub restart: bool,
    pub completion: StockCallbackWord,
}
