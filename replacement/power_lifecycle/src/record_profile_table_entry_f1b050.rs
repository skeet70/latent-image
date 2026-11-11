//! Halfword table entry selection at 0x00f1_b050.
use crate::record_address_space::GuestAddress;

pub const FULL_ROWS: u32 = 0x016a_0400;
pub const SECONDARY_TAIL: u32 = 0x016a_03ca;
pub const PRIMARY_TAIL: u32 = 0x016a_03ee;
pub const CORNER: u32 = 0x016a_03c4;

/// Address of the halfword at `(primary, secondary, index)`, the image-5 body at 0x00f1_b050.
pub fn entry_address(primary: u32, secondary: u32, index: u32) -> GuestAddress {
    let address = if primary > 5 {
        if secondary > 2 {
            CORNER.wrapping_add(index.wrapping_mul(2))
        } else {
            PRIMARY_TAIL.wrapping_add(secondary.wrapping_mul(3).wrapping_add(index).wrapping_mul(2))
        }
    } else if secondary > 2 {
        SECONDARY_TAIL.wrapping_add(primary.wrapping_mul(3).wrapping_add(index).wrapping_mul(2))
    } else {
        FULL_ROWS.wrapping_add(primary.wrapping_mul(9).wrapping_add(secondary.wrapping_mul(3)).wrapping_add(index).wrapping_mul(2))
    };
    GuestAddress::new(address)
}

#[cfg(target_arch = "arm")]
#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f1b050(primary: u32, secondary: u32, index: u32) -> u32 {
    entry_address(primary, secondary, index).get()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_rows_step_nine_halfwords_per_primary_and_three_per_secondary() {
        assert_eq!(entry_address(0, 0, 0), GuestAddress::new(0x016a_0400));
        assert_eq!(entry_address(0, 0, 2), GuestAddress::new(0x016a_0404));
        assert_eq!(entry_address(0, 1, 0), GuestAddress::new(0x016a_0406));
        assert_eq!(entry_address(1, 0, 0), GuestAddress::new(0x016a_0412));
        assert_eq!(entry_address(5, 2, 2), GuestAddress::new(0x016a_046a));
    }

    #[test]
    fn the_three_tail_bases_hold_the_remaining_combinations() {
        assert_eq!(entry_address(0, 3, 0), GuestAddress::new(0x016a_03ca));
        assert_eq!(entry_address(5, 3, 2), GuestAddress::new(0x016a_03ec));
        assert_eq!(entry_address(6, 0, 0), GuestAddress::new(0x016a_03ee));
        assert_eq!(entry_address(6, 2, 2), GuestAddress::new(0x016a_03fe));
        assert_eq!(entry_address(6, 3, 0), GuestAddress::new(0x016a_03c4));
        assert_eq!(entry_address(6, 3, 2), GuestAddress::new(0x016a_03c8));
    }

    #[test]
    fn the_four_bases_are_contiguous() {
        assert_eq!(CORNER + 3 * 2, SECONDARY_TAIL);
        assert_eq!(SECONDARY_TAIL + 6 * 3 * 2, PRIMARY_TAIL);
        assert_eq!(PRIMARY_TAIL + 3 * 3 * 2, FULL_ROWS);
        assert_eq!(entry_address(5, 2, 2).get() + 2, FULL_ROWS + 6 * 3 * 3 * 2);
    }

    #[test]
    fn index_arithmetic_wraps_in_32_bits() {
        assert_eq!(entry_address(6, 3, 0x8000_0000), GuestAddress::new(CORNER));
        assert_eq!(entry_address(0, 0, 0x8000_0001), GuestAddress::new(FULL_ROWS + 2));
    }
}
