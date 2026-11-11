//! Pending-flag clears in the state block at 0x0169_964c.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

const STATE: u32 = 0x0169_964c;

fn clear_if_one<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, offset: u32) -> Result<(), MemoryError> {
    let address = GuestAddress::new(STATE + offset);
    if context.read_u32(address)? == 1 {
        context.write_u32(address, 0)?;
    }
    Ok(())
}

pub fn helper_ee2ca0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    clear_if_one(context, 0x00)?;
    Ok(0)
}

pub fn helper_ee2e94<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    clear_if_one(context, 0x04)?;
    Ok(0)
}

pub fn helper_ee3108<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    if context.read_u32(GuestAddress::new(STATE + 0x10))? == 1 {
        context.write_u32(GuestAddress::new(STATE + 0x10), 0)?;
        for offset in [0x18, 0x1c, 0x20, 0x24, 0x28, 0x2c, 0x30, 0x34, 0x38] {
            clear_if_one(context, offset)?;
        }
    }
    Ok(0)
}

pub fn helper_ee3404<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    clear_if_one(context, 0x08)?;
    clear_if_one(context, 0x0c)?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 bodies at 0x00ee2ca0-0x00ee3454.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    struct Memory { state: [u8; 0x40] }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = RegionDescriptor::new(GuestAddress::new(STATE), ByteCount(0x40), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [MemoryRegion::new(descriptor, &mut memory.state).unwrap()];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    fn word(memory: &Memory, offset: u32) -> u32 { u32::from_le_bytes(memory.state[offset as usize..offset as usize + 4].try_into().unwrap()) }

    #[test]
    fn only_the_one_flags_are_cleared() {
        let mut memory = Memory { state: [0xff; 0x40] };
        memory.state[0x00..0x04].copy_from_slice(&1u32.to_le_bytes());
        memory.state[0x04..0x08].copy_from_slice(&2u32.to_le_bytes());
        memory.state[0x08..0x0c].copy_from_slice(&1u32.to_le_bytes());
        memory.state[0x0c..0x10].copy_from_slice(&1u32.to_le_bytes());
        with_memory(&mut memory, |context| {
            assert_eq!(helper_ee2ca0(context), Ok(0));
            assert_eq!(helper_ee2e94(context), Ok(0));
            assert_eq!(helper_ee3404(context), Ok(0));
        });
        assert_eq!(word(&memory, 0x00), 0);
        assert_eq!(word(&memory, 0x04), 2);
        assert_eq!(word(&memory, 0x08), 0);
        assert_eq!(word(&memory, 0x0c), 0);
    }

    #[test]
    fn the_group_clear_requires_the_sixteen_word() {
        let mut memory = Memory { state: [0; 0x40] };
        for offset in [0x18u32, 0x1c, 0x20, 0x24, 0x28, 0x2c, 0x30, 0x34, 0x38] {
            memory.state[offset as usize..offset as usize + 4].copy_from_slice(&1u32.to_le_bytes());
        }
        with_memory(&mut memory, |context| assert_eq!(helper_ee3108(context), Ok(0)));
        for offset in [0x18u32, 0x1c, 0x20, 0x24, 0x28, 0x2c, 0x30, 0x34, 0x38] { assert_eq!(word(&memory, offset), 1); }
        memory.state[0x10..0x14].copy_from_slice(&1u32.to_le_bytes());
        with_memory(&mut memory, |context| assert_eq!(helper_ee3108(context), Ok(0)));
        assert_eq!(word(&memory, 0x10), 0);
        for offset in [0x18u32, 0x1c, 0x20, 0x24, 0x28, 0x2c, 0x30, 0x34, 0x38] { assert_eq!(word(&memory, offset), 0); }
    }
}
