use super::*;

struct Channels {
    semaphores: [DelaySemaphore; 105],
    remaining: [RemainingDelayTicks; 105],
    done: [bool; 105],
    tokens: u32,
    timer: TimerHandle,
}
impl Channels {
    fn new() -> Self { Self { semaphores: [DelaySemaphore(0); 105], remaining: [RemainingDelayTicks(0); 105], done: [false; 105], tokens: 0, timer: TimerHandle(42) } }
}
impl DelayChannels for Channels {
    fn current_task(&mut self) -> Option<DelayTask> { DelayTask::from_abi(1) }
    fn semaphore(&self, task: DelayTask) -> DelaySemaphore { self.semaphores[task.index() as usize] }
    fn set_semaphore(&mut self, task: DelayTask, semaphore: DelaySemaphore) { self.semaphores[task.index() as usize] = semaphore; }
    fn create_semaphore(&mut self) -> DelaySemaphore { DelaySemaphore(9) }
    fn poll(&mut self, _: DelaySemaphore) { self.tokens = self.tokens.saturating_sub(1); }
    fn wait(&mut self, _: DelaySemaphore) -> ServiceStatus {
        if self.tokens == 0 { ServiceStatus(-50) } else { self.tokens -= 1; ServiceStatus(0) }
    }
    fn signal(&mut self, _: DelaySemaphore) { self.tokens += 1; }
    fn lock_tasks(&mut self) {}
    fn unlock_tasks(&mut self) {}
    fn timer(&self) -> TimerHandle { self.timer }
    fn start_timer(&mut self, _: TimerPeriod) -> ServiceStatus { self.timer = TimerHandle(43); ServiceStatus(0) }
    fn cancel_timer(&mut self, _: TimerHandle) { self.timer = TimerHandle(0); }
    fn set_timer(&mut self, handle: TimerHandle) { self.timer = handle; }
    fn done(&self, task: DelayTask) -> bool { self.done[task.index() as usize] }
    fn set_done(&mut self, task: DelayTask, done: bool) { self.done[task.index() as usize] = done; }
    fn remaining(&self, task: DelayTask) -> RemainingDelayTicks { self.remaining[task.index() as usize] }
    fn set_remaining(&mut self, task: DelayTask, remaining: RemainingDelayTicks) { self.remaining[task.index() as usize] = remaining; }
}

#[test]
fn delay_completion_signals_once_after_requested_ticks() {
    let mut channels = Channels::new();
    let task = DelayTask::from_abi(1).unwrap();
    channels.set_semaphore(task, DelaySemaphore(9));
    channels.set_remaining(task, RemainingDelayTicks(2));
    tick(&mut channels);
    assert_eq!(channels.wait(DelaySemaphore(9)), ServiceStatus(-50));
    tick(&mut channels);
    assert_eq!(channels.wait(DelaySemaphore(9)), ServiceStatus(0));
    assert_eq!(channels.wait(DelaySemaphore(9)), ServiceStatus(-50));
    tick(&mut channels);
    assert_eq!(channels.wait(DelaySemaphore(9)), ServiceStatus(-50));
    assert_eq!(channels.timer(), TimerHandle(0));
}

#[test]
fn semaphore_wait_error_is_not_success() {
    let mut channels = Channels::new();
    assert_eq!(start(&mut channels, TimerPeriod(5)), ServiceStatus(-255));
}

#[test]
fn rounding_and_existing_timer_phase_preserve_raw_delay_counts() {
    for (period, running, count) in [(0, false, 1), (0, true, 1), (1, false, 1), (1, true, 2), (5, false, 2), (5, true, 3), (11, false, 5)] {
        assert_eq!(delay_ticks(TimerPeriod(period), running), RemainingDelayTicks(count));
    }
}
