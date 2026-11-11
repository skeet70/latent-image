use super::*;
use crate::hardware_timer_registry::HardwareTimerRequest;
use crate::queue_construction::QueueConfiguration;
use crate::shutdown_receive::{ReceiveDescriptor, ReceiveTimeout};
use crate::shutdown_rtos::NativeShutdownRtos;
use crate::task6_executor::native::TaskDescriptor;
use core::mem::MaybeUninit;

const SERVICE: u32 = 0x0096_bd40;
const ARGUMENTS: u32 = 0x0089_f770;
fn read(at: u32) -> u32 { unsafe { core::ptr::read_volatile(at as *const u32) } }
fn write(at: u32, value: u32) { unsafe { core::ptr::write_volatile(at as *mut u32, value) }; }
extern "C" {
    fn xm1_power_hardware_timer_acquire(selector: u32, handle: *mut u32) -> i32;
    fn xm1_power_hardware_timer_start(handle: u32, request: *const HardwareTimerRequest) -> i32;
    fn xm1_power_hardware_timer_stop(handle: u32) -> i32;
    fn xm1_power_hardware_timer_release(handle: u32) -> i32;
    fn xm1_power_queue_create(queue: u32, configuration: *const QueueConfiguration) -> i32;
    fn xm1_power_queue_delete(queue: u32) -> i32;
    fn xm1_task_submit(descriptor: u32) -> i32;
    fn xm1_task_release(index: u32) -> i32;
    fn xm1_power_software_timer_tick();
    fn xm1_power_command_diagnostic(class: u32, code: u32) -> i32;
}
fn invoke(address: u32) {
    if address != 0 {
        let callback: unsafe extern "C" fn() = unsafe { core::mem::transmute(address as usize) };
        unsafe { callback() };
    }
}
struct NativeCallbacks;
impl TimerServiceCallbacks for NativeCallbacks {
    fn timer_callback(&mut self) { invoke(read(SERVICE + 16)); }
    fn failed_post_callback(&mut self) { invoke(read(SERVICE + 20)); }
    fn ticks(&self) -> TimerServiceTicks { TimerServiceTicks(read(0x0096_e97c)) }
    fn set_ticks(&mut self, ticks: TimerServiceTicks) { write(0x0096_e97c, ticks.0); }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_software_timer_service_irq() {
    physical_tick(&mut NativeShutdownRtos, &mut NativeCallbacks);
}

fn start_timer(selector: u32, callback: u32, fallback: u32) -> i32 {
    let status = unsafe { xm1_power_hardware_timer_acquire(selector, (SERVICE + 12) as *mut u32) };
    if status != 0 { return status; }
    let request = HardwareTimerRequest { period: 2500, units: 1, mode: 1, callback: Some(xm1_power_software_timer_service_irq) };
    let status = unsafe { xm1_power_hardware_timer_start(read(SERVICE + 12), &request) };
    if status == 0 {
        write(SERVICE + 16, callback);
        write(SERVICE + 20, fallback);
        write(0x0096_e97c, 0);
    }
    status
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_software_timer_service_entry(argument: *const u32) -> ! {
    let selector = unsafe { core::ptr::read_volatile(argument) };
    let callback = unsafe { core::ptr::read_volatile(argument.add(2)) };
    let fallback = unsafe { core::ptr::read_volatile(argument.add(3)) };
    let _ = start_timer(selector, callback, fallback);
    let descriptor = unsafe { &mut *(SERVICE as *mut ReceiveDescriptor) };
    loop {
        let _ = unsafe { NativeShutdownRtos.receive(QueueId(9), descriptor, ReceiveTimeout::Forever) };
        consume(&mut NativeCallbacks);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_software_timer_service_start(selector: u32, priority: u32, callback: u32, fallback: u32) -> i32 {
    let status = unsafe { xm1_power_queue_create(9, &QueueConfiguration { flags: 48, message_size: 12 }) };
    if status != 0 { return -255; }
    write(ARGUMENTS, selector);
    write(ARGUMENTS + 4, priority);
    write(ARGUMENTS + 8, callback);
    write(ARGUMENTS + 12, fallback);
    let descriptor = TaskDescriptor {
        identifier: 9,
        name: 0x0078_1940,
        priority: 3,
        stack: 0xc00,
        entry: xm1_power_software_timer_service_entry,
        argument: ARGUMENTS as *const u32,
        reserved: MaybeUninit::uninit(),
    };
    if unsafe { xm1_task_submit(&descriptor as *const TaskDescriptor as u32) } == 0 { 0 } else { -255 }
}

#[no_mangle]
pub extern "C" fn xm1_power_software_timer_initialize(fallback: u32) -> i32 {
    if read(0x0095_ab1c) != 0 { return -255; }
    write(0x0095_ab1c, 1);
    let status = unsafe { xm1_power_software_timer_service_start(2, 7, xm1_power_software_timer_tick as usize as u32, fallback) };
    if status == 0 { 0 } else {
        let _ = unsafe { xm1_power_command_diagnostic(2, 0x1c) };
        -32
    }
}

#[no_mangle]
pub extern "C" fn xm1_power_software_timer_service_delete() -> i32 {
    let status = unsafe { xm1_power_hardware_timer_stop(read(SERVICE + 12)) };
    if status != 0 { return status; }
    let status = unsafe { xm1_power_hardware_timer_release(read(SERVICE + 12)) };
    if status != 0 { return status; }
    if unsafe { xm1_power_queue_delete(9) } != 0 { return -255; }
    if unsafe { xm1_task_release(9) } == 0 { 0 } else { -255 }
}

extern "C" {
    fn xm1_power_interrupt_lock() -> u32;
    fn xm1_power_interrupt_unlock(saved: u32);
    fn xm1_power_timer_irq_attach(source: u32, handler: unsafe extern "C" fn(), output: *mut u32) -> i32;
    fn xm1_power_timer_release_handle(handle: u32) -> i32;
}
struct NativeDetectionOutput;
impl DetectionOutputBackend for NativeDetectionOutput {
    type SavedCritical = u32;
    fn enter(&mut self) -> u32 { unsafe { xm1_power_interrupt_lock() } }
    fn leave(&mut self, saved: u32) { unsafe { xm1_power_interrupt_unlock(saved) }; }
    fn load(&mut self, field: DetectionField) -> u32 { read(field.address()) }
    fn store(&mut self, field: DetectionField, value: u32) { write(field.address(), value); }
    fn write_register(&mut self, register: DetectionRegister, value: u32) { write(register.address(), value); }
    fn read_register(&mut self, register: DetectionRegister) -> u32 { read(register.address()) }
    fn release_irq(&mut self, token: DetectionIrqToken) -> Result<(), ServiceStatus> {
        let status = unsafe { xm1_power_timer_release_handle(token.0) };
        if status == 0 { Ok(()) } else { Err(ServiceStatus(status)) }
    }
    fn attach_irq(&mut self, mode: DetectionMode) -> Result<DetectionIrqToken, ServiceStatus> {
        let handler: unsafe extern "C" fn() = match mode {
            DetectionMode::Off => detection_low_irq,
            DetectionMode::On | DetectionMode::Probe => detection_high_irq,
        };
        let mut token = 0;
        let status = unsafe { xm1_power_timer_irq_attach(mode.irq_source().index() as u32, handler, &mut token) };
        if status == 0 { Ok(DetectionIrqToken(token)) } else { Err(ServiceStatus(status)) }
    }
    fn invoke(&mut self, callback: DetectionCallback) { invoke(callback.0); }
}
unsafe extern "C" fn detection_low_irq() { detection_interrupt(&mut NativeDetectionOutput, DetectionMode::Off); }
unsafe extern "C" fn detection_high_irq() { detection_interrupt(&mut NativeDetectionOutput, DetectionMode::On); }
#[no_mangle]
pub extern "C" fn xm1_power_detection_initialize() { initialize_detection_output(&mut NativeDetectionOutput); }
#[no_mangle]
pub extern "C" fn xm1_power_detection_set_mode(mode: u32) -> i32 {
    let Some(mode) = DetectionMode::from_abi(mode) else { return -30; };
    match set_detection_mode(&mut NativeDetectionOutput, mode) { Ok(()) => 0, Err(status) => status.0 }
}
#[no_mangle]
pub extern "C" fn xm1_power_detection_output(mode: u32, callback: u32) -> i32 {
    let Some(mode) = DetectionMode::from_abi(mode) else { return -30; };
    let callback = if callback == 0 { None } else { Some(DetectionCallback(callback)) };
    match detection_output(&mut NativeDetectionOutput, mode, callback) { Ok(()) => 0, Err(status) => status.0 }
}
#[no_mangle]
pub extern "C" fn xm1_power_detection_output_continuous(mode: u32, callback: u32) -> i32 {
    let Some(mode) = DetectionMode::from_abi(mode) else { return -30; };
    let callback = if callback == 0 { None } else { Some(DetectionCallback(callback)) };
    match continuous_detection_output(&mut NativeDetectionOutput, mode, callback) { Ok(()) => 0, Err(status) => status.0 }
}
