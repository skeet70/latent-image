//! Stock-shaped storage request queue over the image-4 bodies at
//! 0x0071bc2c-0x0071fde4.
//!
//! Requests are built in guest frames: the payload first, then the 12-byte
//! message header 0x0071fde4 reads. Payload bytes no stock copy covers stay
//! zero.

use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext, StackAlignment};
use crate::record_storage_service_6a7f28::{
    helper_6ff11c, helper_70ec84, helper_70f840, helper_70fb68, helper_7111c4, helper_7111e0,
    helper_711200, helper_71ddf0, StorageKernel, STORAGE_CONTEXT_SLOT,
};
use crate::shutdown::{QueueId, RequestId, ServiceMessage, TaskId};

pub const RESERVATION_OWNER: u32 = 0x0091_0134;
pub const RESERVATION_RECORD: u32 = 0x0091_0138;
pub const RESERVATION_TABLE: u32 = 0x0091_01c0;
pub const RESERVATION_LAST: u32 = 0x0091_0200;
const MAX_PAYLOAD: usize = 0x50;
const DESTINATION: u8 = 0x65;

fn address(base: u32, offset: u32) -> GuestAddress { GuestAddress::new(base.wrapping_add(offset)) }

fn copy_string<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>, destination: GuestAddress, source: GuestAddress, maximum: u32,
) -> Result<(), MemoryError> {
    let mut index = 0;
    while index < maximum {
        let byte = context.read_u8(source.checked_add(index)?)?;
        context.write_u8(destination.checked_add(index)?, byte)?;
        if byte == 0 { return Ok(()); }
        index += 1;
    }
    Ok(())
}

fn write_request<M: GuestAddressSpace, E: StorageKernel>(
    context: &mut RecordContext<M, E>, opcode: u16, payload: GuestAddress, length: u32, request: GuestAddress,
) -> Result<i32, MemoryError> {
    let task = context.effects.current_task().0;
    context.write_u16(request, opcode)?;
    context.write_u8(request.checked_add(2)?, task)?;
    context.write_u8(request.checked_add(3)?, DESTINATION)?;
    context.write_u32(request.checked_add(4)?, length)?;
    context.write_u32(request.checked_add(8)?, payload.get())?;
    helper_71fde4(context, request)
}

pub fn helper_71bc2c<M: GuestAddressSpace, E: StorageKernel>(context: &mut RecordContext<M, E>) -> Result<Option<GuestAddress>, MemoryError> {
    if context.effects.wait_semaphore(0x3a, u32::MAX) != 0 { return Ok(None); }
    let task = context.effects.current_task().0;
    context.write_u32(GuestAddress::new(RESERVATION_OWNER), u32::from(task))?;
    Ok(Some(GuestAddress::new(RESERVATION_RECORD)))
}

pub fn helper_71bc6c<M: GuestAddressSpace, E: StorageKernel>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    context.write_u32(GuestAddress::new(RESERVATION_OWNER), 0)?;
    context.effects.signal_semaphore(0x3a);
    Ok(())
}

pub fn helper_71bc90<M: GuestAddressSpace, E: StorageKernel>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    let task = u32::from(context.effects.current_task().0);
    if task != context.read_u32(GuestAddress::new(RESERVATION_OWNER))? { return Ok(0xffff_ffc1); }
    Ok(context.effects.wait_semaphore(0x3b, u32::MAX) as u32)
}

pub fn helper_71ef18<M: GuestAddressSpace, E: StorageKernel>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    if context.effects.wait_semaphore(0x3c, u32::MAX) != 0 { return Ok(0); }
    let first = context.read_u32(GuestAddress::new(RESERVATION_TABLE))?;
    let mut candidate = context.read_u32(GuestAddress::new(RESERVATION_LAST))?;
    let mut index;
    loop {
        candidate = candidate.wrapping_add(1);
        if (candidate as i32) < 0 { candidate = 1; }
        if candidate == first { continue; }
        let mut used = false;
        for slot in 1..16u32 {
            if context.read_u32(address(RESERVATION_TABLE, slot * 4))? == candidate { used = true; break; }
        }
        if used { continue; }
        context.write_u32(GuestAddress::new(RESERVATION_LAST), candidate)?;
        if first == 0 {
            index = 0;
        } else {
            index = 16;
            for slot in 1..16u32 {
                if context.read_u32(address(RESERVATION_TABLE, slot * 4))? == 0 { index = slot; break; }
            }
        }
        break;
    }
    if index == 16 {
        context.effects.signal_semaphore(0x3c);
        return Ok(0);
    }
    context.write_u32(address(RESERVATION_TABLE, index * 4), candidate)?;
    context.effects.signal_semaphore(0x3c);
    Ok(candidate)
}

pub fn helper_71fde4<M: GuestAddressSpace, E: StorageKernel>(context: &mut RecordContext<M, E>, request: GuestAddress) -> Result<i32, MemoryError> {
    let operation = context.read_u16(request)?;
    let sender = context.read_u8(request.checked_add(2)?)?;
    let destination = context.read_u8(request.checked_add(3)?)?;
    let length = context.read_u32(request.checked_add(4)?)?;
    let payload = context.read_u32(request.checked_add(8)?)?;
    let mut bytes = [0u8; MAX_PAYLOAD];
    let copied = length.min(MAX_PAYLOAD as u32);
    for index in 0..copied {
        bytes[index as usize] = context.read_u8(GuestAddress::new(payload).checked_add(index)?)?;
    }
    let message = ServiceMessage {
        operation: RequestId(operation),
        sender: TaskId(sender),
        destination: QueueId(destination),
        payload: &bytes[..copied as usize],
    };
    Ok(match context.effects.post(message) { Ok(()) => 0, Err(status) => status.0 })
}

pub fn helper_71bfe0<M: GuestAddressSpace, E: StorageKernel>(
    context: &mut RecordContext<M, E>, callback: u32, second: u32, third: u32, fourth: u32, reserved: u32, mode: u32,
) -> Result<u32, MemoryError> {
    let status = helper_71ddf0(context, 0)?;
    if status != 0 {
        context.effects.invoke_callback(callback, status, Some(0));
        return Ok(0);
    }
    let task = u32::from(context.effects.current_task().0);
    let mut reservation = None;
    if reserved != 0 {
        if task == 0x1c || task == 0 {
            context.effects.invoke_callback(callback, 9, Some(0));
            return Ok(0);
        }
        match helper_71bc2c(context)? {
            Some(pointer) => reservation = Some(pointer),
            None => {
                context.effects.invoke_callback(callback, 9, Some(0));
                return Ok(0);
            }
        }
    }
    let opcode = if mode == 1 { 0x2c2f } else { 0x2c19 };
    let pointer = reservation.map(GuestAddress::get).unwrap_or(0);
    let flag = if reservation.is_some() { 1 } else { 0 };
    context.with_guest_frame(ByteCount(40), StackAlignment::Word, |context, frame| -> Result<(), MemoryError> {
        let payload = frame.base();
        context.write_u32(payload, callback)?;
        context.write_u32(payload.checked_add(4)?, second)?;
        context.write_u32(payload.checked_add(8)?, third)?;
        context.write_u32(payload.checked_add(12)?, fourth)?;
        context.write_u32(payload.checked_add(16)?, 0)?;
        context.write_u32(payload.checked_add(20)?, flag)?;
        context.write_u32(payload.checked_add(24)?, pointer)?;
        let request = frame.at(ByteCount(28), ByteCount(12))?;
        let _ = write_request(context, opcode, payload, 28, request)?;
        Ok(())
    })?;
    let mut value = 0;
    if let Some(pointer) = reservation {
        helper_71bc90(context)?;
        if context.read_u32(pointer)? == 0 { value = context.read_u32(pointer.checked_add(4)?)?; }
        helper_71bc6c(context)?;
    }
    Ok(value)
}

pub fn helper_71c134<M: GuestAddressSpace, E: StorageKernel>(
    context: &mut RecordContext<M, E>, callback: u32, second: u32, third: u32, buffer: u32, flags: u32, mode: u32,
) -> Result<u32, MemoryError> {
    let numbering = context.with_guest_frame(ByteCount(16), StackAlignment::Word, |context, frame| -> Result<u32, MemoryError> {
        helper_7111c4(context, frame.base())?;
        context.read_u32(frame.base())
    })?;
    if numbering == 1 {
        context.effects.invoke_callback(callback, 0, None);
        return Ok(0);
    }
    let status = helper_71ddf0(context, 0)?;
    if status != 0 {
        context.effects.invoke_callback(callback, status, None);
        return Ok(0);
    }
    let task = u32::from(context.effects.current_task().0);
    let mut reservation = None;
    if flags != 0 {
        if task == 0x1c || task == 0 {
            context.effects.invoke_callback(callback, 9, None);
            return Ok(0);
        }
        match helper_71bc2c(context)? {
            Some(pointer) => reservation = Some(pointer),
            None => {
                context.effects.invoke_callback(callback, 9, None);
                return Ok(0);
            }
        }
    }
    let opcode = if mode == 1 { 0x2c30 } else { 0x2c1a };
    context.with_guest_frame(ByteCount(52), StackAlignment::Word, |context, frame| -> Result<(), MemoryError> {
        let payload = frame.base();
        context.write_u32(payload, callback)?;
        context.write_u32(payload.checked_add(4)?, second)?;
        context.write_u32(payload.checked_add(8)?, third)?;
        let text = payload.checked_add(12)?;
        if buffer == 0 {
            context.write_u8(text, 0)?;
        } else {
            copy_string(context, text, GuestAddress::new(buffer), 20)?;
        }
        for offset in 32..40 { context.write_u8(payload.checked_add(offset)?, 0)?; }
        let request = frame.at(ByteCount(40), ByteCount(12))?;
        let _ = write_request(context, opcode, payload, 40, request)?;
        Ok(())
    })?;
    let mut value = 0;
    if let Some(pointer) = reservation {
        helper_71bc90(context)?;
        value = context.read_u32(pointer)?;
        helper_71bc6c(context)?;
    }
    Ok(value)
}

pub fn helper_71c2b0<M: GuestAddressSpace, E: StorageKernel>(
    context: &mut RecordContext<M, E>, callback: u32, path: GuestAddress, third: u32, fourth: u32, fifth: u32, mode: u32,
) -> Result<u32, MemoryError> {
    let task = u32::from(context.effects.current_task().0);
    if fourth != 0 && (task == 0x1c || task == 0) {
        context.effects.invoke_callback(callback, 9, Some(0));
        return Ok(0);
    }
    let numbering = context.with_guest_frame(ByteCount(16), StackAlignment::Word, |context, frame| -> Result<u32, MemoryError> {
        helper_7111c4(context, frame.base())?;
        context.read_u32(frame.base())
    })?;
    if numbering == 1 {
        let matches = helper_711200(context, path)?;
        let (status, result) = if matches == 1 { helper_7111e0(context, 0)?; (0u32, 1u32) } else { (6u32, 0u32) };
        context.effects.invoke_callback(callback, status, Some(result));
        return Ok(result);
    }
    let status = helper_71ddf0(context, 0)?;
    if status != 0 {
        context.effects.invoke_callback(callback, status, Some(0));
        return Ok(0);
    }
    if context.read_u8(path.checked_add(1)?)? == b':' {
        let mut drive = context.read_u8(path)?;
        let upper = drive.wrapping_add(0xbf) < 0x1a;
        let lower = drive.wrapping_add(0x9f) < 0x1a;
        if !upper && !lower {
            context.effects.invoke_callback(callback, 8, Some(0));
            return Ok(0);
        }
        if lower { drive = drive.wrapping_sub(0x20); }
        if drive == 0 {
            context.effects.invoke_callback(callback, 8, Some(0));
            return Ok(0);
        }
        let device = helper_70ec84(context, u32::from(drive))?;
        let base = context.read_u32(GuestAddress::new(STORAGE_CONTEXT_SLOT))?;
        if context.read_u32(address(base, 0x114))? != device {
            context.effects.invoke_callback(callback, 5, Some(0));
            return Ok(0);
        }
    }
    let mut reservation = None;
    if fourth != 0 {
        match helper_71bc2c(context)? {
            Some(pointer) => reservation = Some(pointer),
            None => {
                context.effects.invoke_callback(callback, 9, Some(0));
                return Ok(0);
            }
        }
    }
    let mut result = helper_71ef18(context)?;
    let opcode = if mode == 1 { 0x2c2d } else { 0x2c17 };
    let pointer = reservation.map(GuestAddress::get).unwrap_or(0);
    let flag = if reservation.is_some() { 1 } else { 0 };
    context.with_guest_frame(ByteCount(92), StackAlignment::Word, |context, frame| -> Result<(), MemoryError> {
        let payload = frame.base();
        context.write_u32(payload, callback)?;
        copy_string(context, payload.checked_add(4)?, path, 48)?;
        context.write_u32(payload.checked_add(0x34)?, 0)?;
        context.write_u32(payload.checked_add(0x38)?, third)?;
        context.write_u32(payload.checked_add(0x3c)?, result)?;
        context.write_u32(payload.checked_add(0x40)?, flag)?;
        context.write_u32(payload.checked_add(0x44)?, pointer)?;
        context.write_u32(payload.checked_add(0x48)?, pointer.wrapping_add(4))?;
        context.write_u32(payload.checked_add(0x4c)?, fifth)?;
        let request = frame.at(ByteCount(80), ByteCount(12))?;
        let _ = write_request(context, opcode, payload, 80, request)?;
        Ok(())
    })?;
    if let Some(pointer) = reservation {
        helper_71bc90(context)?;
        if context.read_u32(pointer)? != 0 { result = 0; }
        helper_71bc6c(context)?;
    }
    Ok(result)
}

pub fn helper_71c504<M: GuestAddressSpace, E: StorageKernel>(
    context: &mut RecordContext<M, E>, callback: u32, handle: u32, reserved: u32,
) -> Result<u32, MemoryError> {
    let status = helper_71ddf0(context, 0)?;
    if status != 0 {
        context.effects.invoke_callback(callback, status, None);
        return Ok(status);
    }
    helper_71df20(context, handle)?;
    let task = u32::from(context.effects.current_task().0);
    let mut reservation = None;
    if reserved != 0 {
        if task == 0x1c || task == 0 {
            context.effects.invoke_callback(callback, 9, None);
            return Ok(9);
        }
        match helper_71bc2c(context)? {
            Some(pointer) => reservation = Some(pointer),
            None => {
                context.effects.invoke_callback(callback, 9, None);
                return Ok(9);
            }
        }
    }
    let posted = context.with_guest_frame(ByteCount(28), StackAlignment::Word, |context, frame| -> Result<i32, MemoryError> {
        let payload = frame.base();
        context.write_u32(payload, callback)?;
        context.write_u32(payload.checked_add(4)?, handle)?;
        context.write_u32(payload.checked_add(8)?, 0)?;
        context.write_u32(payload.checked_add(12)?, 0)?;
        let request = frame.at(ByteCount(16), ByteCount(12))?;
        write_request(context, 0x2c1d, payload, 16, request)
    })?;
    if posted != 0 { return Ok(9); }
    let mut value = 0;
    if let Some(pointer) = reservation {
        helper_71bc90(context)?;
        value = context.read_u32(pointer)?;
        helper_71bc6c(context)?;
    }
    Ok(value)
}

pub fn helper_71c648<M: GuestAddressSpace, E: StorageKernel>(
    context: &mut RecordContext<M, E>, callback: u32, path: GuestAddress, count: u32,
) -> Result<u32, MemoryError> {
    helper_71c2b0(context, callback, path, count, 0, 1, 0)
}

pub fn helper_71c75c<M: GuestAddressSpace, E: StorageKernel>(
    context: &mut RecordContext<M, E>, callback: u32, second: u32, third: u32, fourth: u32,
) -> Result<u32, MemoryError> {
    helper_71bfe0(context, callback, second, third, fourth, 0, 0)
}

pub fn helper_71c7bc<M: GuestAddressSpace, E: StorageKernel>(
    context: &mut RecordContext<M, E>, callback: u32, second: u32, third: u32, fourth: u32,
) -> Result<u32, MemoryError> {
    helper_71c134(context, callback, second, third, fourth, 0, 0)
}

pub fn helper_71c938<M: GuestAddressSpace, E: StorageKernel>(
    context: &mut RecordContext<M, E>, callback: u32, handle: u32,
) -> Result<u32, MemoryError> {
    helper_71c504(context, callback, handle, 0)
}

pub fn helper_71df20<M: GuestAddressSpace, E: StorageKernel>(context: &mut RecordContext<M, E>, handle: u32) -> Result<u32, MemoryError> {
    helper_6ff11c(context, handle)
}

#[cfg(test)]
mod tests {
    //! Expected opcodes, lengths, payload fields and statuses come from the
    //! image-4 bodies at 0x0071bc2c-0x0071fde4.
    use super::*;
    use crate::record_address_space::{GuestDomain, GuestStack, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};
    use crate::shutdown::{KernelStatus, ServiceMessage, ShutdownRtos, TaskId};

    const CONTEXT: u32 = 0x0031_0000;
    const TABLE: u32 = 0x0030_0000;
    const OUTPUT: u32 = 0x0033_0000;
    const PATH: u32 = 0x0034_0000;
    const GLOBALS: u32 = 0x0035_0000;

    struct Kernel {
        tasks: u32,
        posts: [u32; 4],
        posts_len: usize,
        payloads: [[u8; 0x50]; 4],
        lengths: [u32; 4],
        callbacks: [u32; 8],
        callbacks_len: usize,
        waits: [i32; 4],
        waits_len: usize,
    }

    impl Default for Kernel {
        fn default() -> Self {
            Self {
                tasks: 0, posts: [0; 4], posts_len: 0, payloads: [[0; 0x50]; 4], lengths: [0; 4],
                callbacks: [0; 8], callbacks_len: 0, waits: [0; 4], waits_len: 0,
            }
        }
    }

    impl Kernel {
        fn with_task(task: u32) -> Self { Self { tasks: task, ..Self::default() } }
    }

    impl ShutdownRtos for Kernel {
        fn current_task(&mut self) -> TaskId { TaskId(self.tasks as u8) }
        fn post(&mut self, message: ServiceMessage<'_>) -> Result<(), KernelStatus> {
            let index = self.posts_len;
            self.posts[index] = u32::from(message.operation.0) << 16 | u32::from(message.sender.0) << 8 | u32::from(message.destination.0);
            self.lengths[index] = message.payload.len() as u32;
            self.payloads[index][..message.payload.len()].copy_from_slice(message.payload);
            self.posts_len += 1;
            Ok(())
        }
        fn set_event_flag(&mut self, _flag: u32, _bits: u32) -> Result<(), KernelStatus> { Ok(()) }
    }

    impl StorageKernel for Kernel {
        fn wait_semaphore(&mut self, id: u32, _timeout: u32) -> i32 {
            self.waits[self.waits_len] = id as i32;
            self.waits_len += 1;
            0
        }
        fn signal_semaphore(&mut self, _id: u32) -> i32 { 0 }
        fn kernel_command(&mut self, _category: u32, _code: u32) {}
        fn delay_microseconds(&mut self, _duration: u32) {}
        fn invoke_callback(&mut self, word: u32, first: u32, second: Option<u32>) {
            self.callbacks[self.callbacks_len] = word;
            self.callbacks[self.callbacks_len + 1] = first;
            self.callbacks[self.callbacks_len + 2] = second.unwrap_or(0xffff_ffff);
            self.callbacks_len += 3;
        }
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
        path: [u8; 64],
        reservation: [u8; 12],
        reservation_table: [u8; 0x44],
        globals: [u8; 0x100],
        output: [u8; 0x100],
    }

    fn with_memory<R>(memory: &mut Memory, effects: Kernel, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, Kernel>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(0x0010_0000, 512), &mut memory.stack).unwrap(),
            MemoryRegion::new(descriptor(STORAGE_CONTEXT_SLOT, 4), &mut memory.context_slot).unwrap(),
            MemoryRegion::new(descriptor(CONTEXT, 0x1_8000), &mut memory.context).unwrap(),
            MemoryRegion::new(descriptor(TABLE, 0x1_0000), &mut memory.table).unwrap(),
            MemoryRegion::new(descriptor(0x0081_4730, 24), &mut memory.route).unwrap(),
            MemoryRegion::new(descriptor(0x008f_c8a4, 16), &mut memory.numbering).unwrap(),
            MemoryRegion::new(descriptor(0x0094_4468, 4), &mut memory.count).unwrap(),
            MemoryRegion::new(descriptor(0x008f_c880, 4), &mut memory.file_table_slot).unwrap(),
            MemoryRegion::new(descriptor(0x409c_281c, 4), &mut memory.counters).unwrap(),
            MemoryRegion::new(descriptor(0x0082_99fc, 4), &mut memory.template_slot).unwrap(),
            MemoryRegion::new(descriptor(0x0036_0000, 24), &mut memory.template).unwrap(),
            MemoryRegion::new(descriptor(PATH, 64), &mut memory.path).unwrap(),
            MemoryRegion::new(descriptor(RESERVATION_OWNER, 12), &mut memory.reservation).unwrap(),
            MemoryRegion::new(descriptor(RESERVATION_TABLE, 0x44), &mut memory.reservation_table).unwrap(),
            MemoryRegion::new(descriptor(GLOBALS, 0x100), &mut memory.globals).unwrap(),
            MemoryRegion::new(descriptor(OUTPUT, 0x100), &mut memory.output).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let stack = GuestStack::new(GuestAddress::new(0x0010_0000), ByteCount(512)).unwrap();
        body(&mut RecordContext::with_stack(space, effects, stack))
    }

    fn memory() -> Memory {
        Memory {
            stack: [0; 512], context_slot: [0; 4], context: [0; 0x1_8000], table: [0; 0x1_0000], route: [0; 24],
            numbering: [0; 16], count: [0; 4], file_table_slot: [0; 4], counters: [0; 4],
            template_slot: [0; 4], template: [0; 24], path: [0; 64], reservation: [0; 12], reservation_table: [0; 0x44], globals: [0; 0x100], output: [0; 0x100],
        }
    }

    fn put_word(bytes: &mut [u8], at: usize, value: u32) { bytes[at..at + 4].copy_from_slice(&value.to_le_bytes()); }
    fn put_half(bytes: &mut [u8], at: usize, value: u16) { bytes[at..at + 2].copy_from_slice(&value.to_le_bytes()); }
    fn word(bytes: &[u8], at: usize) -> u32 { u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) }
    fn half(bytes: &[u8], at: usize) -> u16 { u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()) }

    fn attach(memory: &mut Memory) {
        put_word(&mut memory.context_slot, 0, CONTEXT);
        put_word(&mut memory.file_table_slot, 0, TABLE);
        put_word(&mut memory.template_slot, 0, 0x0036_0000);
        put_word(&mut memory.count, 0, 0);
        memory.route[0] = b'C';
        memory.route[1] = 1;
        memory.route[2] = 0;
    }

    #[test]
    fn reservation_waits_and_records_the_owner() {
        let mut memory = memory();
        attach(&mut memory);
        with_memory(&mut memory, Kernel::with_task(7), |context| {
            assert_eq!(helper_71bc2c(context), Ok(Some(GuestAddress::new(RESERVATION_RECORD))));
            assert_eq!(context.effects.waits[0], 0x3a);
        });
        assert_eq!(word(&memory.reservation, 0), 7);
        put_word(&mut memory.reservation, 0, 7);
        with_memory(&mut memory, Kernel::with_task(7), |context| {
            assert_eq!(helper_71bc90(context), Ok(0));
        });
        with_memory(&mut memory, Kernel::with_task(8), |context| {
            assert_eq!(helper_71bc90(context), Ok(0xffff_ffc1));
            assert_eq!(helper_71bc6c(context), Ok(()));
        });
        assert_eq!(word(&memory.reservation, 0), 0);
    }

    #[test]
    fn the_reservation_table_skips_the_owner_and_used_values() {
        let mut memory = memory();
        attach(&mut memory);
        put_word(&mut memory.reservation_table, 0, 0);
        put_word(&mut memory.reservation_table, 4, 3);
        put_word(&mut memory.reservation_table, 0x40, 2);
        with_memory(&mut memory, Kernel::default(), |context| {
            assert_eq!(helper_71ef18(context), Ok(4));
        });
        assert_eq!(word(&memory.reservation_table, 0), 4);
        assert_eq!(word(&memory.reservation_table, 0x40), 4);
        put_word(&mut memory.reservation_table, 0, 5);
        put_word(&mut memory.reservation_table, 0x40, 4);
        with_memory(&mut memory, Kernel::default(), |context| {
            assert_eq!(helper_71ef18(context), Ok(6));
        });
        assert_eq!(word(&memory.reservation_table, 8), 6);
    }

    #[test]
    fn the_message_header_supplies_the_posted_fields() {
        let mut memory = memory();
        attach(&mut memory);
        let payload = GuestAddress::new(OUTPUT);
        memory.output[..4].copy_from_slice(&[1, 2, 3, 4]);
        let request = GuestAddress::new(GLOBALS);
        put_half(&mut memory.globals, 0, 0x2c19);
        memory.globals[2] = 9;
        memory.globals[3] = 0x65;
        put_word(&mut memory.globals, 4, 4);
        put_word(&mut memory.globals, 8, payload.get());
        with_memory(&mut memory, Kernel::default(), |context| {
            assert_eq!(helper_71fde4(context, request), Ok(0));
            assert_eq!(context.effects.posts[0], 0x2c19 << 16 | 9 << 8 | 0x65);
            assert_eq!(context.effects.lengths[0], 4);
            assert_eq!(&context.effects.payloads[0][..4], &[1, 2, 3, 4]);
        });
    }

    #[test]
    fn the_write_request_posts_the_six_word_payload() {
        let mut memory = memory();
        attach(&mut memory);
        put_word(&mut memory.context, 0x114, 0);
        with_memory(&mut memory, Kernel::with_task(0x1c), |context| {
            assert_eq!(helper_71bfe0(context, 0x1000, 1, 2, 3, 0, 1), Ok(0));
            assert_eq!(context.effects.posts[0], 0x2c2f << 16 | 0x1c << 8 | 0x65);
            assert_eq!(context.effects.lengths[0], 28);
            let payload = &context.effects.payloads[0];
            assert_eq!(word(payload, 0), 0x1000);
            assert_eq!(word(payload, 4), 1);
            assert_eq!(word(payload, 8), 2);
            assert_eq!(word(payload, 12), 3);
            assert_eq!(word(payload, 16), 0);
            assert_eq!(word(payload, 20), 0);
            assert_eq!(word(payload, 24), 0);
        });
    }

    #[test]
    fn a_media_failure_reaches_the_callback_and_posts_nothing() {
        let mut memory = memory();
        attach(&mut memory);
        put_word(&mut memory.context, 0x114, u32::MAX);
        with_memory(&mut memory, Kernel::with_task(1), |context| {
            assert_eq!(helper_71bfe0(context, 0x1000, 1, 2, 3, 0, 0), Ok(0));
            assert_eq!(context.effects.posts_len, 0);
            assert_eq!(context.effects.callbacks[0], 0x1000);
            assert_eq!(context.effects.callbacks[1], 5);
            assert_eq!(context.effects.callbacks[2], 0);
        });
    }

    #[test]
    fn the_open_request_carries_the_context_request_and_name() {
        let mut memory = memory();
        attach(&mut memory);
        put_word(&mut memory.context, 0x114, 0);
        memory.path[..4].copy_from_slice(b"abc\0");
        with_memory(&mut memory, Kernel::with_task(3), |context| {
            assert_eq!(helper_71c134(context, 0x2000, 0x3000, 0x4000, PATH, 0, 0), Ok(0));
            assert_eq!(context.effects.posts[0], 0x2c1a << 16 | 3 << 8 | 0x65);
            assert_eq!(context.effects.lengths[0], 40);
            let payload = &context.effects.payloads[0];
            assert_eq!(word(payload, 0), 0x2000);
            assert_eq!(word(payload, 4), 0x3000);
            assert_eq!(word(payload, 8), 0x4000);
            assert_eq!(&payload[12..16], b"abc\0");
        });
        memory.numbering[0] = 1;
        with_memory(&mut memory, Kernel::with_task(3), |context| {
            assert_eq!(helper_71c134(context, 0x2000, 0, 0, 0, 0, 0), Ok(0));
            assert_eq!(context.effects.posts_len, 0);
            assert_eq!(context.effects.callbacks[1], 0);
        });
    }

    #[test]
    fn the_path_transfer_checks_the_drive_letter_and_posts_the_record() {
        let mut memory = memory();
        attach(&mut memory);
        put_word(&mut memory.context, 0x114, 0);
        put_word(&mut memory.context, 0x2210, 1);
        memory.context[0x2600] = 4;
        memory.path[..4].copy_from_slice(b"C:\\\0");
        with_memory(&mut memory, Kernel::with_task(3), |context| {
            assert_eq!(helper_71c2b0(context, 0x5000, GuestAddress::new(PATH), 7, 0, 8, 0), Ok(1));
            assert_eq!(context.effects.posts[0], 0x2c17 << 16 | 3 << 8 | 0x65);
            assert_eq!(context.effects.lengths[0], 80);
            let payload = &context.effects.payloads[0];
            assert_eq!(word(payload, 0), 0x5000);
            assert_eq!(&payload[4..8], b"C:\\\0");
            assert_eq!(word(payload, 0x38), 7);
            assert_eq!(word(payload, 0x3c), 1);
            assert_eq!(word(payload, 0x4c), 8);
        });
    }

    #[test]
    fn the_path_transfer_rejects_an_unknown_drive() {
        let mut memory = memory();
        attach(&mut memory);
        put_word(&mut memory.context, 0x114, 0);
        memory.path[..4].copy_from_slice(b"Z:\\\0");
        with_memory(&mut memory, Kernel::with_task(3), |context| {
            assert_eq!(helper_71c2b0(context, 0x5000, GuestAddress::new(PATH), 0, 0, 0, 0), Ok(0));
            assert_eq!(context.effects.posts_len, 0);
            assert_eq!(context.effects.callbacks[1], 5);
        });
    }

    #[test]
    fn the_finish_request_posts_the_callback_and_handle() {
        let mut memory = memory();
        attach(&mut memory);
        put_word(&mut memory.context, 0x114, 0);
        with_memory(&mut memory, Kernel::with_task(4), |context| {
            assert_eq!(helper_71c504(context, 0x6000, 0x1234, 0), Ok(0));
            assert_eq!(context.effects.posts[0], 0x2c1d << 16 | 4 << 8 | 0x65);
            assert_eq!(context.effects.lengths[0], 16);
            let payload = &context.effects.payloads[0];
            assert_eq!(word(payload, 0), 0x6000);
            assert_eq!(word(payload, 4), 0x1234);
            assert_eq!(word(payload, 8), 0);
            assert_eq!(word(payload, 12), 0);
        });
    }

    #[test]
    fn the_queue_mode_wrappers_fix_their_flags_and_modes() {
        let mut memory = memory();
        attach(&mut memory);
        put_word(&mut memory.context, 0x114, 0);
        memory.path[..4].copy_from_slice(b"C:\\\0");
        with_memory(&mut memory, Kernel::with_task(3), |context| {
            assert_eq!(helper_71c75c(context, 0x1000, 1, 2, 3), Ok(0));
            assert_eq!(context.effects.posts[0], 0x2c19 << 16 | 3 << 8 | 0x65);
            assert_eq!(context.effects.lengths[0], 28);
            assert_eq!(helper_71c7bc(context, 0x2000, 0, 0, 0), Ok(0));
            assert_eq!(context.effects.posts[1], 0x2c1a << 16 | 3 << 8 | 0x65);
            assert_eq!(context.effects.lengths[1], 40);
            assert_eq!(helper_71c938(context, 0x6000, 0x1234), Ok(0));
            assert_eq!(context.effects.posts[2], 0x2c1d << 16 | 3 << 8 | 0x65);
        });
    }
}
