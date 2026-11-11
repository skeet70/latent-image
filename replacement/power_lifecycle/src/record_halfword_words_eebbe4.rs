//! Halfword-to-word widening and the 72-byte constant copy (image 5).

use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_profile_accessors::offset;

const CONSTANT: u32 = 0x0169_b8f0;
const CONSTANT_BYTES: u32 = 72;

pub fn helper_eebbe4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress, source: GuestAddress) -> Result<(), MemoryError> {
    let value = u32::from(context.read_u16(source)?);
    context.write_u32(destination, value)?;
    let value = u32::from(context.read_u16(offset(source, 2))?);
    context.write_u32(offset(destination, 4), value)
}

pub fn helper_eed630<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress) -> Result<(), MemoryError> {
    context.copy_bytes(destination, GuestAddress::new(CONSTANT), ByteCount(CONSTANT_BYTES))
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 bodies at 0x00eebbe4 and 0x00eed630.
    use super::*;
    use crate::record_address_space::{GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const DESTINATION: u32 = 0x0010_0000;
    const SOURCE: u32 = 0x0010_1000;

    struct Memory { destination: [u8; 80], source: [u8; 8], constant: [u8; CONSTANT_BYTES as usize] }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(DESTINATION, 80), &mut memory.destination).unwrap(),
            MemoryRegion::new(descriptor(SOURCE, 8), &mut memory.source).unwrap(),
            MemoryRegion::new(descriptor(CONSTANT, CONSTANT_BYTES), &mut memory.constant).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    #[test]
    fn halfwords_widen_into_two_words() {
        let mut memory = Memory { destination: [0xff; 80], source: [0; 8], constant: [0; CONSTANT_BYTES as usize] };
        memory.source[0..2].copy_from_slice(&0x1234u16.to_le_bytes());
        memory.source[2..4].copy_from_slice(&0xabcdu16.to_le_bytes());
        with_memory(&mut memory, |context| {
            assert_eq!(helper_eebbe4(context, GuestAddress::new(DESTINATION), GuestAddress::new(SOURCE)), Ok(()));
        });
        assert_eq!(&memory.destination[0..4], &0x1234u32.to_le_bytes());
        assert_eq!(&memory.destination[4..8], &0xabcdu32.to_le_bytes());
        assert_eq!(&memory.destination[8..], &[0xff; 72]);
    }

    #[test]
    fn the_constant_copy_moves_seventy_two_bytes() {
        let mut memory = Memory { destination: [0xff; 80], source: [0; 8], constant: [0; CONSTANT_BYTES as usize] };
        for index in 0..CONSTANT_BYTES as usize { memory.constant[index] = index as u8; }
        with_memory(&mut memory, |context| {
            assert_eq!(helper_eed630(context, GuestAddress::new(DESTINATION)), Ok(()));
        });
        assert_eq!(&memory.destination[..CONSTANT_BYTES as usize], &memory.constant[..]);
        assert_eq!(&memory.destination[CONSTANT_BYTES as usize..], &[0xff; 8]);
    }
}
