//! Stock-shaped bodies over the controller block at 0x4171_5ac0.
//!
//! Each body is the image-5 instruction sequence at the address in its name.

use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

pub const CONTROLLER: u32 = 0x4171_5ac0;
const STATUS: u32 = 0x83c;
const STARTED: u32 = 0x840;
const COMMAND: u32 = 0x848;

pub const ENTRY_TABLE: u32 = 0x0106_5f5c;
pub const MODE_BYTES: u32 = 0x014d_8898;
pub const RELEASE: u32 = 0xfff1_4000;
pub const RECORD_STRIDE: u32 = 0xb8;
pub const RECORD_PAYLOAD: u32 = 184;

fn state(offset: u32) -> GuestAddress {
    GuestAddress::new(CONTROLLER.wrapping_add(offset))
}

fn record(base: GuestAddress, index: u32) -> Result<GuestAddress, MemoryError> {
    base.checked_add(4)?.checked_add(index.checked_mul(RECORD_STRIDE).ok_or(MemoryError::AddressOverflow)?)
}

/// 0x01064494
pub fn helper_1064494<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    if context.read_u32(state(STATUS))? == 3 {
        context.write_u8(state(STARTED), 1)?;
    }
    Ok(0)
}

/// 0x010648e8
pub fn helper_10648e8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    context.write_u32(GuestAddress::new(RELEASE), 0)?;
    context.write_u32(GuestAddress::new(RELEASE), 1)
}

/// 0x01065584
pub fn helper_1065584<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32, out: GuestAddress) -> Result<(), MemoryError> {
    let entry = GuestAddress::new(ENTRY_TABLE).checked_add(index.checked_mul(4).ok_or(MemoryError::AddressOverflow)?)?;
    let value = context.read_u32(entry)?;
    context.write_u16(out, value as u16)?;
    let high = context.read_u16(entry.checked_add(2)?)?;
    context.write_u16(out.checked_add(2)?, high)
}

/// 0x0106647c
pub fn helper_106647c<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, value: u32) -> Result<u32, MemoryError> {
    Ok(value.wrapping_mul(value))
}

/// 0x010664ac
pub fn helper_10664ac<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>,
    extra: GuestAddress,
    source: GuestAddress,
    destination: GuestAddress,
) -> Result<(), MemoryError> {
    let value = context.read_u32(source)?;
    context.write_u32(destination, value)?;
    let half = context.read_u16(source.checked_add(4)?)?;
    context.write_u16(destination.checked_add(4)?, half)?;
    context.copy_bytes(destination.checked_add(6)?, extra, ByteCount(4))?;
    context.copy_bytes(destination.checked_add(0x0a)?, extra.checked_add(4)?, ByteCount(4))?;
    let half = context.read_u16(source.checked_add(0x0e)?)?;
    context.write_u16(destination.checked_add(0x0e)?, half)?;
    context.copy_bytes(destination.checked_add(0x10)?, source.checked_add(0x10)?, ByteCount(4))?;
    let value = context.read_u32(source.checked_add(0x54)?)?;
    context.write_u32(destination.checked_add(0x54)?, value)?;
    context.copy_bytes(destination.checked_add(0x14)?, source.checked_add(0x14)?, ByteCount(52))
}

/// 0x01066680
pub fn helper_1066680<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, base: GuestAddress, index: u32) -> Result<(), MemoryError> {
    let payload = base.checked_add(4)?;
    let mut element = [0u8; RECORD_PAYLOAD as usize];
    let at = record(base, index)?;
    for (offset, byte) in element.iter_mut().enumerate() {
        *byte = context.read_u8(at.checked_add(offset as u32)?)?;
    }
    let mut slot = index;
    while slot > 0 {
        let from = record(base, slot - 1)?;
        context.copy_bytes(from.checked_add(RECORD_STRIDE)?, from, ByteCount(RECORD_PAYLOAD))?;
        slot -= 1;
    }
    for (offset, byte) in element.into_iter().enumerate() {
        context.write_u8(payload.checked_add(offset as u32)?, byte)?;
    }
    Ok(())
}

/// 0x010669a4
pub fn helper_10669a4<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>,
    base: GuestAddress,
    first: u32,
    second: u32,
    out: GuestAddress,
) -> Result<u32, MemoryError> {
    let count = u32::from(context.read_u8(base)?);
    if count == 0 {
        return Ok(0);
    }
    let mut index = 0u32;
    loop {
        let at = record(base, index)?;
        if u32::from(context.read_u8(at.checked_add(0xb5)?)?) == first
            && u32::from(context.read_u8(at.checked_add(0xb4)?)?) == second
        {
            if index == 0 {
                return Ok(0);
            }
            context.write_u8(out, index as u8)?;
            return Ok(1);
        }
        index = (index + 1) & 0xff;
        if index == count {
            return Ok(0);
        }
    }
}

/// 0x01066a18
pub fn helper_1066a18<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    context.write_u8(GuestAddress::new(MODE_BYTES.wrapping_add(1)), 0xff)?;
    context.write_u8(GuestAddress::new(MODE_BYTES), 0xff)
}

/// 0x01067e00
pub fn helper_1067e00<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<(), MemoryError> {
    context.write_u32(state(COMMAND), value)
}

/// 0x01069824
pub fn helper_1069824<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    Ok(())
}

/// 0x0106bfa8
pub fn helper_106bfa8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<u32, MemoryError> {
    Ok(u32::from(context.read_u32(state(COMMAND))? == value))
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 bodies at 0x01064494-0x0106bfa8.
    use super::*;
    use crate::record_address_space::{
        GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission,
    };

    const ARRAY: u32 = 0x0030_0000;
    const SOURCE: u32 = 0x0040_0000;
    const EXTRA: u32 = 0x0040_1000;
    const DESTINATION: u32 = 0x0040_2000;

    struct Memory {
        controller: [u8; 0x900],
        entries: [u8; 0x40],
        modes: [u8; 2],
        release: [u8; 4],
        array: [u8; 0x400],
        source: [u8; 0x60],
        extra: [u8; 8],
        destination: [u8; 0x60],
        out: [u8; 4],
    }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(CONTROLLER, 0x900), &mut memory.controller).unwrap(),
            MemoryRegion::new(descriptor(ENTRY_TABLE, 0x40), &mut memory.entries).unwrap(),
            MemoryRegion::new(descriptor(MODE_BYTES, 2), &mut memory.modes).unwrap(),
            MemoryRegion::new(descriptor(RELEASE, 4), &mut memory.release).unwrap(),
            MemoryRegion::new(descriptor(ARRAY, 0x400), &mut memory.array).unwrap(),
            MemoryRegion::new(descriptor(SOURCE, 0x60), &mut memory.source).unwrap(),
            MemoryRegion::new(descriptor(EXTRA, 8), &mut memory.extra).unwrap(),
            MemoryRegion::new(descriptor(DESTINATION, 0x60), &mut memory.destination).unwrap(),
            MemoryRegion::new(descriptor(0x0050_0000, 4), &mut memory.out).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    fn memory() -> Memory {
        let mut memory = Memory {
            controller: [0; 0x900],
            entries: [0; 0x40],
            modes: [0; 2],
            release: [0; 4],
            array: [0; 0x400],
            source: [0; 0x60],
            extra: [0; 8],
            destination: [0; 0x60],
            out: [0; 4],
        };
        for (index, byte) in memory.entries.iter_mut().enumerate() { *byte = index as u8; }
        for (index, byte) in memory.source.iter_mut().enumerate() { *byte = (index as u8).wrapping_add(0x80); }
        memory.extra = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
        memory
    }

    #[test]
    fn the_status_word_three_marks_the_controller_started() {
        let mut memory = memory();
        with_memory(&mut memory, |context| assert_eq!(helper_1064494(context), Ok(0)));
        assert_eq!(memory.controller[STARTED as usize], 0);
        memory.controller[STATUS as usize..STATUS as usize + 4].copy_from_slice(&3u32.to_le_bytes());
        with_memory(&mut memory, |context| assert_eq!(helper_1064494(context), Ok(0)));
        assert_eq!(memory.controller[STARTED as usize], 1);
    }

    #[test]
    fn the_release_register_is_cleared_then_set() {
        let mut memory = memory();
        with_memory(&mut memory, |context| assert_eq!(helper_10648e8(context), Ok(())));
        assert_eq!(&memory.release, &1u32.to_le_bytes());
    }

    #[test]
    fn the_entry_word_is_split_into_two_halfwords() {
        let mut memory = memory();
        with_memory(&mut memory, |context| assert_eq!(helper_1065584(context, 2, GuestAddress::new(0x0050_0000)), Ok(())));
        assert_eq!(&memory.out, &[0x08, 0x09, 0x0a, 0x0b]);
    }

    #[test]
    fn the_square_wraps_at_32_bits() {
        let mut memory = memory();
        with_memory(&mut memory, |context| {
            assert_eq!(helper_106647c(context, 7), Ok(49));
            assert_eq!(helper_106647c(context, 0x0001_0000), Ok(0));
        });
    }

    #[test]
    fn the_record_copy_keeps_its_field_layout() {
        let mut memory = memory();
        with_memory(&mut memory, |context| {
            let copied = helper_10664ac(context, GuestAddress::new(EXTRA), GuestAddress::new(SOURCE), GuestAddress::new(DESTINATION));
            assert_eq!(copied, Ok(()));
        });
        assert_eq!(&memory.destination[0..4], &memory.source[0..4]);
        assert_eq!(&memory.destination[4..6], &memory.source[4..6]);
        assert_eq!(&memory.destination[6..10], &memory.extra[0..4]);
        assert_eq!(&memory.destination[10..14], &memory.extra[4..8]);
        assert_eq!(&memory.destination[0x0e..0x10], &memory.source[0x0e..0x10]);
        assert_eq!(&memory.destination[0x10..0x14], &memory.source[0x10..0x14]);
        assert_eq!(&memory.destination[0x14..0x48], &memory.source[0x14..0x48]);
        assert_eq!(&memory.destination[0x48..0x54], &[0; 12]);
        assert_eq!(&memory.destination[0x54..0x58], &memory.source[0x54..0x58]);
    }

    fn array_memory() -> Memory {
        let mut memory = memory();
        memory.array[0] = 3;
        for index in 1..4u8 {
            for offset in 0..RECORD_PAYLOAD as usize {
                memory.array[4 + (index as usize - 1) * RECORD_STRIDE as usize + offset] = index.wrapping_mul(0x10).wrapping_add(offset as u8);
            }
        }
        memory
    }

    fn element_byte(memory: &Memory, slot: usize, offset: usize) -> u8 {
        memory.array[4 + slot * RECORD_STRIDE as usize + offset]
    }

    #[test]
    fn the_element_at_the_index_moves_to_the_front() {
        let mut memory = array_memory();
        with_memory(&mut memory, |context| assert_eq!(helper_1066680(context, GuestAddress::new(ARRAY), 2), Ok(())));
        assert_eq!(memory.array[0], 3);
        assert_eq!(element_byte(&memory, 0, 0), 0x30);
        assert_eq!(element_byte(&memory, 0, 183), 0x30u8.wrapping_add(183));
        assert_eq!(element_byte(&memory, 1, 0), 0x10);
        assert_eq!(element_byte(&memory, 1, 183), 0x10u8.wrapping_add(183));
        assert_eq!(element_byte(&memory, 2, 0), 0x20);
        assert_eq!(element_byte(&memory, 2, 183), 0x20u8.wrapping_add(183));

        let mut memory = array_memory();
        with_memory(&mut memory, |context| assert_eq!(helper_1066680(context, GuestAddress::new(ARRAY), 0), Ok(())));
        assert_eq!(element_byte(&memory, 0, 0), 0x10);
        assert_eq!(element_byte(&memory, 1, 0), 0x20);
    }

    fn record_array() -> Memory {
        let mut memory = memory();
        memory.array[0] = 3;
        for index in 0..3usize {
            let at = 4 + index * RECORD_STRIDE as usize;
            memory.array[at + 0xb4] = index as u8;
            memory.array[at + 0xb5] = index as u8 + 0x20;
        }
        memory
    }

    #[test]
    fn the_record_search_reports_only_a_nonzero_match() {
        let mut memory = record_array();
        with_memory(&mut memory, |context| {
            assert_eq!(helper_10669a4(context, GuestAddress::new(ARRAY), 0x20, 0, GuestAddress::new(0x0050_0000)), Ok(0));
            assert_eq!(helper_10669a4(context, GuestAddress::new(ARRAY), 0x22, 2, GuestAddress::new(0x0050_0000)), Ok(1));
        });
        assert_eq!(memory.out, [2, 0, 0, 0]);
        with_memory(&mut memory, |context| assert_eq!(helper_10669a4(context, GuestAddress::new(ARRAY), 0x30, 3, GuestAddress::new(0x0050_0000)), Ok(0)));
        memory.array[0] = 0;
        with_memory(&mut memory, |context| assert_eq!(helper_10669a4(context, GuestAddress::new(ARRAY), 0x20, 0, GuestAddress::new(0x0050_0000)), Ok(0)));
    }

    #[test]
    fn the_mode_bytes_and_the_command_word_are_independent() {
        let mut memory = memory();
        with_memory(&mut memory, |context| {
            assert_eq!(helper_1066a18(context), Ok(()));
            assert_eq!(helper_1067e00(context, 0x1234_5678), Ok(()));
            assert_eq!(helper_106bfa8(context, 0x1234_5678), Ok(1));
            assert_eq!(helper_106bfa8(context, 0x1234_5679), Ok(0));
            assert_eq!(helper_1069824(context), Ok(()));
        });
        assert_eq!(&memory.modes, &[0xff, 0xff]);
        assert_eq!(&memory.controller[COMMAND as usize..COMMAND as usize + 4], &0x1234_5678u32.to_le_bytes());
    }
}
