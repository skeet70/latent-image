use super::*;
use crate::shutdown::ShutdownRtos;
use crate::shutdown_rtos::NativeShutdownRtos;
use crate::shutdown_receive::{ReceiveDescriptor, ReceiveTimeout};

const STATE: u32 = 0x0093_ad40;
const SOFTWARE: u32 = 0x0095_aa9c;
fn read(at: u32) -> u32 { unsafe { core::ptr::read_volatile(at as *const u32) } }
fn write(at: u32, value: u32) { unsafe { core::ptr::write_volatile(at as *mut u32, value) }; }
extern "C" {
    fn xm1_app_register_callback(period: u32, mode: i32, callback: u32, token: *mut i32) -> i32;
    fn xm1_app_unregister_callback(token: i32) -> i32;
    fn xm1_power_interrupt_lock() -> u32;
    fn xm1_power_interrupt_unlock(saved: u32);
    fn xm1_power_command_diagnostic(class: u32, code: u32) -> i32;
    fn xm1_power_unregister_registry_handle(handle: u32) -> i32;
}

struct NativeTimers;
impl DeviceTimers for NativeTimers {
    fn requested_index(&self) -> u32 { read(STATE + 12) }
    fn requested_period(&self) -> TimerPeriod { TimerPeriod(read(STATE + 16)) }
    fn requested_callback(&self) -> TimerCallback { TimerCallback(read(STATE + 20)) }
    fn handle(&self, index: DeviceTimerIndex) -> TimerHandle { TimerHandle(read(STATE + 44 + index.index() as u32 * 4)) }
    fn period(&self, index: DeviceTimerIndex) -> TimerPeriod { TimerPeriod(read(STATE + 24 + index.index() as u32 * 4)) }
    fn callback(&self, index: DeviceTimerIndex) -> TimerCallback { TimerCallback(read(STATE + 64 + index.index() as u32 * 4)) }
    fn set_handle(&mut self, index: DeviceTimerIndex, handle: TimerHandle) { write(STATE + 44 + index.index() as u32 * 4, handle.0); }
    fn set_period(&mut self, index: DeviceTimerIndex, period: TimerPeriod) { write(STATE + 24 + index.index() as u32 * 4, period.0); }
    fn set_callback(&mut self, index: DeviceTimerIndex, callback: TimerCallback) { write(STATE + 64 + index.index() as u32 * 4, callback.0); }
    fn register_cyclic(&mut self, index: DeviceTimerIndex, period: TimerPeriod) -> (ServiceStatus, TimerHandle) {
        let mut token = self.handle(index).0 as i32;
        let status = unsafe { xm1_app_register_callback(period.0, 0, producers()[index.index()] as usize as u32, &mut token) };
        (ServiceStatus(status), TimerHandle(token as u32))
    }
    fn register_software(&mut self, index: DeviceTimerIndex, period: TimerPeriod) -> (ServiceStatus, TimerHandle) {
        software_start(period, SoftwareTimerMode::from_abi(1), TimerCallback(producers()[index.index()] as usize as u32), self.handle(index), |handle| write(STATE + 44 + index.index() as u32 * 4, handle.0))
    }
    fn unregister_cyclic(&mut self, handle: TimerHandle) -> ServiceStatus { ServiceStatus(unsafe { xm1_app_unregister_callback(handle.0 as i32) }) }
    fn unregister_software(&mut self, handle: TimerHandle) -> ServiceStatus { software_cancel(handle) }
    fn invoke(&mut self, callback: TimerCallback) {
        let call: extern "C" fn() = unsafe { core::mem::transmute(callback.0) };
        call();
    }
}

fn software_cancel(handle: TimerHandle) -> ServiceStatus {
    ServiceStatus(unsafe { xm1_power_unregister_registry_handle(handle.0) })
}

#[no_mangle]
pub extern "C" fn xm1_power_software_timer_cancel(handle: u32) -> i32 {
    software_cancel(TimerHandle(handle)).0
}

fn software_start(period: TimerPeriod, mode: SoftwareTimerMode, callback: TimerCallback, previous: TimerHandle, publish: impl FnOnce(TimerHandle)) -> (ServiceStatus, TimerHandle) {
    if read(SOFTWARE + 0x80) == 0 {
        let _ = unsafe { xm1_power_command_diagnostic(2, 0x1c) };
        return (ServiceStatus(-255), previous);
    }
    let saved = unsafe { xm1_power_interrupt_lock() };
    let index = (0..32).find(|index| read(SOFTWARE + 0x84 + index * 20) == 0);
    if let Some(index) = index { write(SOFTWARE + 0x84 + index * 20, 2); }
    unsafe { xm1_power_interrupt_unlock(saved) };
    let Some(index) = index else {
        let _ = unsafe { xm1_power_command_diagnostic(2, 0x1f) };
        return (ServiceStatus(-20), previous);
    };
    let saved = unsafe { xm1_power_interrupt_lock() };
    if read(SOFTWARE + index * 4) != 0 {
        unsafe { xm1_power_interrupt_unlock(saved) };
        return (ServiceStatus(-20), previous);
    }
    let handle = loop {
        let next = read(0x409c_2804).wrapping_add(1);
        let next = if next == 0 { 1 } else { next };
        write(0x409c_2804, next);
        if !(0..32).any(|slot| read(SOFTWARE + slot * 4) == next) { break TimerHandle(next); }
    };
    write(SOFTWARE + index * 4, handle.0);
    unsafe { xm1_power_interrupt_unlock(saved) };
    publish(handle);
    let schedule = software_ticks(period);
    let slot = SOFTWARE + 0x84 + index * 20;
    write(slot + 4, schedule.initial.0);
    write(slot + 8, schedule.reload.0);
    write(slot + 12, mode.abi());
    write(slot + 16, callback.0);
    let saved = unsafe { xm1_power_interrupt_lock() };
    write(slot, 1);
    unsafe { xm1_power_interrupt_unlock(saved) };
    (ServiceStatus(0), handle)
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_software_timer_start(period: u32, mode: u32, callback: u32, token: *mut u32) -> i32 {
    let (status, _) = software_start(TimerPeriod(period), SoftwareTimerMode::from_abi(mode), TimerCallback(callback), TimerHandle(0), |handle| unsafe { core::ptr::write_volatile(token, handle.0) });
    status.0
}

fn signal(index: u16) {
    let mut rtos = NativeShutdownRtos;
    let sender = rtos.current_task();
    let _ = rtos.post(ServiceMessage { operation: RequestId(0x3502 + index), sender, destination: QueueId(37), payload: &[] });
}
extern "C" fn signal0() { signal(0); }
extern "C" fn signal1() { signal(1); }
extern "C" fn signal2() { signal(2); }
extern "C" fn signal3() { signal(3); }
extern "C" fn signal4() { signal(4); }
fn producers() -> [extern "C" fn(); 5] { [signal0, signal1, signal2, signal3, signal4] }

#[no_mangle]
pub extern "C" fn xm1_power_device_timer_unregister(index: u32) -> i32 {
    post_unregister(&mut NativeShutdownRtos, index).0
}

#[no_mangle]
pub extern "C" fn xm1_power_device_timer_dispatch() -> i32 {
    let code = unsafe { core::ptr::read_volatile(STATE as *const u16) };
    match DeviceTimerOperation::from_abi(code) {
        Some(operation) => dispatch(&mut NativeTimers, operation).0,
        None => -255,
    }
}

#[no_mangle]
pub extern "C" fn xm1_power_device_timer_receive() -> i32 {
    let descriptor = unsafe { &mut *(STATE as *mut ReceiveDescriptor) };
    match unsafe { NativeShutdownRtos.receive(QueueId(37), descriptor, ReceiveTimeout::Forever) } {
        Ok(()) => 0,
        Err(status) => status.0,
    }
}

#[no_mangle]
pub extern "C" fn xm1_power_device_timer_task() -> ! {
    write(STATE + 8, STATE + 12);
    loop {
        let _ = xm1_power_device_timer_receive();
        let _ = xm1_power_device_timer_dispatch();
    }
}

#[no_mangle]
pub extern "C" fn xm1_power_software_timer_tick() {
    if read(SOFTWARE + 0x80) != 1 { return; }
    for index in 0..32 {
        let slot = SOFTWARE + 0x84 + index * 20;
        if read(slot) != 1 { continue; }
        let next = read(slot + 4).wrapping_sub(1);
        write(slot + 4, next);
        if next != 0 { continue; }
        let callback = read(slot + 16);
        if callback != 0 {
            let call: extern "C" fn() = unsafe { core::mem::transmute(callback) };
            call();
        }
        if SoftwareTimerMode::from_abi(read(slot + 12)).is_periodic() {
            write(slot + 4, read(slot + 8));
        } else {
            write(SOFTWARE + index * 4, 0);
            write(slot, 0);
        }
    }
}
