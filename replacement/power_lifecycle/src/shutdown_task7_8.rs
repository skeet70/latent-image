use crate::primitives::{destroy_queue, QueueRtos, QueueStorage, ServiceStatus};
use crate::shutdown::{KernelStatus, QueueId, RequestId, ServiceMessage, ShutdownRtos, TaskId};
use crate::shutdown_requests::Task8Readiness;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct UsbCompletionWord(pub u32);

pub fn task7_shutdown(
    rtos: &mut impl ShutdownRtos,
    sender: TaskId,
    completion: UsbCompletionWord,
) -> Result<(), KernelStatus> {
    rtos.post(ServiceMessage {
        operation: RequestId(0x1701), sender, destination: QueueId(7),
        payload: &completion.0.to_le_bytes(),
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Task7Dispatch { Handled, OtherUsbRoute }

pub fn usb_release_acknowledged(rtos: &mut impl ShutdownRtos) -> Result<(), KernelStatus> {
    let sender = rtos.current_task();
    rtos.post(ServiceMessage {
        operation: RequestId(0xb401), sender, destination: QueueId(7), payload: &[],
    })
}

pub fn request_usb_release(
    rtos: &mut impl ShutdownRtos,
    residue: &[u8; 24],
) -> Result<(), KernelStatus> {
    let sender = rtos.current_task();
    rtos.post(ServiceMessage {
        operation: RequestId(0x3401), sender, destination: QueueId(36), payload: residue,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct RegistryHandle(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct Task8CompletionWord(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Task8Dispatch { Shutdown(ServiceStatus), OtherTask8Route }

pub trait Task8Shutdown {
    fn readiness(&mut self) -> Task8Readiness;
    fn operation(&mut self) -> RequestId;
    fn clear_operation(&mut self);
    fn clear_configuration_runtime(&mut self);
    fn registry_handle(&mut self) -> RegistryHandle;
    fn unregister(&mut self, handle: RegistryHandle) -> ServiceStatus;
    fn clear_readiness(&mut self);
    fn completion(&mut self) -> Task8CompletionWord;
    fn invoke_completion(&mut self, completion: Task8CompletionWord);
    fn stop_task8(&mut self) -> ServiceStatus;
}

pub fn consume_task8_shutdown(
    task: &mut impl Task8Shutdown,
    queues: &mut impl QueueRtos,
    storage: &mut impl QueueStorage,
) -> ServiceStatus {
    if task.readiness() != Task8Readiness(1) || task.operation() != RequestId(0x1801) {
        return ServiceStatus(0);
    }
    task.clear_operation();
    task.clear_configuration_runtime();
    let handle = task.registry_handle();
    let status = task.unregister(handle);
    if status.0 != 0 { return status; }
    task.clear_readiness();
    let completion = task.completion();
    if completion.0 != 0 { task.invoke_completion(completion); }
    if destroy_queue(queues, storage, 8).0 != 0 { return ServiceStatus(-255); }
    if task.stop_task8().0 == 0 { ServiceStatus(0) } else { ServiceStatus(-255) }
}

#[cfg(target_arch = "arm")]
mod native {
    use super::*;
    use core::ptr::{read_volatile, write_volatile};

    extern "C" {
        fn xm1_power_interrupt_lock() -> u32;
        fn xm1_power_interrupt_unlock(saved: u32);
    }

    pub struct NativeTask8Shutdown;
    impl Task8Shutdown for NativeTask8Shutdown {
        fn readiness(&mut self) -> Task8Readiness { Task8Readiness(unsafe { read_volatile(0x409c34c4 as *const u32) }) }
        fn operation(&mut self) -> RequestId { RequestId(unsafe { read_volatile(0x409c3498 as *const u16) }) }
        fn clear_operation(&mut self) { unsafe { write_volatile(0x409c3498 as *mut u16, 0x7fff) }; }
        fn clear_configuration_runtime(&mut self) { unsafe { write_volatile(0x96b588 as *mut u32, 0) }; }
        fn registry_handle(&mut self) -> RegistryHandle { RegistryHandle(unsafe { read_volatile(0x409c34b4 as *const u32) }) }
        fn unregister(&mut self, handle: RegistryHandle) -> ServiceStatus {
            unsafe { unregister_registry_handle(handle) }
        }
        fn clear_readiness(&mut self) { unsafe { write_volatile(0x409c34c4 as *mut u32, 0) }; }
        fn completion(&mut self) -> Task8CompletionWord { Task8CompletionWord(unsafe { read_volatile(0x409c34b0 as *const u32) }) }
        fn invoke_completion(&mut self, completion: Task8CompletionWord) {
            let callback: unsafe extern "C" fn() = unsafe { core::mem::transmute(completion.0 as usize) };
            unsafe { callback() };
        }
        fn stop_task8(&mut self) -> ServiceStatus {
            unsafe { crate::primitives::retire_current_task(TaskId(8)) }
        }
    }

    pub unsafe fn receive_task8_shutdown(storage: &mut impl QueueStorage) -> Task8Dispatch {
        let descriptor = unsafe { &mut *(0x409c3498 as *mut crate::shutdown_receive::ReceiveDescriptor) };
        let result = unsafe {
            crate::shutdown_rtos::NativeShutdownRtos.receive(
                QueueId(8), descriptor, crate::shutdown_receive::ReceiveTimeout::Forever,
            )
        };
        if result.is_err() { unsafe { notify_error() }; }
        let mut task = NativeTask8Shutdown;
        if task.readiness() != Task8Readiness(1) || task.operation() != RequestId(0x1801) {
            return Task8Dispatch::OtherTask8Route;
        }
        let status = consume_task8_shutdown(
            &mut task, &mut crate::primitives::OwnedQueueRtos, storage,
        );
        if status.0 != 0 { unsafe { notify_error() }; }
        Task8Dispatch::Shutdown(status)
    }

    unsafe fn notify_error() {
        let address = unsafe { read_volatile(0x409c34c8 as *const u32) };
        if address != 0 {
            let callback: unsafe extern "C" fn() = unsafe { core::mem::transmute(address as usize) };
            unsafe { callback() };
        }
    }

    pub unsafe fn consume_task7_shutdown(
        rtos: &mut impl ShutdownRtos,
        release_residue: &[u8; 24],
    ) -> Task7Dispatch {
        let operation = unsafe { read_volatile(0x15c20c4 as *const u16) };
        let state = unsafe { read_volatile(0x15c20ae as *const u16) };
        if operation == 0x1701 {
            let payload = unsafe { read_volatile(0x15c20cc as *const u32) };
            let completion = unsafe { read_volatile(payload as *const u32) };
            unsafe {
                write_volatile(0x15c2120 as *mut u32, completion);
                write_volatile(0x15c20ba as *mut u16, 2);
            }
            if state != 3 && state != 6 { return Task7Dispatch::OtherUsbRoute; }
            unsafe {
                write_volatile(0x146a0e0 as *mut u32, xm1_power_usb_release_acknowledged as usize as u32);
            }
            let _ = request_usb_release(rtos, release_residue);
            unsafe { write_volatile(0x15c20ae as *mut u16, 2) };
            Task7Dispatch::Handled
        } else if operation == 0xb401 {
            unsafe { write_volatile(0x15c20ba as *mut u16, 3) };
            if state != 2 { return Task7Dispatch::OtherUsbRoute; }
            let completion = unsafe { read_volatile(0x15c2120 as *const u32) };
            if completion != 0 {
                let callback: unsafe extern "C" fn() = unsafe { core::mem::transmute(completion as usize) };
                unsafe {
                    callback();
                    write_volatile(0x15c2120 as *mut u32, 0);
                }
            }
            unsafe { write_volatile(0x15c20ae as *mut u16, 0) };
            Task7Dispatch::Handled
        } else {
            Task7Dispatch::OtherUsbRoute
        }
    }

    #[no_mangle]
    pub extern "C" fn xm1_power_usb_release_acknowledged() -> i32 {
        match usb_release_acknowledged(&mut crate::shutdown_rtos::NativeShutdownRtos) {
            Ok(()) => 0,
            Err(status) => status.0,
        }
    }
    pub unsafe fn unregister_registry_handle(handle: RegistryHandle) -> ServiceStatus {
        let saved = unsafe { xm1_power_interrupt_lock() };
        if handle.0 != 0 {
            for index in 0..32 {
                let entry = (0x95aa9c + index * 4) as *mut u32;
                if unsafe { read_volatile(entry) } == handle.0 {
                    unsafe {
                        write_volatile((0x95ab20 + index * 20) as *mut u32, 0);
                        write_volatile(entry, 0);
                    }
                    break;
                }
            }
        }
        unsafe { xm1_power_interrupt_unlock(saved) };
        ServiceStatus(0)
    }

    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_unregister_registry_handle(handle: u32) -> i32 {
        unsafe { unregister_registry_handle(RegistryHandle(handle)).0 }
    }
}
#[cfg(target_arch = "arm")]
pub use native::{consume_task7_shutdown, receive_task8_shutdown, unregister_registry_handle, NativeTask8Shutdown};

#[cfg(test)]
#[path = "shutdown_task7_8_tests.rs"]
mod tests;
