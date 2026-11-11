//! The task 43 auxiliary head: image-5 bodies 0x00f2_2f3c-0x00f3_3164 that
//! publish the exposure record at 0x016b_7864 and the shared block fields.

use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext, StackAlignment};
use crate::record_hardware_requests::{CaptureCommand, CaptureQueue};
use crate::record_profile_accessors::{offset, HeaderField};

/// The task 43 state word.
pub const STATE: u32 = 0x016b_7864;
pub const RECORD_BLOCK: u32 = 0x016b_7918;
pub const RECORD_SLICES: [(u32, u32); 4] = [(0, 56), (56, 16), (72, 16), (88, 56)];
/// The record 0x00f2_f400 publishes.
pub const ADJUSTMENT_BLOCK: u32 = 0x016b_dfe4;
pub const ADJUSTMENT_BYTES: u32 = 24;
/// The handle 0x00f3_3164 stores its fields under.
pub const HANDLE: u32 = 0x016b_fe48;
/// The record 0x00f3_3164 copies when the profile mode byte is one.
pub const AUX_RECORD: u32 = 0x016b_ec44;
pub const AUX_RECORD_BYTES: u32 = 76;
pub const AUX_RECORD_MARK: u32 = 52;

/// The image-5 body at 0x00f2_e600.
pub fn helper_f2e600<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, first: GuestAddress, second: GuestAddress, third: GuestAddress, fourth: GuestAddress) -> Result<(), MemoryError> {
    context.copy_bytes(first, GuestAddress::new(RECORD_BLOCK), ByteCount(56))?;
    context.copy_bytes(second, GuestAddress::new(RECORD_BLOCK + 56), ByteCount(16))?;
    context.copy_bytes(third, GuestAddress::new(RECORD_BLOCK + 72), ByteCount(16))?;
    context.copy_bytes(fourth, GuestAddress::new(RECORD_BLOCK + 88), ByteCount(56))
}

/// The image-5 body at 0x00f2_f400.
pub fn helper_f2f400<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, source: GuestAddress) -> Result<(), MemoryError> {
    context.copy_bytes(GuestAddress::new(ADJUSTMENT_BLOCK), source, ByteCount(ADJUSTMENT_BYTES))
}

/// The image-5 body at 0x00f2_d440.
pub fn helper_f2d440<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    let state = crate::record_helper_constants::helper_ef9c00(context)?;
    let outer = context.read_u32(GuestAddress::new(state + 0x38))?;
    if context.read_u32(GuestAddress::new(outer).checked_add(0x18)?)? == 2 { return Ok(0); }
    let now = crate::record_profile_mapping::helper_76dbb4(context)?;
    Ok(u32::from(now != context.read_u32(GuestAddress::new(STATE + 0xa0))?))
}

/// The image-5 body at 0x00f2_d4e0.
pub fn helper_f2d4e0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    let state = crate::record_helper_constants::helper_ef9c00(context)?;
    let handle = context.read_u32(GuestAddress::new(STATE))?;
    if crate::record_profile_accessors::helper_74e648(context, handle)? == 50 {
        return context.with_guest_frame(ByteCount(40), StackAlignment::Word, |context, frame| {
            let record = frame.base().checked_add(4)?;
            crate::record_task43_selection_timing_state::helper_758940(context, record)?;
            let outer = context.read_u32(GuestAddress::new(state + 0x38))?;
            let selector = context.read_u8(GuestAddress::new(outer))?;
            let index = if selector.wrapping_sub(1) <= 2 { u32::from(selector) } else { 1 };
            let high = context.read_u16(frame.base().checked_add(8 + index * 8)?)?;
            let low = context.read_u16(frame.base().checked_add(index * 8)?)?;
            context.write_u16(GuestAddress::new(STATE + 140), high.wrapping_sub(low))?;
            let value = context.read_u32(frame.base().checked_add(12 + index * 8)?)?;
            context.write_u32(GuestAddress::new(STATE + 0x90), value)
        });
    }
    let mode = u32::from(crate::record_profile_accessors::helper_75093c(context, handle)? == 1);
    let value = crate::record_profile_globals::helper_76da84(context)?;
    let bank = context.read_u32(GuestAddress::new(STATE + 0x48))?;
    let source = crate::record_profile_mapping::helper_76dbb4(context)?;
    let destination = context.read_u32(GuestAddress::new(STATE + 0xa0))?;
    let direction = crate::record_profile_globals::helper_76dad4(context)?;
    let result = crate::record_profile_mapping::helper_74947c(context, value, bank, source, destination, mode, mode, direction)?;
    context.write_u16(GuestAddress::new(STATE + 140), result.wrapping_sub(value) as u16)?;
    let direction = crate::record_profile_globals::helper_76dad4(context)?;
    context.write_u32(GuestAddress::new(STATE + 0x90), direction)
}

/// The image-5 body at 0x00f2_d5d0.
pub fn helper_f2d5d0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<i32, MemoryError> {
    let handle = context.read_u32(GuestAddress::new(STATE))?;
    if crate::record_profile_accessors::helper_74e648(context, handle)? != 50 {
        return helper_f2d440(context).map(|value| value as i32);
    }
    context.with_guest_frame(ByteCount(36), StackAlignment::Word, |context, frame| {
        crate::record_task43_selection_timing_state::helper_758940(context, frame.base())?;
        Ok(i32::from(context.read_u8(frame.base())? > 1))
    })
}

/// The image-5 body at 0x00f3_3164.
#[allow(clippy::too_many_arguments)]
pub fn helper_f33164<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>,
    first: u32,
    second: u32,
    third: u32,
    fourth: u32,
    fifth: i32,
    sixth: i32,
    seventh: i32,
    eighth: u32,
    ninth: u32,
    limit: u32,
    flag: u32,
    twelfth: u32,
    base: u32,
) -> Result<(), MemoryError> {
    let mapped = crate::record_profile_mapping::helper_746bf8(context, first, base)?;
    let head = second.wrapping_sub(third).wrapping_sub(sixth as u32).wrapping_add(mapped) as u16;
    let tail = if twelfth == 1 { head.wrapping_add((fourth.wrapping_sub(100)) as u16) } else { head.wrapping_add(fourth as u16) };
    let handle = context.read_u32(GuestAddress::new(HANDLE))?;
    let calibration = crate::record_hardware_deep_calibration::helper_ef5310(context, ninth)?;
    use crate::record_shared_block_fill_756700::store;
    store(context, handle, HeaderField::Halfword(0x248), head as i16 as i32 as u32)?;
    store(context, handle, HeaderField::Halfword(0x24a), tail as i16 as i32 as u32)?;
    store(context, handle, HeaderField::Word(0x708), first)?;
    store(context, handle, HeaderField::Halfword(0x70c), second)?;
    store(context, handle, HeaderField::Halfword(0x70e), third)?;
    store(context, handle, HeaderField::Halfword(0x710), seventh as i16 as i32 as u32)?;
    store(context, handle, HeaderField::Halfword(0x712), calibration as u32)?;
    store(context, handle, HeaderField::Halfword(0x718), fourth)?;
    store(context, handle, HeaderField::Halfword(0x714), fifth as i16 as i32 as u32)?;
    store(context, handle, HeaderField::Halfword(0x716), sixth as i16 as i32 as u32)?;
    store(context, handle, HeaderField::Word(0x71c), eighth)?;
    store(context, handle, HeaderField::Word(0x724), ninth)?;
    store(context, handle, HeaderField::Halfword(0x728), limit & 0xffff)?;
    store(context, handle, HeaderField::Byte(0x72a), flag & 0xff)?;
    store(context, handle, HeaderField::Word(0x72c), twelfth)?;
    if crate::record_profile_accessors::helper_7503ec(context, handle)? != 1 { return Ok(()); }
    context.with_guest_frame(ByteCount(AUX_RECORD_BYTES), StackAlignment::Word, |context, frame| {
        crate::record_profile_shared_cpu::helper_7588e8(context, frame.base())?;
        context.write_u32(frame.base().checked_add(AUX_RECORD_MARK)?, eighth)?;
        crate::record_task43_selection_timing_state::helper_75890c(context, handle, frame.base())
    })
}

mod exports {
    use super::*;
    use crate::record_native_exports::stock_export;

    stock_export!(xm1_power_record_helper_f2d440, helper_f2d440_abi, (), () -> u32, (), (), crate::record_task43_aux_head_f33164::helper_f2d440, |value| value);
    stock_export!(xm1_power_record_helper_f2d4e0, helper_f2d4e0_abi, (), () -> (), (), (), crate::record_task43_aux_head_f33164::helper_f2d4e0, |_| ());
    stock_export!(xm1_power_record_helper_f2d5d0, helper_f2d5d0_abi, (), () -> i32, (), (), crate::record_task43_aux_head_f33164::helper_f2d5d0, |value| value);
    stock_export!(
        xm1_power_record_helper_f2e600, helper_f2e600_abi, (),
        (first: u32, second: u32, third: u32, fourth: u32) -> (),
        (first: GuestAddress, second: GuestAddress, third: GuestAddress, fourth: GuestAddress),
        (GuestAddress::new(first), GuestAddress::new(second), GuestAddress::new(third), GuestAddress::new(fourth)),
        crate::record_task43_aux_head_f33164::helper_f2e600,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f2f400, helper_f2f400_abi, (),
        (source: u32) -> (),
        (source: GuestAddress),
        (GuestAddress::new(source)),
        crate::record_task43_aux_head_f33164::helper_f2f400,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f33164, helper_f33164_abi, (),
        (first: u32, second: u32, third: u32, fourth: u32, fifth: i32, sixth: i32, seventh: i32, eighth: u32, ninth: u32, limit: u32, flag: u32, twelfth: u32, base: u32) -> (),
        (first: u32, second: u32, third: u32, fourth: u32, fifth: i32, sixth: i32, seventh: i32, eighth: u32, ninth: u32, limit: u32, flag: u32, twelfth: u32, base: u32),
        (first, second, third, fourth, fifth, sixth, seventh, eighth, ninth, limit, flag, twelfth, base),
        crate::record_task43_aux_head_f33164::helper_f33164,
        |_| ()
    );
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 bodies at 0x00f2_2f3c-0x00f3_3164.
    use super::*;
    use crate::record_address_space::{GuestDomain, GuestStack, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    /// Offset of RECORD_BLOCK inside the state region.
    const RECORDS: usize = 0xb4;
    const RESOURCES: usize = 0x2180;

    struct Memory {
        registry: [u8; 0x38c],
        resources: [u8; 0x6480],
        state: [u8; 0x200],
        adjustment: [u8; 0x18],
        destination: [u8; 0x40],
        first: [u8; 0x40],
        second: [u8; 0x20],
        third: [u8; 0x20],
        fourth: [u8; 0x40],
        constants: [u8; 0x40],
        outer: [u8; 0x20],
        globals: [u8; 0x200],
        split: [u8; 1],
        block: [u8; 0x9d4],
        stack: [u8; 0x100],
    }

    impl Memory {
        fn new() -> Self {
            Memory {
                registry: [0; 0x38c], resources: [0; 0x6480], state: [0; 0x200],
                adjustment: [0; 0x18], destination: [0; 0x40], first: [0; 0x40],
                second: [0; 0x20], third: [0; 0x20], fourth: [0; 0x40],
                constants: [0; 0x40], outer: [0; 0x20], globals: [0; 0x200], split: [0; 1],
                block: [0; 0x9d4], stack: [0; 0x100],
            }
        }

        fn with_context<R>(&mut self, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
            let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
            let mut regions = [
                MemoryRegion::new(descriptor(0x0091_5d38, 0x38c), &mut self.registry).unwrap(),
                MemoryRegion::new(descriptor(0x0096_ca50, 0x6480), &mut self.resources).unwrap(),
                MemoryRegion::new(descriptor(STATE, 0x200), &mut self.state).unwrap(),
                MemoryRegion::new(descriptor(ADJUSTMENT_BLOCK, 0x18), &mut self.adjustment).unwrap(),
                MemoryRegion::new(descriptor(0x0010_0000, 0x40), &mut self.destination).unwrap(),
                MemoryRegion::new(descriptor(0x0010_1000, 0x40), &mut self.first).unwrap(),
                MemoryRegion::new(descriptor(0x0010_2000, 0x20), &mut self.second).unwrap(),
                MemoryRegion::new(descriptor(0x0010_3000, 0x20), &mut self.third).unwrap(),
                MemoryRegion::new(descriptor(0x0010_4000, 0x40), &mut self.fourth).unwrap(),
                MemoryRegion::new(descriptor(0x0169_d09c, 0x40), &mut self.constants).unwrap(),
                MemoryRegion::new(descriptor(0x0010_5000, 0x20), &mut self.outer).unwrap(),
                MemoryRegion::new(descriptor(0x0094_2ab0, 0x200), &mut self.globals).unwrap(),
                MemoryRegion::new(descriptor(0x003e_0f82, 1), &mut self.split).unwrap(),
                MemoryRegion::new(descriptor(0x0091_904c, 0x9d4), &mut self.block).unwrap(),
                MemoryRegion::new(descriptor(0x0010_6000, 0x100), &mut self.stack).unwrap(),
            ];
            let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
            let stack = GuestStack::new(GuestAddress::new(0x0010_6000), ByteCount(0x100)).unwrap();
            body(&mut RecordContext::with_stack(space, (), stack))
        }

        fn admitted(&mut self, bank: u32, serial: u32, field: usize, permission: u8) {
            self.registry[0x0c..0x10].copy_from_slice(&serial.to_le_bytes());
            self.registry[0x14..0x18].copy_from_slice(&bank.to_le_bytes());
            self.registry[0x18 + field] = permission;
        }
    }

    #[test]
    fn the_record_slices_leave_in_their_four_lengths() {
        let mut memory = Memory::new();
        for index in 0..0x70 { memory.state[RECORDS + index] = index as u8; }
        memory.with_context(|context| {
            assert_eq!(helper_f2e600(context, GuestAddress::new(0x0010_0000), GuestAddress::new(0x0010_1000), GuestAddress::new(0x0010_2000), GuestAddress::new(0x0010_4000)), Ok(()));
        });
        assert_eq!(&memory.destination[..56], &memory.state[RECORDS..RECORDS + 56]);
        assert_eq!(&memory.first[..16], &memory.state[RECORDS + 56..RECORDS + 72]);
        assert_eq!(&memory.second[..16], &memory.state[RECORDS + 72..RECORDS + 88]);
        assert_eq!(&memory.third[..16], &[0u8; 16]);
        assert_eq!(&memory.fourth[..56], &memory.state[RECORDS + 88..RECORDS + 144]);
    }

    #[test]
    fn the_adjustment_record_takes_twenty_four_bytes() {
        let mut memory = Memory::new();
        for index in 0..0x18 { memory.destination[index] = index as u8 + 1; }
        memory.with_context(|context| assert_eq!(helper_f2f400(context, GuestAddress::new(0x0010_0000)), Ok(())));
        assert_eq!(&memory.adjustment[..0x18], &memory.destination[..0x18]);
    }

    #[test]
    fn the_state_query_answers_from_the_outer_field_and_the_mapped_word() {
        let mut memory = Memory::new();
        memory.constants[0x38..0x3c].copy_from_slice(&0x0010_5000u32.to_le_bytes());
        memory.outer[0x18..0x1c].copy_from_slice(&2u32.to_le_bytes());
        memory.with_context(|context| assert_eq!(helper_f2d440(context), Ok(0)));
        memory.outer[0x18..0x1c].copy_from_slice(&0u32.to_le_bytes());
        memory.state[0xa0..0xa4].copy_from_slice(&7u32.to_le_bytes());
        memory.with_context(|context| assert_eq!(helper_f2d440(context), Ok(1)));
    }

    #[test]
    fn the_record_publication_indexes_the_filled_table() {
        let mut memory = Memory::new();
        memory.admitted(0, 0, 0, 3);
        memory.resources[RESOURCES + 0x31] = 50;
        memory.constants[0x38..0x3c].copy_from_slice(&0x0010_5000u32.to_le_bytes());
        memory.outer[0] = 2;
        memory.block[0x814 + 12..0x814 + 14].copy_from_slice(&100u16.to_le_bytes());
        memory.block[0x814 + 20..0x814 + 22].copy_from_slice(&160u16.to_le_bytes());
        memory.block[0x814 + 24..0x814 + 28].copy_from_slice(&0x1234u32.to_le_bytes());
        memory.with_context(|context| assert_eq!(helper_f2d4e0(context), Ok(())));
        assert_eq!(u16::from_le_bytes(memory.state[140..142].try_into().unwrap()), 60);
        assert_eq!(u32::from_le_bytes(memory.state[0x90..0x94].try_into().unwrap()), 0x1234);
        assert_eq!(u32::from_le_bytes(memory.state[0xa0..0xa4].try_into().unwrap()), 0);
    }

    #[test]
    fn the_state_query_reads_the_mode_byte() {
        let mut memory = Memory::new();
        memory.admitted(0, 0, 0, 3);
        memory.resources[RESOURCES + 0x31] = 50;
        memory.block[0x814] = 2;
        memory.with_context(|context| assert_eq!(helper_f2d5d0(context), Ok(1)));
        memory.block[0x814] = 1;
        memory.with_context(|context| assert_eq!(helper_f2d5d0(context), Ok(0)));
        memory.block[0x814] = 0;
        memory.with_context(|context| assert_eq!(helper_f2d5d0(context), Ok(0)));
    }

    #[test]
    fn the_state_query_falls_back_to_the_outer_field() {
        let mut memory = Memory::new();
        memory.constants[0x38..0x3c].copy_from_slice(&0x0010_5000u32.to_le_bytes());
        memory.outer[0x18..0x1c].copy_from_slice(&2u32.to_le_bytes());
        memory.with_context(|context| assert_eq!(helper_f2d5d0(context), Ok(0)));
    }
}
