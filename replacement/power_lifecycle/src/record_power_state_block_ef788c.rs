//! Image-5 power-state block at 0x0169_c1d4, bodies 0x00ef_788c-0x00ef_bad8.
//!
//! The transition body runs only from mode 1 and the request body only from
//! mode 2 or above. The three 20-byte entries at 0x0169_c1f8 carry a state
//! word at +8.
use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext, RecordError};
use crate::record_native_exports::stock_export;

pub const STATE_BLOCK: u32 = 0x0169_c1d4;
const CONFIGURATION_KEY: u32 = 0x98c0;
const ENTRY_BASE: u32 = STATE_BLOCK + 0x24;
const FLAG_BASE: u32 = 0x0169_cf00 + 0x24;
const FIRST_CHANGE_WORD: u32 = 0x0169_d308;
const SECOND_CHANGE_WORD: u32 = 0x0169_d31c;

fn block(offset: u32) -> GuestAddress { GuestAddress::new(STATE_BLOCK + offset) }

fn entry(index: u32) -> GuestAddress { GuestAddress::new(ENTRY_BASE + index * 20) }

fn fill<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, address: GuestAddress, bytes: u32) -> Result<(), MemoryError> {
    for offset in 0..bytes { context.write_u8(address.checked_add(offset)?, 0)?; }
    Ok(())
}

/// The image-5 body at 0x00ef_7a90.
pub fn helper_ef7a90<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, kind: u32) -> Result<u32, MemoryError> {
    let value = crate::record_configuration::helper_69f8fc(context, CONFIGURATION_KEY)?;
    Ok(match kind {
        2 => value & 1,
        3 => u32::from(value & 2 != 0),
        4 => u32::from(value & 4 != 0),
        5 => u32::from(value & 8 != 0),
        _ => u32::from(value != 0),
    })
}

/// The image-5 body at 0x00ef_9aec.
pub fn helper_ef9aec<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, offset: u32, value: u32) -> Result<(), MemoryError> {
    context.write_u8(GuestAddress::new(FLAG_BASE + offset), u8::from(value != 0))
}

/// The image-5 body at 0x00ef_788c.
pub fn helper_ef788c<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>, base: GuestAddress, record: GuestAddress, found: GuestAddress,
) -> Result<(), MemoryError> {
    let mut selected = 0u32;
    let mut promoted = false;
    for index in 0..3u32 {
        let state = base.checked_add(index * 20)?.checked_add(8)?;
        match context.read_u32(state)? {
            2 => {
                context.write_u32(state, 3)?;
                selected = index;
                promoted = true;
            }
            3 => {}
            _ => context.write_u32(state, 0)?,
        }
    }
    if !promoted {
        return context.write_u16(found, 0);
    }
    context.write_u16(found, 1)?;
    let table = GuestAddress::new(STATE_BLOCK + selected * 20);
    let word = context.read_u32(table.checked_add(0x30)?)?;
    let first = context.read_u16(table.checked_add(0x38)?)?;
    let second = context.read_u16(table.checked_add(0x3a)?)?;
    let third = context.read_u16(table.checked_add(0x3c)?)?;
    context.write_u32(record.checked_add(4)?, word)?;
    context.write_u16(record.checked_add(8)?, first)?;
    context.write_u16(record.checked_add(10)?, second)?;
    context.write_u16(record, 1)?;
    context.write_u16(record.checked_add(16)?, third)
}

/// The image-5 body at 0x00ef_793c.
pub fn helper_ef793c<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>, limit: u32, count: u32, entries: GuestAddress, found: GuestAddress,
) -> Result<(), MemoryError> {
    let mut selected = 0u32;
    let mut kept = 0u32;
    for index in 0..count {
        let address = entries.checked_add(index.wrapping_mul(20))?;
        let value = u32::from(context.read_u16(address)?);
        if value > limit {
            context.write_u16(address, 0)?;
        } else if value != 0 {
            kept = kept.wrapping_add(1) & 0xffff;
            selected = index;
        }
    }
    if context.read_u32(block(0))? == 3 {
        let address = entries.checked_add(selected.wrapping_mul(20))?;
        let width = u32::from(context.read_u16(address.checked_add(8)?)?);
        let height = u32::from(context.read_u16(address.checked_add(10)?)?);
        let base = context.read_u32(address.checked_add(4)?)?;
        context.write_u32(block(0x98), base.wrapping_add(width.wrapping_mul(height).wrapping_mul(2)))?;
    }
    context.write_u16(found, kept as u16)
}

/// The image-5 body at 0x00ef_7d68.
pub fn helper_ef7d68<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    if helper_ef7a90(context, 7)? == 0 { return Ok(132); }
    fill(context, GuestAddress::new(STATE_BLOCK), 0xd28)?;
    let two = helper_ef7a90(context, 2)? != 0;
    let five = helper_ef7a90(context, 5)? != 0;
    if two || five {
        context.write_u32(block(0x30), 0x48a6_1000)?;
        let pointer = context.read_u32(block(0x24))?;
        context.write_u32(block(0x44), pointer.wrapping_add(0x48a6_1000))?;
        if two && five {
            let pointer = context.read_u32(block(0x24))?;
            context.write_u32(block(0x58), pointer.wrapping_add(0x2453_0800).wrapping_mul(2))?;
            context.write_u8(block(0x28), 3)?;
            let value = pointer.wrapping_mul(3).wrapping_add(0x48a6_1000);
            context.write_u32(block(0x94), value)?;
            context.write_u32(block(0x98), value)?;
        } else {
            context.write_u8(block(0x28), 2)?;
            let pointer = context.read_u32(block(0x24))?;
            let value = pointer.wrapping_add(0x2453_0800).wrapping_mul(2);
            context.write_u32(block(0x94), value)?;
            context.write_u32(block(0x98), value)?;
        }
    } else {
        context.write_u32(block(0x30), 0x48a6_1000)?;
        context.write_u8(block(0x28), 1)?;
        let pointer = context.read_u32(block(0x24))?;
        let value = pointer.wrapping_add(0x48a6_1000);
        context.write_u32(block(0x94), value)?;
        context.write_u32(block(0x98), value)?;
    }
    context.write_u32(block(0), 1)?;
    Ok(0)
}

fn advance_entries<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    let count = u32::from(context.read_u8(block(0x28))?);
    if count == 0 { return Ok(()); }
    let mut first = 0;
    while first < count && context.read_u32(entry(first).checked_add(0x2c)?)? != 0 { first += 1; }
    if first >= count { return Ok(()); }
    context.write_u32(block(0x20), first)?;
    let mut next = first + 1;
    while next < count && context.read_u32(entry(next).checked_add(0x2c)?)? != 0 { next += 1; }
    if next < count { context.write_u32(block(0x1c), next)?; }
    Ok(())
}

fn free_entry<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, count: u32) -> Result<u32, MemoryError> {
    let mut selected = 0;
    if count != 0 && context.read_u32(entry(0).checked_add(0x2c)?)? != 0 {
        let mut index = 1;
        while index < count {
            selected = index;
            let marker = context.read_u32(entry(index - 1).checked_add(0x40)?)?;
            index += 1;
            if marker == 0 { break; }
        }
    }
    Ok(selected)
}

fn configuration_halfword<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, key: u32) -> Result<u16, MemoryError> {
    Ok(crate::record_configuration::helper_69f8fc(context, key)? as u16)
}

/// The image-5 body at 0x00ef_7eb4.
pub fn helper_ef7eb4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, mode: u32) -> Result<u32, MemoryError> {
    if helper_ef7a90(context, 7)? == 0 { return Ok(132); }
    if context.read_u32(block(0))? != 1 { return Ok(17); }
    if mode != 7 && mode.wrapping_sub(2) > 3 { return Ok(132); }
    context.write_u32(block(0), mode)?;
    match mode {
        2 => {
            context.write_u32(block(4), 0x24)?;
            context.write_u32(block(0x20), 3)?;
            context.write_u32(block(0x1c), 3)?;
            fill(context, block(0xa0), 20)?;
            context.write_u16(block(0xd20), 0)?;
            advance_entries(context)?;
        }
        5 => {
            context.write_u32(block(4), 0x24)?;
            context.write_u32(block(0x20), 3)?;
            context.write_u32(block(0x1c), 3)?;
            fill(context, block(0xd0c), 20)?;
            context.write_u16(block(0xd26), 0)?;
            advance_entries(context)?;
        }
        3 => {
            context.write_u32(block(0x68), 0x24)?;
            fill(context, block(0x6c), 16)?;
            context.write_u32(block(0x80), 0)?;
            context.write_u16(block(0x84), 0)?;
            context.write_u16(block(0x86), 0)?;
            context.write_u16(block(0x88), 0)?;
            context.write_u32(block(0x8c), 0)?;
            context.write_u8(block(0x90), 0)?;
            fill(context, block(0xb4), 120)?;
            context.write_u16(block(0xd22), 0)?;
            let pointer = context.read_u32(block(0x94))?;
            context.write_u32(block(0x9c), pointer)?;
            context.write_u8(block(0x90), 0)?;
            context.write_u16(block(0x86), 0)?;
            context.write_u16(block(0x88), 0xff)?;
        }
        4 => {
            context.write_u32(block(0x68), 0x24)?;
            fill(context, block(0x6c), 16)?;
            context.write_u32(block(0x80), 0)?;
            context.write_u16(block(0x84), 0)?;
            context.write_u16(block(0x86), 0)?;
            context.write_u16(block(0x88), 0)?;
            context.write_u32(block(0x8c), 0)?;
            context.write_u8(block(0x90), 0)?;
            fill(context, block(0x12c), 0xbe0)?;
            context.write_u16(block(0xd24), 0)?;
            let pointer = context.read_u32(block(0x98))?;
            context.write_u32(block(0x9c), pointer)?;
            let count = u32::from(context.read_u8(block(0x28))?);
            let selected = free_entry(context, count)?;
            let value = context.read_u32(GuestAddress::new(STATE_BLOCK + selected * 20 + 0x30))?;
            context.write_u32(block(0x80), value)?;
            let word = crate::record_configuration::helper_69f8fc(context, 0x98c4)?;
            context.write_u32(block(0x8c), word)?;
            let byte = crate::record_configuration::helper_69f8fc(context, 0x98c3)?;
            context.write_u8(block(0x90), byte as u8)?;
            let first = configuration_halfword(context, 0x98c1)?;
            context.write_u16(block(0x86), if first == 0 { 0 } else { first - 1 })?;
            let second = configuration_halfword(context, 0x98c2)?;
            context.write_u16(block(0x88), if second == 0 { 0 } else { second - 1 })?;
        }
        _ => {}
    }
    Ok(0)
}

/// The image-5 body at 0x00ef_87b0.
pub fn helper_ef87b0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<u32, MemoryError> {
    if helper_ef7a90(context, 7)? == 0 { return Ok(132); }
    let mode = context.read_u32(block(0))?;
    if mode <= 1 { return Ok(17); }
    match mode {
        2 => helper_ef788c(context, GuestAddress::new(ENTRY_BASE), block(0xa0), block(0xd20))?,
        5 => helper_ef788c(context, GuestAddress::new(ENTRY_BASE), block(0xd0c), block(0xd26))?,
        3 | 4 => {
            let count = u32::from(context.read_u16(block(0x84))?);
            if mode == 3 {
                helper_ef793c(context, value, count, block(0xb4), block(0xd22))?;
            } else {
                helper_ef793c(context, value, count, block(0x12c), block(0xd24))?;
            }
        }
        _ => {}
    }
    context.write_u32(block(0), 1)?;
    Ok(0)
}

/// Entry for the callback word stock loads from a parameter block and enters.
pub trait ChangeCallbacks {
    fn enter_change(&mut self, word: u32, change: u32) -> Result<(), RecordError>;
}

fn change<M: GuestAddressSpace, E: ChangeCallbacks>(
    context: &mut RecordContext<M, E>, word: u32, flag: u32, change: u32,
) -> Result<(), RecordError> {
    if change == 20 || change == 21 || (change as i32) <= 0 { helper_ef9aec(context, flag, 0)?; }
    if word == 0 { return Ok(()); }
    context.effects.enter_change(word, change)
}

/// The image-5 body at 0x00ef_ba48.
pub fn helper_efba48<M: GuestAddressSpace, E: ChangeCallbacks>(context: &mut RecordContext<M, E>, change: u32) -> Result<(), RecordError> {
    let word = context.read_u32(GuestAddress::new(FIRST_CHANGE_WORD))?;
    self::change(context, word, 38, change)
}

/// The image-5 body at 0x00ef_bad8.
pub fn helper_efbad8<M: GuestAddressSpace, E: ChangeCallbacks>(context: &mut RecordContext<M, E>, change: u32) -> Result<(), RecordError> {
    let word = context.read_u32(GuestAddress::new(SECOND_CHANGE_WORD))?;
    self::change(context, word, 41, change)
}

/// A refused word is reported; the helper returns as stock's void caller does.
pub fn enter_change_callback(
    registry: &crate::callback_registry::CallbackRegistry,
    word: crate::callback_registry::CallbackWord,
    change: u32,
) -> i32 {
    match registry.resolve::<unsafe extern "C" fn(u32)>(word) {
        Ok(entry) => { unsafe { entry(change) }; 0 }
        Err(error) => crate::callback_registry::refused(error),
    }
}

#[cfg(target_arch = "arm")]
#[derive(Default)]
pub struct NativeChangeCallbacks;

#[cfg(target_arch = "arm")]
impl ChangeCallbacks for NativeChangeCallbacks {
    fn enter_change(&mut self, word: u32, change: u32) -> Result<(), RecordError> {
        let _ = enter_change_callback(crate::callback_registry::registry(), crate::callback_registry::CallbackWord(word as usize), change);
        Ok(())
    }
}

stock_export!(
    xm1_power_record_helper_ef7d68,
    helper_ef7d68_abi,
    (),
    () -> u32,
    (),
    (),
    crate::record_power_state_block_ef788c::helper_ef7d68,
    |value| value
);
stock_export!(
    xm1_power_record_helper_ef7eb4,
    helper_ef7eb4_abi,
    (),
    (mode: u32) -> u32,
    (mode: u32),
    (mode),
    crate::record_power_state_block_ef788c::helper_ef7eb4,
    |value| value
);
stock_export!(
    xm1_power_record_helper_ef87b0,
    helper_ef87b0_abi,
    (),
    (value: u32) -> u32,
    (value: u32),
    (value),
    crate::record_power_state_block_ef788c::helper_ef87b0,
    |value| value
);
stock_export!(
    xm1_power_record_helper_efba48,
    helper_efba48_abi,
    crate::record_power_state_block_ef788c::NativeChangeCallbacks,
    (change: i32) -> (),
    (change: u32),
    (change as u32),
    crate::record_power_state_block_ef788c::helper_efba48,
    |_| (),
    bounds ChangeCallbacks
);
stock_export!(
    xm1_power_record_helper_efbad8,
    helper_efbad8_abi,
    crate::record_power_state_block_ef788c::NativeChangeCallbacks,
    (change: i32) -> (),
    (change: u32),
    (change as u32),
    crate::record_power_state_block_ef788c::helper_efbad8,
    |_| (),
    bounds ChangeCallbacks
);

#[cfg(test)]
mod tests {
    //! Expected values are the image-5 stores and loads at 0x00ef_788c-0x00ef_bad8.
    use super::*;
    use crate::record_address_space::{GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const CONFIGURATION_POINTER: u32 = 0x0096_ca50;
    const CONFIGURATION: u32 = 0x0160_0000;
    const BLOCK_BYTES: u32 = 0xd50;
    const CHANGE_WORDS: u32 = FIRST_CHANGE_WORD;

    struct Memory {
        pointer: [u8; 4],
        configuration: [u8; 0xa000],
        block: [u8; BLOCK_BYTES as usize],
        flags: [u8; 0x100],
        change_words: [u8; 0x18],
    }

    impl Memory {
        fn new() -> Self {
            Memory { pointer: CONFIGURATION.to_le_bytes(), configuration: [0; 0xa000], block: [0; BLOCK_BYTES as usize], flags: [0; 0x100], change_words: [0; 0x18] }
        }

        fn with_context<R>(&mut self, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
            self.with_effects((), body)
        }

        fn with_effects<E, R>(&mut self, effects: E, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, E>) -> R) -> R {
            let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
            let mut regions = [
                MemoryRegion::new(descriptor(CONFIGURATION_POINTER, 4), &mut self.pointer).unwrap(),
                MemoryRegion::new(descriptor(CONFIGURATION, 0xa000), &mut self.configuration).unwrap(),
                MemoryRegion::new(descriptor(STATE_BLOCK, BLOCK_BYTES), &mut self.block).unwrap(),
                MemoryRegion::new(descriptor(FLAG_BASE, 0x100), &mut self.flags).unwrap(),
                MemoryRegion::new(descriptor(CHANGE_WORDS, 0x18), &mut self.change_words).unwrap(),
            ];
            let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
            body(&mut RecordContext::new(space, effects))
        }

        fn word(&self, at: u32) -> u32 {
            u32::from_le_bytes(self.block[at as usize..at as usize + 4].try_into().unwrap())
        }

        fn set_word(&mut self, at: u32, value: u32) {
            self.block[at as usize..at as usize + 4].copy_from_slice(&value.to_le_bytes());
        }

        fn half(&self, at: u32) -> u16 {
            u16::from_le_bytes(self.block[at as usize..at as usize + 2].try_into().unwrap())
        }

        fn set_half(&mut self, at: u32, value: u16) {
            self.block[at as usize..at as usize + 2].copy_from_slice(&value.to_le_bytes());
        }
    }

    #[test]
    fn each_state_predicate_reads_one_bit_of_the_configuration_byte() {
        let mut memory = Memory::new();
        memory.configuration[0x98c0] = 0x0b;
        memory.with_context(|context| {
            assert_eq!(helper_ef7a90(context, 2), Ok(1));
            assert_eq!(helper_ef7a90(context, 3), Ok(1));
            assert_eq!(helper_ef7a90(context, 4), Ok(0));
            assert_eq!(helper_ef7a90(context, 5), Ok(1));
            assert_eq!(helper_ef7a90(context, 7), Ok(1));
        });
        memory.configuration[0x98c0] = 0;
        memory.with_context(|context| {
            assert_eq!(helper_ef7a90(context, 2), Ok(0));
            assert_eq!(helper_ef7a90(context, 3), Ok(0));
            assert_eq!(helper_ef7a90(context, 7), Ok(0));
        });
    }

    #[test]
    fn the_change_flag_is_a_byte_at_the_offset_from_the_flag_base() {
        let mut memory = Memory::new();
        memory.with_context(|context| {
            assert_eq!(helper_ef9aec(context, 38, 5), Ok(()));
            assert_eq!(helper_ef9aec(context, 41, 0), Ok(()));
        });
        assert_eq!(memory.flags[38], 1);
        assert_eq!(memory.flags[41], 0);
    }

    #[test]
    fn the_entry_scan_promotes_two_to_three_and_clears_the_others() {
        let mut memory = Memory::new();
        memory.set_word(0x24 + 8, 2);
        memory.set_word(0x24 + 20 + 8, 2);
        memory.set_word(0x24 + 40 + 8, 1);
        memory.set_word(0x30 + 20, 0x1234_5678);
        memory.set_half(0x30 + 20 + 8, 0x0102);
        memory.set_half(0x30 + 20 + 10, 0x0304);
        memory.set_half(0x30 + 20 + 12, 0x0506);
        memory.with_context(|context| {
            assert_eq!(helper_ef788c(context, GuestAddress::new(ENTRY_BASE), block(0xa0), block(0xd20)), Ok(()));
        });
        assert_eq!(memory.word(0x24 + 8), 3);
        assert_eq!(memory.word(0x24 + 20 + 8), 3);
        assert_eq!(memory.word(0x24 + 40 + 8), 0);
        assert_eq!(memory.half(0xd20), 1);
        assert_eq!(memory.word(0xa4), 0x1234_5678);
        assert_eq!(memory.half(0xa8), 0x0102);
        assert_eq!(memory.half(0xaa), 0x0304);
        assert_eq!(memory.half(0xa0), 1);
        assert_eq!(memory.half(0xb0), 0x0506);
    }

    #[test]
    fn the_entry_scan_without_a_two_reports_zero_and_keeps_three() {
        let mut memory = Memory::new();
        memory.set_word(0x24 + 8, 3);
        memory.set_word(0x24 + 20 + 8, 0);
        memory.with_context(|context| {
            assert_eq!(helper_ef788c(context, GuestAddress::new(ENTRY_BASE), block(0xa0), block(0xd20)), Ok(()));
        });
        assert_eq!(memory.word(0x24 + 8), 3);
        assert_eq!(memory.half(0xd20), 0);
        assert_eq!(memory.half(0xa0), 0);
    }

    #[test]
    fn the_limit_scan_clears_entries_above_the_limit_and_counts_the_rest() {
        let mut memory = Memory::new();
        memory.set_half(0xb4, 3);
        memory.set_half(0xb4 + 20, 0);
        memory.set_half(0xb4 + 40, 4);
        memory.set_half(0xb4 + 60, 2);
        memory.set_word(0, 3);
        memory.set_half(0xb4 + 60 + 8, 5);
        memory.set_half(0xb4 + 60 + 10, 7);
        memory.set_word(0xb4 + 60 + 4, 0x1000);
        memory.with_context(|context| {
            assert_eq!(helper_ef793c(context, 3, 4, block(0xb4), block(0xd22)), Ok(()));
        });
        assert_eq!(memory.half(0xb4), 3);
        assert_eq!(memory.half(0xb4 + 20), 0);
        assert_eq!(memory.half(0xb4 + 40), 0);
        assert_eq!(memory.half(0xb4 + 60), 2);
        assert_eq!(memory.half(0xd22), 2);
        assert_eq!(memory.word(0x98), 0x1000 + 5 * 7 * 2);
    }

    #[test]
    fn the_limit_scan_without_the_third_mode_leaves_the_output_word() {
        let mut memory = Memory::new();
        memory.set_half(0xb4, 9);
        memory.set_word(0, 2);
        memory.set_word(0x98, 0x55);
        memory.with_context(|context| {
            assert_eq!(helper_ef793c(context, 3, 1, block(0xb4), block(0xd22)), Ok(()));
        });
        assert_eq!(memory.half(0xb4), 0);
        assert_eq!(memory.half(0xd22), 0);
        assert_eq!(memory.word(0x98), 0x55);
    }

    #[test]
    fn the_initializer_needs_the_seventh_flag_and_fills_the_block() {
        let mut memory = Memory::new();
        memory.set_word(0x30, 0xdead);
        memory.with_context(|context| assert_eq!(helper_ef7d68(context), Ok(132)));
        assert_eq!(memory.word(0x30), 0xdead);
        memory.configuration[0x98c0] = 0x02;
        memory.set_word(0x24, 0x100);
        memory.with_context(|context| assert_eq!(helper_ef7d68(context), Ok(0)));
        assert_eq!(memory.word(0), 1);
        assert_eq!(memory.block[0x28], 1);
        assert_eq!(memory.word(0x30), 0x48a6_1000);
        assert_eq!(memory.word(0x94), 0x48a6_1000);
        assert_eq!(memory.word(0x98), 0x48a6_1000);
        assert_eq!(memory.word(0x44), 0);
    }

    #[test]
    fn the_initializer_marks_the_second_and_third_modes() {
        let mut memory = Memory::new();
        memory.configuration[0x98c0] = 0x05;
        memory.with_context(|context| assert_eq!(helper_ef7d68(context), Ok(0)));
        assert_eq!(memory.block[0x28], 2);
        assert_eq!(memory.word(0x44), 0x48a6_1000);
        assert_eq!(memory.word(0x94), 0x48a6_1000);
        let mut memory = Memory::new();
        memory.configuration[0x98c0] = 0x09;
        memory.with_context(|context| assert_eq!(helper_ef7d68(context), Ok(0)));
        assert_eq!(memory.block[0x28], 3);
        assert_eq!(memory.word(0x58), 0x48a6_1000);
        assert_eq!(memory.word(0x94), 0x48a6_1000);
    }

    #[test]
    fn the_transition_needs_a_ready_block_and_a_known_mode() {
        let mut memory = Memory::new();
        memory.configuration[0x98c0] = 0x01;
        memory.set_word(0, 0);
        memory.with_context(|context| assert_eq!(helper_ef7eb4(context, 2), Ok(17)));
        memory.set_word(0, 1);
        memory.with_context(|context| assert_eq!(helper_ef7eb4(context, 6), Ok(132)));
        memory.set_word(0, 1);
        memory.with_context(|context| assert_eq!(helper_ef7eb4(context, 2), Ok(0)));
        assert_eq!(memory.word(0), 2);
        assert_eq!(memory.word(4), 0x24);
        assert_eq!(memory.word(0x20), 3);
        assert_eq!(memory.word(0x1c), 3);
        assert_eq!(memory.half(0xd20), 0);
    }

    #[test]
    fn the_transition_clears_the_third_mode_structure() {
        let mut memory = Memory::new();
        memory.configuration[0x98c0] = 0x01;
        memory.set_word(0, 1);
        memory.set_word(0x94, 0x7777);
        memory.with_context(|context| assert_eq!(helper_ef7eb4(context, 3), Ok(0)));
        assert_eq!(memory.word(0x68), 0x24);
        assert_eq!(memory.word(0x9c), 0x7777);
        assert_eq!(memory.half(0x86), 0);
        assert_eq!(memory.half(0x88), 0xff);
    }

    #[test]
    fn the_transition_clears_the_fourth_mode_structure_and_reads_its_configuration() {
        let mut memory = Memory::new();
        memory.configuration[0x98c0] = 0x01;
        memory.configuration[0x98c1] = 4;
        memory.configuration[0x98c2] = 3;
        memory.configuration[0x98c3] = 0x5a;
        memory.configuration[0x98c4] = 0x66;
        memory.set_word(0, 1);
        memory.set_word(0x98, 0x1234);
        memory.block[0x28] = 1;
        memory.set_word(0x30, 0x0bad);
        memory.with_context(|context| assert_eq!(helper_ef7eb4(context, 4), Ok(0)));
        assert_eq!(memory.word(0x9c), 0x1234);
        assert_eq!(memory.word(0x80), 0x0bad);
        assert_eq!(memory.word(0x8c), 0x66);
        assert_eq!(memory.block[0x90], 0x5a);
        assert_eq!(memory.half(0x86), 3);
        assert_eq!(memory.half(0x88), 2);
        assert_eq!(memory.half(0xd24), 0);
    }

    #[test]
    fn the_request_needs_the_seventh_flag_and_a_running_block() {
        let mut memory = Memory::new();
        memory.set_word(0, 2);
        memory.with_context(|context| assert_eq!(helper_ef87b0(context, 3), Ok(132)));
        memory.configuration[0x98c0] = 0x01;
        memory.set_word(0, 1);
        memory.with_context(|context| assert_eq!(helper_ef87b0(context, 3), Ok(17)));
        memory.set_word(0, 7);
        memory.with_context(|context| assert_eq!(helper_ef87b0(context, 3), Ok(0)));
        assert_eq!(memory.word(0), 1);
    }

    #[test]
    fn the_request_scans_the_mode_two_entries() {
        let mut memory = Memory::new();
        memory.configuration[0x98c0] = 0x01;
        memory.set_word(0, 2);
        memory.set_word(0x24 + 20 + 8, 2);
        memory.set_word(0x30 + 20, 0x0102_0304);
        memory.with_context(|context| assert_eq!(helper_ef87b0(context, 0), Ok(0)));
        assert_eq!(memory.word(0x24 + 20 + 8), 3);
        assert_eq!(memory.half(0xd20), 1);
        assert_eq!(memory.word(0xa4), 0x0102_0304);
        assert_eq!(memory.word(0), 1);
    }

    #[test]
    fn the_request_counts_the_mode_three_entries_at_the_argument_limit() {
        let mut memory = Memory::new();
        memory.configuration[0x98c0] = 0x01;
        memory.set_word(0, 3);
        memory.set_half(0x84, 3);
        memory.set_half(0xb4, 3);
        memory.set_half(0xb4 + 20, 4);
        memory.set_half(0xb4 + 40, 2);
        memory.with_context(|context| assert_eq!(helper_ef87b0(context, 3), Ok(0)));
        assert_eq!(memory.half(0xb4 + 20), 0);
        assert_eq!(memory.half(0xd22), 2);
        assert_eq!(memory.word(0), 1);
    }

    struct Callbacks { invoked: Option<u32> }

    impl ChangeCallbacks for Callbacks {
        fn enter_change(&mut self, word: u32, change: u32) -> Result<(), RecordError> {
            self.invoked = Some(word.wrapping_add(change));
            Ok(())
        }
    }

    #[test]
    fn the_change_notification_clears_the_flag_and_enters_the_callback() {
        let mut memory = Memory::new();
        memory.change_words[..4].copy_from_slice(&0x00f0_1234u32.to_le_bytes());
        memory.flags[38] = 1;
        memory.with_effects(Callbacks { invoked: None }, |context| {
            assert_eq!(helper_efba48(context, 20), Ok(()));
            assert_eq!(context.effects.invoked, Some(0x00f0_1234 + 20));
        });
        assert_eq!(memory.flags[38], 0);
    }

    #[test]
    fn the_change_notification_clears_the_flag_for_change_21() {
        let mut memory = Memory::new();
        memory.change_words[..4].copy_from_slice(&0x00f0_1234u32.to_le_bytes());
        memory.flags[38] = 1;
        memory.with_effects(Callbacks { invoked: None }, |context| {
            assert_eq!(helper_efba48(context, 21), Ok(()));
            assert_eq!(context.effects.invoked, Some(0x00f0_1234 + 21));
        });
        assert_eq!(memory.flags[38], 0);
    }

    #[test]
    fn a_refused_change_word_is_reported_and_the_helper_returns() {
        use std::sync::atomic::{AtomicU32, Ordering};
        static CALLS: AtomicU32 = AtomicU32::new(0);
        unsafe extern "C" fn entry(_: u32) { CALLS.fetch_add(1, Ordering::SeqCst); }
        let registry = crate::callback_registry::CallbackRegistry::new();
        assert_eq!(enter_change_callback(&registry, crate::callback_registry::CallbackWord(0x0060_1000), 21), -255);
        registry.ensure(entry as unsafe extern "C" fn(u32)).unwrap();
        assert_eq!(enter_change_callback(&registry, crate::callback_registry::CallbackWord(entry as *const () as usize), 21), 0);
        assert_eq!(CALLS.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn the_change_notification_skips_a_zero_callback_word() {
        let mut memory = Memory::new();
        memory.with_effects(Callbacks { invoked: None }, |context| {
            assert_eq!(helper_efbad8(context, 5), Ok(()));
            assert_eq!(context.effects.invoked, None);
        });
        assert_eq!(memory.flags[41], 0);
    }
}
