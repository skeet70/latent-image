//! State byte and profile copies over 0x016b_5a40, image-5 bodies 0x00f2_3900-0x00f2_3b68.
use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext, StackAlignment};
use crate::record_native_exports::stock_export;

pub const STATE_BLOCK: u32 = 0x016b_5a40;

fn stage_address() -> GuestAddress {
    GuestAddress::new(STATE_BLOCK + 0xec)
}

fn state_word<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    context.read_u32(GuestAddress::new(STATE_BLOCK))
}

fn set_stage<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, bits: u8) -> Result<(), MemoryError> {
    let value = context.read_u8(stage_address())? | bits;
    context.write_u8(stage_address(), value)
}

/// The image-5 body at 0x00f2_3aec.
pub fn set_first_stage<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    set_stage(context, 1)
}

/// The image-5 body at 0x00f2_3b14.
pub fn set_second_stage<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    set_stage(context, 2)
}

/// The image-5 body at 0x00f2_3b3c.
pub fn both_stages_set<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<bool, MemoryError> {
    context.read_u8(stage_address()).map(|value| value == 3)
}

/// The image-5 body at 0x00f2_3900.
pub fn helper_f23900<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    let handle = state_word(context)?;
    let first = crate::record_profile_slotfields::helper_754d9c(context, handle)?;
    let handle = state_word(context)?;
    let second = crate::record_profile_slotfields::helper_754f8c(context, handle)?;
    Ok(if first == 0 || first == 3 { u32::from(second != 0) } else { 1 })
}

/// The image-5 body at 0x00f2_3a04.
pub fn helper_f23a04<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    let parameters = crate::record_task43_selection_tail_parameters::helper_ef9e68(context)?.get();
    let handle = state_word(context)?;
    let first = context.read_u32(GuestAddress::new(parameters + 4))?;
    crate::record_helper_profile_resource::helper_754dec(context, handle, first)?;
    let handle = state_word(context)?;
    let second = context.read_u32(GuestAddress::new(parameters + 8))?;
    crate::record_helper_profile_resource::helper_754e8c(context, handle, second)
}

/// The image-5 body at 0x00f2_3b68.
pub fn helper_f23b68<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    context.with_guest_frame(ByteCount(28), StackAlignment::Word, |context, frame| {
        let handle = state_word(context)?;
        crate::record_profile_shared_accessors::helper_756940(context, handle, frame.base())?;
        context.write_u32(frame.base().checked_add(24)?, 0)?;
        let handle = state_word(context)?;
        crate::record_profile_shared_accessors::helper_756970(context, handle, frame.base())
    })?;
    context.with_guest_frame(ByteCount(4), StackAlignment::Word, |context, frame| {
        let handle = state_word(context)?;
        crate::record_profile_shared_accessors::helper_75741c(context, handle, frame.base())?;
        let value = crate::record_profile_globals::helper_76da84(context)?;
        context.write_u16(frame.base(), value as u16)?;
        let handle = state_word(context)?;
        crate::record_profile_shared_accessors::helper_75743c(context, handle, frame.base())
    })
}

stock_export!(
    xm1_power_record_helper_f23900,
    helper_f23900_abi,
    (),
    () -> u32,
    (),
    (),
    crate::record_bank_flags_f22f3c::helper_f23900,
    |value| value
);
stock_export!(
    xm1_power_record_helper_f23a04,
    helper_f23a04_abi,
    (),
    () -> (),
    (),
    (),
    crate::record_bank_flags_f22f3c::helper_f23a04,
    |_| ()
);
stock_export!(
    xm1_power_record_helper_f23b68,
    helper_f23b68_abi,
    (),
    () -> (),
    (),
    (),
    crate::record_bank_flags_f22f3c::helper_f23b68,
    |_| ()
);
stock_export!(
    xm1_power_record_helper_f23aec,
    helper_f23aec_abi,
    (),
    () -> (),
    (),
    (),
    crate::record_bank_flags_f22f3c::set_first_stage,
    |_| ()
);
stock_export!(
    xm1_power_record_helper_f23b14,
    helper_f23b14_abi,
    (),
    () -> (),
    (),
    (),
    crate::record_bank_flags_f22f3c::set_second_stage,
    |_| ()
);
stock_export!(
    xm1_power_record_helper_f23b3c,
    helper_f23b3c_abi,
    (),
    () -> u32,
    (),
    (),
    crate::record_bank_flags_f22f3c::both_stages_set,
    |set| u32::from(set)
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_address_space::{GuestDomain, GuestStack, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const HANDLE: u32 = 0x00fe_0000;

    struct Memory {
        slot: [u8; 0x38],
        resource: [u8; 0x2158],
        state: [u8; 4],
        parameters: [u8; 12],
        globals: [u8; 0x100],
        block: [u8; 0x244],
        stack: [u8; 0x100],
    }

    impl Memory {
        fn new() -> Self {
            let mut memory = Memory { slot: [0; 0x38], resource: [0; 0x2158], state: HANDLE.to_le_bytes(), parameters: [0; 12], globals: [0; 0x100], block: [0; 0x244], stack: [0; 0x100] };
            for field in 0..43 { memory.slot[12 + field] = 3; }
            memory
        }

        fn with_context<R>(&mut self, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
            let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
            let mut regions = [
                MemoryRegion::new(descriptor(0x0091_5d44, 0x38), &mut self.slot).unwrap(),
                MemoryRegion::new(descriptor(0x0096_ebd0, 0x2158), &mut self.resource).unwrap(),
                MemoryRegion::new(descriptor(0x016b_5a40, 4), &mut self.state).unwrap(),
                MemoryRegion::new(descriptor(0x0169_d29c, 12), &mut self.parameters).unwrap(),
                MemoryRegion::new(descriptor(0x0094_2ab0, 0x100), &mut self.globals).unwrap(),
                MemoryRegion::new(descriptor(0x0091_904c, 0x244), &mut self.block).unwrap(),
                MemoryRegion::new(descriptor(0x0010_0000, 0x100), &mut self.stack).unwrap(),
            ];
            let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
            let stack = GuestStack::new(GuestAddress::new(0x0010_0000), ByteCount(0x100)).unwrap();
            body(&mut RecordContext::with_stack(space, (), stack))
        }

        fn word(&self, at: u32) -> u32 {
            u32::from_le_bytes(self.resource[at as usize..at as usize + 4].try_into().unwrap())
        }

        fn set_word(&mut self, at: u32, value: u32) {
            self.resource[at as usize..at as usize + 4].copy_from_slice(&value.to_le_bytes());
        }
    }

    #[test]
    fn each_setter_adds_one_bit_and_keeps_the_others() {
        let mut block = [0u8; 0x100];
        block[0xec] = 0xfc;
        let descriptor = RegionDescriptor::new(GuestAddress::new(STATE_BLOCK), ByteCount(0x100), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [MemoryRegion::new(descriptor, &mut block).unwrap()];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let mut context = RecordContext::new(space, ());
        assert_eq!(set_first_stage(&mut context), Ok(()));
        assert_eq!(set_second_stage(&mut context), Ok(()));
        assert_eq!(block[0xec], 0xff);
    }

    #[test]
    fn both_stages_requires_exactly_the_two_bits() {
        let mut block = [0u8; 0x100];
        let descriptor = RegionDescriptor::new(GuestAddress::new(STATE_BLOCK), ByteCount(0x100), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [MemoryRegion::new(descriptor, &mut block).unwrap()];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let mut context = RecordContext::new(space, ());
        assert_eq!(both_stages_set(&mut context), Ok(false));
        assert_eq!(set_first_stage(&mut context), Ok(()));
        assert_eq!(both_stages_set(&mut context), Ok(false));
        assert_eq!(set_second_stage(&mut context), Ok(()));
        assert_eq!(both_stages_set(&mut context), Ok(true));
        context.write_u8(GuestAddress::new(STATE_BLOCK + 0xec), 0x83).unwrap();
        assert_eq!(both_stages_set(&mut context), Ok(false));
    }

    #[test]
    fn helper_f23900_reads_two_profile_words() {
        let mut memory = Memory::new();
        memory.set_word(0x20b0, 0);
        memory.set_word(0x20c8, 5);
        memory.with_context(|context| assert_eq!(helper_f23900(context), Ok(1)));
        memory.set_word(0x20c8, 0);
        memory.with_context(|context| assert_eq!(helper_f23900(context), Ok(0)));
        memory.set_word(0x20b0, 3);
        memory.set_word(0x20c8, 7);
        memory.with_context(|context| assert_eq!(helper_f23900(context), Ok(1)));
        memory.set_word(0x20c8, 0);
        memory.with_context(|context| assert_eq!(helper_f23900(context), Ok(0)));
        memory.set_word(0x20b0, 1);
        memory.with_context(|context| assert_eq!(helper_f23900(context), Ok(1)));
    }

    #[test]
    fn helper_f23a04_copies_the_parameter_words() {
        let mut memory = Memory::new();
        memory.parameters[4..8].copy_from_slice(&0x1111_2222u32.to_le_bytes());
        memory.parameters[8..12].copy_from_slice(&0x3333_4444u32.to_le_bytes());
        memory.with_context(|context| assert_eq!(helper_f23a04(context), Ok(())));
        assert_eq!(memory.word(0x20b0), 0x1111_2222);
        assert_eq!(memory.word(0x20b4), 0x3333_4444);
    }

    #[test]
    fn helper_f23b68_clears_the_last_snapshot_word_and_replaces_the_low_halfword() {
        let mut memory = Memory::new();
        for index in 0..7u32 { memory.block[0x120 + index as usize * 4..0x124 + index as usize * 4].copy_from_slice(&(0x100 + index).to_le_bytes()); }
        memory.block[0x240..0x244].copy_from_slice(&0xaabb_ccddu32.to_le_bytes());
        memory.globals[0xd4..0xd6].copy_from_slice(&0x1234u16.to_le_bytes());
        memory.with_context(|context| assert_eq!(helper_f23b68(context), Ok(())));
        for index in 0..6u32 { assert_eq!(&memory.block[0x120 + index as usize * 4..0x124 + index as usize * 4], &(0x100 + index).to_le_bytes()); }
        assert_eq!(&memory.block[0x138..0x13c], &0u32.to_le_bytes());
        assert_eq!(&memory.block[0x240..0x244], &0xaabb_1234u32.to_le_bytes());
    }
}
