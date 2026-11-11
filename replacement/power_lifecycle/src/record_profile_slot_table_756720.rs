//! The sixteen-entry profile slot registry at 0x0091_5d38 and the two profile
//! resources at 0x0096_ebd0: image-4 bodies 0x0075_6720-0x0075_e698.

use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_profile_accessors::{copy_words, selected_bank};

/// Registry header: current slot index, next bank number, selected bank.
pub const REGISTRY: u32 = 0x0091_5d38;
/// First `RegistrySlot`.
pub const SLOTS: u32 = 0x0091_5d44;
pub const SLOT_STRIDE: u32 = 0x38;
pub const SLOT_COUNT: u32 = 16;
pub const RESOURCES: u32 = 0x0096_ebd0;
pub const RESOURCE_STRIDE: u32 = 0x2158;
pub const SELECTED_RESOURCE: u32 = 0x0097_4338;
/// The highest bank number the image-5 allocator hands out.
pub const BANK_LIMIT: u32 = 0xffd;
const INVALID_SERIAL: u32 = u32::MAX;

fn registry_word(at: u32) -> GuestAddress {
    GuestAddress::new(REGISTRY + at)
}

fn slot(index: u32) -> GuestAddress {
    GuestAddress::new(SLOTS.wrapping_add(index.wrapping_mul(SLOT_STRIDE)))
}

fn resource(index: u32) -> GuestAddress {
    GuestAddress::new(RESOURCES.wrapping_add(index.wrapping_mul(RESOURCE_STRIDE)))
}

/// The image-4 body at 0x0075_a984.
pub fn helper_75a984<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, handle: u32) -> Result<u32, MemoryError> {
    Ok(handle & 15)
}

/// The image-4 body at 0x0075_a99c.
pub fn helper_75a99c<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, handle: u32) -> Result<u32, MemoryError> {
    Ok((handle >> 4) & 0xfff)
}

/// The image-4 body at 0x0075_a9e4.
pub fn helper_75a9e4<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, handle: u32) -> Result<u32, MemoryError> {
    Ok((handle >> 16) & 1)
}

/// The image-4 body at 0x0075_ade8.
pub fn helper_75ade8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress, source: GuestAddress) -> Result<(), MemoryError> {
    copy_words::<3>(context, destination, source)
}

/// The image-4 body at 0x0075_cc04.
pub fn helper_75cc04<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, base: GuestAddress) -> Result<i32, MemoryError> {
    context.read_u16(base.checked_add(56)?).map(|value| i32::from(value as i16))
}

/// The image-4 body at 0x0075_e118.
pub fn helper_75e118<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    for offset in 0..0x38c {
        context.write_u8(GuestAddress::new(REGISTRY + offset), 0)?;
    }
    for index in 0..SLOT_COUNT {
        let record = slot(index);
        for offset in 0..SLOT_STRIDE {
            context.write_u8(record.checked_add(offset)?, 0)?;
        }
        context.write_u32(record.checked_add(4)?, 0)?;
        context.write_u32(record, INVALID_SERIAL)?;
        context.write_u32(record.checked_add(8)?, 0)?;
    }
    context.write_u32(registry_word(0), 0)?;
    context.write_u32(registry_word(4), 1)?;
    context.write_u32(registry_word(8), 0)?;
    context.write_u32(GuestAddress::new(SELECTED_RESOURCE), RESOURCES)
}

/// The image-4 body at 0x0075_e20c: only banks zero and one have a resource.
pub fn helper_75e20c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, bank: u32) -> Result<(), MemoryError> {
    if bank > 1 { return Ok(()); }
    context.write_u32(registry_word(8), bank)?;
    context.write_u32(GuestAddress::new(SELECTED_RESOURCE), RESOURCES.wrapping_add(bank.wrapping_mul(RESOURCE_STRIDE)))
}

/// The image-4 body at 0x0075_e250: the index of the first slot at or after the
/// registry index whose serial is unused, or 16 when all sixteen are used. The
/// registry index becomes the slot after the one returned.
pub fn helper_75e250<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    let current = context.read_u32(registry_word(0))?;
    let mut index = current;
    let mut found = context.read_u32(slot(index))? == INVALID_SERIAL;
    if !found {
        let mut remaining = SLOT_COUNT;
        loop {
            index = if index + 1 > SLOT_COUNT - 1 { 0 } else { index + 1 };
            remaining -= 1;
            if remaining == 0 { break; }
            if context.read_u32(slot(index))? == INVALID_SERIAL {
                found = true;
                break;
            }
        }
        if !found {
            if context.read_u32(slot(index))? != INVALID_SERIAL { return Ok(SLOT_COUNT); }
        }
    }
    context.write_u32(registry_word(0), if index + 1 > SLOT_COUNT - 1 { 0 } else { index + 1 })?;
    Ok(index)
}

/// The image-4 body at 0x0075_e2f4: the first bank number at or after the
/// registry candidate that no used slot names, and the next candidate.
pub fn helper_75e2f4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    let mut candidate = context.read_u32(registry_word(4))?;
    if candidate > BANK_LIMIT { candidate = 1; }
    let mut attempts = 0xffe;
    loop {
        let mut used = context.read_u32(slot(0))? == candidate;
        let mut index = 1;
        while !used && index < SLOT_COUNT {
            used = context.read_u32(slot(index))? == candidate;
            index += 1;
        }
        if !used { break; }
        candidate = if candidate <= BANK_LIMIT - 1 { candidate + 1 } else { 1 };
        attempts -= 1;
        if attempts == 0 { break; }
    }
    let next = if candidate <= BANK_LIMIT - 1 { candidate + 1 } else { 1 };
    context.write_u32(registry_word(4), next)?;
    Ok(candidate)
}

/// The image-4 body at 0x0075_e3a0.
pub fn helper_75e3a0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, bank: u32) -> Result<u32, MemoryError> {
    let index = helper_75a984(context, handle)?;
    let serial = helper_75a99c(context, handle)?;
    let record = slot(index);
    context.write_u32(record, serial)?;
    context.write_u32(record.checked_add(8)?, bank)?;
    Ok(1)
}

/// The image-4 body at 0x0075_e3e0: 43 permission bytes from `source`, or the
/// all-zero default table at 0x0081_8e3c when `source` is null.
pub fn helper_75e3e0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, source: Option<GuestAddress>) -> Result<u32, MemoryError> {
    let index = helper_75a984(context, handle)?;
    let permissions = slot(index).checked_add(12)?;
    match source {
        Some(source) => context.copy_bytes(permissions, source, ByteCount(43))?,
        None => {
            for offset in 0..43 {
                context.write_u8(permissions.checked_add(offset)?, 0)?;
            }
        }
    }
    Ok(1)
}

/// The image-4 body at 0x0075_e464.
pub fn helper_75e464<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<(), MemoryError> {
    if handle == INVALID_SERIAL { return Ok(()); }
    let index = helper_75a984(context, handle)?;
    let record = slot(index);
    for offset in 0..SLOT_STRIDE {
        context.write_u8(record.checked_add(offset)?, 0)?;
    }
    context.write_u32(record.checked_add(4)?, 0)?;
    context.write_u32(record, INVALID_SERIAL)?;
    context.write_u32(record.checked_add(8)?, 0)
}

/// The image-4 body at 0x0075_e4f0.
pub fn helper_75e4f0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32) -> Result<u32, MemoryError> {
    context.read_u32(slot(index))
}

/// The image-4 body at 0x0075_e648.
pub fn helper_75e648<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, bank: u32) -> Result<u32, MemoryError> {
    for index in 0..SLOT_COUNT {
        let record = slot(index);
        if context.read_u32(record)? == INVALID_SERIAL { continue; }
        if context.read_u32(record.checked_add(8)?)? == bank { return Ok(1); }
    }
    Ok(0)
}

/// The image-4 body at 0x0075_e698.
pub fn helper_75e698<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32) -> Result<(), MemoryError> {
    let selected = selected_bank(context)?.0;
    context.copy_bytes(resource(index), resource(selected), ByteCount(RESOURCE_STRIDE))
}

/// The image-4 body at 0x0075_a9fc: the next free slot index in the low nibble,
/// the next free bank number in bits 4 to 15, and 0xff in bits 16 to 23.
pub fn helper_75a9fc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    let index = helper_75e250(context)?;
    if index > SLOT_COUNT - 1 { return Ok(INVALID_SERIAL); }
    let bank = helper_75e2f4(context)?;
    if bank > BANK_LIMIT { return Ok(INVALID_SERIAL); }
    Ok((index & 15) | (bank << 4) | 0x00ff_0000)
}

/// The image-4 body at 0x0075_aa3c: the bank a new profile of `kind` takes.
pub fn helper_75aa3c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, kind: u32) -> Result<u32, MemoryError> {
    Ok(match kind {
        0 => 0,
        1 => 1,
        3 => {
            let selected = selected_bank(context)?.0;
            let bank = if selected + 1 > 1 { 0 } else { selected + 1 };
            if helper_75e648(context, bank)? != 0 { 2 } else { bank }
        }
        4 => selected_bank(context)?.0.min(2),
        _ => 2,
    })
}

/// The image-4 body at 0x0075_aab4.
pub fn helper_75aab4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<u32, MemoryError> {
    if handle == INVALID_SERIAL { return Ok(0); }
    let index = helper_75a984(context, handle)?;
    let serial = helper_75a99c(context, handle)?;
    Ok(u32::from(helper_75e4f0(context, index)? == serial))
}

/// The image-4 body at 0x0075_e1b4.
pub fn helper_75e1b4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    helper_75e118(context)?;
    for offset in 0..0x42b0 {
        context.write_u8(GuestAddress::new(RESOURCES + offset), 0)?;
    }
    crate::record_shared_block_fill_756700::helper_756700(context)
}

/// The image-4 body at 0x0075_6720.
pub fn helper_756720<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    helper_75e1b4(context)?;
    crate::record_shared_block_fill_756700::helper_756700(context)
}

/// The image-4 body at 0x0075_a63c: claims a slot for a profile of `kind` and
/// returns its handle, or -1 when the registry has none to give.
pub fn helper_75a63c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, kind: u32, permissions: Option<GuestAddress>) -> Result<u32, MemoryError> {
    let bank = helper_75aa3c(context, kind)?;
    if bank > 1 { return Ok(INVALID_SERIAL); }
    let handle = helper_75a9fc(context)?;
    if handle == INVALID_SERIAL { return Ok(INVALID_SERIAL); }
    if helper_75e3a0(context, handle, bank)? == 0 { return Ok(INVALID_SERIAL); }
    if helper_75e3e0(context, handle, permissions)? == 0 { return Ok(INVALID_SERIAL); }
    if kind == 3 {
        helper_75e698(context, bank)?;
        helper_75e20c(context, bank)?;
    }
    Ok(handle)
}

/// The image-4 body at 0x0075_a6cc.
pub fn helper_75a6cc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<(), MemoryError> {
    if helper_75aab4(context, handle)? == 0 { return Ok(()); }
    if helper_75a9e4(context, handle)? == 0 { return Ok(()); }
    helper_75e464(context, handle)
}

mod exports {
    use super::*;
    use crate::record_native_exports::stock_export;

    stock_export!(xm1_power_record_helper_756720, helper_756720_abi, (), () -> (), (), (), crate::record_profile_slot_table_756720::helper_756720, |_| ());
    stock_export!(
        xm1_power_record_helper_75a63c, helper_75a63c_abi, (),
        (kind: u32, permissions: u32) -> u32,
        (kind: u32, permissions: Option<GuestAddress>),
        (kind, crate::record_native_boundary::optional_address(permissions)),
        crate::record_profile_slot_table_756720::helper_75a63c,
        |handle| handle
    );
    stock_export!(
        xm1_power_record_helper_75a6cc, helper_75a6cc_abi, (),
        (handle: u32) -> (),
        (handle: u32),
        (handle),
        crate::record_profile_slot_table_756720::helper_75a6cc,
        |_| ()
    );
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-4 bodies at 0x0075_6720-0x0075_e698.
    use super::*;
    use crate::record_address_space::{GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const HANDLE: u32 = 0x00fe_0000;

    struct Memory {
        registry: [u8; 0x38c],
        resources: [u8; 0x42b0],
        selected: [u8; 16],
        block: [u8; 0x9d4],
    }

    impl Memory {
        fn new() -> Self {
            Memory { registry: [0; 0x38c], resources: [0; 0x42b0], selected: [0; 16], block: [0; 0x9d4] }
        }

        fn with_context<R>(&mut self, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
            let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
            let mut regions = [
                MemoryRegion::new(descriptor(REGISTRY, 0x38c), &mut self.registry).unwrap(),
                MemoryRegion::new(descriptor(RESOURCES, 0x42b0), &mut self.resources).unwrap(),
                MemoryRegion::new(descriptor(SELECTED_RESOURCE, 16), &mut self.selected).unwrap(),
                MemoryRegion::new(descriptor(0x0091_904c, 0x9d4), &mut self.block).unwrap(),
            ];
            body(&mut RecordContext::new(HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap(), ()))
        }

        fn header(&self, at: usize) -> u32 { u32::from_le_bytes(self.registry[at..at + 4].try_into().unwrap()) }

        fn set_header(&mut self, at: usize, value: u32) { self.registry[at..at + 4].copy_from_slice(&value.to_le_bytes()); }

        fn set_serial(&mut self, index: usize, value: u32) {
            let at = 0x0c + index * 0x38;
            self.registry[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }

        fn serial(&self, index: usize) -> u32 {
            let at = 0x0c + index * 0x38;
            u32::from_le_bytes(self.registry[at..at + 4].try_into().unwrap())
        }

        fn set_bank(&mut self, index: usize, value: u32) {
            let at = 0x0c + index * 0x38 + 8;
            self.registry[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }

        fn pointer(&self) -> u32 { u32::from_le_bytes(self.selected[..4].try_into().unwrap()) }
    }

    #[test]
    fn the_reset_marks_every_slot_unused_and_points_at_bank_zero() {
        let mut memory = Memory::new();
        memory.registry = [0xaa; 0x38c];
        memory.with_context(|context| assert_eq!(helper_75e118(context), Ok(())));
        assert_eq!(memory.header(0), 0);
        assert_eq!(memory.header(4), 1);
        assert_eq!(memory.header(8), 0);
        assert_eq!(memory.pointer(), RESOURCES);
        for index in 0..16 {
            assert_eq!(memory.serial(index), INVALID_SERIAL);
            assert_eq!(memory.header(0x0c + index * 0x38 + 4), 0);
            assert_eq!(memory.header(0x0c + index * 0x38 + 8), 0);
        }
        assert_eq!(&memory.registry[0x2c..0x38], &[0u8; 12]);
    }

    #[test]
    fn selection_moves_the_pointer_for_banks_zero_and_one_only() {
        let mut memory = Memory::new();
        memory.set_header(8, 0);
        memory.with_context(|context| assert_eq!(helper_75e20c(context, 1), Ok(())));
        assert_eq!(memory.header(8), 1);
        assert_eq!(memory.pointer(), RESOURCES + RESOURCE_STRIDE);
        memory.with_context(|context| assert_eq!(helper_75e20c(context, 2), Ok(())));
        assert_eq!(memory.header(8), 1);
        assert_eq!(memory.pointer(), RESOURCES + RESOURCE_STRIDE);
        memory.with_context(|context| assert_eq!(helper_75e20c(context, 0), Ok(())));
        assert_eq!(memory.header(8), 0);
        assert_eq!(memory.pointer(), RESOURCES);
    }

    #[test]
    fn the_next_free_slot_follows_the_registry_index_and_advances_past_it() {
        let mut memory = Memory::new();
        for index in 0..16 { memory.set_serial(index, INVALID_SERIAL); }
        memory.set_header(0, 0);
        memory.with_context(|context| assert_eq!(helper_75e250(context), Ok(0)));
        assert_eq!(memory.header(0), 1);
        memory.set_serial(0, 5);
        memory.set_serial(1, 6);
        memory.with_context(|context| assert_eq!(helper_75e250(context), Ok(2)));
        assert_eq!(memory.header(0), 3);
        memory.set_serial(0, INVALID_SERIAL);
        memory.set_serial(1, INVALID_SERIAL);
        memory.set_header(0, 15);
        memory.set_serial(15, 7);
        memory.with_context(|context| assert_eq!(helper_75e250(context), Ok(0)));
        assert_eq!(memory.header(0), 1);
    }

    #[test]
    fn every_slot_in_use_leaves_the_registry_index_alone() {
        let mut memory = Memory::new();
        for index in 0..16 { memory.set_serial(index, index as u32 + 1); }
        memory.set_header(0, 4);
        memory.with_context(|context| assert_eq!(helper_75e250(context), Ok(16)));
        assert_eq!(memory.header(0), 4);
    }

    #[test]
    fn the_next_bank_skips_the_numbers_used_slots_name() {
        let mut memory = Memory::new();
        for index in 0..16 { memory.set_serial(index, INVALID_SERIAL); }
        memory.set_header(4, 3);
        memory.with_context(|context| assert_eq!(helper_75e2f4(context), Ok(3)));
        assert_eq!(memory.header(4), 4);
        memory.set_serial(1, 3);
        memory.set_header(4, 3);
        memory.with_context(|context| assert_eq!(helper_75e2f4(context), Ok(4)));
        assert_eq!(memory.header(4), 5);
        memory.set_header(4, 0xffe);
        memory.with_context(|context| assert_eq!(helper_75e2f4(context), Ok(1)));
        assert_eq!(memory.header(4), 2);
        memory.set_serial(0, 0xffd);
        memory.set_header(4, 0xffd);
        memory.with_context(|context| assert_eq!(helper_75e2f4(context), Ok(1)));
        assert_eq!(memory.header(4), 2);
    }

    #[test]
    fn a_claim_writes_the_serial_and_the_bank_of_the_named_slot() {
        let mut memory = Memory::new();
        memory.with_context(|context| assert_eq!(helper_75e3a0(context, 0x0000_0213, 1), Ok(1)));
        assert_eq!(memory.serial(3), 0x21);
        assert_eq!(memory.header(0x0c + 3 * 0x38 + 8), 1);
        assert_eq!(memory.serial(2), 0);
    }

    #[test]
    fn the_permission_copy_takes_the_source_or_the_zero_default() {
        let mut memory = Memory::new();
        let mut source = [0u8; 43];
        for (index, byte) in source.iter_mut().enumerate() { *byte = index as u8 | 0x80; }
        let mut source_region = source;
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        {
            let mut regions = [
                MemoryRegion::new(descriptor(REGISTRY, 0x38c), &mut memory.registry).unwrap(),
                MemoryRegion::new(descriptor(0x0010_0000, 43), &mut source_region).unwrap(),
            ];
            let mut context = RecordContext::new(HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap(), ());
            assert_eq!(helper_75e3e0(&mut context, 0x0001_0021, Some(GuestAddress::new(0x0010_0000))), Ok(1));
        }
        assert_eq!(&memory.registry[0x0c + 1 * 0x38 + 12..0x0c + 1 * 0x38 + 55], &source);
        memory.registry[0x0c + 1 * 0x38 + 12] = 0xff;
        memory.with_context(|context| assert_eq!(helper_75e3e0(context, 0x0001_0021, None), Ok(1)));
        assert_eq!(&memory.registry[0x0c + 1 * 0x38 + 12..0x0c + 1 * 0x38 + 55], &[0u8; 43]);
    }

    #[test]
    fn release_clears_the_named_slot_and_ignores_the_invalid_handle() {
        let mut memory = Memory::new();
        memory.set_serial(1, 0x31);
        memory.set_bank(1, 1);
        memory.set_header(0x0c + 1 * 0x38 + 4, 0x55);
        memory.with_context(|context| assert_eq!(helper_75e464(context, 0x0002_0031), Ok(())));
        assert_eq!(memory.serial(1), INVALID_SERIAL);
        assert_eq!(memory.header(0x0c + 1 * 0x38 + 4), 0);
        assert_eq!(memory.header(0x0c + 1 * 0x38 + 8), 0);
        assert_eq!(memory.registry[0x0c + 1 * 0x38 + 12], 0);
        memory.set_serial(1, 0x31);
        memory.with_context(|context| assert_eq!(helper_75e464(context, u32::MAX), Ok(())));
        assert_eq!(memory.serial(1), 0x31);
    }

    #[test]
    fn the_serial_reader_uses_the_index_without_masking() {
        let mut memory = Memory::new();
        memory.set_serial(5, 0x1234);
        memory.with_context(|context| assert_eq!(helper_75e4f0(context, 5), Ok(0x1234)));
        memory.with_context(|context| assert_eq!(helper_75e4f0(context, 4), Ok(0)));
    }

    #[test]
    fn bank_use_needs_a_used_slot_naming_the_bank() {
        let mut memory = Memory::new();
        for index in 0..16 { memory.set_serial(index, INVALID_SERIAL); }
        memory.set_bank(7, 5);
        memory.with_context(|context| assert_eq!(helper_75e648(context, 5), Ok(0)));
        memory.set_serial(7, 0x40);
        memory.with_context(|context| assert_eq!(helper_75e648(context, 5), Ok(1)));
        memory.with_context(|context| assert_eq!(helper_75e648(context, 6), Ok(0)));
    }

    #[test]
    fn the_resource_copy_moves_the_selected_bank_over_the_named_one() {
        let mut memory = Memory::new();
        memory.set_header(8, 1);
        memory.resources[RESOURCE_STRIDE as usize] = 0x5a;
        memory.with_context(|context| assert_eq!(helper_75e698(context, 0), Ok(())));
        assert_eq!(memory.resources[0], 0x5a);
        assert_eq!(memory.resources[1], 0);
    }

    #[test]
    fn allocation_packs_the_slot_the_bank_and_the_top_byte() {
        let mut memory = Memory::new();
        for index in 0..16 { memory.set_serial(index, INVALID_SERIAL); }
        memory.set_header(0, 0);
        memory.set_header(4, 3);
        memory.with_context(|context| assert_eq!(helper_75a9fc(context), Ok(0x00ff_0030)));
        assert_eq!(memory.header(0), 1);
        assert_eq!(memory.header(4), 4);
        for index in 0..16 { memory.set_serial(index, index as u32 + 1); }
        memory.with_context(|context| assert_eq!(helper_75a9fc(context), Ok(INVALID_SERIAL)));
    }

    #[test]
    fn the_kind_selects_the_bank_a_new_profile_takes() {
        let mut memory = Memory::new();
        for index in 0..16 { memory.set_serial(index, INVALID_SERIAL); }
        memory.set_header(8, 0);
        memory.with_context(|context| assert_eq!(helper_75aa3c(context, 0), Ok(0)));
        memory.with_context(|context| assert_eq!(helper_75aa3c(context, 1), Ok(1)));
        memory.with_context(|context| assert_eq!(helper_75aa3c(context, 2), Ok(2)));
        memory.with_context(|context| assert_eq!(helper_75aa3c(context, 5), Ok(2)));
        memory.with_context(|context| assert_eq!(helper_75aa3c(context, 4), Ok(0)));
        memory.with_context(|context| assert_eq!(helper_75aa3c(context, 3), Ok(1)));
        memory.set_serial(1, 0x10);
        memory.set_bank(1, 1);
        memory.with_context(|context| assert_eq!(helper_75aa3c(context, 3), Ok(2)));
        memory.set_header(8, 1);
        memory.with_context(|context| assert_eq!(helper_75aa3c(context, 3), Ok(0)));
        memory.set_header(8, 2);
        memory.with_context(|context| assert_eq!(helper_75aa3c(context, 4), Ok(2)));
    }

    #[test]
    fn a_handle_is_valid_while_its_slot_records_its_serial() {
        let mut memory = Memory::new();
        memory.set_serial(6, 0x0abc);
        memory.with_context(|context| assert_eq!(helper_75aab4(context, 0x0000_abc6), Ok(1)));
        memory.with_context(|context| assert_eq!(helper_75aab4(context, 0x0000_abc7), Ok(0)));
        memory.with_context(|context| assert_eq!(helper_75aab4(context, 0x0000_abd6), Ok(0)));
        memory.with_context(|context| assert_eq!(helper_75aab4(context, u32::MAX), Ok(0)));
    }

    #[test]
    fn the_full_reset_clears_the_registry_the_resources_and_the_shared_block() {
        let mut memory = Memory::new();
        memory.resources = [0xaa; 0x42b0];
        memory.block = [0xaa; 0x9d4];
        memory.with_context(|context| assert_eq!(helper_75e1b4(context), Ok(())));
        assert!(memory.resources.iter().all(|byte| *byte == 0));
        assert!(memory.block.iter().all(|byte| *byte == 0));
        assert_eq!(memory.header(4), 1);
        memory.with_context(|context| assert_eq!(helper_756720(context), Ok(())));
        assert_eq!(memory.header(4), 1);
    }

    #[test]
    fn create_returns_the_handle_it_claimed_or_minus_one() {
        let mut memory = Memory::new();
        for index in 0..16 { memory.set_serial(index, INVALID_SERIAL); }
        memory.set_header(0, 0);
        memory.set_header(4, 3);
        memory.with_context(|context| assert_eq!(helper_75a63c(context, 0, None), Ok(0x00ff_0030)));
        assert_eq!(memory.serial(0), 3);
        assert_eq!(memory.header(0x0c + 8), 0);
        memory.set_header(0, 1);
        memory.with_context(|context| assert_eq!(helper_75a63c(context, 2, None), Ok(INVALID_SERIAL)));
        for index in 0..16 { memory.set_serial(index, index as u32 + 1); }
        memory.with_context(|context| assert_eq!(helper_75a63c(context, 0, None), Ok(INVALID_SERIAL)));
    }

    #[test]
    fn create_of_kind_three_copies_and_selects_its_bank() {
        let mut memory = Memory::new();
        for index in 0..16 { memory.set_serial(index, INVALID_SERIAL); }
        memory.set_header(0, 0);
        memory.set_header(4, 3);
        memory.set_header(8, 0);
        memory.resources[0] = 0x7e;
        memory.with_context(|context| assert_eq!(helper_75a63c(context, 3, None), Ok(0x00ff_0030)));
        assert_eq!(memory.resources[RESOURCE_STRIDE as usize], 0x7e);
        assert_eq!(memory.header(8), 1);
        assert_eq!(memory.pointer(), RESOURCES + RESOURCE_STRIDE);
    }

    #[test]
    fn release_by_handle_needs_a_valid_handle_with_bit_sixteen() {
        let mut memory = Memory::new();
        memory.set_serial(0, 0x55);
        memory.with_context(|context| assert_eq!(helper_75a6cc(context, 0x0000_0550), Ok(())));
        assert_eq!(memory.serial(0), 0x55);
        memory.with_context(|context| assert_eq!(helper_75a6cc(context, 0x0001_0550), Ok(())));
        assert_eq!(memory.serial(0), INVALID_SERIAL);
        memory.set_serial(0, 0x55);
        memory.with_context(|context| assert_eq!(helper_75a6cc(context, 0x0001_0551), Ok(())));
        assert_eq!(memory.serial(0), 0x55);
    }

    #[test]
    fn the_handle_fields_come_apart_as_four_twelve_and_one_bit() {
        let mut memory = Memory::new();
        memory.with_context(|context| {
            assert_eq!(helper_75a984(context, 0x00ff_abcd), Ok(13));
            assert_eq!(helper_75a99c(context, 0x00ff_abcd), Ok(0xabc));
            assert_eq!(helper_75a9e4(context, 0x00ff_abcd), Ok(1));
            assert_eq!(helper_75a9e4(context, 0x00fe_abcd), Ok(0));
        });
    }

    #[test]
    fn the_signed_halfword_sits_fifty_six_bytes_into_the_base() {
        let mut memory = Memory::new();
        memory.resources[56..58].copy_from_slice(&0x8001u16.to_le_bytes());
        memory.with_context(|context| assert_eq!(helper_75cc04(context, GuestAddress::new(RESOURCES)), Ok(-32767)));
        memory.with_context(|context| assert_eq!(helper_75cc04(context, GuestAddress::new(RESOURCES + 2)), Ok(0)));
    }

    #[test]
    fn the_three_word_copy_moves_the_source_words() {
        let mut memory = Memory::new();
        for index in 0..3u32 { memory.resources[index as usize * 4..index as usize * 4 + 4].copy_from_slice(&(0x100 + index).to_le_bytes()); }
        memory.with_context(|context| assert_eq!(helper_75ade8(context, GuestAddress::new(SELECTED_RESOURCE), GuestAddress::new(RESOURCES)), Ok(())));
        assert_eq!(memory.pointer(), 0x100);
        assert_eq!(&memory.selected[4..8], &0x101u32.to_le_bytes());
        assert_eq!(&memory.selected[8..12], &0x102u32.to_le_bytes());
    }
}
