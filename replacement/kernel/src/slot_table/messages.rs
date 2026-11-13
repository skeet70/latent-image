use super::{block, Guest, StockGuest};
use crate::{Error, FlagBits, Form, Header, MessageBlock, PostError, QueueConfig, QueueId, SemaphoreId, Status, TaskId, Timeout};
use core::marker::PhantomData;

pub(super) const CLASSES: u32 = 0x7cad9c;
pub(super) const CAPACITIES: u32 = 0x40a0fa50;
pub(super) const POST_HOOK: u32 = 0x4089ea94;

#[derive(Clone, Copy)]
struct StockCodeWord(u32);
impl StockCodeWord {
    unsafe fn call(self, first: u32, second: u32, third: u32) -> u32 {
        let entry: unsafe extern "C" fn(u32, u32, u32) -> u32 = unsafe { core::mem::transmute(self.0 as usize) };
        unsafe { entry(first, second, third) }
    }
}

fn slot(root: u32, offset: u32) -> StockCodeWord {
    StockCodeWord(unsafe { StockGuest.read32(root + offset) })
}

fn queue_word(id: QueueId) -> u32 {
    object_word(u32::from(id.get()))
}

fn object_word(number: u32) -> u32 {
    let class = unsafe { StockGuest.read32(CLASSES + 4 * (number - 1)) };
    (class << 16) | number
}

fn capacity(id: QueueId) -> usize {
    unsafe { StockGuest.read32(CAPACITIES + 4 * (u32::from(id.get()) - 1)) }.saturating_sub(8) as usize
}

fn report(kind: u32, number: u32, status: Status) {
    unsafe {
        StockGuest.console(kind, number, status.0);
        StockGuest.write32(super::ERROR_STATUS, status.0 as u32);
        StockCodeWord(StockGuest.read32(super::ERROR_CALLBACK)).call(status.0 as u32, 0, 0);
    }
}

#[repr(C)]
struct Descriptor {
    operation: u16,
    sender: u8,
    destination: u8,
    length: u32,
    payload: *const u8,
}

pub(super) fn post(id: QueueId, form: Form, header: Header, payload: &[u8]) -> Result<(), PostError> {
    let root = block(&StockGuest);
    if payload.len() > capacity(id) {
        report(28, u32::from(id.get()), Status(-60));
        return Err(PostError::TooLong);
    }
    let mut descriptor = Descriptor {
        operation: header.operation,
        sender: header.sender,
        destination: header.destination,
        length: payload.len() as u32,
        payload: payload.as_ptr(),
    };
    let hook = StockCodeWord(unsafe { StockGuest.read32(POST_HOOK) });
    unsafe { hook.call((&mut descriptor as *mut Descriptor) as u32, 0, 0) };
    let object = queue_word(id);
    let mut address = 0u32;
    let allocation = Status(unsafe { slot(root, 0x170).call(object, (&mut address as *mut u32) as u32, 0) } as i32);
    if let Err(error) = allocation.check() {
        let status = if error == Error::TimedOut { Status(-62) } else { allocation };
        report(28, u32::from(id.get()), status);
        return Err(if error == Error::TimedOut { PostError::NoBlock } else { PostError::Kernel(error) });
    }
    unsafe {
        core::ptr::write_volatile((address + 4) as *mut u16, descriptor.operation);
        core::ptr::write_volatile((address + 6) as *mut u8, descriptor.sender);
        core::ptr::write_volatile((address + 7) as *mut u8, descriptor.destination);
        core::ptr::write_volatile((address + 8) as *mut u32, descriptor.length);
        if descriptor.length != 0 {
            core::ptr::copy_nonoverlapping(descriptor.payload, (address + 12) as *mut u8, descriptor.length as usize);
        }
    }
    let status = Status(unsafe { slot(root, match form { Form::Task => 0xac, Form::Handler => 0xb0 }).call(object, address, 0) } as i32);
    if let Err(error) = status.check() {
        unsafe { StockGuest.console(28, u32::from(id.get()), status.0) };
        unsafe { slot(root, 0xf8).call(object, address, 0) };
        unsafe {
            StockGuest.write32(super::ERROR_STATUS, status.0 as u32);
            StockCodeWord(StockGuest.read32(super::ERROR_CALLBACK)).call(status.0 as u32, 0, 0);
        }
        return Err(PostError::Kernel(error));
    }
    Ok(())
}

pub struct ReceivedBlock {
    root: u32,
    object: u32,
    address: u32,
    length: usize,
    _not_send: PhantomData<*mut ()>,
}
impl MessageBlock for ReceivedBlock {
    fn header(&self) -> Header {
        unsafe {
            Header {
                operation: core::ptr::read_volatile((self.address + 4) as *const u16),
                sender: core::ptr::read_volatile((self.address + 6) as *const u8),
                destination: core::ptr::read_volatile((self.address + 7) as *const u8),
            }
        }
    }
    fn payload(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts((self.address + 12) as *const u8, self.length) }
    }
}
impl Drop for ReceivedBlock {
    fn drop(&mut self) { unsafe { slot(self.root, 0xf8).call(self.object, self.address, 0) }; }
}

pub(super) fn receive(id: QueueId, timeout: Timeout) -> Result<ReceivedBlock, Error> {
    let root = block(&StockGuest);
    let object = queue_word(id);
    let mut address = 0u32;
    let status = Status(unsafe { slot(root, 0xc8).call(object, (&mut address as *mut u32) as u32, timeout.word()) } as i32);
    if let Err(error) = status.check() {
        if error != Error::TimedOut { report(31, u32::from(id.get()), status); }
        return Err(error);
    }
    let length = unsafe { StockGuest.read32(address + 8) } as usize;
    let message = ReceivedBlock { root, object, address, length, _not_send: PhantomData };
    if length > capacity(id) { return Err(Error::Overflow); }
    Ok(message)
}

pub(super) fn create(id: QueueId, config: QueueConfig) -> Result<(), Error> {
    let descriptor = [config.depth(), config.message_bytes()];
    Status(unsafe { StockCodeWord(0x724cdc).call(u32::from(id.get()), descriptor.as_ptr() as u32, 0) } as i32).check()
}

pub(super) fn delete(id: QueueId) -> Result<(), Error> {
    Status(unsafe { StockCodeWord(0x724f08).call(u32::from(id.get()), 0, 0) } as i32).check()
}

pub(super) fn semaphore_create_binary() -> Result<SemaphoreId, Error> {
    let word = unsafe { StockCodeWord(0x7247ac).call(0, 1, 0) } as i32;
    if word > 0 {
        u8::try_from(word).ok().and_then(SemaphoreId::new).ok_or(Error::InvalidId)
    } else {
        Err(Status(word).check().err().unwrap_or(Error::InvalidId))
    }
}

pub(super) fn semaphore_delete(id: SemaphoreId) -> Result<(), Error> {
    Status(unsafe { StockCodeWord(0x724974).call(u32::from(id.get()), 0, 0) } as i32).check()
}

pub(super) fn semaphore_try_take(id: SemaphoreId) -> Result<(), Error> {
    Status(unsafe { StockCodeWord(0x724ad8).call(u32::from(id.get()), 0, 0) } as i32).check()
}

pub(super) fn flags_wait(id: TaskId, bits: FlagBits, timeout: Timeout) -> Result<FlagBits, Error> {
    let root = block(&StockGuest);
    let mut pattern = 0u32;
    let entry: unsafe extern "C" fn(u32, u32, u32, *mut u32, u32) -> u32 = unsafe { core::mem::transmute(slot(root, 0x9c).0 as usize) };
    let status = Status(unsafe { entry(object_word(u32::from(id.get())), bits.0, 1, &mut pattern, timeout.word()) } as i32);
    if let Err(error) = status.check() {
        if error != Error::TimedOut { report(17, u32::from(id.get()), status); }
        return Err(error);
    }
    Ok(FlagBits(pattern))
}

pub(super) fn flags_clear(id: TaskId) -> Result<(), Error> {
    let root = block(&StockGuest);
    let status = Status(unsafe { slot(root, 0x90).call(object_word(u32::from(id.get())), 0, 0) } as i32);
    if status.0 != 0 { report(20, u32::from(id.get()), status); }
    status.check()
}

pub(super) fn flags_set(id: TaskId, bits: FlagBits, form: Form) -> Result<(), Error> {
    let root = block(&StockGuest);
    let status = Status(unsafe { slot(root, match form { Form::Task => 0x88, Form::Handler => 0x8c }).call(object_word(u32::from(id.get())), bits.0, 0) } as i32);
    if status.0 != 0 { report(19, u32::from(id.get()), status); }
    status.check()
}

pub(super) fn dispatch_disable() -> Result<(), Error> {
    Status(unsafe { StockCodeWord(0x723efc).call(0, 0, 0) } as i32).check()
}

pub(super) fn dispatch_enable() -> Result<(), Error> {
    Status(unsafe { StockCodeWord(0x723f7c).call(0, 0, 0) } as i32).check()
}
