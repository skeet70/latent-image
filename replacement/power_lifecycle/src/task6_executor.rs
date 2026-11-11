use crate::callback_registry::{CallbackError, CallbackWord};
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

/// Registers the executor tick so the timer entry resolves it.
pub fn register_executor_tick(tick: unsafe extern "C" fn()) -> Result<(), CallbackError> {
    crate::callback_registry::registry().ensure(tick)
}

/// The admission `xm1_power_executor_timer_start_callback` performs before it acquires the timer.
pub fn admit_timer_callback(callback: CallbackWord) -> Result<(), CallbackError> {
    crate::callback_registry::registry().resolve::<unsafe extern "C" fn()>(callback).map(|_| ())
}

#[cfg(target_arch = "arm")]
pub mod native {
    use super::*;
    use crate::callback_registry::{registry as callbacks, CallbackWord};
    use crate::hardware_timer_registry::HardwareTimerRequest;
    use crate::shutdown_receive::{ReceiveDescriptor, ReceiveTimeout};
    use crate::shutdown_rtos::NativeShutdownRtos;
    use crate::timer_abi;
    use core::{mem::MaybeUninit, ptr::{read_volatile, write_volatile}};
    use crate::queue_construction::QueueConfiguration;
    use crate::task_abi::{xm1_power_queue_create, xm1_power_queue_delete, xm1_task_release, xm1_task_submit, TaskDescriptor};
    extern "C" {
        fn xm1_power_audio_operation_tick();
    }
    struct NativeCallbacks;
    impl Task6Callbacks for NativeCallbacks {
        fn execute(&mut self) {
            let callback = unsafe { read_volatile(0x95dc18 as *const u32) };
            if let Ok(invoke) = callbacks().resolve::<unsafe extern "C" fn()>(CallbackWord(callback as usize)) { unsafe { invoke() }; }
        }
        fn failure(&mut self) {
            let callback = unsafe { read_volatile(0x95dc1c as *const u32) };
            if let Ok(invoke) = callbacks().resolve::<unsafe extern "C" fn()>(CallbackWord(callback as usize)) { unsafe { invoke() }; }
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
        if let Some(callback) = error_callback {
            if let Err(error) = admit_timer_callback(CallbackWord(callback as usize)) {
                return crate::callback_registry::refused(error);
            }
        }
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
    /// Takes the callback in r0; stock 0x69c430 takes a selector in r0 and the callback in r1.
    pub unsafe extern "C" fn xm1_power_executor_timer_start_callback(callback: unsafe extern "C" fn()) -> i32 {
        if let Err(error) = admit_timer_callback(CallbackWord(callback as usize)) { return crate::callback_registry::refused(error); }
        if let Err(error) = callbacks().ensure(xm1_power_task6_timer_irq as unsafe extern "C" fn()) { return crate::callback_registry::refused(error); }
        let handle = 0x95dc14 as *mut u32;
        let status = unsafe { timer_abi::xm1_power_hardware_timer_acquire(1, handle) };
        if status != 0 { return status; }
        let request = HardwareTimerRequest { period: 1000, units: 1, mode: 1, callback: xm1_power_task6_timer_irq as usize as u32 };
        let status = unsafe { timer_abi::xm1_power_hardware_timer_start(read_volatile(handle), &request) };
        if status == 0 {
            unsafe { write_volatile(0x95dc18 as *mut u32, callback as usize as u32) };
        }
        status
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_executor_timer_stop() -> i32 {
        let handle = unsafe { read_volatile(0x95dc14 as *const u32) };
        let status = unsafe { timer_abi::xm1_power_hardware_timer_stop(handle) };
        if status != 0 { return status; }
        unsafe { timer_abi::xm1_power_hardware_timer_release(handle) }
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_executor_timer_start_owned() -> i32 {
        if let Err(error) = register_executor_tick(xm1_power_audio_operation_tick as unsafe extern "C" fn()) { return crate::callback_registry::refused(error); }
        unsafe { xm1_power_executor_timer_start_callback(xm1_power_audio_operation_tick) }
    }
}

#[cfg(test)]
#[path = "task6_executor_tests.rs"]
mod tests;
