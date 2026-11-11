//! Stock-shaped getters at 0x00f77db4-0x00f77e68 over the 64-byte entries at
//! 0x00f83484.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

const ENTRIES: u32 = 0x00f8_3484;
const ENTRY_BYTES: u32 = 64;

fn entry<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32, at: u32) -> Result<u32, MemoryError> {
    let address = GuestAddress::new(ENTRIES.wrapping_add(index.wrapping_mul(ENTRY_BYTES)).wrapping_add(at));
    context.read_u32(address)
}

/// 0x00f77db4
pub fn helper_f77db4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32) -> Result<u32, MemoryError> {
    entry(context, index, 0x1c)
}

/// 0x00f77dfc
pub fn helper_f77dfc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32) -> Result<u32, MemoryError> {
    entry(context, index, 0x18)
}

/// 0x00f77e68
pub fn helper_f77e68<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32) -> Result<u32, MemoryError> {
    entry(context, index, 0x14)
}

use crate::record_native_exports::stock_export;

stock_export!(
    xm1_power_record_helper_f77db4,
    helper_f77db4_abi,
    (),
    (index: u32) -> u32,
    (index: u32),
    (index),
    crate::record_indexed_state_f77db4::helper_f77db4,
    |value| value
);
stock_export!(
    xm1_power_record_helper_f77dfc,
    helper_f77dfc_abi,
    (),
    (index: u32) -> u32,
    (index: u32),
    (index),
    crate::record_indexed_state_f77db4::helper_f77dfc,
    |value| value
);
stock_export!(
    xm1_power_record_helper_f77e68,
    helper_f77e68_abi,
    (),
    (index: u32) -> u32,
    (index: u32),
    (index),
    crate::record_indexed_state_f77db4::helper_f77e68,
    |value| value
);

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 bodies at 0x00f77db4-0x00f77e68.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    #[test]
    fn each_getter_reads_its_entry_word() {
        let mut entries = [0u8; 0x80];
        for (index, offset) in [(0usize, 0x14usize), (0, 0x18), (0, 0x1c), (1, 0x14), (1, 0x18), (1, 0x1c)] {
            let at = index * 64 + offset;
            entries[at..at + 4].copy_from_slice(&(0x1000 + at as u32).to_le_bytes());
        }
        let descriptor = RegionDescriptor::new(GuestAddress::new(ENTRIES), ByteCount(0x80), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [MemoryRegion::new(descriptor, &mut entries).unwrap()];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let mut context = RecordContext::new(space, ());
        assert_eq!(helper_f77db4(&mut context, 0), Ok(0x1000 + 0x1c));
        assert_eq!(helper_f77dfc(&mut context, 0), Ok(0x1000 + 0x18));
        assert_eq!(helper_f77e68(&mut context, 0), Ok(0x1000 + 0x14));
        assert_eq!(helper_f77db4(&mut context, 1), Ok(0x1000 + 0x40 + 0x1c));
        assert_eq!(helper_f77dfc(&mut context, 1), Ok(0x1000 + 0x40 + 0x18));
        assert_eq!(helper_f77e68(&mut context, 1), Ok(0x1000 + 0x40 + 0x14));
    }
}
