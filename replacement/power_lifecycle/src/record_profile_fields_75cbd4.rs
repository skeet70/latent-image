//! Profile-entry field readers, record advances and constant leaves, image-4 entries 0x0075cbd4-0x00ef6844.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

const PROFILE_ENTRY: u32 = 0x34;
const PROFILE_ENTRY_TAIL: u32 = 0x3c;
const LIST_ENTRY: u32 = 0x414;
const SELECTION_WORD: u32 = 0x0094_2b88;

pub fn helper_75cbd4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress) -> Result<u32, MemoryError> {
    context.read_u32(record.checked_add(PROFILE_ENTRY)?)
}

pub fn helper_75cc34<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress) -> Result<u32, MemoryError> {
    context.read_u32(record.checked_add(PROFILE_ENTRY_TAIL)?)
}

pub fn helper_75d394<M: GuestAddressSpace, E>(_: &mut RecordContext<M, E>, record: GuestAddress) -> Result<GuestAddress, MemoryError> {
    record.checked_add(LIST_ENTRY)
}

pub fn helper_76a1d8<M: GuestAddressSpace, E>(_: &mut RecordContext<M, E>, value: u32) -> Result<u32, MemoryError> {
    Ok(value)
}

pub fn helper_76a244<M: GuestAddressSpace, E>(_: &mut RecordContext<M, E>, value: u32) -> Result<u32, MemoryError> {
    Ok(value)
}

pub fn helper_76a2c8<M: GuestAddressSpace, E>(_: &mut RecordContext<M, E>, value: u32) -> Result<u32, MemoryError> {
    Ok(value)
}

pub fn helper_76a328<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    context.read_u32(GuestAddress::new(SELECTION_WORD))
}

pub fn helper_7718d8<M: GuestAddressSpace, E>(_: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    Ok(1)
}

pub fn helper_7718f0<M: GuestAddressSpace, E>(_: &mut RecordContext<M, E>, value: u32) -> Result<u32, MemoryError> {
    Ok(value)
}

pub fn helper_ef6844<M: GuestAddressSpace, E>(_: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    Ok(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const RECORD: u32 = 0x0169_d29c;

    struct Memory {
        record: [u8; 0x420],
        globals: [u8; 0x100],
    }

    impl Memory {
        fn with_context<R>(&mut self, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
            let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
            let mut regions = [
                MemoryRegion::new(descriptor(RECORD, 0x420), &mut self.record).unwrap(),
                MemoryRegion::new(descriptor(0x0094_2ab0, 0x100), &mut self.globals).unwrap(),
            ];
            let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
            body(&mut RecordContext::new(space, ()))
        }
    }

    #[test]
    fn the_entry_readers_take_the_words_at_fifty_two_and_sixty_past_the_record() {
        let mut memory = Memory { record: [0; 0x420], globals: [0; 0x100] };
        memory.record[0x34..0x38].copy_from_slice(&0x0102_0304u32.to_le_bytes());
        memory.record[0x3c..0x40].copy_from_slice(&0x0506_0708u32.to_le_bytes());
        memory.with_context(|context| {
            assert_eq!(helper_75cbd4(context, GuestAddress::new(RECORD)), Ok(0x0102_0304));
            assert_eq!(helper_75cc34(context, GuestAddress::new(RECORD)), Ok(0x0506_0708));
        });
    }

    #[test]
    fn the_advance_is_the_list_entry_of_the_record() {
        let mut memory = Memory { record: [0; 0x420], globals: [0; 0x100] };
        memory.with_context(|context| {
            assert_eq!(helper_75d394(context, GuestAddress::new(0x0096_ebd0)), Ok(GuestAddress::new(0x0096_efe4)));
        });
    }

    #[test]
    fn the_selection_leaf_reads_its_global_word() {
        let mut memory = Memory { record: [0; 0x420], globals: [0; 0x100] };
        memory.globals[0xd8..0xdc].copy_from_slice(&0x0000_0002u32.to_le_bytes());
        memory.with_context(|context| assert_eq!(helper_76a328(context), Ok(2)));
    }

    #[test]
    fn the_passthrough_and_constant_leaves() {
        let mut memory = Memory { record: [0; 0x420], globals: [0; 0x100] };
        memory.with_context(|context| {
            assert_eq!(helper_76a1d8(context, 0x1111), Ok(0x1111));
            assert_eq!(helper_76a244(context, 0x2222), Ok(0x2222));
            assert_eq!(helper_76a2c8(context, 0x3333), Ok(0x3333));
            assert_eq!(helper_7718f0(context, 0x4444), Ok(0x4444));
            assert_eq!(helper_7718d8(context), Ok(1));
            assert_eq!(helper_ef6844(context), Ok(1));
        });
    }
}
