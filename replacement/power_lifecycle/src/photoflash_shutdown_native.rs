use super::*;
use crate::callback_registry::registry as callbacks;
use crate::shutdown::{QueueId, RequestId, ServiceMessage, ShutdownRtos};
use crate::shutdown_receive::{ReceiveDescriptor, ReceiveTimeout};
use crate::shutdown_rtos::NativeShutdownRtos;
use core::ptr::{read_volatile, write_volatile};

extern "C" {
    fn xm1_power_software_timer_cancel(handle: u32) -> i32;
    fn xm1_power_software_timer_start(period: u32, mode: u32, callback: u32, token: *mut u32) -> i32;
    fn xm1_power_owned_callback_register(word: u32, shape: u32) -> i32;
    fn xm1_power_timer_release_handle(handle: u32) -> i32;
    fn xm1_power_interrupt_lock() -> u32;
    fn xm1_power_interrupt_unlock(saved: u32);
    fn xm1_power_command_diagnostic(category: u32, code: u32) -> i32;
    fn xm1_power_append_stream_text(task: u32, text: u32);
}
pub struct NativePhotoflash;
impl Runtime for NativePhotoflash {
    fn read8(&self, address: Address) -> u8 { unsafe { read_volatile(address.0 as *const u8) } }
    fn read16(&self, address: Address) -> u16 { unsafe { read_volatile(address.0 as *const u16) } }
    fn read32(&self, address: Address) -> u32 { unsafe { read_volatile(address.0 as *const u32) } }
    fn write8(&mut self, address: Address, value: u8) { unsafe { write_volatile(address.0 as *mut u8, value); } }
    fn write16(&mut self, address: Address, value: u16) { unsafe { write_volatile(address.0 as *mut u16, value); } }
    fn write32(&mut self, address: Address, value: u32) { unsafe { write_volatile(address.0 as *mut u32, value); } }
    fn cancel_software_timer(&mut self, handle: TimerHandle) -> i32 { unsafe { xm1_power_software_timer_cancel(handle.0) } }
    fn stop_device_timer(&mut self, handle: TimerHandle) -> i32 { timer::stop(self, handle) }
    fn delete_device_timer(&mut self, handle: TimerHandle) -> i32 { timer::delete(self, handle) }
    fn invoke(&mut self, callback: Callback, argument: Option<Address>) {
        if let Some(argument) = argument {
            let call: unsafe extern "C" fn(*const u32) = unsafe { core::mem::transmute(callback.0) };
            unsafe { call(argument.0 as *const u32); }
        } else {
            let _ = invoke_completion(callbacks(), callback);
        }
    }
    fn log_stop(&mut self, message: Address) { unsafe { xm1_power_append_stream_text(10, message.0); } }
}
impl timer::TimerRuntime for NativePhotoflash {
    fn release_irq_handle(&mut self, handle: TimerHandle) -> i32 { unsafe { xm1_power_timer_release_handle(handle.0) } }
    fn lock_interrupts(&mut self) -> timer::SavedInterrupts { timer::SavedInterrupts(unsafe { xm1_power_interrupt_lock() }) }
    fn unlock_interrupts(&mut self, saved: timer::SavedInterrupts) { unsafe { xm1_power_interrupt_unlock(saved.0); } }
    fn timer_diagnostic(&mut self, category: u32, code: u32) { unsafe { let _ = xm1_power_command_diagnostic(category, code); } }
}

#[no_mangle]
pub extern "C" fn xm1_power_photoflash_poll_timer() {
    let mut rtos = NativeShutdownRtos;
    let sender = rtos.current_task();
    let _ = rtos.post(ServiceMessage { operation: RequestId(0x1a18), sender, destination: QueueId(10), payload: &[] });
}
#[no_mangle]
pub extern "C" fn xm1_power_photoflash_reset() { reset(&mut NativePhotoflash); }
#[no_mangle]
pub extern "C" fn xm1_power_photoflash_initialize(completion: u32) -> i32 {
    reset(&mut NativePhotoflash);
    NativePhotoflash.write32(at(CONTEXT, 0x60), completion);
    let registered = unsafe { xm1_power_owned_callback_register(xm1_power_photoflash_poll_timer as *const () as u32, crate::callback_registry::COMPLETION_SHAPE) };
    if registered != 0 {
        return registered;
    }
    unsafe { xm1_power_software_timer_start(50, 1, xm1_power_photoflash_poll_timer as *const () as u32, at(CONTEXT, 0x68).0 as *mut u32) }
}
#[no_mangle]
pub extern "C" fn xm1_power_photoflash_receive() -> i32 {
    let descriptor = unsafe { &mut *(CONTEXT.0 as *mut ReceiveDescriptor) };
    match unsafe { NativeShutdownRtos.receive(QueueId(10), descriptor, ReceiveTimeout::Forever) } {
        Ok(()) => 0,
        Err(status) => { receive_failure(&mut NativePhotoflash); status.0 },
    }
}
#[no_mangle]
pub extern "C" fn xm1_power_photoflash_dispatch_shutdown(configuration: u32) -> i32 {
    match dispatch_shutdown(&mut NativePhotoflash, Address(configuration)) { Ok(()) => 0, Err(_) => -255 }
}
#[no_mangle]
pub extern "C" fn xm1_power_photoflash_timer_stop(handle: u32) -> i32 { timer::stop(&mut NativePhotoflash, TimerHandle(handle)) }
#[no_mangle]
pub extern "C" fn xm1_power_photoflash_timer_delete(handle: u32) -> i32 { timer::delete(&mut NativePhotoflash, TimerHandle(handle)) }
