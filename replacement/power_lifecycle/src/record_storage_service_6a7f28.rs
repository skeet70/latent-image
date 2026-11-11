//! Stock-shaped storage service helpers over the image-4 bodies at
//! 0x006a7f28-0x00716324 and the image-5 numbering body at 0x0070fb68.
//!
//! Guest addresses are the pointer slots and state blocks the instructions
//! name: the storage context slot, the drive route table, the file table and
//! the profile counter words.

use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::shutdown::{KernelStatus, ShutdownRtos};

pub const STORAGE_CONTEXT_SLOT: u32 = 0x0091_0204;
pub const ROUTE_TABLE: u32 = 0x0081_4730;
pub const NUMBERING_STATE: u32 = 0x008f_c8a4;
pub const FILE_TABLE_COUNT: u32 = 0x0094_4468;
pub const FILE_TABLE: u32 = 0x008f_c880;
pub const PROFILE_COUNTERS: u32 = 0x409c_281c;
pub const PATH_TEMPLATE: u32 = 0x0082_99fc;
const FILE_STRIDE: u32 = 0x0e00;

pub trait StorageKernel: ShutdownRtos {
    fn wait_semaphore(&mut self, id: u32, timeout: u32) -> i32;
    fn signal_semaphore(&mut self, id: u32) -> i32;
    fn kernel_command(&mut self, category: u32, code: u32);
    fn delay_microseconds(&mut self, duration: u32);
    fn invoke_callback(&mut self, word: u32, first: u32, second: Option<u32>);
}

fn address(base: u32, offset: u32) -> GuestAddress { GuestAddress::new(base.wrapping_add(offset)) }

pub fn storage_context<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    context.read_u32(GuestAddress::new(STORAGE_CONTEXT_SLOT))
}

fn route_byte<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, offset: u32) -> Result<u8, MemoryError> {
    context.read_u8(address(ROUTE_TABLE, offset))
}

pub fn helper_6a7f28<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    let value = context.read_u16(GuestAddress::new(PROFILE_COUNTERS))?;
    Ok(if value > 9999 { 0 } else { u32::from(value) })
}

pub fn helper_6a7fd8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    let value = context.read_u16(address(PROFILE_COUNTERS, 2))?;
    Ok(if value.wrapping_sub(100) >= 900 { 100 } else { u32::from(value) })
}

pub fn helper_6a801c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    helper_6a7f28(context)
}

pub fn helper_6a8040<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    helper_6a7fd8(context)
}

pub fn helper_703b1c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<u32, MemoryError> {
    if handle == 0 { return Ok(0x10b); }
    let index = handle - 1;
    let count = context.read_u32(GuestAddress::new(FILE_TABLE_COUNT))?;
    if index > count { return Ok(0x10b); }
    let table = context.read_u32(GuestAddress::new(FILE_TABLE))?;
    let record = GuestAddress::new(table.wrapping_add(index.wrapping_mul(FILE_STRIDE)));
    if context.read_u32(record)? != u32::MAX {
        context.write_u32(record.checked_add(0x80)?, 1)?;
        Ok(0)
    } else {
        Ok(0x10b)
    }
}

pub fn helper_6ff11c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<u32, MemoryError> {
    Ok(if helper_703b1c(context, handle)? == 0 { 0 } else { 13 })
}

pub fn helper_70e6c4<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>, destination: GuestAddress, source: GuestAddress, bytes: u32,
) -> Result<(), MemoryError> {
    context.copy_bytes(destination, source, ByteCount(bytes))
}

pub fn helper_70ea78<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>, destination: GuestAddress, value: u32, count: u32,
) -> Result<(), MemoryError> {
    for index in 0..count {
        context.write_u8(destination.checked_add(index)?, value as u8)?;
    }
    Ok(())
}

pub fn helper_70eaa0<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>, output: GuestAddress, value: u32, width: u32, mode: u32,
) -> Result<u32, MemoryError> {
    let mut digits = [0u8; 16];
    let mut length = 0usize;
    let mut remaining = value;
    let mut divisor = 1_000_000_000u32;
    for _ in 0..10 {
        let digit = remaining / divisor;
        if length != 0 || digit != 0 {
            digits[length] = b'0' + digit as u8;
            length += 1;
        }
        remaining %= divisor;
        divisor /= 10;
    }
    if width < length as u32 { return Ok(u32::MAX); }
    let offset = if mode <= 1 {
        for index in 0..width { context.write_u8(output.checked_add(index)?, b'0')?; }
        width - length as u32
    } else if mode.wrapping_sub(2) <= 1 {
        for index in 0..width { context.write_u8(output.checked_add(index)?, b' ')?; }
        width - length as u32
    } else {
        0
    };
    for (index, digit) in digits[..length].iter().copied().enumerate() {
        context.write_u8(output.checked_add(offset + index as u32)?, digit)?;
    }
    if mode & 1 == 0 {
        context.write_u8(output.checked_add(offset + length as u32)?, 0)?;
    }
    Ok(length as u32)
}

pub fn helper_70ec08<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, device: u32) -> Result<u32, MemoryError> {
    let index = if route_byte(context, 1)? == 1 && u32::from(route_byte(context, 2)?) == device {
        0
    } else if route_byte(context, 9)? == 1 {
        if u32::from(route_byte(context, 10)?) == device { 1 } else { 2 }
    } else {
        2
    };
    Ok(u32::from(route_byte(context, index * 8)?))
}

pub fn helper_70ec84<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, drive: u32) -> Result<u32, MemoryError> {
    let index = if route_byte(context, 1)? == 1 && u32::from(route_byte(context, 0)?) == drive {
        0
    } else {
        if route_byte(context, 9)? != 1 { return Ok(u32::MAX); }
        if u32::from(route_byte(context, 8)?) != drive { return Ok(u32::MAX); }
        1
    };
    Ok(u32::from(route_byte(context, index * 8 + 2)?))
}

pub fn helper_70f840<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, text: GuestAddress, size: u32) -> Result<u32, MemoryError> {
    if size == 0 { return Ok(0); }
    for index in 0..size {
        let character = context.read_u8(text.checked_add(index)?)?;
        let valid = character.wrapping_sub(b'A') <= 25
            || character.wrapping_sub(b'a') <= 25
            || character.wrapping_sub(b'0') <= 9
            || character == b'_';
        if !valid { return Ok(8); }
    }
    Ok(0)
}

pub fn helper_7111c4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress) -> Result<(), MemoryError> {
    context.copy_bytes(destination, GuestAddress::new(NUMBERING_STATE), ByteCount(16))
}

pub fn helper_7111e0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, option: u32) -> Result<(), MemoryError> {
    context.write_u32(address(NUMBERING_STATE, 0xc), option)
}

fn route_index_for_device<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, device: u32) -> Result<u32, MemoryError> {
    Ok(if route_byte(context, 1)? == 1 && u32::from(route_byte(context, 2)?) == device {
        0
    } else if route_byte(context, 9)? == 1 {
        if u32::from(route_byte(context, 10)?) == device { 1 } else { 2 }
    } else {
        2
    })
}

pub fn helper_712d08<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress) -> Result<(), MemoryError> {
    let base = storage_context(context)?;
    let device = u32::from(context.read_u8(address(base, 0x114))?);
    let index = route_index_for_device(context, device)?;
    let drive = route_byte(context, index * 8)?;
    context.write_u8(destination, drive)?;
    context.write_u8(destination.checked_add(1)?, b':')?;
    context.write_u8(destination.checked_add(2)?, 0)
}

pub fn helper_715f0c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, mask: u32) -> Result<u32, MemoryError> {
    let base = storage_context(context)?;
    let count = context.read_u8(address(base, 0x2600))?;
    if count == 0 { return Ok(0xff); }
    if mask & 1 != 0 { return Ok(0); }
    let mut index = 1u32;
    loop {
        let current = index as u8;
        if current >= count { return Ok(0xff); }
        if mask & 1u32.checked_shl(u32::from(current)).unwrap_or(0) != 0 {
            return Ok(u32::from(current));
        }
        index += 1;
    }
}

pub fn helper_7159f8<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>, _first: u32, _second: u32, interval: GuestAddress, counter: GuestAddress,
) -> Result<(), MemoryError> {
    let limit = helper_6a8040(context)?;
    context.write_u16(interval, limit as u16)?;
    let value = helper_6a801c(context)?;
    context.write_u16(counter, value as u16)
}

/// category-2 diagnostic.
pub fn helper_71ddf0<M: GuestAddressSpace, E: StorageKernel>(context: &mut RecordContext<M, E>, mode: u32) -> Result<u32, MemoryError> {
    let base = storage_context(context)?;
    if base == 0 { return Ok(5); }
    if context.read_u32(address(base, 0x114))? == u32::MAX { return Ok(5); }
    let flags = context.read_u8(address(base, 0x16bd8))?;
    if flags & 1 != 0 {
        context.effects.kernel_command(2, 0x50);
        return Ok(1);
    }
    if flags & 2 != 0 {
        if mode > 1 { return Ok(0); }
        context.effects.kernel_command(2, 0x55);
        return Ok(5);
    }
    if flags & 4 != 0 {
        context.effects.kernel_command(2, 0x53);
        return Ok(5);
    }
    if flags & 8 == 0 { return Ok(0); }
    if mode == 0 {
        context.effects.kernel_command(2, 0x52);
        return Ok(2);
    }
    Ok(0)
}

pub fn helper_71f708<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    helper_71fb18(context)
}

/// device.
pub fn helper_71fb18<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    let base = storage_context(context)?;
    if base == 0 { return Ok(0); }
    let device = context.read_u32(address(base, 0x114))?;
    if device > 1 { return Ok(0); }
    helper_70ec08(context, device)
}

fn magic_tens(value: u32) -> u32 { value.wrapping_sub((value / 10).wrapping_mul(10)) }

/// words select.
pub fn helper_716324<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>, _device: u32, group: u32, directory: GuestAddress, file: GuestAddress,
) -> Result<u32, MemoryError> {
    let base = storage_context(context)?;
    let record = address(base, group.wrapping_add(0x21d).wrapping_mul(0x10));
    let mut dir = u32::from(context.read_u16(record.checked_add(4)?)?);
    let mut number = u32::from(context.read_u16(record.checked_add(6)?)?);
    if dir <= 99 {
        let largest = context.read_u32(address(base, 0x2168))?;
        if largest <= 100 && context.read_u32(address(base, 0x2794))? == 0 {
            dir = 100;
            number = 0;
        } else {
            dir = largest & 0xffff;
            number = magic_tens(dir).wrapping_mul(1000).wrapping_add(999) & 0xffff;
        }
    }
    let mut saved_dir;
    let mut saved_number;
    if context.read_u32(address(base, 0x16c6c))? == 0 {
        saved_dir = 100u32;
        saved_number = 0u32;
    } else {
        let (interval, counter) = context.with_guest_frame(
            ByteCount(4), crate::record_address_space::StackAlignment::Word,
            |context, frame| -> Result<(u32, u32), MemoryError> {
                let interval = frame.base();
                let counter = frame.at(ByteCount(2), ByteCount(2))?;
                helper_7159f8(context, _device, group, interval, counter)?;
                Ok((u32::from(context.read_u16(interval)?), u32::from(context.read_u16(counter)?)))
            },
        )?;
        if dir < interval || (dir == interval && number < counter) {
            let selected = context.read_u8(address(base, interval.wrapping_add(0x2218)))?;
            if u32::from(selected) != group && selected != 0xff {
                saved_dir = u32::from(context.read_u16(address(base, 0x2168))?);
                saved_number = magic_tens(saved_dir).wrapping_mul(1000).wrapping_add(999) & 0xffff;
            } else {
                saved_dir = interval;
                saved_number = counter;
            }
        } else {
            saved_dir = interval;
            saved_number = counter;
        }
    }
    if dir < saved_dir || (dir == saved_dir && number < saved_number) {
        dir = saved_dir;
        number = saved_number;
    }
    if group == 0 {
        let original = dir;
        let largest = context.read_u32(address(base, 0x2168))?;
        if dir < largest {
            dir = largest & 0xffff;
            number = magic_tens(dir).wrapping_mul(1000).wrapping_add(999) & 0xffff;
        } else if dir == largest
            && dir != context.read_u32(address(base, 0x21b8))?
            && (dir > 100 || context.read_u32(address(base, 0x2794))? != 0)
        {
            dir = original;
            number = magic_tens(dir).wrapping_mul(1000).wrapping_add(999) & 0xffff;
        }
    }
    context.write_u16(directory, dir as u16)?;
    context.write_u16(file, number as u16)?;
    Ok(0)
}

pub fn helper_70fb68<M: GuestAddressSpace, E: StorageKernel>(
    context: &mut RecordContext<M, E>, directory: GuestAddress, file: GuestAddress,
) -> Result<u32, MemoryError> {
    let base = storage_context(context)?;
    let identifier = context.read_u32(address(base, 0x2210))?;
    let status = helper_71ddf0(context, 0)?;
    if status != 0 { return Ok(status); }
    let device = helper_71f708(context)?;
    if device == 0 { return Ok(8); }
    let group = helper_715f0c(context, identifier)?;
    if group == 0xff { return Ok(8); }
    let status = helper_716324(context, device, group, directory, file)?;
    Ok(status)
}

pub fn helper_711200<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, path: GuestAddress) -> Result<u32, MemoryError> {
    let template = context.read_u32(GuestAddress::new(PATH_TEMPLATE))?;
    let difference = crate::record_interface_text_72232c::helper_72232c(context, path, GuestAddress::new(template), 21)?;
    Ok(if difference == 0 { 1 } else { 0 })
}

#[cfg(target_arch = "arm")]
mod native {
    use super::*;
    use crate::shutdown::{KernelStatus, ServiceMessage};

    extern "C" {
        fn xm1_power_semaphore_wait(id: i32, timeout: i32) -> i32;
        fn xm1_power_semaphore_signal(id: i32) -> i32;
        fn xm1_power_command_diagnostic(category: u32, code: u32) -> i32;
        fn xm1_power_delay_us(duration: u32);
    }

    #[derive(Default)]
    pub struct NativeStorageKernel;

    impl ShutdownRtos for NativeStorageKernel {
        fn current_task(&mut self) -> crate::shutdown::TaskId { crate::shutdown_rtos::NativeShutdownRtos.current_task() }
        fn post(&mut self, message: ServiceMessage<'_>) -> Result<(), KernelStatus> { crate::shutdown_rtos::NativeShutdownRtos.post(message) }
        fn set_event_flag(&mut self, flag: u32, bits: u32) -> Result<(), KernelStatus> { crate::shutdown_rtos::NativeShutdownRtos.set_event_flag(flag, bits) }
    }

    impl StorageKernel for NativeStorageKernel {
        fn wait_semaphore(&mut self, id: u32, timeout: u32) -> i32 { unsafe { xm1_power_semaphore_wait(id as i32, timeout as i32) } }
        fn signal_semaphore(&mut self, id: u32) -> i32 { unsafe { xm1_power_semaphore_signal(id as i32) } }
        fn kernel_command(&mut self, category: u32, code: u32) { let _ = unsafe { xm1_power_command_diagnostic(category, code) }; }
        fn delay_microseconds(&mut self, duration: u32) { unsafe { xm1_power_delay_us(duration) }; }
        fn invoke_callback(&mut self, word: u32, first: u32, second: Option<u32>) {
            use crate::callback_registry::{registry, CallbackWord};
            if word == 0 { return; }
            match second {
                Some(second) => {
                    if let Ok(callback) = registry().resolve::<unsafe extern "C" fn(u32, u32)>(CallbackWord(word as usize)) {
                        unsafe { callback(first, second) };
                    }
                }
                None => {
                    if let Ok(callback) = registry().resolve::<unsafe extern "C" fn(u32)>(CallbackWord(word as usize)) {
                        unsafe { callback(first) };
                    }
                }
            }
        }
    }
}

#[cfg(target_arch = "arm")]
pub use native::NativeStorageKernel;

#[cfg(test)]
mod tests {
    //! Expected values come from the image-4 bodies at 0x006a7f28-0x00716324
    //! and the image-5 body at 0x0070fb68.
    use super::*;
    use crate::record_address_space::{GuestDomain, GuestStack, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};
    use crate::shutdown::{KernelStatus, ServiceMessage, TaskId};

    const CONTEXT: u32 = 0x0031_0000;
    const TABLE: u32 = 0x0030_0000;
    const TEMPLATE: u32 = 0x0033_0000;
    const OUTPUT: u32 = 0x0034_0000;
    const PATH: u32 = 0x0035_0000;

    #[derive(Default)]
    struct Kernel {
        commands: [u32; 4],
        commands_len: usize,
        waits: [i32; 4],
        waits_len: usize,
    }

    impl ShutdownRtos for Kernel {
        fn current_task(&mut self) -> TaskId { TaskId(7) }
        fn post(&mut self, _message: ServiceMessage<'_>) -> Result<(), KernelStatus> { Ok(()) }
        fn set_event_flag(&mut self, _flag: u32, _bits: u32) -> Result<(), KernelStatus> { Ok(()) }
    }

    impl StorageKernel for Kernel {
        fn wait_semaphore(&mut self, id: u32, timeout: u32) -> i32 {
            self.waits[self.waits_len] = (id << 16 | timeout) as i32;
            self.waits_len += 1;
            0
        }
        fn signal_semaphore(&mut self, _id: u32) -> i32 { 0 }
        fn kernel_command(&mut self, category: u32, code: u32) {
            self.commands[self.commands_len] = category << 16 | code;
            self.commands_len += 1;
        }
        fn delay_microseconds(&mut self, _duration: u32) {}
        fn invoke_callback(&mut self, _word: u32, _first: u32, _second: Option<u32>) {}
    }

    struct Memory {
        stack: [u8; 512],
        context_slot: [u8; 4],
        context: [u8; 0x1_8000],
        table: [u8; 0x1_0000],
        route: [u8; 24],
        numbering: [u8; 16],
        count: [u8; 4],
        file_table_slot: [u8; 4],
        counters: [u8; 4],
        template_slot: [u8; 4],
        template: [u8; 24],
        path: [u8; 24],
        output: [u8; 0x100],
    }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, Kernel>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(0x0010_0000, 512), &mut memory.stack).unwrap(),
            MemoryRegion::new(descriptor(STORAGE_CONTEXT_SLOT, 4), &mut memory.context_slot).unwrap(),
            MemoryRegion::new(descriptor(CONTEXT, 0x1_8000), &mut memory.context).unwrap(),
            MemoryRegion::new(descriptor(TABLE, 0x1_0000), &mut memory.table).unwrap(),
            MemoryRegion::new(descriptor(ROUTE_TABLE, 24), &mut memory.route).unwrap(),
            MemoryRegion::new(descriptor(NUMBERING_STATE, 16), &mut memory.numbering).unwrap(),
            MemoryRegion::new(descriptor(FILE_TABLE_COUNT, 4), &mut memory.count).unwrap(),
            MemoryRegion::new(descriptor(FILE_TABLE, 4), &mut memory.file_table_slot).unwrap(),
            MemoryRegion::new(descriptor(PROFILE_COUNTERS, 4), &mut memory.counters).unwrap(),
            MemoryRegion::new(descriptor(PATH_TEMPLATE, 4), &mut memory.template_slot).unwrap(),
            MemoryRegion::new(descriptor(TEMPLATE, 24), &mut memory.template).unwrap(),
            MemoryRegion::new(descriptor(PATH, 24), &mut memory.path).unwrap(),
            MemoryRegion::new(descriptor(OUTPUT, 0x100), &mut memory.output).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let stack = GuestStack::new(GuestAddress::new(0x0010_0000), ByteCount(512)).unwrap();
        body(&mut RecordContext::with_stack(space, Kernel::default(), stack))
    }

    fn memory() -> Memory {
        Memory {
            stack: [0; 512], context_slot: [0; 4], context: [0; 0x1_8000], table: [0; 0x1_0000], route: [0; 24],
            numbering: [0; 16], count: [0; 4], file_table_slot: [0; 4], counters: [0; 4],
            template_slot: [0; 4], template: [0; 24], path: [0; 24], output: [0; 0x100],
        }
    }

    fn put_word(bytes: &mut [u8], at: usize, value: u32) { bytes[at..at + 4].copy_from_slice(&value.to_le_bytes()); }
    fn put_half(bytes: &mut [u8], at: usize, value: u16) { bytes[at..at + 2].copy_from_slice(&value.to_le_bytes()); }
    fn word(bytes: &[u8], at: usize) -> u32 { u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) }
    fn half(bytes: &[u8], at: usize) -> u16 { u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()) }

    fn attach(memory: &mut Memory) {
        put_word(&mut memory.context_slot, 0, CONTEXT);
        put_word(&mut memory.file_table_slot, 0, TABLE);
        put_word(&mut memory.template_slot, 0, TEMPLATE);
        put_word(&mut memory.counters, 0, 0x1234_5678);
    }

    #[test]
    fn profile_words_clamp_outside_their_ranges() {
        let mut memory = memory();
        attach(&mut memory);
        put_half(&mut memory.counters, 0, 9999);
        put_half(&mut memory.counters, 2, 999);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_6a7f28(context), Ok(9999));
            assert_eq!(helper_6a801c(context), Ok(9999));
            assert_eq!(helper_6a7fd8(context), Ok(999));
            assert_eq!(helper_6a8040(context), Ok(999));
        });
        put_half(&mut memory.counters, 0, 10000);
        put_half(&mut memory.counters, 2, 1099);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_6a7f28(context), Ok(0));
            assert_eq!(helper_6a7fd8(context), Ok(100));
        });
        put_half(&mut memory.counters, 2, 99);
        with_memory(&mut memory, |context| assert_eq!(helper_6a7fd8(context), Ok(100)));
        put_half(&mut memory.counters, 2, 100);
        with_memory(&mut memory, |context| assert_eq!(helper_6a7fd8(context), Ok(100)));
    }

    #[test]
    fn marking_a_file_pending_checks_the_handle_and_the_slot() {
        let mut memory = memory();
        attach(&mut memory);
        put_word(&mut memory.count, 0, 1);
        put_word(&mut memory.table, 0, 0x1111_1111);
        put_word(&mut memory.table, FILE_STRIDE as usize, 0x2222_2222);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_703b1c(context, 0), Ok(0x10b));
            assert_eq!(helper_703b1c(context, 3), Ok(0x10b));
            assert_eq!(helper_703b1c(context, 2), Ok(0));
            assert_eq!(helper_703b1c(context, 1), Ok(0));
        });
        assert_eq!(word(&memory.table, 0x80), 1);
        assert_eq!(word(&memory.table, FILE_STRIDE as usize + 0x80), 1);
        put_word(&mut memory.table, 0, u32::MAX);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_703b1c(context, 1), Ok(0x10b));
            assert_eq!(helper_6ff11c(context, 1), Ok(13));
            assert_eq!(helper_6ff11c(context, 0), Ok(13));
        });
        put_word(&mut memory.table, 0, 0);
        with_memory(&mut memory, |context| assert_eq!(helper_6ff11c(context, 1), Ok(0)));
    }

    #[test]
    fn the_decimal_formatter_pads_terminates_and_reports_its_length() {
        let mut memory = memory();
        attach(&mut memory);
        let output = GuestAddress::new(OUTPUT);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_70eaa0(context, output, 1234, 6, 1), Ok(4));
        });
        assert_eq!(&memory.output[..6], b"001234");
        with_memory(&mut memory, |context| {
            assert_eq!(helper_70eaa0(context, output, 1234, 6, 2), Ok(4));
        });
        assert_eq!(&memory.output[..6], b"  1234");
        assert_eq!(memory.output[6], 0);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_70eaa0(context, output, 0, 3, 1), Ok(0));
        });
        assert_eq!(&memory.output[..3], b"000");
        with_memory(&mut memory, |context| {
            assert_eq!(helper_70eaa0(context, output, 1234, 3, 0), Ok(u32::MAX));
            assert_eq!(helper_70eaa0(context, output, 42, 4, 4), Ok(2));
        });
        assert_eq!(&memory.output[..3], b"42\0");
        with_memory(&mut memory, |context| {
            assert_eq!(helper_70eaa0(context, output, 7, 1, 5), Ok(1));
        });
        assert_eq!(memory.output[0], b'7');
    }

    #[test]
    fn the_route_table_selects_by_device_and_drive() {
        let mut memory = memory();
        attach(&mut memory);
        memory.route[0] = b'C';
        memory.route[1] = 1;
        memory.route[2] = 0;
        memory.route[8] = b'D';
        memory.route[9] = 1;
        memory.route[10] = 1;
        memory.route[16] = b'E';
        with_memory(&mut memory, |context| {
            assert_eq!(helper_70ec08(context, 0), Ok(u32::from(b'C')));
            assert_eq!(helper_70ec08(context, 1), Ok(u32::from(b'D')));
            assert_eq!(helper_70ec08(context, 2), Ok(u32::from(b'E')));
            assert_eq!(helper_70ec84(context, u32::from(b'C')), Ok(0));
            assert_eq!(helper_70ec84(context, u32::from(b'D')), Ok(1));
            assert_eq!(helper_70ec84(context, u32::from(b'E')), Ok(u32::MAX));
        });
        memory.route[9] = 0;
        with_memory(&mut memory, |context| {
            assert_eq!(helper_70ec08(context, 1), Ok(u32::from(b'E')));
            assert_eq!(helper_70ec84(context, u32::from(b'D')), Ok(u32::MAX));
        });
    }

    #[test]
    fn names_accept_alnum_and_underscore_only() {
        let mut memory = memory();
        attach(&mut memory);
        memory.template[..5].copy_from_slice(b"DSC_1");
        let text = GuestAddress::new(TEMPLATE);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_70f840(context, text, 0), Ok(0));
            assert_eq!(helper_70f840(context, text, 5), Ok(0));
            assert_eq!(helper_70f840(context, text, 6), Ok(8));
        });
        memory.template[0] = b'-';
        with_memory(&mut memory, |context| assert_eq!(helper_70f840(context, text, 1), Ok(8)));
    }

    #[test]
    fn numbering_state_copies_and_stores_the_option() {
        let mut memory = memory();
        attach(&mut memory);
        for index in 0..16 { memory.numbering[index] = index as u8 + 1; }
        let expected = memory.numbering;
        let destination = GuestAddress::new(OUTPUT);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_7111c4(context, destination), Ok(()));
            assert_eq!(helper_7111e0(context, 0x1122_3344), Ok(()));
        });
        assert_eq!(&memory.output[..16], &expected[..16]);
        assert_eq!(word(&memory.numbering, 0xc), 0x1122_3344);
    }

    #[test]
    fn the_drive_prefix_uses_the_context_device() {
        let mut memory = memory();
        attach(&mut memory);
        put_word(&mut memory.context, 0x114, 1);
        memory.route[0] = b'C';
        memory.route[1] = 1;
        memory.route[2] = 0;
        memory.route[8] = b'D';
        memory.route[9] = 1;
        memory.route[10] = 1;
        let destination = GuestAddress::new(OUTPUT);
        with_memory(&mut memory, |context| assert_eq!(helper_712d08(context, destination), Ok(())));
        assert_eq!(&memory.output[..3], b"D:\0");
        put_word(&mut memory.context, 0x114, 0);
        with_memory(&mut memory, |context| assert_eq!(helper_712d08(context, destination), Ok(())));
        assert_eq!(&memory.output[..3], b"C:\0");
    }

    #[test]
    fn the_group_mask_scans_from_bit_one() {
        let mut memory = memory();
        attach(&mut memory);
        memory.context[0x2600] = 4;
        with_memory(&mut memory, |context| {
            assert_eq!(helper_715f0c(context, 0), Ok(0xff));
            assert_eq!(helper_715f0c(context, 1), Ok(0));
            assert_eq!(helper_715f0c(context, 2), Ok(1));
            assert_eq!(helper_715f0c(context, 6), Ok(1));
            assert_eq!(helper_715f0c(context, 8), Ok(3));
            assert_eq!(helper_715f0c(context, 0x10), Ok(0xff));
        });
        memory.context[0x2600] = 0;
        with_memory(&mut memory, |context| assert_eq!(helper_715f0c(context, 3), Ok(0xff)));
    }

    #[test]
    fn timing_options_write_the_two_profile_words() {
        let mut memory = memory();
        attach(&mut memory);
        put_half(&mut memory.counters, 0, 321);
        put_half(&mut memory.counters, 2, 456);
        let interval = GuestAddress::new(OUTPUT);
        let counter = GuestAddress::new(OUTPUT + 2);
        with_memory(&mut memory, |context| assert_eq!(helper_7159f8(context, 0, 0, interval, counter), Ok(())));
        assert_eq!(half(&memory.output, 0), 456);
        assert_eq!(half(&memory.output, 2), 321);
    }

    #[test]
    fn media_ready_reports_each_flag_and_returns_the_stock_status() {
        let mut memory = memory();
        attach(&mut memory);
        put_word(&mut memory.context, 0x114, u32::MAX);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_71ddf0(context, 0), Ok(5));
            assert_eq!(context.effects.commands_len, 0);
        });
        memory.context_slot = [0; 4];
        with_memory(&mut memory, |context| assert_eq!(helper_71ddf0(context, 0), Ok(5)));
        attach(&mut memory);
        put_word(&mut memory.context, 0x114, u32::MAX);
        with_memory(&mut memory, |context| assert_eq!(helper_71ddf0(context, 0), Ok(5)));
        put_word(&mut memory.context, 0x114, 0);
        memory.context[0x16bd8] = 1;
        with_memory(&mut memory, |context| assert_eq!(helper_71ddf0(context, 0), Ok(1)));
        memory.context[0x16bd8] = 2;
        with_memory(&mut memory, |context| assert_eq!(helper_71ddf0(context, 2), Ok(0)));
        with_memory(&mut memory, |context| {
            assert_eq!(helper_71ddf0(context, 1), Ok(5));
            assert_eq!(context.effects.commands[0], 2 << 16 | 0x55);
        });
        memory.context[0x16bd8] = 4;
        with_memory(&mut memory, |context| assert_eq!(helper_71ddf0(context, 0), Ok(5)));
        memory.context[0x16bd8] = 8;
        with_memory(&mut memory, |context| assert_eq!(helper_71ddf0(context, 0), Ok(2)));
        with_memory(&mut memory, |context| assert_eq!(helper_71ddf0(context, 1), Ok(0)));
        memory.context[0x16bd8] = 0;
        with_memory(&mut memory, |context| assert_eq!(helper_71ddf0(context, 0), Ok(0)));
    }

    #[test]
    fn the_current_drive_follows_the_context_device() {
        let mut memory = memory();
        attach(&mut memory);
        memory.route[0] = b'C';
        memory.route[1] = 1;
        memory.route[2] = 0;
        put_word(&mut memory.context, 0x114, 0);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_71fb18(context), Ok(u32::from(b'C')));
            assert_eq!(helper_71f708(context), Ok(u32::from(b'C')));
        });
        put_word(&mut memory.context, 0x114, 2);
        with_memory(&mut memory, |context| assert_eq!(helper_71fb18(context), Ok(0)));
        memory.context_slot = [0; 4];
        with_memory(&mut memory, |context| assert_eq!(helper_71fb18(context), Ok(0)));
    }

    #[test]
    fn the_catalog_pair_takes_the_larger_directory_and_number() {
        let mut memory = memory();
        attach(&mut memory);
        memory.route[0] = b'C';
        memory.route[1] = 1;
        memory.route[2] = 0;
        put_word(&mut memory.context, 0x114, 0);
        put_word(&mut memory.context, 0x2168, 150);
        put_word(&mut memory.context, 0x21b8, 0);
        put_word(&mut memory.context, 0x2794, 0);
        put_word(&mut memory.context, 0x16c6c, 0);
        let record = 0x21d0;
        put_half(&mut memory.context, record + 4, 120);
        put_half(&mut memory.context, record + 6, 2000);
        let directory = GuestAddress::new(OUTPUT);
        let file = GuestAddress::new(OUTPUT + 2);
        with_memory(&mut memory, |context| assert_eq!(helper_716324(context, 0, 0, directory, file), Ok(0)));
        assert_eq!(half(&memory.output, 0), 150);
        assert_eq!(half(&memory.output, 2), 999);
    }

    #[test]
    fn the_catalog_pair_prefers_the_timing_words_when_the_record_is_older() {
        let mut memory = memory();
        attach(&mut memory);
        put_word(&mut memory.context, 0x2168, 100);
        put_word(&mut memory.context, 0x21b8, 0);
        put_word(&mut memory.context, 0x2794, 0);
        put_word(&mut memory.context, 0x16c6c, 1);
        put_half(&mut memory.counters, 0, 321);
        put_half(&mut memory.counters, 2, 456);
        let record = 0x21d0 + 2 * 0x10;
        put_half(&mut memory.context, record + 4, 300);
        put_half(&mut memory.context, record + 6, 100);
        memory.context[456 + 0x2218] = 2;
        let directory = GuestAddress::new(OUTPUT);
        let file = GuestAddress::new(OUTPUT + 2);
        with_memory(&mut memory, |context| assert_eq!(helper_716324(context, 0, 2, directory, file), Ok(0)));
        assert_eq!(half(&memory.output, 0), 456);
        assert_eq!(half(&memory.output, 2), 321);
    }

    #[test]
    fn the_catalog_pair_rewrites_the_saved_pair_for_another_group() {
        let mut memory = memory();
        attach(&mut memory);
        put_word(&mut memory.context, 0x2168, 123);
        put_word(&mut memory.context, 0x21b8, 0);
        put_word(&mut memory.context, 0x2794, 0);
        put_word(&mut memory.context, 0x16c6c, 1);
        put_half(&mut memory.counters, 0, 321);
        put_half(&mut memory.counters, 2, 456);
        let record = 0x21d0 + 3 * 0x10;
        put_half(&mut memory.context, record + 4, 100);
        put_half(&mut memory.context, record + 6, 10);
        memory.context[456 + 0x2218] = 5;
        let directory = GuestAddress::new(OUTPUT);
        let file = GuestAddress::new(OUTPUT + 2);
        with_memory(&mut memory, |context| assert_eq!(helper_716324(context, 0, 3, directory, file), Ok(0)));
        assert_eq!(half(&memory.output, 0), 123);
        assert_eq!(half(&memory.output, 2), 3 * 1000 + 999);
    }

    #[test]
    fn reading_numbers_walks_ready_drive_group_and_pair() {
        let mut memory = memory();
        attach(&mut memory);
        memory.route[0] = b'C';
        memory.route[1] = 1;
        memory.route[2] = 0;
        put_word(&mut memory.context, 0x114, 0);
        put_word(&mut memory.context, 0x2210, 1);
        memory.context[0x2600] = 4;
        put_word(&mut memory.context, 0x2168, 150);
        put_word(&mut memory.context, 0x21b8, 0);
        put_word(&mut memory.context, 0x2794, 0);
        put_word(&mut memory.context, 0x16c6c, 0);
        let record = 0x21d0;
        put_half(&mut memory.context, record + 4, 100);
        put_half(&mut memory.context, record + 6, 1);
        let directory = GuestAddress::new(OUTPUT);
        let file = GuestAddress::new(OUTPUT + 2);
        with_memory(&mut memory, |context| assert_eq!(helper_70fb68(context, directory, file), Ok(0)));
        assert_eq!(half(&memory.output, 0), 150);
        assert_eq!(half(&memory.output, 2), 999);
        memory.context[0x16bd8] = 1;
        with_memory(&mut memory, |context| assert_eq!(helper_70fb68(context, directory, file), Ok(1)));
        memory.context[0x16bd8] = 0;
        memory.route[0] = 0;
        memory.route[1] = 0;
        memory.route[9] = 0;
        with_memory(&mut memory, |context| assert_eq!(helper_70fb68(context, directory, file), Ok(8)));
        memory.route[0] = b'C';
        memory.route[1] = 1;
        memory.context[0x2210] = 0;
        with_memory(&mut memory, |context| assert_eq!(helper_70fb68(context, directory, file), Ok(8)));
        memory.context[0x2210] = 2;
        memory.context[0x2600] = 0;
        with_memory(&mut memory, |context| assert_eq!(helper_70fb68(context, directory, file), Ok(8)));
    }

    #[test]
    fn the_path_template_compare_returns_one_only_on_a_full_match() {
        let mut memory = memory();
        attach(&mut memory);
        memory.template[..5].copy_from_slice(b"ABCDE");
        memory.path[..5].copy_from_slice(b"ABCDE");
        let path = GuestAddress::new(PATH);
        with_memory(&mut memory, |context| assert_eq!(helper_711200(context, path), Ok(1)));
        memory.path[4] = b'F';
        with_memory(&mut memory, |context| assert_eq!(helper_711200(context, path), Ok(0)));
    }
}
