//! The image-4 lens configuration and lens control services the search's and the drive's
//! adjustments read.

/// The table a resource's bias and position are read from, 0x747374's and 0x7479c8's second
/// argument: 0 for the area setup, 1 stepping to lower resources, 2 stepping to higher ones.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BiasTable {
    Area,
    Descending,
    Ascending,
}

pub trait LensConfiguration {
    /// 0x747d48.
    fn channel_value(&mut self, channel: i32) -> u32;
    /// 0x748818.
    fn channel_curve(&mut self, channel: i32) -> u32;
    /// 0x747374.
    fn bias(&mut self, resource: i32, table: BiasTable) -> i16;
    /// The resource's base position plus its bias (0x7479c8).
    fn biased_position(&mut self, resource: i32, table: BiasTable) -> i16;
    /// 1 to 100 (0x747b50).
    fn resource_count(&mut self) -> u8;
    /// 0x748398 with second argument 0.
    fn selection_word(&mut self, channel: i32, flags: u32, offset: i16) -> u32;
    /// 0x74947c with arguments 3, 5 and 6 at 0, 0 and 1.
    fn remap(&mut self, word: u32, channel: i32, mode: u32) -> i32;
    /// The lower and upper position limits (0x74a244, 0x74a298).
    fn position_limits(&mut self, key: u32) -> [i16; 2];
    /// 0x746864 with second argument 0.
    fn mapped_value(&mut self, key: u32) -> u32;
}

pub trait LensControl {
    /// 0x76da84.
    fn position(&mut self) -> u16;
    /// Configuration value 0x3e0243 (0x76da94).
    fn travel_margin(&mut self) -> u16;
}

/// The profile service's reference scale for a group's entry (0x1007e44).
pub trait ReferenceScale {
    fn reference_scale(&mut self, group: u16, entry: u8) -> u32;
}
