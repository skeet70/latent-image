use crate::shutdown::{KernelStatus, QueueId, RequestId, ServiceMessage, ShutdownRtos};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Task8Readiness(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OwnedCompletionAddress(u32);
impl OwnedCompletionAddress {
    pub const fn word(self) -> u32 { self.0 }
    pub fn from_entry(entry: extern "C" fn() -> i32) -> Self { Self(entry as usize as u32) }
    #[cfg(target_arch = "arm")]
    pub fn calendar() -> Self { Self::from_entry(xm1_power_calendar_shutdown_callback) }
    #[cfg(target_arch = "arm")]
    pub fn task8() -> Self { Self(xm1_power_task8_shutdown_callback as usize as u32) }
    #[cfg(target_arch = "arm")]
    pub fn storage() -> Self { Self::from_entry(xm1_power_storage_shutdown_callback) }
    #[cfg(target_arch = "arm")]
    pub fn settings() -> Self { Self::from_entry(xm1_power_settings_shutdown_callback) }
    #[cfg(target_arch = "arm")]
    pub fn device() -> Self { Self::from_entry(xm1_power_device_shutdown_callback) }
    #[cfg(target_arch = "arm")]
    pub fn task7() -> Self { Self::from_entry(xm1_power_task7_shutdown_callback) }
    #[cfg(target_arch = "arm")]
    pub fn display() -> Self { Self::from_entry(xm1_power_display_shutdown_callback) }
}

pub fn optional_shutdown(rtos: &mut impl ShutdownRtos) -> Result<(), KernelStatus> {
    let sender = rtos.current_task();
    rtos.post(ServiceMessage { operation: RequestId(0xf107), sender, destination: QueueId(2), payload: &[] })
}

pub fn task8_shutdown(
    rtos: &mut impl ShutdownRtos,
    readiness: Task8Readiness,
    completion: OwnedCompletionAddress,
    caller_payload: &mut [u8; 16],
) -> Result<(), KernelStatus> {
    if readiness.0 != 1 { return Err(KernelStatus(-255)); }
    caller_payload[12..16].copy_from_slice(&completion.0.to_le_bytes());
    let sender = rtos.current_task();
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
    let sender = rtos.current_task();
    rtos.post(ServiceMessage { operation: RequestId(0x2c01), sender, destination: QueueId(101), payload: &payload })
}

#[cfg(target_arch = "arm")]
#[no_mangle]
pub extern "C" fn xm1_power_calendar_shutdown_callback() -> i32 {
    let rtos = &mut crate::shutdown_rtos::NativeShutdownRtos;
    let sender = rtos.current_task();
    match rtos.post(ServiceMessage { operation: RequestId(0xac01), sender, destination: QueueId(2), payload: &[] }) {
        Ok(()) => 0,
        Err(status) => status.0,
    }
}

#[cfg(target_arch = "arm")]
#[no_mangle]
pub extern "C" fn xm1_power_task8_shutdown_callback() -> i32 {
    let rtos = &mut crate::shutdown_rtos::NativeShutdownRtos;
    let sender = rtos.current_task();
    match rtos.post(ServiceMessage { operation: RequestId(0x9801), sender, destination: QueueId(2), payload: &[] }) {
        Ok(()) => 0,
        Err(status) => status.0,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DevicePhase(pub u32);
pub fn device_shutdown(
    rtos: &mut impl ShutdownRtos,
    phase: DevicePhase,
    completion: OwnedCompletionAddress,
) -> Result<(), KernelStatus> {
    if phase.0 == 14 { return Ok(()); }
    let sender = rtos.current_task();
    rtos.post(ServiceMessage {
        operation: RequestId(0x1a0c),
        sender,
        destination: QueueId(10),
        payload: &completion.word().to_le_bytes(),
    }).map_err(|_| KernelStatus(-255))
}

pub fn controller_shutdown(rtos: &mut impl ShutdownRtos) -> Result<(), KernelStatus> {
    let sender = rtos.current_task();
    rtos.post(ServiceMessage { operation: RequestId(0xc901), sender, destination: QueueId(2), payload: &[] })
}

#[cfg(target_arch = "arm")]
macro_rules! completion_entry {
    ($name:ident, $event:expr) => {
        #[no_mangle]
        pub extern "C" fn $name() -> i32 {
            let rtos = &mut crate::shutdown_rtos::NativeShutdownRtos;
            let sender = rtos.current_task();
            match rtos.post(ServiceMessage { operation: RequestId($event), sender, destination: QueueId(2), payload: &[] }) {
                Ok(()) => 0,
                Err(status) => status.0,
            }
        }
    };
}
#[cfg(target_arch = "arm")]
completion_entry!(xm1_power_storage_shutdown_callback, 0xf401);
#[cfg(target_arch = "arm")]
completion_entry!(xm1_power_settings_shutdown_callback, 0xe301);
#[cfg(target_arch = "arm")]
completion_entry!(xm1_power_device_shutdown_callback, 0x9a0c);
#[cfg(target_arch = "arm")]
completion_entry!(xm1_power_task7_shutdown_callback, 0x9701);

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
        assert_eq!(task8_shutdown(&mut queue, Task8Readiness(0), OwnedCompletionAddress(0x1234), &mut payload), Err(KernelStatus(-255)));
        assert!(queue.0.is_empty());
        assert_eq!(payload, [0x9d; 16]);
    }
    #[test]
    fn shutdown_preserves_all_unwritten_payload_bytes() {
        let mut queue = Queue::default();
        let mut payload = [0x9d; 16];
        assert_eq!(task8_shutdown(&mut queue, Task8Readiness(1), OwnedCompletionAddress(0x1234), &mut payload), Ok(()));
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
