//! Field accessors over one guest record block (image 4, 0x0075bfd8-0x0075d344).
//!
//! Each body is the image-4 instruction sequence at the address in its name.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_profile_accessors::{offset, read_header, write_header, HeaderField};

const FIELD_75BFD8: HeaderField = HeaderField::SignedHalfword(0x00);
const FIELD_75C488: HeaderField = HeaderField::Word(0x00);
const FIELD_75C4B8: HeaderField = HeaderField::Word(0x08);
const FIELD_75CAB4: HeaderField = HeaderField::SignedHalfword(0x18);
const FIELD_75CB14: HeaderField = HeaderField::Byte(0x2c);
const FIELD_75CF1C: HeaderField = HeaderField::Word(0x94);
const FIELD_75CF4C: HeaderField = HeaderField::Word(0x98);
const FIELD_75CF64: HeaderField = HeaderField::Word(0x9c);
const FIELD_75CF94: HeaderField = HeaderField::Word(0xa0);

pub fn helper_75bfd8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress) -> Result<u32, MemoryError> {
    read_header(context, record, FIELD_75BFD8)
}

pub fn helper_75c488<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress) -> Result<u32, MemoryError> {
    read_header(context, record, FIELD_75C488)
}

pub fn helper_75c4b8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress) -> Result<u32, MemoryError> {
    read_header(context, record, FIELD_75C4B8)
}

pub fn helper_75cab4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress) -> Result<u32, MemoryError> {
    read_header(context, record, FIELD_75CAB4)
}

pub fn helper_75cb14<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress) -> Result<u32, MemoryError> {
    read_header(context, record, FIELD_75CB14)
}

pub fn helper_75cf1c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress, value: u32) -> Result<(), MemoryError> {
    write_header(context, record, FIELD_75CF1C, value)
}

pub fn helper_75cf4c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress, value: u32) -> Result<(), MemoryError> {
    write_header(context, record, FIELD_75CF4C, value)
}

pub fn helper_75cf64<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress) -> Result<u32, MemoryError> {
    read_header(context, record, FIELD_75CF64)
}

pub fn helper_75cf94<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress) -> Result<u32, MemoryError> {
    read_header(context, record, FIELD_75CF94)
}

pub fn helper_75d264<M: GuestAddressSpace, E>(_: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    Ok(())
}

pub fn helper_75d2c4<M: GuestAddressSpace, E>(_: &mut RecordContext<M, E>, record: GuestAddress) -> Result<GuestAddress, MemoryError> {
    Ok(offset(record, 0x828))
}

pub fn helper_75d334<M: GuestAddressSpace, E>(_: &mut RecordContext<M, E>, record: GuestAddress) -> Result<GuestAddress, MemoryError> {
    Ok(offset(record, 0x1050))
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-4 bodies at 0x0075bfd8-0x0075d344.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const RECORD: u32 = 0x0010_0000;

    struct Memory { record: [u8; 0x1100] }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = RegionDescriptor::new(GuestAddress::new(RECORD), ByteCount(0x1100), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [MemoryRegion::new(descriptor, &mut memory.record).unwrap()];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    fn at() -> GuestAddress { GuestAddress::new(RECORD) }

    #[test]
    fn getters_read_their_typed_fields() {
        let mut memory = Memory { record: [0; 0x1100] };
        memory.record[0x00..0x02].copy_from_slice(&0xfffdu16.to_le_bytes());
        memory.record[0x08..0x0c].copy_from_slice(&0x0102_0304u32.to_le_bytes());
        memory.record[0x18..0x1a].copy_from_slice(&0x8000u16.to_le_bytes());
        memory.record[0x2c] = 0x7f;
        memory.record[0x9c..0xa0].copy_from_slice(&0x5566_7788u32.to_le_bytes());
        memory.record[0xa0..0xa4].copy_from_slice(&0x99aa_bbccu32.to_le_bytes());
        with_memory(&mut memory, |context| {
            assert_eq!(helper_75bfd8(context, at()), Ok(0xffff_fffd));
            assert_eq!(helper_75c488(context, at()), Ok(0xfffd));
            assert_eq!(helper_75c4b8(context, at()), Ok(0x0102_0304));
            assert_eq!(helper_75cab4(context, at()), Ok(0xffff_8000));
            assert_eq!(helper_75cb14(context, at()), Ok(0x7f));
            assert_eq!(helper_75cf64(context, at()), Ok(0x5566_7788));
            assert_eq!(helper_75cf94(context, at()), Ok(0x99aa_bbcc));
        });
    }

    #[test]
    fn setters_store_their_words() {
        let mut memory = Memory { record: [0; 0x1100] };
        with_memory(&mut memory, |context| {
            assert_eq!(helper_75cf1c(context, at(), 0x1122_3344), Ok(()));
            assert_eq!(helper_75cf4c(context, at(), 0x5566_7788), Ok(()));
            assert_eq!(helper_75d264(context), Ok(()));
        });
        assert_eq!(&memory.record[0x94..0x98], &0x1122_3344u32.to_le_bytes());
        assert_eq!(&memory.record[0x98..0x9c], &0x5566_7788u32.to_le_bytes());
    }

    #[test]
    fn derived_addresses_are_relative_to_the_record() {
        let mut memory = Memory { record: [0; 0x1100] };
        with_memory(&mut memory, |context| {
            assert_eq!(helper_75d2c4(context, at()), Ok(GuestAddress::new(RECORD + 0x828)));
            assert_eq!(helper_75d334(context, at()), Ok(GuestAddress::new(RECORD + 0x1050)));
        });
    }
}
