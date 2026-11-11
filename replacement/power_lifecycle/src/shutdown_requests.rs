use crate::callback_registry::{CallbackError, CallbackRegistry, CallbackWord};
use crate::shutdown::{KernelStatus, QueueId, RequestId, ServiceMessage, ShutdownRtos};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Task8Readiness(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OwnedCompletionAddress(CallbackWord);
impl OwnedCompletionAddress {
    pub const fn word(self) -> u32 { self.0.0 as u32 }
    pub const fn callback(self) -> CallbackWord { self.0 }
    pub fn from_entry(entry: extern "C" fn() -> i32) -> Self { Self(CallbackWord(entry as usize)) }
    /// The entry is registered for the shape the completion consumers resolve.
    pub fn register(registry: &CallbackRegistry, entry: extern "C" fn() -> i32) -> Result<Self, CallbackError> {
        let callback: unsafe extern "C" fn() -> i32 = entry;
        registry.ensure(callback)?;
        Ok(Self::from_entry(entry))
    }
    #[cfg(target_arch = "arm")]
    pub fn calendar() -> Self { Self::from_entry(xm1_power_calendar_shutdown_callback) }
    #[cfg(target_arch = "arm")]
    pub fn task8() -> Result<Self, CallbackError> { Self::register(crate::callback_registry::registry(), xm1_power_task8_shutdown_callback) }
    /// The word `start_native` registers before the task 2 completion loop.
    #[cfg(target_arch = "arm")]
    pub fn task8_word() -> Self { Self::from_entry(xm1_power_task8_shutdown_callback) }
    #[cfg(target_arch = "arm")]
    pub fn storage() -> Self { Self::from_entry(xm1_power_storage_shutdown_callback) }
    #[cfg(target_arch = "arm")]
    pub fn settings() -> Result<Self, CallbackError> { Self::register(crate::callback_registry::registry(), xm1_power_settings_shutdown_callback) }
    /// The device consumer resolves the status shape.
    pub fn device(registry: &CallbackRegistry, entry: extern "C" fn() -> i32) -> Result<Self, CallbackError> {
        Self::register(registry, entry)
    }
    #[cfg(target_arch = "arm")]
    pub fn task7() -> Result<Self, CallbackError> { Self::register(crate::callback_registry::registry(), xm1_power_task7_shutdown_callback) }
    #[cfg(target_arch = "arm")]
    pub fn display() -> Self { Self::from_entry(xm1_power_display_shutdown_callback) }
}

/// 0x6b0eb0..0x6b0ebc and 0xecc2a0..0xecc2ac discard the status.
pub fn invoke_status_completion(registry: &CallbackRegistry, word: CallbackWord) -> Result<(), CallbackError> {
    let entry: unsafe extern "C" fn() -> i32 = registry.resolve(word)?;
    unsafe { entry() };
    Ok(())
}

/// The audio release consumer resolves the status shape.
pub fn register_audio_completion(registry: &CallbackRegistry, entry: unsafe extern "C" fn() -> i32) -> Result<CallbackWord, CallbackError> {
    registry.ensure(entry)?;
    Ok(CallbackWord(entry as usize))
}

fn post_completion(rtos: &mut impl ShutdownRtos, event: u16) -> i32 {
    let sender = match rtos.try_current_task() { Ok(sender) => sender, Err(status) => return status.0 };
    match rtos.post(ServiceMessage { operation: RequestId(event), sender, destination: QueueId(2), payload: &[] }) {
        Ok(()) => 0,
        Err(status) => status.0,
    }
}
pub fn task7_completion(rtos: &mut impl ShutdownRtos) -> i32 { post_completion(rtos, 0x9701) }
pub fn task8_completion(rtos: &mut impl ShutdownRtos) -> i32 { post_completion(rtos, 0x9801) }
pub fn settings_completion(rtos: &mut impl ShutdownRtos) -> i32 { post_completion(rtos, 0xe301) }
pub fn storage_completion(rtos: &mut impl ShutdownRtos) -> i32 { post_completion(rtos, 0xf401) }
pub fn device_completion(rtos: &mut impl ShutdownRtos) -> i32 { post_completion(rtos, 0x9a0c) }

pub fn optional_shutdown(rtos: &mut impl ShutdownRtos) -> Result<(), KernelStatus> {
    let sender = rtos.try_current_task()?;
    rtos.post(ServiceMessage { operation: RequestId(0xf107), sender, destination: QueueId(2), payload: &[] })
}

pub fn task8_shutdown(
    rtos: &mut impl ShutdownRtos,
    readiness: Task8Readiness,
    completion: OwnedCompletionAddress,
    caller_payload: &mut [u8; 16],
) -> Result<(), KernelStatus> {
    if readiness.0 != 1 { return Err(KernelStatus(-255)); }
    caller_payload[12..16].copy_from_slice(&completion.word().to_le_bytes());
    let sender = rtos.try_current_task()?;
    rtos.post(ServiceMessage { operation: RequestId(0x1801), sender, destination: QueueId(8), payload: caller_payload })
        .map_err(|_| KernelStatus(-255))
}

pub fn calendar_shutdown(
    rtos: &mut impl ShutdownRtos,
    completion: OwnedCompletionAddress,
) -> Result<(), KernelStatus> {
    let mut payload = [0u8; 8];
    payload[..4].copy_from_slice(&completion.word().to_le_bytes());
    payload[4..].copy_from_slice(&1u32.to_le_bytes());
    let sender = rtos.try_current_task()?;
    rtos.post(ServiceMessage { operation: RequestId(0x2c01), sender, destination: QueueId(101), payload: &payload })
}

pub fn calendar_completion(rtos: &mut impl ShutdownRtos) -> i32 { post_completion(rtos, 0xac01) }

#[cfg(target_arch = "arm")]
#[no_mangle]
pub extern "C" fn xm1_power_calendar_shutdown_callback() -> i32 {
    calendar_completion(&mut crate::shutdown_rtos::NativeShutdownRtos)
}

#[cfg(target_arch = "arm")]
#[no_mangle]
pub extern "C" fn xm1_power_task8_shutdown_callback() -> i32 {
    task8_completion(&mut crate::shutdown_rtos::NativeShutdownRtos)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DevicePhase(pub u32);
pub fn device_shutdown(
    rtos: &mut impl ShutdownRtos,
    phase: DevicePhase,
    completion: OwnedCompletionAddress,
) -> Result<(), KernelStatus> {
    if phase.0 == 14 { return Ok(()); }
    let sender = rtos.try_current_task()?;
    rtos.post(ServiceMessage {
        operation: RequestId(0x1a0c),
        sender,
        destination: QueueId(10),
        payload: &completion.word().to_le_bytes(),
    }).map_err(|_| KernelStatus(-255))
}

pub fn controller_shutdown(rtos: &mut impl ShutdownRtos) -> Result<(), KernelStatus> {
    let sender = rtos.try_current_task()?;
    rtos.post(ServiceMessage { operation: RequestId(0xc901), sender, destination: QueueId(2), payload: &[] })
}

#[cfg(target_arch = "arm")]
macro_rules! completion_entry {
    ($name:ident, $body:path) => {
        #[no_mangle]
        pub extern "C" fn $name() -> i32 { $body(&mut crate::shutdown_rtos::NativeShutdownRtos) }
    };
}
#[cfg(target_arch = "arm")]
completion_entry!(xm1_power_storage_shutdown_callback, storage_completion);
#[cfg(target_arch = "arm")]
completion_entry!(xm1_power_settings_shutdown_callback, settings_completion);
#[cfg(target_arch = "arm")]
completion_entry!(xm1_power_device_shutdown_callback, device_completion);
#[cfg(target_arch = "arm")]
completion_entry!(xm1_power_task7_shutdown_callback, task7_completion);

#[cfg(target_arch = "arm")]
#[no_mangle]
pub extern "C" fn xm1_power_display_shutdown_callback() -> i32 {
    let rtos = &mut crate::shutdown_rtos::NativeShutdownRtos;
    match rtos.post(ServiceMessage {
        operation: RequestId(0x1200),
        sender: crate::shutdown::TaskId(40),
        destination: QueueId(2),
        payload: &[],
    }) {
        Ok(()) => 0,
        Err(status) => status.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shutdown::TaskId;
    #[derive(Default)]
    struct Queue(std::vec::Vec<(RequestId, QueueId, std::vec::Vec<u8>)>);
    impl ShutdownRtos for Queue {
        fn current_task(&mut self) -> TaskId { TaskId(2) }
        fn post(&mut self, message: ServiceMessage<'_>) -> Result<(), KernelStatus> {
            self.0.push((message.operation, message.destination, message.payload.to_vec()));
            Ok(())
        }
        fn set_event_flag(&mut self, _: u32, _: u32) -> Result<(), KernelStatus> { unreachable!() }
    }
    #[test]
    fn unavailable_task8_never_enqueues_a_shutdown() {
        let mut queue = Queue::default();
        let mut payload = [0x9d; 16];
        assert_eq!(task8_shutdown(&mut queue, Task8Readiness(0), OwnedCompletionAddress(CallbackWord(0x1234)), &mut payload), Err(KernelStatus(-255)));
        assert!(queue.0.is_empty());
        assert_eq!(payload, [0x9d; 16]);
    }
    #[test]
    fn shutdown_preserves_all_unwritten_payload_bytes() {
        let mut queue = Queue::default();
        let mut payload = [0x9d; 16];
        assert_eq!(task8_shutdown(&mut queue, Task8Readiness(1), OwnedCompletionAddress(CallbackWord(0x1234)), &mut payload), Ok(()));
        assert_eq!(queue.0[0].0, RequestId(0x1801));
        assert_eq!(queue.0[0].1, QueueId(8));
        assert_eq!(&queue.0[0].2[..12], &[0x9d; 12]);
        assert_eq!(&queue.0[0].2[12..], &0x1234u32.to_le_bytes());
    }
    #[test]
    fn optional_stub_really_posts_f107_without_a_device_wait() {
        let mut queue = Queue::default();
        assert_eq!(optional_shutdown(&mut queue), Ok(()));
        assert_eq!(queue.0, [(RequestId(0xf107), QueueId(2), std::vec![])]);
    }
}
