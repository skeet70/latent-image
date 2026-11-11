//! Publication of the 0x016b_5a40 state word, image-5 entries 0x00f23944 and 0x00f240b0.

use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext, StackAlignment};

pub const STATE_BLOCK: u32 = 0x016b_5a40;

const FIRST_SNAPSHOT_WORDS: u32 = 7;
const SECOND_SNAPSHOT_WORDS: u32 = 12;
const THIRD_SNAPSHOT_WORDS: u32 = 7;
const SECOND_SNAPSHOT_GATE: u32 = 32;

pub trait CompletionPublish {
    fn publish(&mut self, status: u32, first: u32, second: u32, third: u32, fourth: u32);
}

#[cfg(target_arch = "arm")]
#[derive(Default)]
pub struct StockCompletion;

#[cfg(target_arch = "arm")]
impl CompletionPublish for StockCompletion {
    fn publish(&mut self, status: u32, first: u32, second: u32, third: u32, fourth: u32) {
        unsafe { crate::record_task43::xm1_power_record_helper_efa258(status, first, second, third, fourth) }
    }
}

pub fn helper_f23944<M: GuestAddressSpace, E: CompletionPublish>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    let handle = context.read_u32(GuestAddress::new(STATE_BLOCK))?;
    let first = crate::record_profile_slotfields::helper_752714(context, handle)?;
    let fourth = (if crate::record_profile_globals::helper_76d9a8(context)? == 2 { 2 } else { 0 })
        | (if crate::record_profile_globals::helper_76da38(context)? == 2 { 1 } else { 0 });
    let second = crate::record_profile_globals::helper_76da48(context)?;
    context.effects.publish(0, first, second, 0, fourth);
    Ok(())
}

pub fn helper_f240b0<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>, handle: u32, value: u32, target: u32, zoom: u32, output: GuestAddress,
) -> Result<u32, MemoryError> {
    context.write_u16(output, value as u16)?;
    let empty = context.with_guest_frame(ByteCount(FIRST_SNAPSHOT_WORDS * 4), StackAlignment::Word, |context, frame| {
        crate::record_profile_shared_cpu::helper_758c68(context, handle, frame.base())?;
        let converted = crate::record_task43_resource_forward::helper_f47330(context, handle)?;
        Ok::<bool, MemoryError>(converted == 1 && context.read_u32(frame.base())? == 0)
    })?;
    if empty { return Ok(0); }
    context.with_guest_frame(ByteCount(SECOND_SNAPSHOT_WORDS * 4), StackAlignment::Word, |context, frame| {
        crate::record_shared_block_756720::helper_756840(context, handle, frame.base())?;
        if context.read_u32(frame.base().checked_add(SECOND_SNAPSHOT_GATE)?)? != 1 { return Ok(()); }
        let format = u32::from(crate::record_profile_accessors::helper_75093c(context, handle)? == 1);
        let position = context.read_u16(output)?;
        let kind = crate::record_profile_globals::helper_76dad4(context)?;
        let result = crate::record_task43_resource_forward::helper_f47300(context, position as i16 as i32 as u32, zoom, 0, target, 0, format, kind)?;
        context.write_u16(output, result as u16)
    })?;
    context.with_guest_frame(ByteCount(THIRD_SNAPSHOT_WORDS * 4), StackAlignment::Word, |context, frame| {
        crate::record_profile_shared_accessors::helper_756940(context, handle, frame.base())
    })?;
    Ok(1)
}

mod exports {
    use super::*;
    use crate::record_native_exports::stock_export;

    stock_export!(
        xm1_power_record_helper_f23944,
        helper_f23944_abi,
        StockCompletion,
        () -> (),
        (),
        (),
        crate::record_state_publication_f23944::helper_f23944,
        |_| (),
        bounds CompletionPublish
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_address_space::{GuestDomain, GuestStack, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const SLOT: u32 = 0x0091_5d44;
    const SELECTED: u32 = 0x0091_5d40;
    const RESOURCE: u32 = 0x0096_ebd0;
    const SHARED: u32 = 0x0091_904c;
    const POINTER: u32 = 0x0096_ca50;
    const GLOBALS: u32 = 0x0094_2ab0;
    const STACK: u32 = 0x0010_0000;
    const OUTPUT: u32 = 0x0080_0000;
    const HANDLE: u32 = u32::MAX;

    #[derive(Default)]
    struct Recording {
        published: [Option<(u32, u32, u32, u32, u32)>; 4],
        count: usize,
    }

    impl CompletionPublish for Recording {
        fn publish(&mut self, status: u32, first: u32, second: u32, third: u32, fourth: u32) {
            if self.count < 4 { self.published[self.count] = Some((status, first, second, third, fourth)); }
            self.count += 1;
        }
    }

    struct Memory {
        slots: [u8; 16 * 0x38],
        selected: [u8; 4],
        resource: [u8; 0x2158],
        shared: [u8; 0x900],
        state: [u8; 4],
        pointer: [u8; 4],
        enabled: [u8; 2],
        calibration: [u8; 2],
        globals: [u8; 0x120],
        stack: [u8; 0x1000],
        output: [u8; 2],
    }

    impl Memory {
        fn new() -> Self {
            let mut memory = Memory {
                slots: [0; 16 * 0x38], selected: [0; 4], resource: [0; 0x2158], shared: [0; 0x900], state: [0; 4],
                pointer: [0; 4], enabled: [0; 2], calibration: [0; 2], globals: [0; 0x120], stack: [0; 0x1000], output: [0; 2],
            };
            memory.state.copy_from_slice(&0x1230u32.to_le_bytes());
            memory
        }

        fn with_context<R>(&mut self, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, Recording>) -> R) -> R {
            let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
            let mut regions = [
                MemoryRegion::new(descriptor(SLOT, 16 * 0x38), &mut self.slots).unwrap(),
                MemoryRegion::new(descriptor(SELECTED, 4), &mut self.selected).unwrap(),
                MemoryRegion::new(descriptor(RESOURCE, 0x2158), &mut self.resource).unwrap(),
                MemoryRegion::new(descriptor(SHARED, 0x900), &mut self.shared).unwrap(),
                MemoryRegion::new(descriptor(STATE_BLOCK, 4), &mut self.state).unwrap(),
                MemoryRegion::new(descriptor(POINTER, 4), &mut self.pointer).unwrap(),
                MemoryRegion::new(descriptor(0x887c, 2), &mut self.enabled).unwrap(),
                MemoryRegion::new(descriptor(0x003e_0240, 2), &mut self.calibration).unwrap(),
                MemoryRegion::new(descriptor(GLOBALS, 0x120), &mut self.globals).unwrap(),
                MemoryRegion::new(descriptor(STACK, 0x1000), &mut self.stack).unwrap(),
                MemoryRegion::new(descriptor(OUTPUT, 2), &mut self.output).unwrap(),
            ];
            let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
            let stack = GuestStack::new(GuestAddress::new(STACK), ByteCount(0x1000)).unwrap();
            body(&mut RecordContext::with_stack(space, Recording::default(), stack))
        }

        fn set_word(&mut self, at: u32, value: u32) { self.shared[at as usize..at as usize + 4].copy_from_slice(&value.to_le_bytes()); }
        fn set_global(&mut self, at: u32, value: u32) { self.globals[at as usize..at as usize + 4].copy_from_slice(&value.to_le_bytes()); }
        fn output(&self) -> u16 { u16::from_le_bytes(self.output) }
    }

    #[test]
    fn the_state_word_publishes_the_slot_word_and_the_two_selection_bits() {
        let mut memory = Memory::new();
        memory.resource[0x160..0x164].copy_from_slice(&0x0000_7788u32.to_le_bytes());
        memory.set_global(0xf4, 1);
        memory.set_global(0xf8, 1);
        memory.set_global(0x2c, 1);
        memory.set_global(0x1c, 0x80);
        memory.with_context(|context| {
            assert_eq!(helper_f23944(context), Ok(()));
            assert_eq!(context.effects.count, 1);
            assert_eq!(context.effects.published[0], Some((0, 0x7788, 0x80, 0, 3)));
        });
    }

    #[test]
    fn the_state_word_publishes_no_bits_and_the_default_second_word() {
        let mut memory = Memory::new();
        memory.with_context(|context| {
            assert_eq!(helper_f23944(context), Ok(()));
            assert_eq!(context.effects.published[0], Some((0, 0, 128, 0, 0)));
        });
    }

    #[test]
    fn an_empty_first_snapshot_returns_zero() {
        let mut memory = Memory::new();
        memory.enabled[0] = 1;
        memory.resource[0x30] = 0;
        memory.resource[0x47] = 4;
        memory.set_word(0x840, 0);
        memory.with_context(|context| assert_eq!(helper_f240b0(context, HANDLE, 5, 0, 0, GuestAddress::new(OUTPUT)), Ok(0)));
        assert_eq!(memory.output(), 5);
    }

    #[test]
    fn a_foreign_first_snapshot_word_continues() {
        let mut memory = Memory::new();
        memory.enabled[0] = 1;
        memory.resource[0x30] = 0;
        memory.resource[0x47] = 4;
        memory.set_word(0x840, 1);
        memory.set_word(0xb0 + 32, 0);
        memory.with_context(|context| assert_eq!(helper_f240b0(context, HANDLE, 5, 0, 0, GuestAddress::new(OUTPUT)), Ok(1)));
        assert_eq!(memory.output(), 5);
    }

    #[test]
    fn the_gate_word_publishes_the_converted_position() {
        let mut memory = Memory::new();
        memory.enabled[0] = 1;
        memory.resource[0x30] = 0;
        memory.resource[0x47] = 4;
        memory.set_word(0x840, 1);
        memory.set_word(0xb0 + 32, 1);
        memory.resource[0x64] = 1;
        memory.globals[0x104..0x108].copy_from_slice(&1u32.to_le_bytes());
        memory.calibration.copy_from_slice(&0u16.to_le_bytes());
        let output = memory.with_context(|context| {
            let expected = crate::record_task43_resource_forward::helper_f47300(context, 5, 0, 0, 0, 0, 1, 1);
            assert_eq!(helper_f240b0(context, HANDLE, 5, 0, 0, GuestAddress::new(OUTPUT)), Ok(1));
            (expected, context.read_u16(GuestAddress::new(OUTPUT)))
        });
        assert_eq!(output.0, output.1.map(u32::from));
    }
}
