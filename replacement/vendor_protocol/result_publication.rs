//! X-M1 1.50 service result route and controller queue publication.

use crate::configuration_actions_low::{ActionCompletion, ActionNotification, CpuReturn, ResultFlags};
use xm1_power_services::{
    resource_requested_route::{RequestedLevel, RequestedSlot},
    shutdown::{KernelStatus, QueueId, RequestId, ServiceMessage, ShutdownRtos},
};
#[cfg(target_arch = "arm")]
use xm1_power_services::resource_requested_route::NativeRequestedRoute;

/// The stock requested-output route is a mapped table; `NativeRequestedRoute` is its target form.
pub trait RequestedRoute {
    fn request(&mut self, slot: RequestedSlot, level: RequestedLevel);
}

#[cfg(target_arch = "arm")]
impl RequestedRoute for NativeRequestedRoute {
    fn request(&mut self, slot: RequestedSlot, level: RequestedLevel) { NativeRequestedRoute::request(self, slot, level) }
}

pub fn scan_send_event4(flags: ResultFlags, rtos: &mut impl ShutdownRtos) -> Result<(), KernelStatus> {
    let mut payload = [0; 24];
    payload[..4].copy_from_slice(&4u32.to_le_bytes());
    payload[4..8].copy_from_slice(&flags.get().to_le_bytes());
    let sender = rtos.current_task();
    rtos.post(ServiceMessage { operation: RequestId(0x3408), sender, destination: QueueId(36), payload: &payload })
}

pub fn service_update_signal_result(
    flags: ResultFlags, route: &mut impl RequestedRoute, rtos: &mut impl ShutdownRtos,
) -> Result<(), KernelStatus> {
    route.request(RequestedSlot::SERVICE_RESULT, RequestedLevel::SERVICE_RESULT);
    scan_send_event4(flags, rtos)
}

pub fn publish_completion(
    completion: ActionCompletion, route: &mut impl RequestedRoute, rtos: &mut impl ShutdownRtos,
) -> Result<CpuReturn, KernelStatus> {
    match completion.notification {
        ActionNotification::None => {}
        ActionNotification::ServiceResult(flags) => service_update_signal_result(flags, route, rtos)?,
        ActionNotification::ScanEvent4(flags) => scan_send_event4(flags, rtos)?,
    }
    Ok(completion.return_code)
}

#[cfg(test)]
#[path = "result_publication_tests.rs"]
mod tests;
