use crate::primitives::ServiceStatus;
use crate::shutdown::{QueueId, RequestId, ServiceMessage, ShutdownRtos};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct DeviceTimerIndex(u8);
impl DeviceTimerIndex {
    pub const fn from_abi(value: u32) -> Option<Self> {
        if value < 5 { Some(Self(value as u8)) } else { None }
    }
    pub const fn index(self) -> usize { self.0 as usize }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct TimerPeriod(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct SoftwareTimerMode(u32);
impl SoftwareTimerMode {
    pub const fn from_abi(value: u32) -> Self { Self(value) }
    pub const fn abi(self) -> u32 { self.0 }
    pub const fn is_periodic(self) -> bool { self.0 == 1 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct TimerHandle(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct TimerCallback(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeviceTimerOperation { Start, Unregister, Invoke(DeviceTimerIndex) }
impl DeviceTimerOperation {
    pub const fn from_abi(code: u16) -> Option<Self> {
        match code {
            0x3500 => Some(Self::Start),
            0x3501 => Some(Self::Unregister),
            0x3502..=0x3506 => Some(Self::Invoke(DeviceTimerIndex((code - 0x3502) as u8))),
            _ => None,
        }
    }
}

pub trait DeviceTimers {
    fn requested_index(&self) -> u32;
    fn requested_period(&self) -> TimerPeriod;
    fn requested_callback(&self) -> TimerCallback;
    fn handle(&self, index: DeviceTimerIndex) -> TimerHandle;
    fn period(&self, index: DeviceTimerIndex) -> TimerPeriod;
    fn callback(&self, index: DeviceTimerIndex) -> TimerCallback;
    fn set_handle(&mut self, index: DeviceTimerIndex, handle: TimerHandle);
    fn set_period(&mut self, index: DeviceTimerIndex, period: TimerPeriod);
    fn set_callback(&mut self, index: DeviceTimerIndex, callback: TimerCallback);
    fn register_cyclic(&mut self, index: DeviceTimerIndex, period: TimerPeriod) -> (ServiceStatus, TimerHandle);
    fn register_software(&mut self, index: DeviceTimerIndex, period: TimerPeriod) -> (ServiceStatus, TimerHandle);
    fn unregister_cyclic(&mut self, handle: TimerHandle) -> ServiceStatus;
    fn unregister_software(&mut self, handle: TimerHandle) -> ServiceStatus;
    fn invoke(&mut self, callback: TimerCallback);
}

pub fn unregister_requested(timers: &mut impl DeviceTimers) -> ServiceStatus {
    let Some(index) = DeviceTimerIndex::from_abi(timers.requested_index()) else { return ServiceStatus(-30); };
    let handle = timers.handle(index);
    if handle.0 == 0 { return ServiceStatus(0); }
    timers.set_callback(index, TimerCallback(0));
    let status = if timers.period(index).0 % 10 == 0 {
        let status = timers.unregister_cyclic(handle);
        ServiceStatus(if status.0 == 0 { 0 } else { -255 })
    } else { timers.unregister_software(handle) };
    timers.set_period(index, TimerPeriod(0));
    timers.set_handle(index, TimerHandle(0));
    status
}

pub fn dispatch(timers: &mut impl DeviceTimers, operation: DeviceTimerOperation) -> ServiceStatus {
    match operation {
        DeviceTimerOperation::Unregister => unregister_requested(timers),
        DeviceTimerOperation::Invoke(index) => {
            let callback = timers.callback(index);
            if callback.0 != 0 { timers.invoke(callback); }
            ServiceStatus(0)
        }
        DeviceTimerOperation::Start => {
            let Some(index) = DeviceTimerIndex::from_abi(timers.requested_index()) else { return ServiceStatus(-30); };
            let callback = timers.requested_callback();
            if callback.0 == 0 { return ServiceStatus(-30); }
            if timers.handle(index).0 != 0 { return ServiceStatus(-255); }
            let period = timers.requested_period();
            timers.set_callback(index, callback);
            timers.set_period(index, period);
            let (status, handle) = if period.0 % 10 == 0 {
                timers.register_cyclic(index, period)
            } else { timers.register_software(index, period) };
            timers.set_handle(index, handle);
            if period.0 % 10 == 0 && status.0 != 0 { ServiceStatus(-255) } else { status }
        }
    }
}

pub fn post_unregister(rtos: &mut impl ShutdownRtos, index: u32) -> ServiceStatus {
    let sender = match rtos.try_current_task() { Ok(sender) => sender, Err(status) => return ServiceStatus(status.0) };
    let payload = index.to_le_bytes();
    let result = rtos.post(ServiceMessage { operation: RequestId(0x3501), sender, destination: QueueId(37), payload: &payload });
    ServiceStatus(if result.is_ok() { 0 } else { -255 })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct SoftwareTickCount(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SoftwareTickSchedule {
    pub initial: SoftwareTickCount,
    pub reload: SoftwareTickCount,
}
pub const fn software_ticks(period: TimerPeriod) -> SoftwareTickSchedule {
    if period.0 == 0 { return SoftwareTickSchedule { initial: SoftwareTickCount(1), reload: SoftwareTickCount(1) }; }
    let micros = period.0 as u64 * 1000;
    let reload = (micros / 2500 + if micros % 2500 != 0 { 1 } else { 0 }) as u32;
    SoftwareTickSchedule { initial: SoftwareTickCount(reload + 1), reload: SoftwareTickCount(reload) }
}

#[cfg(target_arch = "arm")]
#[path = "device_timer_release_native.rs"]
mod native;
#[cfg(test)]
#[path = "device_timer_release_tests.rs"]
mod tests;
