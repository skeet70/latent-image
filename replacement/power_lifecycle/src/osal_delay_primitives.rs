use crate::callback_registry::{registry as callbacks, CallbackWord};
use core::ptr::{read_volatile, write_volatile};

unsafe fn read(at: u32) -> u32 { unsafe { read_volatile(at as *const u32) } }
unsafe fn write(at: u32, value: u32) { unsafe { write_volatile(at as *mut u32, value) }; }
unsafe fn core_id() -> u32 {
    let current: unsafe extern "C" fn() -> u32 = match callbacks().resolve(CallbackWord(read(read(0x96bd20) + 0x1618))) { Ok(entry) => entry, Err(_) => return 0 };
    unsafe { current() }
}
unsafe fn method(core: u32, offset: u32) -> u32 { unsafe { read(read(0x96bd20 + core * 4) + offset) } }
extern "C" { fn xm1_power_service_error(operation: u32, id: i32, status: i32); }

#[repr(C)]
struct SemaphoreCreation { attributes: u32, initial: u32, maximum: u32 }
#[derive(Clone, Copy)]
struct SemaphoreClass(u32);
impl SemaphoreClass {
    fn for_core(core: u32) -> Option<Self> { match core { 0 => Some(Self(1)), 1 => Some(Self(2)), _ => None } }
    fn encode(self, identifier: u32) -> u32 { identifier as u16 as u32 | self.0 << 16 }
}
unsafe fn create(core: u32, class: SemaphoreClass, identifier: u32, creation: &SemaphoreCreation) -> i32 {
    let call: unsafe extern "C" fn(u32, *const u32) -> i32 = match callbacks().resolve(CallbackWord(method(core, 0x64))) { Ok(entry) => entry, Err(error) => return error.status() };
    unsafe { call(class.encode(identifier), creation as *const SemaphoreCreation as *const u32) }
}
unsafe fn report(operation: u32, id: u32, status: i32) { unsafe { xm1_power_service_error(operation, id as i32, status) }; }

#[no_mangle]
pub unsafe extern "C" fn xm1_power_semaphore_create(identifier: u32, count: u32) -> i32 {
    let core = unsafe { core_id() };
    let Some(class) = SemaphoreClass::for_core(core) else {
        unsafe { report(21, identifier, -63) };
        return -63;
    };
    let creation = SemaphoreCreation { attributes: 0, initial: count, maximum: count };
    if identifier != 0 {
        let status = unsafe { create(core, class, identifier, &creation) };
        if status == 0 { unsafe { write(0x93e6f4 + identifier.wrapping_sub(1).wrapping_mul(4), class.0) }; }
        else { unsafe { report(21, identifier, status) }; }
        return status;
    }
    let mut candidate = unsafe { read(0x40a0f86c) } as i32;
    let mut status = -34;
    if candidate > 168 {
        loop {
            status = unsafe { create(core, class, candidate as u32, &creation) };
            if status == 0 {
                let mask: unsafe extern "C" fn() -> u32 = match callbacks().resolve(CallbackWord(method(core, 0x164))) { Ok(entry) => entry, Err(error) => return error.status() };
                let restore: unsafe extern "C" fn(u32) = match callbacks().resolve(CallbackWord(method(core, 0x168))) { Ok(entry) => entry, Err(error) => return error.status() };
                let saved = unsafe { mask() };
                unsafe {
                    write(0x93e6f4 + (candidate as u32 - 1) * 4, class.0);
                    write(0x40a0f86c, candidate as u32 - 1);
                    restore(saved);
                }
                return candidate;
            }
            candidate -= 1;
            if candidate == 168 { break; }
        }
    }
    unsafe { report(21, candidate as u32, status) };
    status
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_semaphore_poll(identifier: i32) -> i32 {
    let core = unsafe { core_id() };
    let slot = identifier as u16 as u32;
    let class = if identifier <= 169 { unsafe { read(0x93e6f4u32.wrapping_add(slot.wrapping_sub(1).wrapping_mul(4))) } } else { 1 };
    let call: unsafe extern "C" fn(u32) -> i32 = match callbacks().resolve(CallbackWord(method(core, 0x78))) { Ok(entry) => entry, Err(error) => return error.status() };
    let status = unsafe { call(slot | class << 16) };
    if status != 0 && status != -50 { unsafe { report(23, identifier as u32, status) }; }
    status
}

unsafe fn dispatch_control(offset: u32, operation: u32) -> i32 {
    let core = unsafe { core_id() };
    let call: unsafe extern "C" fn() -> i32 = match callbacks().resolve(CallbackWord(method(core, offset))) { Ok(entry) => entry, Err(error) => return error.status() };
    let status = unsafe { call() };
    if status != 0 {
        let current: unsafe extern "C" fn() -> u32 = match callbacks().resolve(CallbackWord(method(core, 0x16c))) { Ok(entry) => entry, Err(error) => return error.status() };
        unsafe { report(operation, current(), status) };
    }
    status
}
#[no_mangle]
pub unsafe extern "C" fn xm1_power_task_lock() -> i32 { unsafe { dispatch_control(0x140, 11) } }
#[no_mangle]
pub unsafe extern "C" fn xm1_power_task_unlock() -> i32 { unsafe { dispatch_control(0x144, 12) } }

#[derive(Clone, Copy)]
#[repr(transparent)]
struct TaskDelayTicks(u32);
#[no_mangle]
pub unsafe extern "C" fn xm1_power_task_delay(ticks: u32) -> i32 {
    let core = unsafe { core_id() };
    let ticks = TaskDelayTicks(ticks);
    if ticks.0 == 0 {
        let rotate: unsafe extern "C" fn(u32) -> i32 = match callbacks().resolve(CallbackWord(method(core, 0x11c))) { Ok(entry) => entry, Err(error) => return error.status() };
        let _ = unsafe { rotate(0) };
        return 0;
    }
    let delay: unsafe extern "C" fn(u32) -> i32 = match callbacks().resolve(CallbackWord(method(core, 0x4c))) { Ok(entry) => entry, Err(error) => return error.status() };
    let status = unsafe { delay(ticks.0) };
    if status != 0 { unsafe { report(9, ticks.0, status) }; }
    status
}
