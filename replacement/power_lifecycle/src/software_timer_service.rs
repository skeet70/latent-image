use crate::primitives::ServiceStatus;
use crate::shutdown::{QueueId, RequestId, ServiceMessage, ShutdownRtos, TaskId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct TimerServiceTicks(pub u32);
pub trait TimerServiceCallbacks {
    fn timer_callback(&mut self);
    fn failed_post_callback(&mut self);
    fn ticks(&self) -> TimerServiceTicks;
    fn set_ticks(&mut self, ticks: TimerServiceTicks);
}
pub fn physical_tick(rtos: &mut impl ShutdownRtos, callbacks: &mut impl TimerServiceCallbacks) {
    let posted = rtos.post(ServiceMessage { operation: RequestId(0x1900), sender: TaskId(9), destination: QueueId(9), payload: &[] });
    if posted.is_err() { callbacks.failed_post_callback(); }
    let ticks = callbacks.ticks();
    callbacks.set_ticks(TimerServiceTicks(ticks.0.wrapping_add(1)));
}
pub fn consume(callbacks: &mut impl TimerServiceCallbacks) { callbacks.timer_callback(); }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum DetectionMode { Off = 0, On = 1, Probe = 2 }
impl DetectionMode {
    pub const fn from_abi(value: u32) -> Option<Self> {
        match value { 0 => Some(Self::Off), 1 => Some(Self::On), 2 => Some(Self::Probe), _ => None }
    }
    pub const fn irq_source(self) -> crate::charge_timers::IrqSource {
        let source = match self { Self::Off => 0x75, Self::On => 0x74, Self::Probe => 0x76 };
        match crate::charge_timers::IrqSource::from_abi(source) { Some(source) => source, None => panic!("detection IRQ source") }
    }
    pub const fn irq_route(self) -> crate::charge_timers::IrqRoute {
        let trigger = match self { Self::Off => 2, Self::On => 3, Self::Probe => 1 };
        match crate::charge_timers::IrqRoute::from_abi(15, 12, trigger) { Some(route) => route, None => panic!("detection IRQ route") }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DetectionCallback(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DetectionIrqToken(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DetectionField { Phase, Callback, IrqToken, Mode, ClockEnabled }
impl DetectionField {
    pub const fn address(self) -> u32 {
        0x0091_a548 + match self { Self::Phase => 0x9c, Self::Callback => 0xa0, Self::IrqToken => 0xa4, Self::Mode => 0xa8, Self::ClockEnabled => 0xac }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DetectionRegister { SetClock, ClearClock, Input }
impl DetectionRegister {
    pub const fn address(self) -> u32 {
        match self { Self::SetClock => 0xfff8_9004, Self::ClearClock => 0xfff8_9000, Self::Input => 0xfff8_9014 }
    }
}
pub trait DetectionOutputBackend {
    type SavedCritical;
    fn enter(&mut self) -> Self::SavedCritical;
    fn leave(&mut self, saved: Self::SavedCritical);
    fn load(&mut self, field: DetectionField) -> u32;
    fn store(&mut self, field: DetectionField, value: u32);
    fn write_register(&mut self, register: DetectionRegister, value: u32);
    fn read_register(&mut self, register: DetectionRegister) -> u32;
    fn release_irq(&mut self, token: DetectionIrqToken) -> Result<(), ServiceStatus>;
    fn attach_irq(&mut self, mode: DetectionMode) -> Result<DetectionIrqToken, ServiceStatus>;
    fn invoke(&mut self, callback: DetectionCallback);
}
pub fn initialize_detection_output(backend: &mut impl DetectionOutputBackend) {
    backend.write_register(DetectionRegister::SetClock, 0x800000);
    backend.write_register(DetectionRegister::ClearClock, 0x800000);
    backend.store(DetectionField::Phase, 1);
    backend.store(DetectionField::Mode, 0);
    backend.store(DetectionField::ClockEnabled, 1);
}
pub fn set_detection_mode(backend: &mut impl DetectionOutputBackend, mode: DetectionMode) -> Result<(), ServiceStatus> {
    let mut result = Ok(());
    if mode == DetectionMode::Off {
        backend.write_register(DetectionRegister::SetClock, 0x800000);
    } else {
        let saved = backend.enter();
        if backend.load(DetectionField::Phase) > 1 { backend.store(DetectionField::Phase, 1); }
        backend.store(DetectionField::Callback, 0);
        let token = DetectionIrqToken(backend.load(DetectionField::IrqToken));
        backend.store(DetectionField::IrqToken, 0);
        backend.leave(saved);
        if token.0 != 0 { result = backend.release_irq(token); }
        if backend.load(DetectionField::ClockEnabled) == 0 {
            backend.write_register(DetectionRegister::SetClock, 0x800080);
        }
    }
    backend.store(DetectionField::Mode, mode as u32);
    result
}
pub fn detection_output(backend: &mut impl DetectionOutputBackend, mode: DetectionMode, callback: Option<DetectionCallback>) -> Result<(), ServiceStatus> {
    register_detection_output(backend, mode, callback, false)
}
pub fn continuous_detection_output(backend: &mut impl DetectionOutputBackend, mode: DetectionMode, callback: Option<DetectionCallback>) -> Result<(), ServiceStatus> {
    register_detection_output(backend, mode, callback, true)
}
fn register_detection_output(backend: &mut impl DetectionOutputBackend, mode: DetectionMode, callback: Option<DetectionCallback>, continuous: bool) -> Result<(), ServiceStatus> {
    let phase = if continuous { 3 } else { 2 };
    let saved = backend.enter();
    let mut pending = if continuous { None } else { callback };
    let token = if callback.is_none() || backend.load(DetectionField::Callback) != callback.map_or(0, |value| value.0) {
        if backend.load(DetectionField::Phase) > 1 { backend.store(DetectionField::Phase, 1); }
        backend.store(DetectionField::Callback, 0);
        let token = DetectionIrqToken(backend.load(DetectionField::IrqToken));
        backend.store(DetectionField::IrqToken, 0);
        token
    } else { DetectionIrqToken(0) };
    if let Some(callback) = callback {
        if backend.load(DetectionField::Mode) == 0 && backend.load(DetectionField::Phase) == 1 {
            backend.store(DetectionField::Phase, phase);
            backend.store(DetectionField::Callback, callback.0);
            pending = None;
        }
    }
    backend.leave(saved);
    let released = if token.0 != 0 { backend.release_irq(token) } else { Ok(()) };
    if let Some(callback) = pending { backend.invoke(callback); }
    let saved = backend.enter();
    let attached = if backend.load(DetectionField::Phase) == phase {
        let irq_mode = if continuous && mode == DetectionMode::Probe { DetectionMode::On } else { mode };
        let attached = backend.attach_irq(irq_mode);
        if let Ok(token) = attached { backend.store(DetectionField::IrqToken, token.0); }
        attached.map(|_| ())
    } else { Ok(()) };
    backend.leave(saved);
    attached?;
    released
}
pub fn detection_interrupt(backend: &mut impl DetectionOutputBackend, mode: DetectionMode) {
    let expected = if mode == DetectionMode::Off { 0 } else { 0x80 };
    for _ in 0..10 {
        if backend.read_register(DetectionRegister::Input) & 0x80 != expected { return; }
    }
    let saved = backend.enter();
    let callback = DetectionCallback(backend.load(DetectionField::Callback));
    if backend.load(DetectionField::Phase) == 2 { backend.store(DetectionField::Callback, 0); }
    backend.leave(saved);
    if callback.0 != 0 { backend.invoke(callback); }
}

#[cfg(target_arch = "arm")]
#[path = "software_timer_service_native.rs"]
mod native;
#[cfg(test)]
mod tests {
    use super::*;
    use crate::shutdown::KernelStatus;
    struct Queue { posted: bool, reject: bool }
    impl ShutdownRtos for Queue {
        fn current_task(&mut self) -> TaskId { panic!("timer IRQ uses task9 as sender") }
        fn post(&mut self, message: ServiceMessage<'_>) -> Result<(), KernelStatus> {
            assert_eq!(message.operation, RequestId(0x1900));
            assert_eq!(message.sender, TaskId(9));
            assert_eq!(message.destination, QueueId(9));
            assert!(message.payload.is_empty());
            if self.reject { Err(KernelStatus(-50)) } else { self.posted = true; Ok(()) }
        }
        fn set_event_flag(&mut self, _: u32, _: u32) -> Result<(), KernelStatus> { panic!("timer service does not signal a flag") }
    }
    struct Output { deliveries: u32, failures: u32, ticks: TimerServiceTicks }
    impl TimerServiceCallbacks for Output {
        fn timer_callback(&mut self) { self.deliveries += 1; }
        fn failed_post_callback(&mut self) { self.failures += 1; self.ticks = TimerServiceTicks(40); }
        fn ticks(&self) -> TimerServiceTicks { self.ticks }
        fn set_ticks(&mut self, ticks: TimerServiceTicks) { self.ticks = ticks; }
    }
    #[test]
    fn queued_tick_waits_for_task_delivery() {
        let mut queue = Queue { posted: false, reject: false };
        let mut output = Output { deliveries: 0, failures: 0, ticks: TimerServiceTicks(u32::MAX) };
        physical_tick(&mut queue, &mut output);
        assert!(queue.posted);
        assert_eq!(output.deliveries, 0);
        assert_eq!(output.failures, 0);
        assert_eq!(output.ticks, TimerServiceTicks(0));
        consume(&mut output);
        assert_eq!(output.deliveries, 1);
    }
    #[test]
    fn failed_tick_runs_fallback_before_increment() {
        let mut queue = Queue { posted: false, reject: true };
        let mut output = Output { deliveries: 0, failures: 0, ticks: TimerServiceTicks(0) };
        physical_tick(&mut queue, &mut output);
        assert!(!queue.posted);
        assert_eq!(output.failures, 1);
        assert_eq!(output.deliveries, 0);
        assert_eq!(output.ticks, TimerServiceTicks(41));
    }
}

#[cfg(test)]
mod detection_tests {
    use super::*;
    struct Machine {
        fields: [u32; 5],
        clocks: [u32; 2],
        input: u32,
        delivered: u32,
        attached: Option<DetectionMode>,
        attach_status: ServiceStatus,
    }
    impl Machine {
        fn new() -> Self {
            Self { fields: [0; 5], clocks: [0; 2], input: 0, delivered: 0, attached: None, attach_status: ServiceStatus(0) }
        }
    }
    impl DetectionOutputBackend for Machine {
        type SavedCritical = ();
        fn enter(&mut self) {}
        fn leave(&mut self, _: ()) {}
        fn load(&mut self, field: DetectionField) -> u32 { self.fields[((field.address() - 0x91a5e4) / 4) as usize] }
        fn store(&mut self, field: DetectionField, value: u32) { self.fields[((field.address() - 0x91a5e4) / 4) as usize] = value; }
        fn write_register(&mut self, register: DetectionRegister, value: u32) {
            match register { DetectionRegister::SetClock => self.clocks[0] = value, DetectionRegister::ClearClock => self.clocks[1] = value, DetectionRegister::Input => panic!("input register") }
        }
        fn read_register(&mut self, _: DetectionRegister) -> u32 { self.input }
        fn release_irq(&mut self, _: DetectionIrqToken) -> Result<(), ServiceStatus> { self.attached = None; Ok(()) }
        fn attach_irq(&mut self, mode: DetectionMode) -> Result<DetectionIrqToken, ServiceStatus> {
            if self.attach_status.0 != 0 { return Err(self.attach_status); }
            self.attached = Some(mode);
            Ok(DetectionIrqToken(7))
        }
        fn invoke(&mut self, callback: DetectionCallback) { self.delivered += callback.0; }
    }
    #[test]
    fn initialization_programs_both_physical_clocks() {
        let mut machine = Machine::new();
        initialize_detection_output(&mut machine);
        assert_eq!(machine.clocks, [0x800000, 0x800000]);
    }
    #[test]
    fn deferred_completion_requires_matching_input_and_runs_once() {
        let mut machine = Machine::new();
        initialize_detection_output(&mut machine);
        assert_eq!(detection_output(&mut machine, DetectionMode::Probe, Some(DetectionCallback(1))), Ok(()));
        assert_eq!(machine.delivered, 0);
        assert_eq!(machine.attached, Some(DetectionMode::Probe));
        detection_interrupt(&mut machine, DetectionMode::Probe);
        assert_eq!(machine.delivered, 0);
        machine.input = 0x80;
        detection_interrupt(&mut machine, DetectionMode::Probe);
        detection_interrupt(&mut machine, DetectionMode::Probe);
        assert_eq!(machine.delivered, 1);
    }
    #[test]
    fn nonzero_mode_completes_synchronously_without_irq() {
        let mut machine = Machine::new();
        initialize_detection_output(&mut machine);
        assert_eq!(set_detection_mode(&mut machine, DetectionMode::On), Ok(()));
        assert_eq!(detection_output(&mut machine, DetectionMode::On, Some(DetectionCallback(1))), Ok(()));
        assert_eq!(machine.delivered, 1);
        assert_eq!(machine.attached, None);
    }
    #[test]
    fn cancelled_output_cannot_deliver_and_install_failure_is_returned() {
        let mut machine = Machine::new();
        initialize_detection_output(&mut machine);
        machine.attach_status = ServiceStatus(-42);
        assert_eq!(detection_output(&mut machine, DetectionMode::Off, Some(DetectionCallback(1))), Err(ServiceStatus(-42)));
        assert_eq!(detection_output(&mut machine, DetectionMode::Off, None), Ok(()));
        detection_interrupt(&mut machine, DetectionMode::Off);
        assert_eq!(machine.delivered, 0);
    }
    #[test]
    fn continuous_completion_remains_registered_until_cancelled() {
        let mut machine = Machine::new();
        initialize_detection_output(&mut machine);
        assert_eq!(continuous_detection_output(&mut machine, DetectionMode::Off, Some(DetectionCallback(1))), Ok(()));
        detection_interrupt(&mut machine, DetectionMode::Off);
        detection_interrupt(&mut machine, DetectionMode::Off);
        assert_eq!(machine.delivered, 2);
        assert_eq!(continuous_detection_output(&mut machine, DetectionMode::Off, None), Ok(()));
        detection_interrupt(&mut machine, DetectionMode::Off);
        assert_eq!(machine.delivered, 2);
    }
}
