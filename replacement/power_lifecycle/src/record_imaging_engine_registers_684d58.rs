//! Imaging engine log append and the 0xfff2_8000 service register, image-4
//! bodies 0x00684d58, 0x006856cc and 0x006858e0.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

pub const ENGINE_STATE: u32 = 0x0082_0f88;
pub const SERVICE: u32 = 0xfff2_8000;

fn operation<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<(), MemoryError> {
    context.write_u32(GuestAddress::new(ENGINE_STATE + 4), value)
}

/// The image-4 body at 0x00684d58.
pub fn helper_684d58<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>,
    destination: GuestAddress,
    source: GuestAddress,
    mut count: i32,
) -> Result<GuestAddress, MemoryError> {
    let mut cursor = destination;
    let mut read = source;
    while count != 0 {
        let value = context.read_u8(read)?;
        if value == 0 { break; }
        context.write_u8(cursor, value)?;
        cursor = cursor.checked_add(1)?;
        read = read.checked_add(1)?;
        count -= 1;
    }
    Ok(cursor)
}

/// The image-4 body at 0x006856cc.
pub fn helper_6856cc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, _argument: u32, output: GuestAddress) -> Result<u32, MemoryError> {
    operation(context, 8)?;
    let value = context.read_u32(GuestAddress::new(SERVICE))?;
    context.write_u32(output, (value >> 12) & 1)?;
    context.write_u32(output.checked_add(4)?, (value >> 13) & 1)?;
    Ok(0)
}

/// The image-4 body at 0x006858e0.
pub fn helper_6858e0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, _argument: u32, output: GuestAddress) -> Result<u32, MemoryError> {
    operation(context, 9)?;
    let value = context.read_u32(GuestAddress::new(SERVICE))?;
    let stage = (value >> 16) & 3;
    let stage = match stage { 1 => 1, 2 => 2, 3 => 3, _ => 0 };
    context.write_u32(output, stage)?;
    context.write_u32(output.checked_add(4)?, (value >> 18) & 1)?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_address_space::{
        ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission,
    };

    const STATE: u32 = 0x0082_0f88;
    const SOURCE: u32 = 0x0010_0000;
    const DESTINATION: u32 = 0x0010_0100;
    const OUTPUT: u32 = 0x0010_0200;

    struct Memory {
        state: [u8; 0x20],
        source: [u8; 0x40],
        destination: [u8; 0x40],
        output: [u8; 0x10],
        service: [u8; 4],
    }

    impl Memory {
        fn new() -> Self {
            Self { state: [0; 0x20], source: [0; 0x40], destination: [0; 0x40], output: [0; 0x10], service: [0; 4] }
        }

        fn with_context<R>(&mut self, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
            let descriptor = |base, length| {
                RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap()
            };
            let mut regions = [
                MemoryRegion::new(descriptor(STATE, 0x20), &mut self.state).unwrap(),
                MemoryRegion::new(descriptor(SOURCE, 0x40), &mut self.source).unwrap(),
                MemoryRegion::new(descriptor(DESTINATION, 0x40), &mut self.destination).unwrap(),
                MemoryRegion::new(descriptor(OUTPUT, 0x10), &mut self.output).unwrap(),
                MemoryRegion::new(descriptor(SERVICE, 4), &mut self.service).unwrap(),
            ];
            body(&mut RecordContext::new(HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap(), ()))
        }

        fn operation(&self) -> u32 { u32::from_le_bytes(self.state[4..8].try_into().unwrap()) }
        fn output(&self, offset: usize) -> u32 { u32::from_le_bytes(self.output[offset..offset + 4].try_into().unwrap()) }
    }

    #[test]
    fn append_copies_until_the_terminator_or_the_count() {
        let mut memory = Memory::new();
        memory.source[..4].copy_from_slice(b"abcd");
        memory.source[4] = 0;
        memory.source[5..9].copy_from_slice(b"efgh");
        memory.with_context(|context| {
            assert_eq!(helper_684d58(context, GuestAddress::new(DESTINATION), GuestAddress::new(SOURCE), 16), Ok(GuestAddress::new(DESTINATION + 4)));
        });
        assert_eq!(&memory.destination[..4], b"abcd");
        assert_eq!(memory.destination[4], 0);
        memory.with_context(|context| {
            assert_eq!(helper_684d58(context, GuestAddress::new(DESTINATION + 0x20), GuestAddress::new(SOURCE + 5), 3), Ok(GuestAddress::new(DESTINATION + 0x23)));
        });
        assert_eq!(&memory.destination[0x20..0x23], b"efg");
        memory.with_context(|context| {
            assert_eq!(helper_684d58(context, GuestAddress::new(DESTINATION), GuestAddress::new(SOURCE), 0), Ok(GuestAddress::new(DESTINATION)));
        });
    }

    #[test]
    fn enable_pair_reports_the_two_gate_bits() {
        let mut memory = Memory::new();
        memory.service.copy_from_slice(&0x0000_3000u32.to_le_bytes());
        memory.with_context(|context| assert_eq!(helper_6856cc(context, 0, GuestAddress::new(OUTPUT)), Ok(0)));
        assert_eq!([memory.output(0), memory.output(4)], [1, 1]);
        memory.service.copy_from_slice(&0x0000_1000u32.to_le_bytes());
        memory.with_context(|context| assert_eq!(helper_6856cc(context, 0, GuestAddress::new(OUTPUT)), Ok(0)));
        assert_eq!([memory.output(0), memory.output(4)], [1, 0]);
        assert_eq!(memory.operation(), 8);
    }

    #[test]
    fn stage_flags_map_the_two_stage_bits_and_the_ready_bit() {
        let mut memory = Memory::new();
        for (value, stage, ready) in [(0x0000_0000u32, 0u32, 0u32), (0x0001_0000, 1, 0), (0x0006_0004, 2, 1), (0x0003_0000, 3, 0), (0x0004_0004, 0, 1)] {
            memory.service.copy_from_slice(&value.to_le_bytes());
            memory.with_context(|context| assert_eq!(helper_6858e0(context, 0, GuestAddress::new(OUTPUT)), Ok(0)));
            assert_eq!([memory.output(0), memory.output(4)], [stage, ready]);
        }
        assert_eq!(memory.operation(), 9);
    }
}
