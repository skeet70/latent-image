//! The overlay text table at 0x4171_716c: image-6 bodies 0x0111_0918 and
//! 0x0117_8c0c.

use crate::osal::DelayTimer;
use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext, StackAlignment};
use crate::shutdown::ShutdownRtos;

pub const OVERLAY_TABLE: u32 = 0x4171_716c;
pub const ENTRY_BYTES: u32 = 70;
pub const ENTRY_LIMIT: u32 = 20;
pub const ENTRY_COLUMN: u32 = 0;
pub const ENTRY_ROW: u32 = 1;
pub const ENTRY_FLAGS: u32 = 2;
pub const ENTRY_PADDING: u32 = 3;
pub const ENTRY_TEXT: u32 = 4;
pub const DIRTY_WORD: u32 = 4;
/// The halfword every text cell carries above its character.
pub const CELL_TAG: u16 = 0xe100;
/// Characters a cell holds, and the byte count the copy terminates.
pub const TEXT_CHARS: u32 = 24;
pub const TEXT_BYTES: u32 = 25;

fn entry(index: u32) -> GuestAddress {
    GuestAddress::new(OVERLAY_TABLE.wrapping_add(8).wrapping_add(index.wrapping_mul(ENTRY_BYTES)))
}

/// The image-6 body at 0x0117_8c0c.
pub fn helper_1178c0c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, source: GuestAddress, destination: GuestAddress) -> Result<u32, MemoryError> {
    let first = context.read_u8(source)?;
    let mut cursor = destination;
    if first == 0 {
        context.write_u16(cursor, 0)?;
        return Ok(0);
    }
    context.write_u16(cursor, CELL_TAG.wrapping_add(u16::from(first)))?;
    cursor = cursor.checked_add(2)?;
    let mut index = 1;
    loop {
        if index == 32 { break; }
        let value = context.read_u8(source.checked_add(index)?)?;
        if value == 0 { break; }
        context.write_u16(cursor, CELL_TAG.wrapping_add(u16::from(value)))?;
        cursor = cursor.checked_add(2)?;
        index += 1;
    }
    context.write_u16(cursor, 0)?;
    Ok(index)
}

/// The image-6 body at 0x0111_0918: publishes at most 24 characters to the
/// overlay cell of `column` and `row`, appending an entry when the table has
/// room and marking it dirty when the text was written.
pub fn helper_1110918<M: GuestAddressSpace, E: ShutdownRtos + DelayTimer>(
    context: &mut RecordContext<M, E>, text: GuestAddress, column: u32, row: u32, flags: u32, padding: u32,
) -> Result<(), MemoryError> {
    if (column.wrapping_sub(1) & 0xff) > 31 { return Ok(()); }
    if (row.wrapping_sub(1) & 0xff) > 9 { return Ok(()); }
    let row_key = (row.wrapping_mul(2).wrapping_sub(1) & 0xff) as u8;
    let count = context.read_u8(GuestAddress::new(OVERLAY_TABLE))?;
    let mut slot = None;
    for index in 0..u32::from(count) {
        let candidate = entry(index);
        if context.read_u8(candidate)? == column as u8 && context.read_u8(candidate.checked_add(ENTRY_ROW)?)? == row_key {
            slot = Some(candidate);
            break;
        }
    }
    let target = match slot {
        Some(target) => target,
        None => {
            if u32::from(count) >= ENTRY_LIMIT { return Ok(()); }
            context.write_u8(GuestAddress::new(OVERLAY_TABLE), count + 1)?;
            entry(u32::from(count))
        }
    };
    context.write_u8(target.checked_add(ENTRY_COLUMN)?, column as u8)?;
    context.write_u8(target.checked_add(ENTRY_ROW)?, row_key)?;
    context.write_u8(target.checked_add(ENTRY_FLAGS)?, flags as u8)?;
    context.write_u8(target.checked_add(ENTRY_PADDING)?, padding as u8)?;
    context.with_guest_frame(ByteCount(TEXT_BYTES), StackAlignment::Word, |context, frame| {
        let mut index = 0;
        while index < TEXT_CHARS {
            let value = context.read_u8(text.checked_add(index)?)?;
            if value == 0 { break; }
            context.write_u8(frame.base().checked_add(index)?, value)?;
            index += 1;
        }
        context.write_u8(frame.base().checked_add(index)?, 0)?;
        if helper_1178c0c(context, frame.base(), target.checked_add(ENTRY_TEXT)?)? == 0 { return Ok(()); }
        context.write_u32(GuestAddress::new(OVERLAY_TABLE + DIRTY_WORD), 1)?;
        crate::record_date_token_db6464::helper_db6688(context, 2)
    })
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-6 bodies at 0x0111_0918 and 0x0117_8c0c.
    use super::*;
    use crate::osal::TimerReload;
    use crate::record_address_space::{GuestDomain, GuestStack, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};
    use crate::shutdown::{KernelStatus, ServiceMessage, TaskId};

    const STACK: u32 = 0x0010_0000;
    const TEXT: u32 = 0x0010_1000;
    const GLOBALS: u32 = 0x014e_32b8;

    #[derive(Default)]
    struct Kernel {
        sends: u32,
    }

    impl ShutdownRtos for Kernel {
        fn current_task(&mut self) -> TaskId { TaskId(9) }
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
        text: [u8; 0x40],
        stack: [u8; 0x100],
    }

    fn with_context<R>(memory: &mut Memory, kernel: &mut Kernel, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, Kernel>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(OVERLAY_TABLE, 0x800), &mut memory.table).unwrap(),
            MemoryRegion::new(descriptor(GLOBALS, 0x40), &mut memory.globals).unwrap(),
            MemoryRegion::new(descriptor(TEXT, 0x40), &mut memory.text).unwrap(),
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
        Memory { table: [0; 0x800], globals: [0; 0x40], text: [0; 0x40], stack: [0; 0x100] }
    }

    #[test]
    fn the_cell_copy_tags_each_character_and_stops_at_the_terminator() {
        let mut memory = empty();
        memory.text[..4].copy_from_slice(b"Ab\0d");
        with_context(&mut memory, &mut Kernel::default(), |context| {
            assert_eq!(helper_1178c0c(context, GuestAddress::new(TEXT), GuestAddress::new(OVERLAY_TABLE)), Ok(2));
        });
        assert_eq!(&memory.table[..6], &[0x41, 0xe1, 0x62, 0xe1, 0, 0]);
        assert_eq!(memory.table[6], 0);
    }

    #[test]
    fn the_cell_copy_returns_zero_and_terminates_an_empty_string() {
        let mut memory = empty();
        with_context(&mut memory, &mut Kernel::default(), |context| {
            assert_eq!(helper_1178c0c(context, GuestAddress::new(TEXT), GuestAddress::new(OVERLAY_TABLE)), Ok(0));
        });
        assert_eq!(&memory.table[..2], &[0, 0]);
    }

    #[test]
    fn the_cell_copy_stops_after_thirty_two_characters() {
        let mut memory = empty();
        memory.text = [b'z'; 0x40];
        with_context(&mut memory, &mut Kernel::default(), |context| {
            assert_eq!(helper_1178c0c(context, GuestAddress::new(TEXT), GuestAddress::new(OVERLAY_TABLE)), Ok(32));
        });
        assert_eq!(memory.table[64], 0);
        assert_eq!(&memory.table[62..64], &[0x7a, 0xe1]);
    }

    #[test]
    fn a_new_cell_takes_the_next_entry_and_marks_the_table_dirty() {
        let mut memory = empty();
        let mut kernel = Kernel::default();
        memory.text[..4].copy_from_slice(b"3.7\0");
        with_context(&mut memory, &mut kernel, |context| {
            assert_eq!(helper_1110918(context, GuestAddress::new(TEXT), 2, 5, 1, 4), Ok(()));
        });
        assert_eq!(memory.table[0], 1);
        assert_eq!(memory.table[8], 2);
        assert_eq!(memory.table[9], 9);
        assert_eq!(memory.table[10], 1);
        assert_eq!(memory.table[11], 4);
        assert_eq!(&memory.table[12..18], &[0x33, 0xe1, 0x2e, 0xe1, 0x37, 0xe1]);
        assert_eq!(memory.table[18..20], [0, 0]);
        assert_eq!(u32::from_le_bytes(memory.table[4..8].try_into().unwrap()), 1);
        assert_eq!(kernel.sends, 1);
    }

    #[test]
    fn an_existing_cell_is_overwritten_in_place() {
        let mut memory = empty();
        let mut kernel = Kernel::default();
        memory.table[0] = 3;
        memory.table[8] = 2;
        memory.table[9] = 9;
        memory.text[..2].copy_from_slice(b"x\0");
        with_context(&mut memory, &mut kernel, |context| {
            assert_eq!(helper_1110918(context, GuestAddress::new(TEXT), 2, 5, 7, 8), Ok(()));
        });
        assert_eq!(memory.table[0], 3);
        assert_eq!(memory.table[10], 7);
        assert_eq!(memory.table[11], 8);
        assert_eq!(&memory.table[12..16], &[0x78, 0xe1, 0, 0]);
        assert_eq!(kernel.sends, 1);
    }

    #[test]
    fn the_column_and_row_bounds_and_a_full_table_stop_the_write() {
        let mut memory = empty();
        let mut kernel = Kernel::default();
        memory.text[..2].copy_from_slice(b"q\0");
        with_context(&mut memory, &mut kernel, |context| {
            assert_eq!(helper_1110918(context, GuestAddress::new(TEXT), 33, 5, 0, 0), Ok(()));
            assert_eq!(helper_1110918(context, GuestAddress::new(TEXT), 0, 5, 0, 0), Ok(()));
            assert_eq!(helper_1110918(context, GuestAddress::new(TEXT), 2, 11, 0, 0), Ok(()));
        });
        assert_eq!(memory.table[0], 0);
        assert_eq!(kernel.sends, 0);
        memory.table[0] = 20;
        with_context(&mut memory, &mut kernel, |context| {
            assert_eq!(helper_1110918(context, GuestAddress::new(TEXT), 2, 5, 0, 0), Ok(()));
        });
        assert_eq!(memory.table[0], 20);
        assert_eq!(kernel.sends, 0);
    }

    #[test]
    fn an_empty_string_still_writes_the_cell_terminator_but_not_the_dirty_word() {
        let mut memory = empty();
        let mut kernel = Kernel::default();
        with_context(&mut memory, &mut kernel, |context| {
            assert_eq!(helper_1110918(context, GuestAddress::new(TEXT), 1, 1, 0, 0), Ok(()));
        });
        assert_eq!(memory.table[0], 1);
        assert_eq!(&memory.table[12..14], &[0, 0]);
        assert_eq!(u32::from_le_bytes(memory.table[4..8].try_into().unwrap()), 0);
        assert_eq!(kernel.sends, 0);
    }
}
