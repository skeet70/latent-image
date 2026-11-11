use super::*;
const STATE: u32 = 0x0095_4de4;
fn read(at: u32) -> u32 { unsafe { core::ptr::read_volatile(at as *const u32) } }
fn write(at: u32, value: u32) { unsafe { core::ptr::write_volatile(at as *mut u32, value) }; }
fn task_word(task: DelayTask, offset: u32) -> u32 { STATE + task.index() * 12 + offset }
extern "C" {
    fn xm1_power_current_task() -> u32;
    fn xm1_power_semaphore_create(identifier: u32, count: u32) -> i32;
    fn xm1_power_semaphore_poll(identifier: i32) -> i32;
    fn xm1_power_semaphore_wait(identifier: i32, timeout: i32) -> i32;
    fn xm1_power_semaphore_signal(identifier: i32) -> i32;
    fn xm1_power_task_lock() -> i32;
    fn xm1_power_task_unlock() -> i32;
    fn xm1_power_software_timer_start(period: u32, mode: u32, callback: u32, token: *mut u32) -> i32;
    fn xm1_power_software_timer_cancel(handle: u32) -> i32;
}
struct NativeChannels;
impl DelayChannels for NativeChannels {
    fn current_task(&mut self) -> Option<DelayTask> { DelayTask::from_abi(unsafe { xm1_power_current_task() }) }
    fn semaphore(&self, task: DelayTask) -> DelaySemaphore { DelaySemaphore(read(task_word(task, 4)) as i32) }
    fn set_semaphore(&mut self, task: DelayTask, semaphore: DelaySemaphore) { write(task_word(task, 4), semaphore.0 as u32); }
    fn create_semaphore(&mut self) -> DelaySemaphore { DelaySemaphore(unsafe { xm1_power_semaphore_create(0, 1) }) }
    fn poll(&mut self, semaphore: DelaySemaphore) { let _ = unsafe { xm1_power_semaphore_poll(semaphore.0) }; }
    fn wait(&mut self, semaphore: DelaySemaphore) -> ServiceStatus { ServiceStatus(unsafe { xm1_power_semaphore_wait(semaphore.0, -1) }) }
    fn signal(&mut self, semaphore: DelaySemaphore) { let _ = unsafe { xm1_power_semaphore_signal(semaphore.0) }; }
    fn lock_tasks(&mut self) { let _ = unsafe { xm1_power_task_lock() }; }
    fn unlock_tasks(&mut self) { let _ = unsafe { xm1_power_task_unlock() }; }
    fn timer(&self) -> TimerHandle { TimerHandle(read(STATE)) }
    fn start_timer(&mut self, period: TimerPeriod) -> ServiceStatus {
        ServiceStatus(unsafe { xm1_power_software_timer_start(period.0, 1, xm1_power_software_delay_tick as usize as u32, STATE as *mut u32) })
    }
    fn cancel_timer(&mut self, handle: TimerHandle) { let _ = unsafe { xm1_power_software_timer_cancel(handle.0) }; }
    fn set_timer(&mut self, handle: TimerHandle) { write(STATE, handle.0); }
    fn done(&self, task: DelayTask) -> bool { read(task_word(task, 12)) != 0 }
    fn set_done(&mut self, task: DelayTask, done: bool) { write(task_word(task, 12), u32::from(done)); }
    fn remaining(&self, task: DelayTask) -> RemainingDelayTicks { RemainingDelayTicks(read(task_word(task, 8))) }
    fn set_remaining(&mut self, task: DelayTask, remaining: RemainingDelayTicks) { write(task_word(task, 8), remaining.0); }
}
#[no_mangle]
pub extern "C" fn xm1_power_software_delay_start(period: u32) -> i32 { start(&mut NativeChannels, TimerPeriod(period)).0 }
#[no_mangle]
pub extern "C" fn xm1_power_software_delay_tick() { tick(&mut NativeChannels); }
