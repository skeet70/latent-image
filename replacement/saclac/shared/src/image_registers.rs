#![forbid(unsafe_code)]

//! The image pipeline's register groups: restoring them from the register snapshot and packing
//! a configuration into the snapshot.

/// The fifteen groups, in the order the pipeline restores and packs them (0xda2354, 0xda21cc).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegisterGroup {
    Group0,
    Group1,
    Group2,
    Group3,
    Group4,
    Group5,
    Group6,
    Group7,
    Group8,
    Group9,
    Group10,
    Group11,
    Group12,
    Group13,
    Group14,
}

impl RegisterGroup {
    pub const ALL: [Self; 15] = [
        Self::Group0,
        Self::Group1,
        Self::Group2,
        Self::Group3,
        Self::Group4,
        Self::Group5,
        Self::Group6,
        Self::Group7,
        Self::Group8,
        Self::Group9,
        Self::Group10,
        Self::Group11,
        Self::Group12,
        Self::Group13,
        Self::Group14,
    ];

    pub const fn index(self) -> usize {
        match self {
            Self::Group0 => 0,
            Self::Group1 => 1,
            Self::Group2 => 2,
            Self::Group3 => 3,
            Self::Group4 => 4,
            Self::Group5 => 5,
            Self::Group6 => 6,
            Self::Group7 => 7,
            Self::Group8 => 8,
            Self::Group9 => 9,
            Self::Group10 => 10,
            Self::Group11 => 11,
            Self::Group12 => 12,
            Self::Group13 => 13,
            Self::Group14 => 14,
        }
    }
}

/// Group 2's three limits, signed halfwords at configuration bytes 0x3e, 0x40 and 0x42.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowLimits(pub [i16; 3]);

/// Group 2's snapshot words at bytes 0x3c and 0x40 with `limits` packed into them (0x691e28).
pub fn pack_window_limits(limits: WindowLimits, words: [u32; 2]) -> [u32; 2] {
    let field = |limit: i16| if limit < 0 { 0x7fff } else { limit as u32 };
    let [first, second, third] = limits.0;
    let [low, high] = words;
    [
        low & 0x8000_8000 | field(first) | field(second) << 16,
        high & !0x7fff | field(third),
    ]
}

pub trait RegisterSnapshot {
    type Configuration: Copy;

    /// Copies `group`'s words from the snapshot into the group's registers.
    fn restore(&mut self, group: RegisterGroup);
    fn window_limits(&self, configuration: Self::Configuration) -> WindowLimits;
    fn window_words(&self) -> [u32; 2];
    fn set_window_words(&mut self, words: [u32; 2]);
    /// Packs `group`'s fields of `configuration` into the snapshot (0x69408c..0x6942bc).
    fn pack(&mut self, group: RegisterGroup, configuration: Self::Configuration);
}
