//! The exposure tenths string and its two overlay cells: image-5 body
//! 0x00f5_21d8.

use crate::osal::DelayTimer;
use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext, StackAlignment};
use crate::record_log::{helper_722218, DecimalStyle, DecimalWidth};
use crate::shutdown::ShutdownRtos;

/// The image-5 strings this body copies, at 0x00f533d8-0x00f533fc, 0x00f80130,
/// 0x00e49b80 and 0x00eeb2d0.
const INITIAL: &[u8] = b"0.0\0";
const ACTIVE: &[u8] = b"ACT   \0";
const STILL: &[u8] = b"STIL  \0";
const FAILED: &[u8] = b"FAIL  \0";
const BLANK: &[u8] = b"----- \0";
const SPACE: &[u8] = b" \0";
const ZERO: &[u8] = b"0\0";
const DOT: &[u8] = b".\0";

/// The columns and rows the two cells publish to.
pub const VALUE_COLUMN: u32 = 2;
pub const VALUE_ROW: u32 = 5;
pub const MODE_COLUMN: u32 = 2;
pub const MODE_ROW: u32 = 6;
pub const REPLACED_COLUMN: u32 = 8;
pub const REPLACED_ROW: u32 = 5;
pub const REPLACED_MODE_ROW: u32 = 6;

fn put_text<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress, text: &[u8]) -> Result<(), MemoryError> {
    for (offset, byte) in text.iter().enumerate() {
        context.write_u8(destination.checked_add(offset as u32)?, *byte)?;
    }
    Ok(())
}

fn append_text<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress, text: &[u8]) -> Result<(), MemoryError> {
    let length = crate::record_power_mode_6a570c::helper_721f30(context, destination)?;
    put_text(context, destination.checked_add(length)?, text)
}

/// The image-5 body at 0x00f5_21d8: writes the tenths of `value` and the mode
/// label into the exposure overlay.
pub fn helper_f521d8<M: GuestAddressSpace, E: ShutdownRtos + DelayTimer>(
    context: &mut RecordContext<M, E>, value: u32, mode: u32, enabled: u32,
) -> Result<(), MemoryError> {
    context.with_guest_frame(ByteCount(16), StackAlignment::Word, |context, frame| {
        let value_text = frame.base();
        let mode_text = frame.base().checked_add(8)?;
        put_text(context, value_text, INITIAL)?;
        put_text(context, mode_text, INITIAL)?;
        let whole = value / 10;
        let fraction = value - whole * 10;
        if whole != 0 {
            helper_722218(context, value_text, whole, DecimalWidth(10), DecimalStyle::UnpaddedTerminated)?;
        } else {
            put_text(context, value_text, ZERO)?;
        }
        append_text(context, value_text, DOT)?;
        if fraction != 0 {
            let length = crate::record_power_mode_6a570c::helper_721f30(context, value_text)?;
            helper_722218(context, value_text.checked_add(length)?, fraction, DecimalWidth(10), DecimalStyle::UnpaddedTerminated)?;
        } else {
            append_text(context, value_text, ZERO)?;
        }
        let length = crate::record_power_mode_6a570c::helper_721f30(context, value_text)?;
        if length < 6 {
            for _ in length..6 { append_text(context, value_text, SPACE)?; }
        }
        let (label, mode_padding, value_padding) = match mode {
            0 => (STILL, 7, 7),
            1 => (ACTIVE, 10, 10),
            2 => (ACTIVE, 4, 10),
            3 => (FAILED, 4, 4),
            _ => (BLANK, 4, 4),
        };
        put_text(context, mode_text, label)?;
        if enabled != 0 {
            return crate::record_display_text_1110918::helper_1110918(context, value_text, REPLACED_COLUMN, REPLACED_ROW, 1, mode_padding)
                .and_then(|()| crate::record_display_text_1110918::helper_1110918(context, mode_text, REPLACED_COLUMN, REPLACED_MODE_ROW, 1, value_padding));
        }
        crate::record_display_text_1110918::helper_1110918(context, value_text, VALUE_COLUMN, VALUE_ROW, 1, mode_padding)?;
        crate::record_display_text_1110918::helper_1110918(context, mode_text, MODE_COLUMN, MODE_ROW, 1, value_padding)?;
        put_text(context, value_text, BLANK)?;
        put_text(context, mode_text, BLANK)?;
        crate::record_display_text_1110918::helper_1110918(context, value_text, REPLACED_COLUMN, REPLACED_ROW, 1, 7)?;
        crate::record_display_text_1110918::helper_1110918(context, mode_text, REPLACED_COLUMN, REPLACED_MODE_ROW, 1, 7)
    })
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 body at 0x00f5_21d8.
    use super::*;
    use crate::osal::TimerReload;
    use crate::record_address_space::{GuestDomain, GuestStack, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};
    use crate::shutdown::{KernelStatus, ServiceMessage, TaskId};

    const STACK: u32 = 0x0010_0000;
    const TABLE: u32 = 0x4171_716c;
    const GLOBALS: u32 = 0x014e_32b8;
    const ZERO_TEXT: u32 = 0x0010_2000;

    #[derive(Default)]
    struct Kernel {
        sends: u32,
    }

    impl ShutdownRtos for Kernel {
        fn current_task(&mut self) -> TaskId { TaskId(3) }
        fn post(&mut self, _message: ServiceMessage<'_>) -> Result<(), KernelStatus> { self.sends += 1; Ok(()) }
        fn set_event_flag(&mut self, _flag: u32, _bits: u32) -> Result<(), KernelStatus> { Ok(()) }
    }

    impl DelayTimer for Kernel {
        fn reload(&mut self) -> u32 { 1 }
        fn current(&mut self) -> u32 { 0 }
    }

    struct Memory {
        table: [u8; 0x800],
        globals: [u8; 0x40],
        zero: [u8; 8],
        stack: [u8; 0x100],
    }

    fn with_context<R>(memory: &mut Memory, kernel: &mut Kernel, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, Kernel>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(TABLE, 0x800), &mut memory.table).unwrap(),
            MemoryRegion::new(descriptor(GLOBALS, 0x40), &mut memory.globals).unwrap(),
            MemoryRegion::new(descriptor(ZERO_TEXT, 8), &mut memory.zero).unwrap(),
            MemoryRegion::new(descriptor(STACK, 0x100), &mut memory.stack).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let stack = GuestStack::new(GuestAddress::new(STACK), ByteCount(0x100)).unwrap();
        let mut context = RecordContext::with_stack(space, core::mem::take(kernel), stack);
        let result = body(&mut context);
        *kernel = core::mem::take(&mut context.effects);
        result
    }

    fn empty() -> Memory {
        Memory { table: [0; 0x800], globals: [0; 0x40], zero: [0; 8], stack: [0; 0x100] }
    }

    fn cell(memory: &Memory, index: usize) -> &[u8] {
        let at = 8 + index * 70 + 4;
        &memory.table[at..at + 14]
    }

    #[test]
    fn the_value_cell_holds_the_tenths_and_the_mode_cell_the_label() {
        let mut memory = empty();
        let mut kernel = Kernel::default();
        with_context(&mut memory, &mut kernel, |context| assert_eq!(helper_f521d8(context, 37, 2, 1), Ok(())));
        assert_eq!(memory.table[0], 2);
        assert_eq!(&memory.table[8..12], &[8, 9, 1, 4]);
        assert_eq!(cell(&memory, 0), &[0x33, 0xe1, 0x2e, 0xe1, 0x37, 0xe1, 0x20, 0xe1, 0x20, 0xe1, 0x20, 0xe1, 0, 0]);
        assert_eq!(&memory.table[78..82], &[8, 11, 1, 10]);
        assert_eq!(cell(&memory, 1), &[0x41, 0xe1, 0x43, 0xe1, 0x54, 0xe1, 0x20, 0xe1, 0x20, 0xe1, 0x20, 0xe1, 0, 0]);
    }

    #[test]
    fn a_zero_value_reads_zero_point_zero() {
        let mut memory = empty();
        let mut kernel = Kernel::default();
        with_context(&mut memory, &mut kernel, |context| assert_eq!(helper_f521d8(context, 0, 0, 1), Ok(())));
        assert_eq!(cell(&memory, 0), &[0x30, 0xe1, 0x2e, 0xe1, 0x30, 0xe1, 0x20, 0xe1, 0x20, 0xe1, 0x20, 0xe1, 0, 0]);
        assert_eq!(cell(&memory, 1), &[0x53, 0xe1, 0x54, 0xe1, 0x49, 0xe1, 0x4c, 0xe1, 0x20, 0xe1, 0x20, 0xe1, 0, 0]);
    }

    #[test]
    fn the_short_value_is_padded_to_six_cells() {
        let mut memory = empty();
        let mut kernel = Kernel::default();
        with_context(&mut memory, &mut kernel, |context| assert_eq!(helper_f521d8(context, 5, 3, 1), Ok(())));
        assert_eq!(cell(&memory, 0), &[0x30, 0xe1, 0x2e, 0xe1, 0x35, 0xe1, 0x20, 0xe1, 0x20, 0xe1, 0x20, 0xe1, 0, 0]);
        assert_eq!(cell(&memory, 1), &[0x46, 0xe1, 0x41, 0xe1, 0x49, 0xe1, 0x4c, 0xe1, 0x20, 0xe1, 0x20, 0xe1, 0, 0]);
    }

    #[test]
    fn the_disabled_state_blank_cell_keeps_the_seven_padding() {
        let mut memory = empty();
        let mut kernel = Kernel::default();
        with_context(&mut memory, &mut kernel, |context| assert_eq!(helper_f521d8(context, 123, 4, 0), Ok(())));
        assert_eq!(memory.table[0], 4);
        assert_eq!(&memory.table[8..12], &[2, 9, 1, 4]);
        assert_eq!(cell(&memory, 0), &[0x31, 0xe1, 0x32, 0xe1, 0x2e, 0xe1, 0x33, 0xe1, 0x20, 0xe1, 0x20, 0xe1, 0, 0]);
        assert_eq!(cell(&memory, 1), &[0x2d, 0xe1, 0x2d, 0xe1, 0x2d, 0xe1, 0x2d, 0xe1, 0x2d, 0xe1, 0x20, 0xe1, 0, 0]);
        assert_eq!(&memory.table[148..152], &[8, 9, 1, 7]);
        assert_eq!(&memory.table[218..222], &[8, 11, 1, 7]);
        assert_eq!(kernel.sends, 4);
    }
}
