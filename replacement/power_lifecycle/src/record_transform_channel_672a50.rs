//! Transform channel window at 0xfff1_2000, image-4 bodies 0x00672a50,
//! 0x00672b2c and 0x00673f1c.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

pub const TRANSFORM_STATE: u32 = 0x0082_0f30;
pub const CHANNEL_WORDS: u32 = 0xfff1_2000;
pub const CHANNEL_PAGE: u32 = 0x1000;
pub const CONTROL: u32 = 0x1c;
pub const WINDOW: u32 = 0x20;
pub const TABLE: u32 = 0x30;
pub const TABLE_ENTRIES: u32 = 31;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct TransformChannel(pub u32);

impl TransformChannel {
    pub const fn base(self) -> u32 { CHANNEL_WORDS.wrapping_add(self.0.wrapping_mul(CHANNEL_PAGE)) }
    pub const fn control(self) -> GuestAddress { GuestAddress::new(self.base().wrapping_add(CONTROL)) }
    pub const fn window(self) -> GuestAddress { GuestAddress::new(self.base().wrapping_add(WINDOW)) }
    pub const fn table(self, index: u32) -> GuestAddress { GuestAddress::new(self.base().wrapping_add(TABLE).wrapping_add(index.wrapping_mul(4))) }
}

fn operation<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<(), MemoryError> {
    context.write_u32(GuestAddress::new(TRANSFORM_STATE + 4), value)
}

/// The image-4 body at 0x00672a50.
pub fn helper_672a50<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>,
    _handle: u32,
    fields: GuestAddress,
    channel: TransformChannel,
) -> Result<u32, MemoryError> {
    operation(context, 16)?;
    let window = context.read_u32(channel.window())?;
    context.write_u16(fields, (window & 0x7fff) as u16)?;
    context.write_u16(fields.checked_add(2)?, ((window >> 16) & 0x7fff) as u16)?;
    let packed = context.read_u32(channel.window().checked_add(4)?)?;
    context.write_u8(fields.checked_add(4)?, (packed & 0xf) as u8)?;
    context.write_u8(fields.checked_add(5)?, ((packed >> 16) & 0x3f) as u8)?;
    Ok(0)
}

/// The image-4 body at 0x00672b2c.
pub fn helper_672b2c<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>,
    _category: u32,
    table: GuestAddress,
    bank: u32,
) -> Result<u32, MemoryError> {
    operation(context, 17)?;
    let target = TransformChannel(bank);
    for index in 0..TABLE_ENTRIES {
        let value = context.read_u32(table.checked_add(index * 4)?)? & 0x1_ffff;
        context.write_u32(target.table(index), value)?;
    }
    Ok(0)
}

/// The image-4 body at 0x00673f1c.
pub fn helper_673f1c<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>,
    _handle: u32,
    channel: TransformChannel,
) -> Result<u32, MemoryError> {
    operation(context, 36)?;
    let control = channel.control();
    let value = context.read_u32(control)? & !1 & !(1 << 16);
    context.write_u32(control, value)?;
    while context.read_u32(control)? & 1 == 0 { core::hint::spin_loop(); }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_address_space::{
        ByteCount, GuestDomain, HostAddressSpace, MemoryError, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission,
    };

    const STATE: u32 = 0x0082_0f30;
    const TABLE_SOURCE: u32 = 0x0010_0000;
    const FIELDS: u32 = 0x0010_0100;

    struct Memory {
        channels: [u8; 0x3000],
        state: [u8; 0x20],
        source: [u8; 0x80],
        fields: [u8; 0x10],
    }

    impl Memory {
        fn new() -> Self {
            Self { channels: [0; 0x3000], state: [0; 0x20], source: [0; 0x80], fields: [0; 0x10] }
        }

        fn with_context<R>(&mut self, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
            let descriptor = |base, length| {
                RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap()
            };
            let mut regions = [
                MemoryRegion::new(descriptor(CHANNEL_WORDS, 0x3000), &mut self.channels).unwrap(),
                MemoryRegion::new(descriptor(STATE, 0x20), &mut self.state).unwrap(),
                MemoryRegion::new(descriptor(TABLE_SOURCE, 0x80), &mut self.source).unwrap(),
                MemoryRegion::new(descriptor(FIELDS, 0x10), &mut self.fields).unwrap(),
            ];
            body(&mut RecordContext::new(HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap(), ()))
        }

        fn channel_word(&self, channel: u32, offset: u32) -> u32 {
            let at = (channel * CHANNEL_PAGE + offset) as usize;
            u32::from_le_bytes(self.channels[at..at + 4].try_into().unwrap())
        }

        fn set_channel_word(&mut self, channel: u32, offset: u32, value: u32) {
            let at = (channel * CHANNEL_PAGE + offset) as usize;
            self.channels[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }

        fn field(&self, offset: usize, length: usize) -> u32 {
            let mut bytes = [0; 4];
            bytes[..length].copy_from_slice(&self.fields[offset..offset + length]);
            u32::from_le_bytes(bytes)
        }

        fn operation(&self) -> u32 { u32::from_le_bytes(self.state[4..8].try_into().unwrap()) }
    }

    /// The control word reports the awaited bit after a scripted read count.
    struct ScriptedSpace {
        control: u32,
        reads: u32,
        set_after: u32,
        diagnostic: u32,
    }

    impl GuestAddressSpace for ScriptedSpace {
        fn domain(&self) -> GuestDomain { GuestDomain::A }
        fn read_u8(&mut self, address: GuestAddress) -> Result<u8, MemoryError> { Err(MemoryError::Unmapped(address)) }
        fn read_u16(&mut self, address: GuestAddress) -> Result<u16, MemoryError> { Err(MemoryError::Unmapped(address)) }
        fn read_u32(&mut self, address: GuestAddress) -> Result<u32, MemoryError> {
            if address.get() == CHANNEL_WORDS + CONTROL {
                self.reads += 1;
                Ok(if self.reads >= self.set_after { self.control | 1 } else { self.control })
            } else if address.get() == TRANSFORM_STATE + 4 {
                Ok(self.diagnostic)
            } else {
                Err(MemoryError::Unmapped(address))
            }
        }
        fn write_u8(&mut self, address: GuestAddress, _: u8) -> Result<(), MemoryError> { Err(MemoryError::Unmapped(address)) }
        fn write_u16(&mut self, address: GuestAddress, _: u16) -> Result<(), MemoryError> { Err(MemoryError::Unmapped(address)) }
        fn write_u32(&mut self, address: GuestAddress, value: u32) -> Result<(), MemoryError> {
            if address.get() == CHANNEL_WORDS + CONTROL {
                self.control = value;
                Ok(())
            } else if address.get() == TRANSFORM_STATE + 4 {
                self.diagnostic = value;
                Ok(())
            } else {
                Err(MemoryError::Unmapped(address))
            }
        }
        fn copy_bytes(&mut self, address: GuestAddress, _: GuestAddress, _: ByteCount) -> Result<(), MemoryError> { Err(MemoryError::Unmapped(address)) }
    }

    #[test]
    fn read_window_unpacks_the_control_and_window_words() {
        let mut memory = Memory::new();
        memory.set_channel_word(1, WINDOW, 0x1234_abcd);
        memory.set_channel_word(1, WINDOW + 4, 0x0021_000f);
        memory.with_context(|context| assert_eq!(helper_672a50(context, 0, GuestAddress::new(FIELDS), TransformChannel(1)), Ok(0)));
        assert_eq!(memory.field(0, 2), 0x2bcd);
        assert_eq!(memory.field(2, 2), 0x1234);
        assert_eq!(memory.field(4, 1), 0xf);
        assert_eq!(memory.field(5, 1), 0x21);
        assert_eq!(memory.operation(), 16);
    }

    #[test]
    fn program_table_writes_seventeen_bit_words_into_the_bank_page() {
        let mut memory = Memory::new();
        for index in 0..31u32 {
            memory.source[index as usize * 4..index as usize * 4 + 4].copy_from_slice(&(0x1_0000 | index).to_le_bytes());
        }
        memory.with_context(|context| assert_eq!(helper_672b2c(context, 0, GuestAddress::new(TABLE_SOURCE), 2), Ok(0)));
        for index in 0..31u32 {
            assert_eq!(memory.channel_word(2, TABLE + index * 4), 0x1_0000 | index);
        }
        assert_eq!(memory.channel_word(2, TABLE + 31 * 4), 0);
        assert_eq!(memory.operation(), 17);
    }

    #[test]
    fn clear_and_wait_drops_the_two_bits_and_waits_for_bit_zero() {
        let mut context = RecordContext::new(
            ScriptedSpace { control: 0x0001_0003, reads: 0, set_after: 2, diagnostic: 0 },
            (),
        );
        assert_eq!(helper_673f1c(&mut context, 0, TransformChannel(0)), Ok(0));
        assert_eq!(context.memory.control, 2);
        assert_eq!(context.memory.reads, 2);
        assert_eq!(context.memory.diagnostic, 36);
    }
}
