//! Input pipeline registers at 0xfff1_0000 and the channel diagnostic word at
//! 0x0082_0f24, image-4 bodies 0x0065e2c4 through 0x00665088.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

pub const CHANNEL_DIAGNOSTIC: u32 = 0x0082_0f20;
pub const CONTROL: u32 = 0xfff1_0000;
pub const TRANSFER_INTERRUPT_CLEAR: u32 = 0xfff1_015c;
pub const TRANSFER_BUSY: u32 = 0xfff1_0160;
pub const TRANSFER_CONFIGURATION: u32 = 0xfff1_0170;
pub const SECOND_FLAGS: u32 = 0xfff1_0350;
pub const SECOND_CONFIGURATION: u32 = 0xfff1_0354;
pub const OTHER_ENABLE: u32 = 0xfff1_0388;
pub const OTHER_STATUS: u32 = 0xfff1_038c;
pub const OTHER_CONTROL: u32 = 0xfff1_03ac;
pub const OTHER_FLAGS: u32 = 0xfff1_03b4;
pub const CHANNEL_STRIDE: u32 = 0x100;

fn operation<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<(), MemoryError> {
    context.write_u32(GuestAddress::new(CHANNEL_DIAGNOSTIC + 4), value)
}

fn bit(value: u32, index: u32) -> u32 { (value >> index) & 1 }

fn poll<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, address: GuestAddress, mask: u32) -> Result<(), MemoryError> {
    while context.read_u32(address)? & mask == 0 { core::hint::spin_loop(); }
    Ok(())
}

/// The image-4 body at 0x0065e2c4.
pub fn helper_65e2c4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, _command: u32, config: GuestAddress) -> Result<u32, MemoryError> {
    operation(context, 5)?;
    let mut value = bit(context.read_u32(config.checked_add(4)?)?, 0) << 0;
    if context.read_u32(config.checked_add(8)?)? == 1 { value |= 2; }
    if context.read_u32(config)? != 0 { value |= 0x1_0000; }
    context.write_u32(GuestAddress::new(CONTROL + 4), value)?;
    Ok(0)
}

/// The image-4 body at 0x0066049c.
pub fn helper_66049c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    operation(context, 30)?;
    let control = GuestAddress::new(CONTROL);
    let value = context.read_u32(control)?;
    context.write_u32(control, value & !1)?;
    poll(context, control, 1)?;
    Ok(0)
}

/// The image-4 body at 0x00663cdc.
pub fn helper_663cdc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    operation(context, 69)?;
    let control = GuestAddress::new(CONTROL);
    let value = context.read_u32(control)?;
    context.write_u32(control, value & !2)?;
    Ok(0)
}

/// The image-4 body at 0x00664580.
pub fn helper_664580<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32, flags: u32) -> Result<(), MemoryError> {
    let value = (flags & 1) | ((flags >> 1) & 1) << 1 | ((flags >> 2) & 1) << 2;
    if index == 0 || index == 1 {
        context.write_u32(GuestAddress::new(TRANSFER_INTERRUPT_CLEAR.wrapping_add((index & 0xff).wrapping_mul(CHANNEL_STRIDE))), value)?;
    } else if index == 2 || index == 3 {
        let offset = (index & 0xff).wrapping_sub(2) & 0xff;
        context.write_u32(GuestAddress::new(SECOND_FLAGS.wrapping_add(offset.wrapping_mul(0x1c))), value)?;
    } else {
        let value = if flags & 1 != 0 { 0x1000_0000 } else { 0 }
            | if flags & 2 != 0 { 1 } else { 0 }
            | if flags & 4 != 0 { 2 } else { 0 };
        context.write_u32(GuestAddress::new(OTHER_FLAGS), value)?;
    }
    Ok(())
}

/// The image-4 body at 0x00664760.
pub fn helper_664760<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32, value: u32, mask: u32) -> Result<(), MemoryError> {
    let bit_value = value & 1;
    let replace = |word: u32| {
        let mut word = word;
        for (flag, position) in [(1, 28), (2, 29), (4, 30)] {
            if mask & flag != 0 { word = (word & !(1 << position)) | bit_value << position; }
        }
        word
    };
    if index == 0 || index == 1 {
        let address = GuestAddress::new(TRANSFER_CONFIGURATION.wrapping_add((index & 0xff).wrapping_mul(CHANNEL_STRIDE)));
        let word = replace(context.read_u32(address)?);
        context.write_u32(address, word)?;
    } else if index == 2 || index == 3 {
        let offset = (index & 0xff).wrapping_sub(2) & 0xff;
        let address = GuestAddress::new(SECOND_CONFIGURATION.wrapping_add(offset.wrapping_mul(0x1c)));
        let word = replace(context.read_u32(address)?);
        context.write_u32(address, word)?;
    } else {
        let address = GuestAddress::new(OTHER_CONTROL);
        let mut word = context.read_u32(address)?;
        if mask & 1 != 0 {
            word = (word & 0xffff_7fff) | bit_value << 15;
            if value == 0 { word = (word & 0xffff_3fff) | bit_value << 14; }
        }
        if mask & 2 != 0 { word = (word & 0xffff_efff) | bit_value << 12; }
        if mask & 4 != 0 { word = (word & 0xffff_dfff) | bit_value << 13; }
        context.write_u32(address, word)?;
    }
    Ok(())
}

/// The image-4 body at 0x00664b74.
pub fn helper_664b74<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32) -> Result<u32, MemoryError> {
    if index != 2 && index != 3 {
        if index == 0 || index == 1 {
            let address = GuestAddress::new(TRANSFER_BUSY.wrapping_add((index & 0xff).wrapping_mul(CHANNEL_STRIDE)));
            poll(context, address, u32::MAX)?;
        } else {
            poll(context, GuestAddress::new(CONTROL), 2)?;
        }
    }
    Ok(index)
}

/// The image-4 body at 0x00664e14.
pub fn helper_664e14<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, config: GuestAddress) -> Result<u32, MemoryError> {
    operation(context, 81)?;
    let value = u32::from(context.read_u32(config.checked_add(0x10)?)? == 1);
    context.write_u32(GuestAddress::new(OTHER_ENABLE), value)?;
    Ok(0)
}

/// The image-4 body at 0x00665088.
pub fn helper_665088<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress) -> Result<u32, MemoryError> {
    operation(context, 83)?;
    let value = context.read_u32(GuestAddress::new(OTHER_STATUS))?;
    if bit(value, 16) == 0 {
        context.write_u32(destination, 0)?;
        context.write_u32(destination.checked_add(4)?, 0)?;
        context.write_u32(destination.checked_add(12)?, bit(value, 1))?;
        context.write_u32(destination.checked_add(8)?, bit(value, 9))?;
    } else {
        context.write_u32(destination.checked_add(4)?, bit(value, 1))?;
        context.write_u32(destination, bit(value, 9))?;
        context.write_u32(destination.checked_add(8)?, 0)?;
        context.write_u32(destination.checked_add(12)?, 0)?;
    }
    context.write_u32(destination.checked_add(0x10)?, bit(value, 0))?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_address_space::{
        ByteCount, GuestDomain, HostAddressSpace, MemoryError, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission,
    };

    const REGISTERS: u32 = 0xfff1_0000;
    const DIAGNOSTIC: u32 = 0x0082_0f20;
    const CONFIG: u32 = 0x0010_0000;
    const DESTINATION: u32 = 0x0010_0100;

    struct Memory {
        registers: [u8; 0x400],
        diagnostic: [u8; 0x20],
        config: [u8; 0x20],
        destination: [u8; 0x20],
    }

    impl Memory {
        fn new() -> Self {
            Self { registers: [0; 0x400], diagnostic: [0; 0x20], config: [0; 0x20], destination: [0; 0x20] }
        }

        fn with_context<R>(&mut self, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
            let descriptor = |base, length| {
                RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap()
            };
            let mut regions = [
                MemoryRegion::new(descriptor(REGISTERS, 0x400), &mut self.registers).unwrap(),
                MemoryRegion::new(descriptor(DIAGNOSTIC, 0x20), &mut self.diagnostic).unwrap(),
                MemoryRegion::new(descriptor(CONFIG, 0x20), &mut self.config).unwrap(),
                MemoryRegion::new(descriptor(DESTINATION, 0x20), &mut self.destination).unwrap(),
            ];
            body(&mut RecordContext::new(HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap(), ()))
        }

        fn register(&self, at: u32) -> u32 {
            let offset = (at - REGISTERS) as usize;
            u32::from_le_bytes(self.registers[offset..offset + 4].try_into().unwrap())
        }

        fn set_register(&mut self, at: u32, value: u32) {
            let offset = (at - REGISTERS) as usize;
            self.registers[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }

        fn operation(&self) -> u32 {
            u32::from_le_bytes(self.diagnostic[4..8].try_into().unwrap())
        }

        fn destination(&self, offset: usize) -> u32 {
            u32::from_le_bytes(self.destination[offset..offset + 4].try_into().unwrap())
        }
    }

    /// A register window whose control word reports the awaited bit after a
    /// scripted number of reads.
    struct ScriptedSpace {
        control: u32,
        diagnostic: u32,
        reads: u32,
        set_after: u32,
        busy: u32,
        busy_reads: u32,
        busy_set_after: u32,
    }

    impl GuestAddressSpace for ScriptedSpace {
        fn domain(&self) -> GuestDomain { GuestDomain::A }
        fn read_u8(&mut self, address: GuestAddress) -> Result<u8, MemoryError> { Err(MemoryError::Unmapped(address)) }
        fn read_u16(&mut self, address: GuestAddress) -> Result<u16, MemoryError> { Err(MemoryError::Unmapped(address)) }
        fn read_u32(&mut self, address: GuestAddress) -> Result<u32, MemoryError> {
            match address.get() {
                CONTROL => {
                    self.reads += 1;
                    Ok(if self.reads >= self.set_after { self.control | 3 } else { self.control })
                }
                TRANSFER_BUSY => {
                    self.busy_reads += 1;
                    Ok(if self.busy_reads >= self.busy_set_after { self.busy } else { 0 })
                }
                0x0082_0f24 => Ok(self.diagnostic),
                _ => Err(MemoryError::Unmapped(address)),
            }
        }
        fn write_u8(&mut self, address: GuestAddress, _: u8) -> Result<(), MemoryError> { Err(MemoryError::Unmapped(address)) }
        fn write_u16(&mut self, address: GuestAddress, _: u16) -> Result<(), MemoryError> { Err(MemoryError::Unmapped(address)) }
        fn write_u32(&mut self, address: GuestAddress, value: u32) -> Result<(), MemoryError> {
            match address.get() {
                CONTROL => { self.control = value; Ok(()) }
                0x0082_0f24 => { self.diagnostic = value; Ok(()) }
                _ => Err(MemoryError::Unmapped(address)),
            }
        }
        fn copy_bytes(&mut self, address: GuestAddress, _: GuestAddress, _: ByteCount) -> Result<(), MemoryError> { Err(MemoryError::Unmapped(address)) }
    }

    fn scripted() -> ScriptedSpace {
        ScriptedSpace { control: 0, diagnostic: 0, reads: 0, set_after: 2, busy: 1, busy_reads: 0, busy_set_after: 2 }
    }

    #[test]
    fn submit_input_command_packs_the_config_words() {
        let mut memory = Memory::new();
        memory.config[0..4].copy_from_slice(&9u32.to_le_bytes());
        memory.config[4..8].copy_from_slice(&1u32.to_le_bytes());
        memory.config[8..12].copy_from_slice(&1u32.to_le_bytes());
        memory.with_context(|context| assert_eq!(helper_65e2c4(context, 0, GuestAddress::new(CONFIG)), Ok(0)));
        assert_eq!(memory.register(CONTROL + 4), 0x1_0003);
        assert_eq!(memory.operation(), 5);
        memory.config[0..4].copy_from_slice(&0u32.to_le_bytes());
        memory.config[4..8].copy_from_slice(&0u32.to_le_bytes());
        memory.config[8..12].copy_from_slice(&0u32.to_le_bytes());
        memory.with_context(|context| assert_eq!(helper_65e2c4(context, 0, GuestAddress::new(CONFIG)), Ok(0)));
        assert_eq!(memory.register(CONTROL + 4), 0);
    }

    #[test]
    fn request_reset_clears_the_request_bit_then_waits_for_the_acknowledgement() {
        let mut space = scripted();
        space.control = 3;
        let mut context = RecordContext::new(space, ());
        assert_eq!(helper_66049c(&mut context), Ok(0));
        assert_eq!(context.memory.control, 2);
        assert_eq!(context.memory.reads, 2);
        assert_eq!(context.memory.diagnostic, 30);
    }

    #[test]
    fn finish_output_clears_the_completion_bit() {
        let mut memory = Memory::new();
        memory.set_register(CONTROL, 0xffff_ffff);
        memory.with_context(|context| assert_eq!(helper_663cdc(context), Ok(0)));
        assert_eq!(memory.register(CONTROL), 0xffff_fffd);
        assert_eq!(memory.operation(), 69);
    }

    #[test]
    fn flag_writes_reach_the_channel_pair_of_each_index() {
        let mut memory = Memory::new();
        memory.with_context(|context| {
            assert_eq!(helper_664580(context, 0, 5), Ok(()));
            assert_eq!(helper_664580(context, 1, 2), Ok(()));
            assert_eq!(helper_664580(context, 2, 7), Ok(()));
            assert_eq!(helper_664580(context, 3, 1), Ok(()));
            assert_eq!(helper_664580(context, 4, 7), Ok(()));
        });
        assert_eq!(memory.register(TRANSFER_INTERRUPT_CLEAR), 5);
        assert_eq!(memory.register(TRANSFER_INTERRUPT_CLEAR + CHANNEL_STRIDE), 2);
        assert_eq!(memory.register(SECOND_FLAGS), 7);
        assert_eq!(memory.register(SECOND_FLAGS + 0x1c), 1);
        assert_eq!(memory.register(OTHER_FLAGS), 0x1000_0003);
    }

    #[test]
    fn configuration_bits_are_replaced_per_mask() {
        let mut memory = Memory::new();
        memory.set_register(TRANSFER_CONFIGURATION, 0xa000_0000);
        memory.with_context(|context| assert_eq!(helper_664760(context, 0, 1, 1), Ok(())));
        assert_eq!(memory.register(TRANSFER_CONFIGURATION), 0xb000_0000);
        memory.with_context(|context| assert_eq!(helper_664760(context, 1, 0, 7), Ok(())));
        assert_eq!(memory.register(TRANSFER_CONFIGURATION + CHANNEL_STRIDE), 0);
        memory.set_register(SECOND_CONFIGURATION, 0xf000_0000);
        memory.with_context(|context| assert_eq!(helper_664760(context, 2, 1, 6), Ok(())));
        assert_eq!(memory.register(SECOND_CONFIGURATION), 0xf000_0000);
        memory.set_register(OTHER_CONTROL, 0xffff_ffff);
        memory.with_context(|context| assert_eq!(helper_664760(context, 5, 0, 3), Ok(())));
        assert_eq!(memory.register(OTHER_CONTROL), 0xffff_2fff);
        memory.with_context(|context| assert_eq!(helper_664760(context, 5, 1, 7), Ok(())));
        assert_eq!(memory.register(OTHER_CONTROL), 0xffff_bfff);
    }

    #[test]
    fn busy_waits_follow_the_index_class() {
        let mut space = scripted();
        let mut context = RecordContext::new(space, ());
        assert_eq!(helper_664b74(&mut context, 0), Ok(0));
        assert_eq!(context.memory.busy_reads, 2);
        space = scripted();
        let mut context = RecordContext::new(space, ());
        assert_eq!(helper_664b74(&mut context, 2), Ok(2));
        assert_eq!(context.memory.busy_reads, 0);
        assert_eq!(context.memory.reads, 0);
        space = scripted();
        let mut context = RecordContext::new(space, ());
        assert_eq!(helper_664b74(&mut context, 5), Ok(5));
        assert_eq!(context.memory.reads, 2);
    }

    #[test]
    fn enable_follows_configuration_word_four() {
        let mut memory = Memory::new();
        memory.config[0x10..0x14].copy_from_slice(&1u32.to_le_bytes());
        memory.with_context(|context| assert_eq!(helper_664e14(context, GuestAddress::new(CONFIG)), Ok(0)));
        assert_eq!(memory.register(OTHER_ENABLE), 1);
        assert_eq!(memory.operation(), 81);
        memory.config[0x10..0x14].copy_from_slice(&2u32.to_le_bytes());
        memory.with_context(|context| assert_eq!(helper_664e14(context, GuestAddress::new(CONFIG)), Ok(0)));
        assert_eq!(memory.register(OTHER_ENABLE), 0);
    }

    #[test]
    fn status_words_swap_sides_with_the_bank_bit() {
        let mut memory = Memory::new();
        memory.set_register(OTHER_STATUS, 0x0002_0201);
        memory.with_context(|context| assert_eq!(helper_665088(context, GuestAddress::new(DESTINATION)), Ok(0)));
        assert_eq!([memory.destination(0), memory.destination(4), memory.destination(8), memory.destination(12), memory.destination(16)], [0, 0, 1, 0, 1]);
        memory.set_register(OTHER_STATUS, 0x0001_0200);
        memory.with_context(|context| assert_eq!(helper_665088(context, GuestAddress::new(DESTINATION)), Ok(0)));
        assert_eq!([memory.destination(0), memory.destination(4), memory.destination(8), memory.destination(12), memory.destination(16)], [1, 0, 0, 0, 0]);
        assert_eq!(memory.operation(), 83);
    }
}
