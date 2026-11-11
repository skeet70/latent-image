use super::super::vendor_wireless_lifecycle::{Synchronization, SynchronizationReply};
#[cfg(target_arch = "arm")]
use super::super::vendor_wireless_lifecycle::DriverSemaphore;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemaphoreHandle(i32);
impl SemaphoreHandle {
    pub const fn from_abi(value: i32) -> Option<Self> {
        if value > 0 { Some(Self(value)) } else { None }
    }
    pub const fn abi(self) -> i32 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemaphoreStatus(pub i32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SynchronizationObservation {
    pub reply: SynchronizationReply,
    pub status: SemaphoreStatus,
}

pub trait DriverSynchronization {
    fn execute(&mut self, operation: Synchronization) -> SynchronizationObservation;
}

#[cfg(target_arch = "arm")]
pub struct NativeDriverSynchronization { global: SemaphoreHandle, adapter: SemaphoreHandle }
#[cfg(target_arch = "arm")]
impl NativeDriverSynchronization {
    /// # Safety
    /// Both initialized semaphores remain owned until this provider is dropped.
    pub unsafe fn adopt(global: SemaphoreHandle, adapter: SemaphoreHandle) -> Self { Self { global, adapter } }
}
#[cfg(target_arch = "arm")]
impl DriverSynchronization for NativeDriverSynchronization {
    fn execute(&mut self, operation: Synchronization) -> SynchronizationObservation {
        let semaphore = match operation {
            Synchronization::Acquire { semaphore, .. } | Synchronization::Release(semaphore) => semaphore,
        };
        let handle = match semaphore { DriverSemaphore::Global => self.global, DriverSemaphore::Adapter => self.adapter };
        match operation {
            Synchronization::Acquire { timeout_ticks, .. } => {
                let status = unsafe { xm1_power_semaphore_wait(handle.0, timeout_ticks as i32) };
                SynchronizationObservation { reply: if status == 0 { SynchronizationReply::Acquired } else { SynchronizationReply::TimedOut },
                    status: SemaphoreStatus(status) }
            }
            Synchronization::Release(_) => SynchronizationObservation {
                reply: SynchronizationReply::Released,
                status: SemaphoreStatus(unsafe { xm1_power_semaphore_signal(handle.0) }),
            },
        }
    }
}
#[cfg(target_arch = "arm")]
extern "C" {
    fn xm1_power_semaphore_wait(id: i32, timeout: i32) -> i32;
    fn xm1_power_semaphore_signal(id: i32) -> i32;
}
