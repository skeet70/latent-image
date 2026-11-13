#![forbid(unsafe_code)]

//! Helpers and state more than one shot step uses.

/// The capture slot a step works on: the requested slot when it is one of at most three counted
/// slots, else the first.
pub const fn active_slot(requested: u32, count: u32) -> u32 {
    if requested == 0 || count == 0 || count > 3 || requested > count { 1 } else { requested }
}

pub const SECONDARY_BLOCK_BYTES: usize = 84;

/// What the capture controller and its secondary and primary jobs share about the secondary job.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SecondaryShared {
    pub enabled: u32,
    pub markers: [u16; 2],
    pub block: [u8; SECONDARY_BLOCK_BYTES],
}

impl Default for SecondaryShared {
    fn default() -> Self { Self { enabled: 0, markers: [0; 2], block: [0; SECONDARY_BLOCK_BYTES] } }
}

impl SecondaryShared {
    pub fn reset(&mut self) {
        self.enabled = 0;
        self.block = [0; SECONDARY_BLOCK_BYTES];
        self.markers = [0x8000; 2];
    }

    pub fn first_word(&self) -> u32 { u32::from_le_bytes([self.block[0], self.block[1], self.block[2], self.block[3]]) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_requested_slot_holds_only_within_three_counted_slots() {
        let cases = [((2, 3), 2), ((3, 3), 3), ((1, 1), 1), ((0, 3), 1), ((2, 0), 1), ((4, 4), 1), ((3, 2), 1)];
        for ((requested, count), slot) in cases { assert_eq!(active_slot(requested, count), slot, "{requested} of {count}"); }
    }

    #[test]
    fn a_reset_clears_the_block_and_marks_both_halves() {
        let mut shared = SecondaryShared { enabled: 1, markers: [1, 2], block: [7; SECONDARY_BLOCK_BYTES] };
        assert_eq!(shared.first_word(), 0x0707_0707);
        shared.reset();
        assert_eq!(shared, SecondaryShared { enabled: 0, markers: [0x8000; 2], block: [0; SECONDARY_BLOCK_BYTES] });
    }
}
