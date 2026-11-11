use crate::shutdown::{KernelStatus, QueueId, RequestId, ServiceMessage, ShutdownRtos};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Task6Command { Execute }
impl Task6Command {
    pub const fn from_abi(word: u16) -> Option<Self> { match word { 0x1600 => Some(Self::Execute), _ => None } }
    pub const fn word(self) -> u16 { match self { Self::Execute => 0x1600 } }
}
pub trait Task6Callbacks {
    fn execute(&mut self);
    fn failure(&mut self);
}
pub fn notify(rtos: &mut impl ShutdownRtos, callbacks: &mut impl Task6Callbacks) -> Result<(), KernelStatus> {
    let sender = rtos.current_task();
    let result = rtos.post(ServiceMessage { operation: RequestId(Task6Command::Execute.word()), sender, destination: QueueId(6), payload: &[] });
    if result.is_err() { callbacks.failure(); }
    result
}
pub fn dispatch(callbacks: &mut impl Task6Callbacks, operation: RequestId) -> Result<(), KernelStatus> {
    match Task6Command::from_abi(operation.0) { Some(Task6Command::Execute) => { callbacks.execute(); Ok(()) }, None => Err(KernelStatus(-255)) }
}
pub fn consume(callbacks: &mut impl Task6Callbacks, receive: Result<(), KernelStatus>, residue: RequestId) {
    if receive.is_err() { callbacks.failure(); }
    if dispatch(callbacks, residue).is_err() { callbacks.failure(); }
}

#[cfg(target_arch = "arm")]
pub mod native {
    use super::*;
    use crate::shutdown_receive::{ReceiveDescriptor, ReceiveTimeout};
    use crate::shutdown_rtos::NativeShutdownRtos;
    use core::{mem::MaybeUninit, ptr::{read_volatile, write_volatile}};
    #[repr(C)]
    struct TimerRequest { period: u32, units: u32, mode: u32, callback: Option<unsafe extern "C" fn()> }
    #[repr(C)]
    pub struct TaskDescriptor { pub identifier: u32, pub name: u32, pub priority: u32, pub stack: u32, pub entry: unsafe extern "C" fn(*const u32) -> !, pub argument: *const u32, pub reserved: MaybeUninit<u32> }
    #[repr(C)]
    pub struct QueueConfiguration { pub flags: u32, pub message_size: u32 }
    extern "C" {
        fn xm1_power_hardware_timer_acquire(selector: u32, handle: *mut u32) -> i32;
        fn xm1_power_hardware_timer_start(handle: u32, request: *const TimerRequest) -> i32;
        fn xm1_power_hardware_timer_stop(handle: u32) -> i32;
        fn xm1_power_hardware_timer_release(handle: u32) -> i32;
        fn xm1_power_audio_operation_tick();
        fn xm1_power_queue_create(queue: u32, configuration: *const QueueConfiguration) -> i32;
        fn xm1_task_submit(descriptor: u32) -> i32;
        fn xm1_power_queue_delete(queue: u32) -> i32;
        fn xm1_task_release(index: u32) -> i32;
    }
    struct NativeCallbacks;
    impl Task6Callbacks for NativeCallbacks {
        fn execute(&mut self) {
            let callback = unsafe { read_volatile(0x95dc18 as *const u32) };
            if callback != 0 {
                let invoke: unsafe extern "C" fn() = unsafe { core::mem::transmute(callback as usize) };
                unsafe { invoke() };
            }
        }
        fn failure(&mut self) {
            let callback = unsafe { read_volatile(0x95dc1c as *const u32) };
            if callback != 0 {
                let invoke: unsafe extern "C" fn() = unsafe { core::mem::transmute(callback as usize) };
                unsafe { invoke() };
            }
        }
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_task6_timer_irq() { let _ = notify(&mut NativeShutdownRtos, &mut NativeCallbacks); }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_task6_entry(argument: *const u32) -> ! {
        unsafe { write_volatile(0x95dc1c as *mut u32, read_volatile(argument)) };
        let descriptor = unsafe { &mut *(0x95dc08 as *mut ReceiveDescriptor) };
        loop {
            let received = unsafe { NativeShutdownRtos.receive(QueueId(6), descriptor, ReceiveTimeout::Forever) };
            consume(&mut NativeCallbacks, received, descriptor.header().operation);
        }
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_task6_start(error_callback: Option<unsafe extern "C" fn()>) -> i32 {
        if unsafe { xm1_power_queue_create(6, &QueueConfiguration { flags: 0x30, message_size: 12 }) } != 0 { return -255; }
        unsafe { write_volatile(0x89f76c as *mut u32, error_callback.map_or(0, |callback| callback as usize as u32)) };
        let descriptor = TaskDescriptor { identifier: 6, name: 0x781938, priority: 3, stack: 0xc00, entry: xm1_power_task6_entry, argument: 0x89f76c as *const u32, reserved: MaybeUninit::uninit() };
        if unsafe { xm1_task_submit(&descriptor as *const TaskDescriptor as u32) } == 0 { 0 } else { -255 }
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_task6_delete() -> i32 {
        if unsafe { xm1_power_queue_delete(6) } != 0 { return -255; }
        if unsafe { xm1_task_release(6) } == 0 { 0 } else { -255 }
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_executor_timer_start(callback: unsafe extern "C" fn()) -> i32 {
        let handle = 0x95dc14 as *mut u32;
        let status = unsafe { xm1_power_hardware_timer_acquire(1, handle) };
        if status != 0 { return status; }
        let request = TimerRequest { period: 1000, units: 1, mode: 1, callback: Some(xm1_power_task6_timer_irq) };
        let status = unsafe { xm1_power_hardware_timer_start(read_volatile(handle), &request) };
        if status == 0 { unsafe { write_volatile(0x95dc18 as *mut u32, callback as usize as u32) }; }
        status
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_executor_timer_stop() -> i32 {
        let handle = unsafe { read_volatile(0x95dc14 as *const u32) };
        let status = unsafe { xm1_power_hardware_timer_stop(handle) };
        if status != 0 { return status; }
        unsafe { xm1_power_hardware_timer_release(handle) }
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_executor_timer_start_owned() -> i32 { unsafe { xm1_power_executor_timer_start(xm1_power_audio_operation_tick) } }
}

#[cfg(test)]
#[path = "task6_executor_tests.rs"]
mod tests;
