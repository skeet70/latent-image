use super::*;
use core::ptr::{read_volatile, write_volatile};
extern "C" {
    fn xm1_power_interrupt_lock() -> u32;
    fn xm1_power_interrupt_unlock(saved: u32);
    fn xm1_rtos_semaphore_timed_wait(id: i32, timeout: i32) -> i32;
}
pub struct NativeMemory;
impl ConstructionMemory for NativeMemory {
    fn word(&self, address: Address) -> u32 { unsafe { read_volatile(address.0 as *const u32) } }
    fn put(&mut self, address: Address, value: u32) { unsafe { write_volatile(address.0 as *mut u32, value); } }
    fn byte(&self, address: Address) -> u8 { unsafe { read_volatile(address.0 as *const u8) } }
    fn put_byte(&mut self, address: Address, value: u8) { unsafe { write_volatile(address.0 as *mut u8, value); } }
    fn half(&self, address: Address) -> u16 { unsafe { read_volatile(address.0 as *const u16) } }
    fn put_half(&mut self, address: Address, value: u16) { unsafe { write_volatile(address.0 as *mut u16, value); } }
}
pub struct NativeIo;
pub struct NativeGuard(u32);
impl ConstructionIo for NativeIo {
    type Guard = NativeGuard;
    fn enter(&mut self) -> NativeGuard { NativeGuard(unsafe { xm1_power_interrupt_lock() }) }
    fn leave(&mut self, guard: NativeGuard) { unsafe { xm1_power_interrupt_unlock(guard.0); } }
    fn wait(&mut self, semaphore: SemaphoreHandle) -> ServiceStatus { ServiceStatus(unsafe { xm1_rtos_semaphore_timed_wait(semaphore.0, -1) }) }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_power_audio_controller_acquire(bank: u32, output: *mut u32) -> i32 {
    let bank = match bank { 0 => Bank::First, 1 => Bank::Second, _ => return -20 };
    controller_acquire(&mut NativeMemory, &mut NativeIo, bank, Address(output as u32)).0
}
#[no_mangle]
pub unsafe extern "C" fn xm1_power_audio_resource_acquire(configuration: *const u32) -> i32 {
    resource_acquire(&mut NativeMemory, &mut NativeIo, Address(configuration as u32)).0
}
#[no_mangle]
pub extern "C" fn xm1_power_audio_resource_configure() -> i32 { resource_configure(&mut NativeMemory).0 }
#[no_mangle]
pub unsafe extern "C" fn xm1_power_audio_pipeline_acquire(request: *mut u32, output: *mut u32) -> i32 {
    pipeline_acquire(&mut NativeMemory, &mut NativeIo, Address(request as u32), Address(output as u32)).0
}
#[no_mangle]
pub extern "C" fn xm1_power_audio_resource_release() -> i32 { resource_release(&mut NativeMemory).0 }
