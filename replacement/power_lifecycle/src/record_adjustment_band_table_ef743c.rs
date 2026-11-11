//! The encoded level pair of a configuration band row.
//!
//! Image-5 body 0x00ef743c.
use crate::record_address_space::{ByteCount, GuestAddressSpace, MemoryError, RecordContext, StackAlignment};
use crate::record_adjustment_range_ef7380::helper_ef7380;
use crate::record_configuration_classification_69fffc::helper_69fffc;

const BAND_ROW_LAST: u32 = 0x4a28;
const BAND_ROWS: u32 = 4;

/// The image-5 body at 0x00ef743c.
pub fn helper_ef743c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32, scaled: u32) -> Result<u32, MemoryError> {
    context.with_guest_frame(ByteCount(2), StackAlignment::Word, |context, frame| {
        let mut count = BAND_ROWS;
        let mut key = BAND_ROW_LAST;
        loop {
            let band = helper_69fffc(context, key)? as u32;
            if (value as i32) >= (band.wrapping_mul(10) as i32) {
                break;
            }
            count -= 1;
            key = key.wrapping_sub(1);
            if count == 0 {
                break;
            }
        }
        context.write_u8(frame.base(), (scaled as i32 / 100) as u8)?;
        context.write_u8(frame.base().checked_add(1)?, count as u8)?;
        helper_ef7380(context, frame.base())
    })
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 bodies at 0x00ef743c and 0x00ef7380.
    use super::*;
    use crate::record_address_space::{GuestAddress, GuestDomain, GuestStack, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const CONFIG_POINTER: u32 = 0x0096_ca50;
    const CONFIG_DATA: u32 = 0x0020_0000;
    const CONFIG_LENGTH: usize = 0x40_0000;
    const STACK: u32 = 0x0080_0000;

    fn with_config<R>(config: &mut [u8], stack: &mut [u8], body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let mut pointer = CONFIG_DATA.to_le_bytes();
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(CONFIG_POINTER, 4), &mut pointer).unwrap(),
            MemoryRegion::new(descriptor(CONFIG_DATA, config.len() as u32), config).unwrap(),
            MemoryRegion::new(descriptor(STACK, 0x40), stack).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let stack = GuestStack::new(GuestAddress::new(STACK), ByteCount(0x40)).unwrap();
        body(&mut RecordContext::with_stack(space, (), stack))
    }

    #[test]
    fn the_first_band_at_or_below_the_value_ends_the_count() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        let mut stack = [0u8; 0x40];
        config[BAND_ROW_LAST as usize] = 10;
        config[BAND_ROW_LAST as usize - 1] = 10;
        config[BAND_ROW_LAST as usize - 2] = 5;
        config[BAND_ROW_LAST as usize - 3] = 5;
        with_config(&mut config, &mut stack, |context| {
            assert_eq!(helper_ef743c(context, 50, 100), Ok(4));
        });
    }

    #[test]
    fn a_value_below_every_band_counts_the_rows_out() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        let mut stack = [0u8; 0x40];
        for key in 0..4 {
            config[BAND_ROW_LAST as usize - key] = 10;
        }
        with_config(&mut config, &mut stack, |context| {
            assert_eq!(helper_ef743c(context, 0, 100), Ok(2));
            assert_eq!(helper_ef743c(context, 1000, 1234), Ok(1));
        });
    }
}
