//! Image register copy list, image-4 body 0x006deb7c.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

pub const COPY_DESCRIPTOR_BYTES: u32 = 12;

/// The image-4 body at 0x006deb7c.
pub fn helper_6deb7c<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>,
    base: GuestAddress,
    copies: GuestAddress,
    count: i32,
) -> Result<(), MemoryError> {
    let mut descriptor = copies;
    let mut completed = 0;
    while completed != count {
        let source = GuestAddress::new(context.read_u32(descriptor)?);
        let offset = context.read_u32(descriptor.checked_add(4)?)?;
        let words = context.read_u16(descriptor.checked_add(8)?)?;
        if words != 0 {
            let destination = base.get().wrapping_add(offset);
            let mut index = 0;
            while index < words {
                let value = context.read_u32(source.checked_add(u32::from(index) * 4)?)?;
                context.write_u32(GuestAddress::new(destination.wrapping_add(u32::from(index) * 4)), value)?;
                index += 1;
            }
        }
        completed += 1;
        descriptor = descriptor.checked_add(COPY_DESCRIPTOR_BYTES)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_address_space::{
        ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission,
    };

    const BASE: u32 = 0x0020_0000;
    const COPIES: u32 = 0x0010_0000;
    const FIRST: u32 = 0x0030_0000;
    const SECOND: u32 = 0x0030_0100;

    struct Memory {
        base: [u8; 0x40],
        copies: [u8; 0x40],
        first: [u8; 0x10],
        second: [u8; 0x10],
    }

    impl Memory {
        fn new() -> Self {
            Self { base: [0; 0x40], copies: [0; 0x40], first: [0; 0x10], second: [0; 0x10] }
        }

        fn descriptor(&mut self, index: usize, source: u32, offset: u32, words: u16) {
            let at = index * 12;
            self.copies[at..at + 4].copy_from_slice(&source.to_le_bytes());
            self.copies[at + 4..at + 8].copy_from_slice(&offset.to_le_bytes());
            self.copies[at + 8..at + 10].copy_from_slice(&words.to_le_bytes());
        }

        fn with_context<R>(&mut self, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
            let descriptor = |base, length| {
                RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap()
            };
            let mut regions = [
                MemoryRegion::new(descriptor(BASE, 0x40), &mut self.base).unwrap(),
                MemoryRegion::new(descriptor(COPIES, 0x40), &mut self.copies).unwrap(),
                MemoryRegion::new(descriptor(FIRST, 0x10), &mut self.first).unwrap(),
                MemoryRegion::new(descriptor(SECOND, 0x10), &mut self.second).unwrap(),
            ];
            body(&mut RecordContext::new(HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap(), ()))
        }

        fn word(&self, offset: usize) -> u32 { u32::from_le_bytes(self.base[offset..offset + 4].try_into().unwrap()) }
    }

    #[test]
    fn copies_each_descriptor_words_to_its_offset() {
        let mut memory = Memory::new();
        for index in 0..3u32 {
            memory.first[index as usize * 4..index as usize * 4 + 4].copy_from_slice(&(0x11 + index).to_le_bytes());
            memory.second[index as usize * 4..index as usize * 4 + 4].copy_from_slice(&(0x21 + index).to_le_bytes());
        }
        memory.descriptor(0, FIRST, 4, 3);
        memory.descriptor(1, SECOND, 0x20, 2);
        memory.with_context(|context| assert_eq!(helper_6deb7c(context, GuestAddress::new(BASE), GuestAddress::new(COPIES), 2), Ok(())));
        assert_eq!([memory.word(4), memory.word(8), memory.word(12)], [0x11, 0x12, 0x13]);
        assert_eq!([memory.word(0x20), memory.word(0x24)], [0x21, 0x22]);
        assert_eq!(memory.word(0), 0);
    }

    #[test]
    fn zero_counts_copy_nothing() {
        let mut memory = Memory::new();
        memory.descriptor(0, FIRST, 4, 0);
        memory.first[0..4].copy_from_slice(&0x55u32.to_le_bytes());
        memory.with_context(|context| assert_eq!(helper_6deb7c(context, GuestAddress::new(BASE), GuestAddress::new(COPIES), 1), Ok(())));
        assert_eq!(memory.word(4), 0);
        memory.with_context(|context| assert_eq!(helper_6deb7c(context, GuestAddress::new(BASE), GuestAddress::new(COPIES), 0), Ok(())));
        assert_eq!(memory.word(4), 0);
    }
}
