//! Eight configuration bytes at keys 0x3e0000 through 0x3e0007, image-4 body
//! 0x006c1630.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

pub const FIRST_KEY: u32 = 0x003e_0000;
pub const BYTES: u32 = 8;

/// The image-4 body at 0x006c1630.
pub fn helper_6c1630<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, output: GuestAddress) -> Result<(), MemoryError> {
    for index in 0..BYTES {
        let value = crate::record_configuration::helper_69f8fc(context, FIRST_KEY + index)?;
        context.write_u8(output.checked_add(index)?, value as u8)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_address_space::{
        ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission,
    };

    const PROVIDER: u32 = 0x0096_ca50;
    const OUTPUT: u32 = 0x0010_0000;

    #[test]
    fn reads_each_configuration_byte_through_the_provider() {
        let mut pointer = 0x1000u32.to_le_bytes();
        let mut configuration = [0u8; 8];
        for (index, value) in [0xfe, 0x81, 0x72, 0x63, 0x14, 0x25, 0x36, 0x47].into_iter().enumerate() {
            configuration[index] = value;
        }
        let mut output = [0u8; 8];
        let descriptor = |base, length| {
            RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap()
        };
        let mut regions = [
            MemoryRegion::new(descriptor(PROVIDER, 4), &mut pointer).unwrap(),
            MemoryRegion::new(descriptor(0x1000 + FIRST_KEY, 8), &mut configuration).unwrap(),
            MemoryRegion::new(descriptor(OUTPUT, 8), &mut output).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let mut context = RecordContext::new(space, ());
        assert_eq!(helper_6c1630(&mut context, GuestAddress::new(OUTPUT)), Ok(()));
        assert_eq!(output, [0xfe, 0x81, 0x72, 0x63, 0x14, 0x25, 0x36, 0x47]);
    }
}
