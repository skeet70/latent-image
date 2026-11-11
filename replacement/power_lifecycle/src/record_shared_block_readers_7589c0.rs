//! Readers of the shared block at 0x0091_904c, image-4 bodies 0x0075_89c0-0x0075_8c68.
use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_helper_shared_state::SHARED_BLOCK;
use crate::record_native_exports::stock_export;

fn signed_halfword<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, at: u32) -> Result<i32, MemoryError> {
    context.read_u16(GuestAddress::new(SHARED_BLOCK + at)).map(|value| i32::from(value as i16))
}

/// The word at block offset 0x83c, the body at 0x0075_89c0.
pub fn resource_word<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    context.read_u32(GuestAddress::new(SHARED_BLOCK + 0x83c))
}

/// The image-4 body at 0x0075_8a0c.
pub fn signed_halfword_6f8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<i32, MemoryError> {
    signed_halfword(context, 0x6f8)
}

/// The image-4 body at 0x0075_8a5c.
pub fn signed_halfword_6fa<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<i32, MemoryError> {
    signed_halfword(context, 0x6fa)
}

/// The image-4 body at 0x0075_8aac.
pub fn signed_halfword_6fc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<i32, MemoryError> {
    signed_halfword(context, 0x6fc)
}

/// The image-4 body at 0x0075_8afc.
pub fn signed_halfword_6fe<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<i32, MemoryError> {
    signed_halfword(context, 0x6fe)
}

/// The image-4 body at 0x0075_8b4c.
pub fn signed_halfword_700<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<i32, MemoryError> {
    signed_halfword(context, 0x700)
}

/// The image-4 body at 0x0075_8b9c.
pub fn halfword_702<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    context.read_u16(GuestAddress::new(SHARED_BLOCK + 0x702)).map(u32::from)
}

/// The image-4 body at 0x0075_8bec.
pub fn word_704<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    context.read_u32(GuestAddress::new(SHARED_BLOCK + 0x704))
}

/// Copies the seven words at block offset 0x840, the body at 0x0075_8c68.
pub fn copy_record<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress) -> Result<(), MemoryError> {
    context.copy_bytes(destination, GuestAddress::new(SHARED_BLOCK + 0x840), ByteCount(28))
}

stock_export!(
    xm1_power_record_helper_7589c0,
    helper_7589c0_abi,
    (),
    (_resource: u32) -> u32,
    (),
    (),
    crate::record_shared_block_readers_7589c0::resource_word,
    |value| value
);
stock_export!(
    xm1_power_record_helper_758a0c,
    helper_758a0c_abi,
    (),
    (_item: u32) -> i32,
    (),
    (),
    crate::record_shared_block_readers_7589c0::signed_halfword_6f8,
    |value| value
);
stock_export!(
    xm1_power_record_helper_758a5c,
    helper_758a5c_abi,
    (),
    (_item: u32) -> i32,
    (),
    (),
    crate::record_shared_block_readers_7589c0::signed_halfword_6fa,
    |value| value
);
stock_export!(
    xm1_power_record_helper_758aac,
    helper_758aac_abi,
    (),
    (_item: u32) -> i32,
    (),
    (),
    crate::record_shared_block_readers_7589c0::signed_halfword_6fc,
    |value| value
);
stock_export!(
    xm1_power_record_helper_758afc,
    helper_758afc_abi,
    (),
    (_item: u32) -> i32,
    (),
    (),
    crate::record_shared_block_readers_7589c0::signed_halfword_6fe,
    |value| value
);
stock_export!(
    xm1_power_record_helper_758b4c,
    helper_758b4c_abi,
    (),
    (_item: u32) -> i32,
    (),
    (),
    crate::record_shared_block_readers_7589c0::signed_halfword_700,
    |value| value
);
stock_export!(
    xm1_power_record_helper_758b9c,
    helper_758b9c_abi,
    (),
    (_item: u32) -> u32,
    (),
    (),
    crate::record_shared_block_readers_7589c0::halfword_702,
    |value| value
);
stock_export!(
    xm1_power_record_helper_758bec,
    helper_758bec_abi,
    (),
    (_item: u32) -> u32,
    (),
    (),
    crate::record_shared_block_readers_7589c0::word_704,
    |value| value
);
stock_export!(
    xm1_power_record_helper_758c68,
    helper_758c68_abi,
    (),
    (_request: u32, output: u32) -> (),
    (destination: GuestAddress),
    (GuestAddress::new(output)),
    crate::record_shared_block_readers_7589c0::copy_record,
    |_| ()
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_address_space::{GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    struct Memory {
        block: [u8; 0x1000],
        destination: [u8; 0x40],
    }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(SHARED_BLOCK, 0x1000), &mut memory.block).unwrap(),
            MemoryRegion::new(descriptor(0x0010_0000, 0x40), &mut memory.destination).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    #[test]
    fn readers_load_the_block_offsets_at_their_widths() {
        let mut memory = Memory { block: [0; 0x1000], destination: [0; 0x40] };
        memory.block[0x83c..0x840].copy_from_slice(&0x8091_a0b0u32.to_le_bytes());
        memory.block[0x6f8..0x6fa].copy_from_slice(&0x8001u16.to_le_bytes());
        memory.block[0x6fa..0x6fc].copy_from_slice(&0xffffu16.to_le_bytes());
        memory.block[0x6fc..0x6fe].copy_from_slice(&0x7fffu16.to_le_bytes());
        memory.block[0x6fe..0x700].copy_from_slice(&0x0102u16.to_le_bytes());
        memory.block[0x700..0x702].copy_from_slice(&0xfedcu16.to_le_bytes());
        memory.block[0x702..0x704].copy_from_slice(&0xfedcu16.to_le_bytes());
        memory.block[0x704..0x708].copy_from_slice(&0x0a0b_0c0du32.to_le_bytes());
        with_memory(&mut memory, |context| {
            assert_eq!(resource_word(context), Ok(0x8091_a0b0));
            assert_eq!(signed_halfword_6f8(context), Ok(-0x7fff));
            assert_eq!(signed_halfword_6fa(context), Ok(-1));
            assert_eq!(signed_halfword_6fc(context), Ok(0x7fff));
            assert_eq!(signed_halfword_6fe(context), Ok(0x0102));
            assert_eq!(signed_halfword_700(context), Ok(-0x0124));
            assert_eq!(halfword_702(context), Ok(0xfedc));
            assert_eq!(word_704(context), Ok(0x0a0b_0c0d));
        });
    }

    #[test]
    fn copy_record_writes_seven_words_in_ascending_order() {
        let mut memory = Memory { block: [0; 0x1000], destination: [0; 0x40] };
        for index in 0..7 { memory.block[0x840 + index * 4..0x844 + index * 4].copy_from_slice(&(0x2000 + index as u32).to_le_bytes()); }
        with_memory(&mut memory, |context| {
            assert_eq!(copy_record(context, GuestAddress::new(0x0010_0000)), Ok(()));
        });
        for index in 0..7 { assert_eq!(&memory.destination[index * 4..index * 4 + 4], &(0x2000 + index as u32).to_le_bytes()); }
        assert_eq!(&memory.destination[28..32], &0u32.to_le_bytes());
    }
}
