use super::*;
use core::cell::Cell;
use crate::eeprom_task100::CallbackRange;
use std::collections::VecDeque;
use std::vec::Vec;

struct Device {
    input: Cell<u32>,
    samples: RefCell<VecDeque<u32>>,
    clocks: RefCell<Vec<(DetectionRegister, u32)>>,
    dma: RefCell<Vec<(LensDmaRegister, u32)>>,
    attached: RefCell<Vec<(DetectionIrqToken, DetectionSource)>>,
    next_token: Cell<u32>,
    attach_error: Cell<Option<ServiceStatus>>,
    release_error: Cell<Option<ServiceStatus>>,
}
impl Device {
    fn new() -> Self {
        Self { input: Cell::new(0), samples: RefCell::new(VecDeque::new()), clocks: RefCell::new(Vec::new()), dma: RefCell::new(Vec::new()), attached: RefCell::new(Vec::new()), next_token: Cell::new(1), attach_error: Cell::new(None), release_error: Cell::new(None) }
    }
}
impl DetectionIo for Device {
    type SavedCritical = ();
    fn enter(&self) {}
    fn leave(&self, _: ()) {}
    fn write_register(&self, register: DetectionRegister, value: u32) { self.clocks.borrow_mut().push((register, value)); }
    fn read_register(&self, _: DetectionRegister) -> u32 { self.samples.borrow_mut().pop_front().unwrap_or(self.input.get()) }
    fn write_dma_register(&self, register: LensDmaRegister, value: u32) { self.dma.borrow_mut().push((register, value)); }
    fn attach_irq(&self, source: DetectionSource) -> Result<DetectionIrqToken, ServiceStatus> {
        if let Some(error) = self.attach_error.get() { return Err(error); }
        let token = DetectionIrqToken(self.next_token.get());
        self.next_token.set(token.0 + 1);
        self.attached.borrow_mut().push((token, source));
        Ok(token)
    }
    fn release_irq(&self, token: DetectionIrqToken) -> Result<(), ServiceStatus> {
        if let Some(error) = self.release_error.get() { return Err(error); }
        self.attached.borrow_mut().retain(|(attached, _)| *attached != token);
        Ok(())
    }
}
fn events() -> EepromEvents { EepromEvents::new(CallbackRange::new(CallbackId(1), CallbackId(100)).unwrap()) }

#[test]
fn uninitialized_provider_does_not_attach_or_complete() {
    let state = RefCell::new(DetectionState::new());
    let device = Device::new();
    let events = events();
    let provider = DetectionProvider::new(&state, &device, &events);
    let callback = events.arm(LensCompletion::Dma).unwrap();
    assert_eq!(provider.prepare_service(), Err(ServiceStatus(-30)));
    assert_eq!(provider.register_dma_callback(Some(callback)), Err(ServiceStatus(-30)));
    assert!(device.attached.borrow().is_empty());
    assert!(!provider.interrupt(DetectionSource::Low, callback));
    assert!(events.signal(callback, LensCompletion::Dma, MediumStatus(0)));
}

#[test]
fn initialization_programs_clocks_and_service_mode_defers_dma_until_irq() {
    let state = RefCell::new(DetectionState::new());
    let device = Device::new();
    let events = events();
    let provider = DetectionProvider::new(&state, &device, &events);
    assert_eq!(provider.initialize(), Ok(()));
    assert_eq!(&*device.clocks.borrow(), &[(DetectionRegister::SetClock, 0x800000), (DetectionRegister::ClearClock, 0x800000)]);
    assert_eq!(provider.prepare_service(), Ok(()));
    let callback = events.arm(LensCompletion::Dma).unwrap();
    assert_eq!(provider.register_dma_callback(Some(callback)), Ok(()));
    assert_eq!(device.attached.borrow()[0].1, DetectionSource::Low);
    device.input.set(0x80);
    assert!(!provider.interrupt_current(DetectionSource::Low));
    device.input.set(0);
    assert!(provider.interrupt_current(DetectionSource::Low));
    assert!(!events.signal(callback, LensCompletion::Dma, MediumStatus(0)));
    assert!(!provider.interrupt_current(DetectionSource::Low));
    assert_eq!(provider.cancel(), Ok(()));
    assert!(device.attached.borrow().is_empty());
}

#[test]
fn changing_input_during_qualification_does_not_complete_dma() {
    let state = RefCell::new(DetectionState::new());
    let device = Device::new();
    let events = events();
    let provider = DetectionProvider::new(&state, &device, &events);
    provider.initialize().unwrap();
    let callback = events.arm(LensCompletion::Dma).unwrap();
    provider.register_dma_callback(Some(callback)).unwrap();
    device.samples.borrow_mut().extend([0, 0, 0, 0, 0x80]);
    assert!(!provider.interrupt_current(DetectionSource::Low));
    assert!(events.signal(callback, LensCompletion::Dma, MediumStatus(0)));
}

#[test]
fn cancellation_and_rearm_reject_previous_generation_and_wrong_source() {
    let state = RefCell::new(DetectionState::new());
    let device = Device::new();
    let events = events();
    let provider = DetectionProvider::new(&state, &device, &events);
    provider.initialize().unwrap();
    let previous = events.arm(LensCompletion::Dma).unwrap();
    provider.register_dma_callback(Some(previous)).unwrap();
    provider.cancel().unwrap();
    assert!(!events.signal(previous, LensCompletion::Dma, MediumStatus(0)));
    assert!(!provider.interrupt(DetectionSource::Low, previous));
    assert!(device.attached.borrow().is_empty());
    let current = events.arm(LensCompletion::Dma).unwrap();
    assert_ne!(previous, current);
    provider.register_dma_callback(Some(current)).unwrap();
    assert!(!provider.interrupt(DetectionSource::Low, previous));
    assert!(!provider.interrupt(DetectionSource::High, current));
    assert!(!events.signal(previous, LensCompletion::Dma, MediumStatus(0)));
    assert!(provider.interrupt(DetectionSource::Low, current));
}

#[test]
fn one_shot_probe_and_high_mode_attach_their_physical_sources() {
    for (mode, source) in [(DetectionMode::Probe, DetectionSource::Probe), (DetectionMode::On, DetectionSource::High)] {
        let state = RefCell::new(DetectionState::new());
        let device = Device::new();
        let events = events();
        let provider = DetectionProvider::new(&state, &device, &events);
        provider.initialize().unwrap();
        let callback = events.arm(LensCompletion::Dma).unwrap();
        provider.register(mode, Some(callback), DetectionKind::Once).unwrap();
        assert_eq!(device.attached.borrow()[0].1, source);
        assert!(!provider.interrupt(source, callback));
        device.input.set(0x80);
        assert!(provider.interrupt(source, callback));
        assert!(!provider.interrupt(source, callback));
        provider.cancel().unwrap();
        assert!(device.attached.borrow().is_empty());
    }
}

#[test]
fn attachment_failure_invalidates_callback_and_preserves_status() {
    let state = RefCell::new(DetectionState::new());
    let device = Device::new();
    let events = events();
    let provider = DetectionProvider::new(&state, &device, &events);
    provider.initialize().unwrap();
    let callback = events.arm(LensCompletion::Dma).unwrap();
    device.attach_error.set(Some(ServiceStatus(-42)));
    assert_eq!(provider.register_dma_callback(Some(callback)), Err(ServiceStatus(-42)));
    assert_eq!(provider.active_callback(), None);
    assert!(!provider.interrupt(DetectionSource::Low, callback));
    assert!(device.attached.borrow().is_empty());
    assert!(events.signal(callback, LensCompletion::Dma, MediumStatus(-42)));
}

#[test]
fn failed_detachment_keeps_handle_for_cleanup_without_delivering_cancelled_callback() {
    let state = RefCell::new(DetectionState::new());
    let device = Device::new();
    let events = events();
    let provider = DetectionProvider::new(&state, &device, &events);
    provider.initialize().unwrap();
    let callback = events.arm(LensCompletion::Dma).unwrap();
    provider.register_dma_callback(Some(callback)).unwrap();
    device.release_error.set(Some(ServiceStatus(-43)));
    assert_eq!(provider.cancel(), Err(ServiceStatus(-43)));
    assert!(!provider.interrupt(DetectionSource::Low, callback));
    device.release_error.set(None);
    provider.cancel().unwrap();
    assert!(device.attached.borrow().is_empty());
}

#[test]
fn dma_writes_reach_the_physical_register_boundary() {
    let state = RefCell::new(DetectionState::new());
    let device = Device::new();
    let events = events();
    let provider = DetectionProvider::new(&state, &device, &events);
    provider.write_dma_register(LensDmaRegister::Control, 0x10000000);
    provider.write_dma_register(LensDmaRegister::Mask, 0x03000000);
    provider.write_dma_register(LensDmaRegister::Output, 0x10001000);
    assert_eq!(&*device.dma.borrow(), &[(LensDmaRegister::Control, 0x10000000), (LensDmaRegister::Mask, 0x03000000), (LensDmaRegister::Output, 0x10001000)]);
}

#[test]
fn shared_detection_routes_actuator_irq_and_ignores_stale_eeprom_cancellation() {
    let state = RefCell::new(DetectionState::new());
    let device = Device::new();
    let eeprom_events = events();
    let actuator_events = ActuatorEvents::new();
    let eeprom = DetectionProvider::new(&state, &device, &eeprom_events);
    let actuator = DetectionProvider::for_actuator(&state, &device, &actuator_events);
    eeprom.initialize().unwrap();
    actuator.initialize().unwrap();
    assert_eq!(device.clocks.borrow().len(), 2);
    let callback = eeprom_events.arm(LensCompletion::Dma).unwrap();
    eeprom.register_dma_callback(Some(callback)).unwrap();
    actuator_events.arm(callback);
    actuator.register(DetectionMode::On, Some(callback), DetectionKind::Once).unwrap();
    eeprom.cancel().unwrap();
    assert!(!eeprom_events.signal(callback, LensCompletion::Dma, MediumStatus(0)));
    assert_eq!(device.attached.borrow().len(), 1);
    assert_eq!(device.attached.borrow()[0].1, DetectionSource::High);
    device.input.set(0x80);
    assert_eq!(actuator_events.take(), None);
    assert!(!eeprom.interrupt(DetectionSource::High, callback));
    assert!(actuator.interrupt(DetectionSource::High, callback));
    assert_eq!(actuator_events.take(), Some(callback));
    assert_eq!(actuator_events.take(), None);
    actuator.cancel().unwrap();
    assert!(device.attached.borrow().is_empty());
}

#[test]
fn stale_actuator_cancellation_does_not_detach_eeprom_dma() {
    let state = RefCell::new(DetectionState::new());
    let device = Device::new();
    let eeprom_events = events();
    let actuator_events = ActuatorEvents::new();
    let eeprom = DetectionProvider::new(&state, &device, &eeprom_events);
    let actuator = DetectionProvider::for_actuator(&state, &device, &actuator_events);
    actuator.initialize().unwrap();
    let callback = eeprom_events.arm(LensCompletion::Dma).unwrap();
    actuator_events.arm(callback);
    actuator.register(DetectionMode::On, Some(callback), DetectionKind::Once).unwrap();
    eeprom.register_dma_callback(Some(callback)).unwrap();
    actuator.cancel().unwrap();
    assert_eq!(actuator_events.take(), None);
    assert_eq!(device.attached.borrow().len(), 1);
    assert_eq!(device.attached.borrow()[0].1, DetectionSource::Low);
    assert!(!actuator.interrupt(DetectionSource::Low, callback));
    assert!(eeprom.interrupt(DetectionSource::Low, callback));
}

#[test]
fn set_mode_uses_canonical_cancellation_and_immediate_enabled_mode_completion() {
    let state = RefCell::new(DetectionState::new());
    let device = Device::new();
    let events = ActuatorEvents::new();
    let actuator = DetectionProvider::for_actuator(&state, &device, &events);
    actuator.initialize().unwrap();
    let cancelled = CallbackId(7);
    events.arm(cancelled);
    actuator.register(DetectionMode::On, Some(cancelled), DetectionKind::Once).unwrap();
    actuator.set_mode(DetectionMode::On).unwrap();
    assert!(device.attached.borrow().is_empty());
    device.input.set(0x80);
    assert!(!actuator.interrupt(DetectionSource::High, cancelled));
    assert_eq!(events.take(), None);
    let immediate = CallbackId(8);
    events.arm(immediate);
    actuator.register(DetectionMode::On, Some(immediate), DetectionKind::Once).unwrap();
    assert!(device.attached.borrow().is_empty());
    assert_eq!(events.take(), Some(immediate));
}

struct Task99Queue {
    messages: std::sync::Mutex<Vec<ControlMessage>>,
    fail: core::sync::atomic::AtomicBool,
}
impl Task99Queue {
    const fn new() -> Self { Self { messages: std::sync::Mutex::new(Vec::new()), fail: core::sync::atomic::AtomicBool::new(false) } }
}
impl ControlSender for Task99Queue {
    fn post(&self, message: ControlMessage) -> Result<(), ControlError> {
        if self.fail.load(core::sync::atomic::Ordering::Acquire) { return Err(ControlError::Queue); }
        self.messages.lock().unwrap().push(message);
        Ok(())
    }
}

#[test]
fn task99_physical_callbacks_enqueue_device_completion_before_actor_transition() {
    use xm1_lens_transport::control_types::{ControlPower, DetectionMode as Mode};
    for (mode, completion, source, callback) in [
        (Mode::Off, DetectionCompletion::Off, DetectionSource::Low, DeviceCallback::DetectionOff),
        (Mode::On, DetectionCompletion::On, DetectionSource::High, DeviceCallback::DetectionOn),
        (Mode::Probe, DetectionCompletion::Probe, DetectionSource::Probe, DeviceCallback::DetectionOn),
    ] {
        let state = RefCell::new(DetectionState::new());
        let device = Device::new();
        let events = Task99DetectionEvents::new();
        static SENDER: Task99Queue = Task99Queue::new();
        let sender = &SENDER;
        sender.messages.lock().unwrap().clear();
        events.bind_sender(sender).unwrap();
        let provider = DetectionProvider::for_task99(&state, &device, &events);
        let mut power = Task99DetectionPower::new(&state, &device, &events);
        provider.initialize().unwrap();
        power.detection_output(mode, Some(completion));
        assert_eq!(power.take_failure(), None);
        assert!(sender.messages.lock().unwrap().is_empty());
        assert_eq!(device.attached.borrow()[0].1, source);
        device.input.set(if source == DetectionSource::Low { 0 } else { 0x80 });
        assert!(provider.interrupt_current(source));
        assert_eq!(&*sender.messages.lock().unwrap(), &[ControlMessage {
            opcode: ControlOpcode::Poll,
            payload: ControlPayload::DeviceCompletion { callback, status: CompletionStatus::SUCCESS, data: [0; 16] },
        }]);
        assert!(!provider.interrupt_current(source));
        power.detection_output(mode, None);
        assert!(device.attached.borrow().is_empty());
    }
}

#[test]
fn task99_callback_generation_is_unique_and_cancellation_rejects_old_signal() {
    let events = Task99DetectionEvents::new();
    static SENDER: Task99Queue = Task99Queue::new();
    let sender = &SENDER;
    assert_eq!(events.arm(DetectionCompletion::Off), Err(ServiceStatus(-30)));
    events.bind_sender(sender).unwrap();
    let previous = events.arm(DetectionCompletion::Off).unwrap();
    assert_eq!(previous, CallbackId(0x30000000));
    events.cancel();
    let current = events.arm(DetectionCompletion::On).unwrap();
    assert_ne!(previous, current);
    assert!(!events.signal(previous));
    assert!(sender.messages.lock().unwrap().is_empty());
    assert!(events.signal(current));
    assert_eq!(sender.messages.lock().unwrap().len(), 1);
}

#[test]
fn task99_reports_real_queue_failure_without_synthesizing_an_actor_transition() {
    let state = RefCell::new(DetectionState::new());
    let device = Device::new();
    let events = Task99DetectionEvents::new();
    static SENDER: Task99Queue = Task99Queue::new();
    let sender = &SENDER;
    sender.fail.store(true, core::sync::atomic::Ordering::Release);
    events.bind_sender(sender).unwrap();
    let provider = DetectionProvider::for_task99(&state, &device, &events);
    provider.initialize().unwrap();
    let callback = events.arm(DetectionCompletion::Off).unwrap();
    provider.register(DetectionMode::Off, Some(callback), DetectionKind::Once).unwrap();
    assert!(!provider.interrupt_current(DetectionSource::Low));
    assert_eq!(events.take_failure(), Some(ControlError::Queue));
    assert_eq!(events.take_failure(), None);
    assert!(sender.messages.lock().unwrap().is_empty());
}

#[test]
fn stale_task99_cancellation_preserves_active_actuator_registration() {
    use xm1_lens_transport::control_types::{ControlPower, DetectionMode as Mode};
    let state = RefCell::new(DetectionState::new());
    let device = Device::new();
    let task99 = Task99DetectionEvents::new();
    let actuator_events = ActuatorEvents::new();
    static SENDER: Task99Queue = Task99Queue::new();
    let sender = &SENDER;
    task99.bind_sender(sender).unwrap();
    let mut power = Task99DetectionPower::new(&state, &device, &task99);
    let actuator = DetectionProvider::for_actuator(&state, &device, &actuator_events);
    actuator.initialize().unwrap();
    power.detection_output(Mode::Off, Some(DetectionCompletion::Off));
    let callback = CallbackId(0x30000000);
    actuator_events.arm(callback);
    actuator.register(DetectionMode::On, Some(callback), DetectionKind::Once).unwrap();
    power.detection_output(Mode::Off, None);
    assert_eq!(power.take_failure(), None);
    assert_eq!(device.attached.borrow().len(), 1);
    assert_eq!(device.attached.borrow()[0].1, DetectionSource::High);
    device.input.set(0x80);
    assert!(actuator.interrupt_current(DetectionSource::High));
    assert_eq!(actuator_events.take(), Some(callback));
    assert!(sender.messages.lock().unwrap().is_empty());
}

unsafe extern "C" fn detection_irq() {}

#[test]
fn detection_handler_registers_before_the_timer_entry_resolves_it() {
    let handler = detection_irq as unsafe extern "C" fn();
    let word = register_handler(handler).unwrap();
    assert_eq!(word, handler.word());
    assert_eq!(callbacks().resolve::<unsafe extern "C" fn()>(word).map(|callback| callback.word()), Ok(word));
}
