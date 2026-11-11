use super::*;

struct Timers {
    requested: u32,
    requested_period: TimerPeriod,
    periods: [TimerPeriod; 5],
    handles: [TimerHandle; 5],
    callbacks: [TimerCallback; 5],
    released: ServiceStatus,
    output: u32,
}
impl Timers {
    fn new(period: u32) -> Self {
        Self { requested: 0, requested_period: TimerPeriod(period), periods: [TimerPeriod(0); 5], handles: [TimerHandle(0); 5], callbacks: [TimerCallback(0); 5], released: ServiceStatus(0), output: 0 }
    }
    fn deliver_tick(&mut self, index: u32) {
        let operation = DeviceTimerOperation::from_abi(0x3502 + index as u16).unwrap();
        assert_eq!(dispatch(self, operation), ServiceStatus(0));
    }
}
impl DeviceTimers for Timers {
    fn requested_index(&self) -> u32 { self.requested }
    fn requested_period(&self) -> TimerPeriod { self.requested_period }
    fn requested_callback(&self) -> TimerCallback { TimerCallback(7) }
    fn handle(&self, index: DeviceTimerIndex) -> TimerHandle { self.handles[index.index()] }
    fn period(&self, index: DeviceTimerIndex) -> TimerPeriod { self.periods[index.index()] }
    fn callback(&self, index: DeviceTimerIndex) -> TimerCallback { self.callbacks[index.index()] }
    fn set_handle(&mut self, index: DeviceTimerIndex, handle: TimerHandle) { self.handles[index.index()] = handle; }
    fn set_period(&mut self, index: DeviceTimerIndex, period: TimerPeriod) { self.periods[index.index()] = period; }
    fn set_callback(&mut self, index: DeviceTimerIndex, callback: TimerCallback) { self.callbacks[index.index()] = callback; }
    fn register_cyclic(&mut self, _: DeviceTimerIndex, _: TimerPeriod) -> (ServiceStatus, TimerHandle) { (ServiceStatus(0), TimerHandle(30001)) }
    fn register_software(&mut self, _: DeviceTimerIndex, _: TimerPeriod) -> (ServiceStatus, TimerHandle) { (ServiceStatus(0), TimerHandle(42)) }
    fn unregister_cyclic(&mut self, _: TimerHandle) -> ServiceStatus { self.released }
    fn unregister_software(&mut self, _: TimerHandle) -> ServiceStatus { self.released }
    fn invoke(&mut self, callback: TimerCallback) { self.output += callback.0; }
}

#[test]
fn queued_ticks_after_unregister_cannot_invoke_removed_callback() {
    for period in [0, 10, 11] {
        let mut timers = Timers::new(period);
        assert_eq!(dispatch(&mut timers, DeviceTimerOperation::Start), ServiceStatus(0));
        timers.deliver_tick(0);
        assert_eq!(timers.output, 7);
        assert_eq!(dispatch(&mut timers, DeviceTimerOperation::Unregister), ServiceStatus(0));
        timers.deliver_tick(0);
        assert_eq!(timers.output, 7);
        assert_eq!(dispatch(&mut timers, DeviceTimerOperation::Start), ServiceStatus(0));
    }
}

#[test]
fn failed_cancellation_still_removes_callback_and_allows_restart() {
    for (period, expected) in [(10, -255), (11, -41)] {
        let mut timers = Timers::new(period);
        assert_eq!(dispatch(&mut timers, DeviceTimerOperation::Start), ServiceStatus(0));
        timers.released = ServiceStatus(-41);
        assert_eq!(dispatch(&mut timers, DeviceTimerOperation::Unregister), ServiceStatus(expected));
        timers.deliver_tick(0);
        assert_eq!(timers.output, 0);
        assert_eq!(dispatch(&mut timers, DeviceTimerOperation::Unregister), ServiceStatus(0));
        assert_eq!(dispatch(&mut timers, DeviceTimerOperation::Start), ServiceStatus(0));
    }
}

#[test]
fn invalid_requested_index_leaves_other_timers_running() {
    let mut timers = Timers::new(10);
    assert_eq!(dispatch(&mut timers, DeviceTimerOperation::Start), ServiceStatus(0));
    timers.requested = 5;
    assert_eq!(dispatch(&mut timers, DeviceTimerOperation::Unregister), ServiceStatus(-30));
    timers.deliver_tick(0);
    assert_eq!(timers.output, 7);
}

#[test]
fn software_tick_delays_preserve_initial_extra_tick_and_rounding() {
    for (period, initial, reload) in [(0, 1, 1), (1, 2, 1), (5, 3, 2), (11, 6, 5), (u32::MAX, 1_717_986_919, 1_717_986_918)] {
        assert_eq!(software_ticks(TimerPeriod(period)), SoftwareTickSchedule { initial: SoftwareTickCount(initial), reload: SoftwareTickCount(reload) });
    }
}

#[test]
fn posting_unregister_does_not_run_consumer_or_turn_failure_into_completion() {
    use crate::shutdown::{KernelStatus, TaskId};
    struct Queue { fail: bool, payload: Option<[u8; 4]> }
    impl ShutdownRtos for Queue {
        fn current_task(&mut self) -> TaskId { TaskId(9) }
        fn post(&mut self, message: ServiceMessage<'_>) -> Result<(), KernelStatus> {
            if self.fail { return Err(KernelStatus(-50)); }
            assert_eq!(message.operation, RequestId(0x3501));
            assert_eq!(message.sender, TaskId(9));
            assert_eq!(message.destination, QueueId(37));
            self.payload = Some(message.payload.try_into().unwrap());
            Ok(())
        }
        fn set_event_flag(&mut self, _: u32, _: u32) -> Result<(), KernelStatus> { panic!("unregister does not signal a completion flag") }
    }
    let mut queue = Queue { fail: false, payload: None };
    assert_eq!(post_unregister(&mut queue, 4), ServiceStatus(0));
    assert_eq!(queue.payload, Some([4, 0, 0, 0]));
    queue.fail = true;
    assert_eq!(post_unregister(&mut queue, 0), ServiceStatus(-255));
    assert_eq!(queue.payload, Some([4, 0, 0, 0]));
}

#[test]
fn software_timer_modes_preserve_raw_word_and_only_one_repeats() {
    for word in [0, 1, 2, u32::MAX] {
        let mode = SoftwareTimerMode::from_abi(word);
        assert_eq!(mode.abi(), word);
        assert_eq!(mode.is_periodic(), word == 1);
    }
}
