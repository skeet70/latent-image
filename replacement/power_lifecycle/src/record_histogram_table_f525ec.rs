//! Stock-shaped body at 0x00f525ec over the table at 0x1e00 from the base
//! pointer.

use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_configuration::helper_69f8fc;

const TABLE: u32 = 0x1e00;
const HEAD_MAGIC: u32 = 0x5354_534d;
const TAIL_MAGIC: u32 = 0x4c41_534d;
const TAIL: u32 = 0x17c4;
const SOURCE: u32 = 0x5ffe_7854;
const SOURCE_BYTES: u32 = 0x17ac;
const ENTRY_INDEX: u32 = 0x016c_c81c;
const ENTRY_BYTES: u32 = 60;

/// 0x00f525ec
pub fn helper_f525ec<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, base: GuestAddress) -> Result<(), MemoryError> {
    let table = base.checked_add(TABLE)?;
    context.write_u32(table, HEAD_MAGIC)?;
    for (index, key) in [0x75ffu32, 0x7600, 0x7601, 0x7602, 0x7603].into_iter().enumerate() {
        let value = helper_69f8fc(context, key)?.wrapping_mul(10);
        context.write_u32(table.checked_add(4 + index as u32 * 4)?, value)?;
    }
    let entries = table.checked_add(24)?;
    context.copy_bytes(entries, GuestAddress::new(SOURCE), ByteCount(SOURCE_BYTES))?;
    context.write_u32(table.checked_add(TAIL)?, TAIL_MAGIC)?;
    let index = u32::from(context.read_u16(GuestAddress::new(ENTRY_INDEX))?);
    let entry = entries.checked_add(index.wrapping_mul(ENTRY_BYTES))?;
    for word in 0..ENTRY_BYTES / 4 {
        context.write_u32(entry.checked_add(word * 4)?, 0)?;
    }
    Ok(())
}

use crate::record_native_exports::stock_export;

stock_export!(
    xm1_power_record_helper_f525ec,
    helper_f525ec_abi,
    (),
    (base: u32) -> (),
    (base: GuestAddress),
    (GuestAddress::new(base)),
    crate::record_histogram_table_f525ec::helper_f525ec,
    |_| ()
);

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 body at 0x00f525ec.
    use super::*;
    use crate::record_address_space::{GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const BASE: u32 = 0x0020_0000;
    const CONFIGURATION: u32 = 0x0010_0000;
    const CONFIGURATION_POINTER: u32 = 0x0096_ca50;

    #[test]
    fn the_table_copies_the_source_and_zeroes_the_indexed_entry() {
        let mut block = [0xa5u8; 0x3700];
        let mut configuration = [0u8; 0x7700];
        for (index, key) in [0x75ffusize, 0x7600, 0x7601, 0x7602, 0x7603].into_iter().enumerate() {
            configuration[key] = 3 + index as u8;
        }
        let mut pointer = CONFIGURATION.to_le_bytes();
        let mut source = [0x5au8; SOURCE_BYTES as usize];
        let mut entry = 1u16.to_le_bytes();
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(BASE, 0x3700), &mut block).unwrap(),
            MemoryRegion::new(descriptor(CONFIGURATION, 0x7700), &mut configuration).unwrap(),
            MemoryRegion::new(descriptor(CONFIGURATION_POINTER, 4), &mut pointer).unwrap(),
            MemoryRegion::new(descriptor(SOURCE, SOURCE_BYTES), &mut source).unwrap(),
            MemoryRegion::new(descriptor(ENTRY_INDEX, 2), &mut entry).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let mut context = RecordContext::new(space, ());
        assert_eq!(helper_f525ec(&mut context, GuestAddress::new(BASE)), Ok(()));

        let table = TABLE as usize;
        assert_eq!(u32::from_le_bytes(block[table..table + 4].try_into().unwrap()), HEAD_MAGIC);
        for (index, value) in [30u32, 40, 50, 60, 70].into_iter().enumerate() {
            let at = table + 4 + index * 4;
            assert_eq!(u32::from_le_bytes(block[at..at + 4].try_into().unwrap()), value);
        }
        assert_eq!(&block[table + 24..table + 28], &[0x5a; 4]);
        assert_eq!(block[table + 24 + SOURCE_BYTES as usize - 1], 0x5a);
        let tail = table + TAIL as usize;
        assert_eq!(u32::from_le_bytes(block[tail..tail + 4].try_into().unwrap()), TAIL_MAGIC);
        let entry = table + 24 + ENTRY_BYTES as usize;
        for offset in 0..ENTRY_BYTES as usize { assert_eq!(block[entry + offset], 0); }
        assert_eq!(block[entry + ENTRY_BYTES as usize], 0x5a);
    }
}
