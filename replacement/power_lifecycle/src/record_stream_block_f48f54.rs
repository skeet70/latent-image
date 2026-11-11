//! Stock-shaped body at 0x00f49138 over the stream block at 0x00084f74 from
//! the base pointer.

use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_configuration::helper_69f8fc;
use crate::record_numeric_configuration::helper_fa89fc;

const BLOCK: u32 = 0x0008_4f74;
const BLOCK_BYTES: u32 = 0x17f5c;
const TAIL: u32 = 0x17f58;
const HEAD_MAGIC: u32 = 0x5354_4d44;
const TAIL_MAGIC: u32 = 0x4c41_4d44;
const INITIALIZED: u32 = 0x016c_2220;
const FILL_START: u32 = 0xc0f0;
const CLEARED_BYTES: u32 = 0xbe68;
const FIRST_FILL: u32 = 0x2c00;
const SECOND_START: u32 = 0xed08;
const SECOND_FILL: u32 = 0x8400;
const THIRD_START: u32 = 0x17108;
const THIRD_FILL: u32 = 0x168;
const COPY_DESTINATION: u32 = 0x91064;
const COPY_SOURCE: u32 = 0x5ffd_b9ec;

fn fill<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress, value: u8, length: u32) -> Result<(), MemoryError> {
    for offset in 0..length {
        context.write_u8(destination.checked_add(offset)?, value)?;
    }
    Ok(())
}

/// 0x00f49138
pub fn helper_f49138<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, base: GuestAddress, mode: u32) -> Result<(), MemoryError> {
    let block = base.checked_add(BLOCK)?;
    let copy = mode == 1;
    if context.read_u32(GuestAddress::new(INITIALIZED))? == 0 && copy {
        helper_fa89fc(context, block)?;
        context.write_u32(block, HEAD_MAGIC)?;
        context.write_u32(block.checked_add(TAIL)?, TAIL_MAGIC)?;
        context.write_u32(GuestAddress::new(INITIALIZED), 1)?;
    }
    if helper_69f8fc(context, 0x5257)? == 0 {
        let start = block.checked_add(FILL_START)?;
        fill(context, start, 0, CLEARED_BYTES)?;
        fill(context, start, 0xff, FIRST_FILL)?;
        fill(context, block.checked_add(SECOND_START)?, 0xff, SECOND_FILL)?;
        fill(context, block.checked_add(THIRD_START)?, 0xff, THIRD_FILL)
    } else if copy {
        context.copy_bytes(base.checked_add(COPY_DESTINATION)?, GuestAddress::new(COPY_SOURCE), ByteCount(CLEARED_BYTES))
    } else {
        fill(context, block, 0, BLOCK_BYTES)?;
        context.write_u32(block, HEAD_MAGIC)?;
        context.write_u32(block.checked_add(TAIL)?, TAIL_MAGIC)?;
        context.write_u32(GuestAddress::new(INITIALIZED), 0)
    }
}

use crate::record_native_exports::stock_export;

stock_export!(
    xm1_power_record_helper_f49138,
    helper_f49138_abi,
    (),
    (base: u32, mode: u32) -> (),
    (base: GuestAddress, mode: u32),
    (GuestAddress::new(base), mode),
    crate::record_stream_block_f48f54::helper_f49138,
    |_| ()
);

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 body at 0x00f49138.
    use super::*;
    use crate::record_address_space::{GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const BASE: u32 = 0x0020_0000;
    const CONFIGURATION: u32 = 0x0010_0000;
    const CONFIGURATION_POINTER: u32 = 0x0096_ca50;

    fn with_memory<R>(block: &mut [u8], configuration: &mut [u8], pointer: &mut [u8], source: &mut [u8], flag: &mut [u8], body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(BASE, block.len() as u32), block).unwrap(),
            MemoryRegion::new(descriptor(CONFIGURATION, configuration.len() as u32), configuration).unwrap(),
            MemoryRegion::new(descriptor(CONFIGURATION_POINTER, 4), pointer).unwrap(),
            MemoryRegion::new(descriptor(COPY_SOURCE, source.len() as u32), source).unwrap(),
            MemoryRegion::new(descriptor(INITIALIZED, 4), flag).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    fn block() -> std::vec::Vec<u8> {
        let mut bytes = std::vec::Vec::new();
        bytes.resize(0x9cf00, 0);
        bytes
    }

    fn configuration() -> std::vec::Vec<u8> {
        let mut bytes = std::vec::Vec::new();
        bytes.resize(0x16100, 0);
        bytes
    }

    fn pointer() -> [u8; 4] { CONFIGURATION.to_le_bytes() }

    #[test]
    fn an_uninitialised_block_is_published_then_filled() {
        let mut block = block();
        let mut configuration = configuration();
        let mut pointer = pointer();
        let mut source = [0u8; CLEARED_BYTES as usize];
        let mut flag = [0u8; 4];
        with_memory(&mut block, &mut configuration, &mut pointer, &mut source, &mut flag, |context| {
            assert_eq!(helper_f49138(context, GuestAddress::new(BASE), 1), Ok(()));
        });
        assert_eq!(flag, 1u32.to_le_bytes());
        let base = BLOCK as usize;
        assert_eq!(u32::from_le_bytes(block[base..base + 4].try_into().unwrap()), HEAD_MAGIC);
        assert_eq!(u32::from_le_bytes(block[base + TAIL as usize..base + TAIL as usize + 4].try_into().unwrap()), TAIL_MAGIC);
        for offset in [FILL_START as usize, FILL_START as usize + 0x2bff, SECOND_START as usize, THIRD_START as usize + 0x167] {
            assert_eq!(block[base + offset], 0xff);
        }
        assert_eq!(block[base + FILL_START as usize + FIRST_FILL as usize], 0);
        assert_eq!(block[base + THIRD_START as usize + THIRD_FILL as usize], 0);
    }

    #[test]
    fn a_configured_block_is_reset_when_the_mode_is_not_one() {
        let mut block = block();
        for byte in block.iter_mut() { *byte = 0x5a; }
        let mut configuration = configuration();
        configuration[0x5257] = 1;
        let mut pointer = pointer();
        let mut source = [0u8; CLEARED_BYTES as usize];
        let mut flag = [0u8; 4];
        with_memory(&mut block, &mut configuration, &mut pointer, &mut source, &mut flag, |context| {
            assert_eq!(helper_f49138(context, GuestAddress::new(BASE), 0), Ok(()));
        });
        assert_eq!(flag, [0; 4]);
        let base = BLOCK as usize;
        assert_eq!(u32::from_le_bytes(block[base..base + 4].try_into().unwrap()), HEAD_MAGIC);
        assert_eq!(u32::from_le_bytes(block[base + TAIL as usize..base + TAIL as usize + 4].try_into().unwrap()), TAIL_MAGIC);
        assert_eq!(block[base + 4], 0);
        assert_eq!(block[base + TAIL as usize - 1], 0);
        assert_eq!(block[base + TAIL as usize + 4], 0x5a);
    }

    #[test]
    fn a_configured_initialised_block_copies_the_retained_bytes() {
        let mut block = block();
        let mut configuration = configuration();
        configuration[0x5257] = 1;
        let mut pointer = pointer();
        let mut source = [0xa5u8; CLEARED_BYTES as usize];
        let mut flag = [0u8; 4];
        with_memory(&mut block, &mut configuration, &mut pointer, &mut source, &mut flag, |context| {
            context.write_u32(GuestAddress::new(INITIALIZED), 1).unwrap();
            assert_eq!(helper_f49138(context, GuestAddress::new(BASE), 1), Ok(()));
        });
        assert_eq!(flag, 1u32.to_le_bytes());
        let destination = COPY_DESTINATION as usize;
        assert_eq!(block[destination..destination + 4], [0xa5; 4]);
        assert_eq!(block[destination + CLEARED_BYTES as usize - 1], 0xa5);
        assert_eq!(block[destination + CLEARED_BYTES as usize], 0);
    }
}
