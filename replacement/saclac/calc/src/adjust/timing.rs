//! The capture pipeline's line timing and the exposure level scale the adjustment layer reads.

/// The capture pipeline's timing for a pipeline source.
pub trait CaptureTiming {
    /// The line the source's readout starts at: coordinate 1 of 0x1004d04 with selector 1.
    fn readout_start(&mut self, source: u32) -> u32;
    /// The source's reference line (0x1004ce0).
    fn reference_line(&mut self, source: u32) -> u16;
    /// The line period; `None` where 0x1004d68 returns 0x85 and writes no period.
    fn line_period(&mut self, source: u32) -> Option<u32>;
}

/// Exposure levels in hundredths of a doubling.
pub trait LevelScale {
    /// The linear value of `level` (0xee9ab0).
    fn linear(&mut self, level: i16) -> u32;
    /// 0xee99f0.
    fn log2_difference(&mut self, value: u32, reference: u32) -> i16;
}
