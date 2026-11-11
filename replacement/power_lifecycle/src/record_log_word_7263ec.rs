//! Append a word to the byte log at 0x40a5_b5e0 (image 4, 0x007263ec).

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

const LOG_BLOCK: u32 = 0x40a5_b5e0;
const LOG_OFFSET: u32 = 0x04;
const LOG_DATA: u32 = 0x0c;

fn block(at: u32) -> GuestAddress { GuestAddress::new(LOG_BLOCK + at) }

pub fn helper_7263ec<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, length: u32, value: u32) -> Result<(), MemoryError> {
    let data = context.read_u32(block(LOG_DATA))?;
    if data == 0 { return Ok(()); }
    let mut offset = context.read_u32(block(LOG_OFFSET))?;
    let padding = (4 - (length & 3)) & 3;
    for index in 0..padding {
        context.write_u8(GuestAddress::new(data.wrapping_add(offset).wrapping_add(index)), 0)?;
    }
    if padding != 0 {
        offset = offset.wrapping_add(padding);
        context.write_u32(block(LOG_OFFSET), offset)?;
    }
    context.write_u32(GuestAddress::new(data.wrapping_add(offset)), value)
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-4 body at 0x007263ec.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const DATA: u32 = 0x0010_0000;

    struct Memory { block: [u8; 16], data: [u8; 16] }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(LOG_BLOCK, 16), &mut memory.block).unwrap(),
            MemoryRegion::new(descriptor(DATA, 16), &mut memory.data).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    fn pointed(memory: &mut Memory, offset: u32) {
        memory.block[0x0c..0x10].copy_from_slice(&DATA.to_le_bytes());
        memory.block[0x04..0x08].copy_from_slice(&offset.to_le_bytes());
    }

    #[test]
    fn aligned_append_stores_at_the_current_offset() {
        let mut memory = Memory { block: [0; 16], data: [0; 16] };
        pointed(&mut memory, 4);
        with_memory(&mut memory, |context| assert_eq!(helper_7263ec(context, 4, 0x1122_3344), Ok(())));
        assert_eq!(&memory.data[4..8], &0x1122_3344u32.to_le_bytes());
        assert_eq!(&memory.block[0x04..0x08], &4u32.to_le_bytes());
    }

    #[test]
    fn unaligned_append_pads_with_zero_bytes() {
        let mut memory = Memory { block: [0; 16], data: [0xff; 16] };
        pointed(&mut memory, 2);
        with_memory(&mut memory, |context| assert_eq!(helper_7263ec(context, 6, 0x5566_7788), Ok(())));
        assert_eq!(&memory.data[2..4], &[0, 0]);
        assert_eq!(&memory.data[4..8], &0x5566_7788u32.to_le_bytes());
        assert_eq!(&memory.block[0x04..0x08], &4u32.to_le_bytes());
    }

    #[test]
    fn one_byte_short_pads_three_bytes() {
        let mut memory = Memory { block: [0; 16], data: [0xff; 16] };
        pointed(&mut memory, 1);
        with_memory(&mut memory, |context| assert_eq!(helper_7263ec(context, 1, 0x99aa_bbcc), Ok(())));
        assert_eq!(&memory.data[1..4], &[0, 0, 0]);
        assert_eq!(&memory.data[4..8], &0x99aa_bbccu32.to_le_bytes());
        assert_eq!(&memory.block[0x04..0x08], &4u32.to_le_bytes());
    }

    #[test]
    fn a_clear_data_pointer_skips_the_store() {
        let mut memory = Memory { block: [0; 16], data: [0xff; 16] };
        with_memory(&mut memory, |context| assert_eq!(helper_7263ec(context, 1, 0x99aa_bbcc), Ok(())));
        assert_eq!(&memory.data[..], &[0xff; 16]);
        assert_eq!(&memory.block[0x04..0x08], &0u32.to_le_bytes());
    }
}
