use crate::device_timer_release::{TimerHandle, TimerPeriod};
use crate::primitives::ServiceStatus;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct DelayTask(u8);
impl DelayTask {
    pub const fn from_abi(value: u32) -> Option<Self> { if value <= 104 { Some(Self(value as u8)) } else { None } }
    pub const fn index(self) -> u32 { self.0 as u32 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct DelaySemaphore(pub i32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct RemainingDelayTicks(pub u32);

pub trait DelayChannels {
    fn current_task(&mut self) -> Option<DelayTask>;
    fn semaphore(&self, task: DelayTask) -> DelaySemaphore;
    fn set_semaphore(&mut self, task: DelayTask, semaphore: DelaySemaphore);
    fn create_semaphore(&mut self) -> DelaySemaphore;
    fn poll(&mut self, semaphore: DelaySemaphore);
    fn wait(&mut self, semaphore: DelaySemaphore) -> ServiceStatus;
    fn signal(&mut self, semaphore: DelaySemaphore);
    fn lock_tasks(&mut self);
    fn unlock_tasks(&mut self);
    fn timer(&self) -> TimerHandle;
    fn start_timer(&mut self, period: TimerPeriod) -> ServiceStatus;
    fn cancel_timer(&mut self, handle: TimerHandle);
    fn set_timer(&mut self, handle: TimerHandle);
    fn done(&self, task: DelayTask) -> bool;
    fn set_done(&mut self, task: DelayTask, done: bool);
    fn remaining(&self, task: DelayTask) -> RemainingDelayTicks;
    fn set_remaining(&mut self, task: DelayTask, remaining: RemainingDelayTicks);
}

pub const fn delay_ticks(period: TimerPeriod, already_running: bool) -> RemainingDelayTicks {
    if period.0 == 0 { return RemainingDelayTicks(1); }
    let micros = period.0 as u64 * 1000;
    RemainingDelayTicks((micros / 2500 + if micros % 2500 == 0 { 0 } else { 1 }) as u32 + already_running as u32)
}

pub fn start(channels: &mut impl DelayChannels, period: TimerPeriod) -> ServiceStatus {
    let Some(task) = channels.current_task() else { return ServiceStatus(-255); };
    let mut status = ServiceStatus(0);
    if channels.semaphore(task).0 == 0 {
        let semaphore = channels.create_semaphore();
        channels.set_semaphore(task, semaphore);
        if semaphore.0 <= 0 { status = ServiceStatus(-255); }
    }
    channels.poll(channels.semaphore(task));
    if status.0 != 0 { return status; }
    channels.lock_tasks();
    channels.set_done(task, false);
    channels.set_remaining(task, delay_ticks(period, channels.timer().0 != 0));
    if channels.timer().0 == 0 {
        if channels.start_timer(TimerPeriod(if period.0 == 0 { 0 } else { 1 })).0 != 0 { status = ServiceStatus(-255); }
    }
    channels.unlock_tasks();
    if status.0 == 0 && channels.wait(channels.semaphore(task)).0 != 0 { status = ServiceStatus(-255); }
    status
}

pub fn tick(channels: &mut impl DelayChannels) {
    let mut pending = false;
    for index in 1..=104 {
        let task = DelayTask(index);
        if channels.semaphore(task).0 == 0 || channels.done(task) { continue; }
        pending = true;
        let remaining = channels.remaining(task);
        if remaining.0 != 0 { channels.set_remaining(task, RemainingDelayTicks(remaining.0 - 1)); }
        if channels.remaining(task).0 == 0 {
            channels.signal(channels.semaphore(task));
            channels.set_done(task, true);
        }
    }
    if !pending {
        channels.cancel_timer(channels.timer());
        channels.set_timer(TimerHandle(0));
    }
}

#[cfg(target_arch = "arm")]
#[path = "software_delay_channel_native.rs"]
mod native;
#[cfg(test)]
#[path = "software_delay_channel_tests.rs"]
mod tests;
