use crate::shutdown::{KernelStatus, QueueId, RequestId, SaveArm, ServiceMessage, ShutdownRtos};
use crate::shutdown_requests::OwnedCompletionAddress;

pub fn request_continuation(
    rtos: &mut impl ShutdownRtos,
    arm: SaveArm,
    completion: OwnedCompletionAddress,
) -> Result<(), KernelStatus> {
    let selector: u32 = match arm { SaveArm::Close => 0, SaveArm::Snapshot => 1 };
    let mut payload = [0; 12];
    payload[..4].copy_from_slice(&selector.to_le_bytes());
    payload[4..8].copy_from_slice(&0u32.to_le_bytes());
    payload[8..].copy_from_slice(&completion.word().to_le_bytes());
    let sender = rtos.current_task();
    rtos.post(ServiceMessage { operation: RequestId(0x13e7), sender, destination: QueueId(3), payload: &payload })
}

#[cfg(target_arch = "arm")]
#[no_mangle]
pub extern "C" fn xm1_power_persistence_shutdown_callback() -> i32 {
    let rtos = &mut crate::shutdown_rtos::NativeShutdownRtos;
    let sender = rtos.current_task();
    match rtos.post(ServiceMessage { operation: RequestId(0x93e7), sender, destination: QueueId(2), payload: &[] }) {
        Ok(()) => 0,
        Err(error) => error.0,
    }
}
