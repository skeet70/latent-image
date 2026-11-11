use crate::shutdown::RequestId;
use crate::storage_shutdown_lens_service::LensCallback;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct LensPendingCommand {
    tagged_operation: u32,
    pub command: u32,
    pub callback: LensCallback,
    skipped: u32,
}
impl LensPendingCommand {
    pub const fn new(operation: RequestId, last: bool, command: u32, callback: LensCallback, skipped: bool) -> Self {
        Self { tagged_operation: operation.0 as u32 | ((last as u32) << 16), command, callback, skipped: skipped as u32 }
    }
    pub const fn operation(self) -> RequestId { RequestId(self.tagged_operation as u16) }
    pub const fn is_last(self) -> bool { self.tagged_operation >> 16 != 0 }
    pub const fn is_skipped(self) -> bool { self.skipped != 0 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RingFull;

pub struct LensCommandRing<'a> {
    slots: &'a mut [LensPendingCommand; 16],
    write: &'a mut u32,
    read: &'a mut u32,
    count: &'a mut u32,
}
impl<'a> LensCommandRing<'a> {
    pub fn new(slots: &'a mut [LensPendingCommand; 16], write: &'a mut u32, read: &'a mut u32, count: &'a mut u32) -> Self {
        Self { slots, write, read, count }
    }
    pub fn clear(&mut self) { *self.write = 0; *self.read = 0; *self.count = 0; }
    pub fn push(&mut self, command: LensPendingCommand) -> Result<(), RingFull> {
        if *self.count >= 1 && self.write == self.read { return Err(RingFull); }
        self.slots[*self.write as usize] = command;
        *self.write = if *self.write < 15 { *self.write + 1 } else { 0 };
        *self.count += 1;
        Ok(())
    }
    pub fn peek(&self) -> Option<LensPendingCommand> {
        if *self.count == 0 { None } else { Some(self.slots[*self.read as usize]) }
    }
    pub fn pop(&mut self) -> Option<LensPendingCommand> {
        let command = self.peek()?;
        *self.read = if *self.read < 15 { *self.read + 1 } else { 0 };
        *self.count -= 1;
        Some(command)
    }
}

#[cfg(target_arch = "arm")]
pub mod native {
    use super::*;
    extern "C" {
        fn xm1_power_interrupt_lock() -> u32;
        fn xm1_power_interrupt_unlock(saved: u32);
    }
    unsafe fn adopt<'a>() -> LensCommandRing<'a> {
        unsafe {
            LensCommandRing::new(
                &mut *(0x9737b8 as *mut [LensPendingCommand; 16]),
                &mut *((0x9737b8 + 0x100) as *mut u32),
                &mut *((0x9737b8 + 0x104) as *mut u32),
                &mut *((0x9737b8 + 0x108) as *mut u32),
            )
        }
    }
    pub unsafe fn clear() {
        let saved = unsafe { xm1_power_interrupt_lock() };
        unsafe { adopt().clear(); xm1_power_interrupt_unlock(saved); }
    }
    pub unsafe fn push(command: LensPendingCommand) -> Result<(), RingFull> {
        let saved = unsafe { xm1_power_interrupt_lock() };
        let result = unsafe { adopt().push(command) };
        unsafe { xm1_power_interrupt_unlock(saved) };
        result
    }
    pub unsafe fn pop() -> Option<LensPendingCommand> {
        let saved = unsafe { xm1_power_interrupt_lock() };
        let result = unsafe { adopt().pop() };
        unsafe { xm1_power_interrupt_unlock(saved) };
        result
    }
    pub unsafe fn peek() -> Option<LensPendingCommand> {
        let saved = unsafe { xm1_power_interrupt_lock() };
        let result = unsafe { adopt().peek() };
        unsafe { xm1_power_interrupt_unlock(saved) };
        result
    }
}

#[cfg(test)]
#[path = "storage_shutdown_lens_ring_tests.rs"]
mod tests;
