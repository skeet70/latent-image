use core::cell::RefCell;
use crate::eeprom_task100::EepromEvents;
use crate::lens_queue_runtime::packets::CallbackId;
use crate::lens_actuator_cpu::ActuatorEvents;
use xm1_lens_transport::control_device::DeviceCallback;
use xm1_lens_transport::control_runtime::ControlSender;
use xm1_lens_transport::control_types::{CompletionStatus, ControlError, ControlMessage, ControlOpcode, ControlPayload, DetectionCompletion};
pub use xm1_power_services::primitives::ServiceStatus;
use xm1_power_services::callback_registry::{registry as callbacks, CallbackError, CallbackFn, CallbackWord};
use xm1_power_services::software_timer_service::{self as detection, DetectionCallback, DetectionField, DetectionIrqToken, DetectionOutputBackend, DetectionRegister};
pub use xm1_power_services::software_timer_service::DetectionMode;
use xm1_power_services::storage_shutdown_actor::MediumStatus;
use xm1_power_services::storage_shutdown_lifecycle::{LensCompletion, LensDmaRegister};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DetectionSource { High, Low, Probe }
impl DetectionSource {
    pub const fn number(self) -> u32 { self.mode().irq_source().index() as u32 }
    pub const fn mode(self) -> DetectionMode { match self { Self::High => DetectionMode::On, Self::Low => DetectionMode::Off, Self::Probe => DetectionMode::Probe } }
    pub const fn from_mode(mode: DetectionMode) -> Self { match mode { DetectionMode::Off => Self::Low, DetectionMode::On => Self::High, DetectionMode::Probe => Self::Probe } }
}

/// Registers a detection handler so the timer entry resolves it, and returns its word.
pub fn register_handler(handler: unsafe extern "C" fn()) -> Result<CallbackWord, CallbackError> {
    callbacks().ensure(handler)?;
    Ok(handler.word())
}
pub trait DetectionIo {
    type SavedCritical;
    fn enter(&self) -> Self::SavedCritical;
    fn leave(&self, saved: Self::SavedCritical);
    fn write_register(&self, register: DetectionRegister, value: u32);
    fn read_register(&self, register: DetectionRegister) -> u32;
    fn write_dma_register(&self, register: LensDmaRegister, value: u32);
    fn attach_irq(&self, source: DetectionSource) -> Result<DetectionIrqToken, ServiceStatus>;
    fn release_irq(&self, token: DetectionIrqToken) -> Result<(), ServiceStatus>;
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase { Uninitialized, Ready, Once, Continuous }
impl Phase {
    fn word(self) -> u32 { match self { Self::Uninitialized => 0, Self::Ready => 1, Self::Once => 2, Self::Continuous => 3 } }
    fn from_word(value: u32) -> Self { match value { 0 => Self::Uninitialized, 1 => Self::Ready, 2 => Self::Once, 3 => Self::Continuous, _ => unreachable!() } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DetectionKind { Once, Continuous }

struct Task99EventState {
    sender: Option<&'static dyn ControlSender>,
    next: Option<CallbackId>,
    active: Option<(CallbackId, DetectionCompletion)>,
    failure: Option<ControlError>,
}
pub struct Task99DetectionEvents { state: RefCell<Task99EventState> }
impl Task99DetectionEvents {
    pub const fn new() -> Self {
        Self { state: RefCell::new(Task99EventState { sender: None, next: Some(CallbackId(0x30000000)), active: None, failure: None }) }
    }
    pub fn bind_sender(&self, sender: &'static dyn ControlSender) -> Result<(), ServiceStatus> {
        let _critical = Task99Critical::enter();
        let mut state = self.state.borrow_mut();
        if state.sender.is_some() { return Err(ServiceStatus(-255)); }
        state.sender = Some(sender);
        Ok(())
    }
    pub fn arm(&self, completion: DetectionCompletion) -> Result<CallbackId, ServiceStatus> {
        let _critical = Task99Critical::enter();
        let mut state = self.state.borrow_mut();
        if state.sender.is_none() { return Err(ServiceStatus(-30)); }
        if state.active.is_some() { return Err(ServiceStatus(-255)); }
        let callback = state.next.ok_or(ServiceStatus(-255))?;
        state.next = if callback.0 == 0x3fffffff { None } else { Some(CallbackId(callback.0 + 1)) };
        state.active = Some((callback, completion));
        Ok(callback)
    }
    pub fn cancel(&self) {
        let _critical = Task99Critical::enter();
        self.state.borrow_mut().active = None;
    }
    pub fn signal(&self, callback: CallbackId) -> bool {
        let _critical = Task99Critical::enter();
        let pending = {
            let mut state = self.state.borrow_mut();
            let Some((active, completion)) = state.active else { return false; };
            if active != callback { return false; }
            state.active = None;
            state.sender.map(|sender| (sender, completion))
        };
        let Some((sender, completion)) = pending else { return false; };
        let callback = match completion { DetectionCompletion::Off => DeviceCallback::DetectionOff, DetectionCompletion::On | DetectionCompletion::Probe => DeviceCallback::DetectionOn };
        match sender.post(ControlMessage { opcode: ControlOpcode::Poll, payload: ControlPayload::DeviceCompletion { callback, status: CompletionStatus::SUCCESS, data: [0; 16] } }) {
            Ok(()) => true,
            Err(error) => { self.state.borrow_mut().failure = Some(error); false }
        }
    }
    pub fn take_failure(&self) -> Option<ControlError> {
        let _critical = Task99Critical::enter();
        self.state.borrow_mut().failure.take()
    }
}
impl Default for Task99DetectionEvents { fn default() -> Self { Self::new() } }
struct Task99Critical {
    #[cfg(target_arch = "arm")]
    saved: u32,
}
impl Task99Critical {
    fn enter() -> Self {
        #[cfg(target_arch = "arm")]
        {
            extern "C" { fn xm1_power_interrupt_lock() -> u32; }
            Self { saved: unsafe { xm1_power_interrupt_lock() } }
        }
        #[cfg(not(target_arch = "arm"))]
        Self {}
    }
}
impl Drop for Task99Critical {
    fn drop(&mut self) {
        #[cfg(target_arch = "arm")]
        {
            extern "C" { fn xm1_power_interrupt_unlock(saved: u32); }
            unsafe { xm1_power_interrupt_unlock(self.saved) };
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DetectionOwner { Eeprom, Actuator, Task99 }
#[derive(Clone, Copy)]
pub enum DetectionRecipient<'a> { Eeprom(&'a EepromEvents), Actuator(&'a ActuatorEvents), Task99(&'a Task99DetectionEvents) }
impl DetectionRecipient<'_> {
    fn owner(self) -> DetectionOwner { match self { Self::Eeprom(_) => DetectionOwner::Eeprom, Self::Actuator(_) => DetectionOwner::Actuator, Self::Task99(_) => DetectionOwner::Task99 } }
    fn signal(self, callback: CallbackId) -> bool {
        match self { Self::Eeprom(events) => events.signal(callback, LensCompletion::Dma, MediumStatus(0)), Self::Actuator(events) => events.signal(callback), Self::Task99(events) => events.signal(callback) }
    }
    fn cancel(self) {
        match self { Self::Eeprom(events) => events.cancel_dma(), Self::Actuator(events) => events.cancel(), Self::Task99(events) => events.cancel() }
    }
}
pub struct DetectionState {
    initialized: bool,
    phase: Phase,
    callback: Option<DetectionCallback>,
    irq: Option<DetectionIrqToken>,
    source: Option<DetectionSource>,
    mode: DetectionMode,
    clock_enabled: bool,
    active: Option<CallbackId>,
    owner: Option<DetectionOwner>,
}
impl DetectionState {
    pub const fn new() -> Self {
        Self { initialized: false, phase: Phase::Uninitialized, callback: None, irq: None, source: None, mode: DetectionMode::Off, clock_enabled: false, active: None, owner: None }
    }
}
impl Default for DetectionState { fn default() -> Self { Self::new() } }

pub struct DetectionProvider<'a, B: DetectionIo> {
    state: &'a RefCell<DetectionState>,
    io: &'a B,
    recipient: DetectionRecipient<'a>,
}
impl<'a, B: DetectionIo> DetectionProvider<'a, B> {
    pub const fn new(state: &'a RefCell<DetectionState>, io: &'a B, events: &'a EepromEvents) -> Self { Self { state, io, recipient: DetectionRecipient::Eeprom(events) } }
    pub const fn for_actuator(state: &'a RefCell<DetectionState>, io: &'a B, events: &'a ActuatorEvents) -> Self { Self { state, io, recipient: DetectionRecipient::Actuator(events) } }
    pub const fn for_task99(state: &'a RefCell<DetectionState>, io: &'a B, events: &'a Task99DetectionEvents) -> Self { Self { state, io, recipient: DetectionRecipient::Task99(events) } }
    fn with_backend<R>(&self, operation: impl FnOnce(&mut Backend<'_, B>) -> R) -> (R, bool) {
        let saved = self.io.enter();
        let (result, pending) = {
            let mut state = self.state.borrow_mut();
            let mut backend = Backend { state: &mut state, io: self.io, pending: None, owner: self.recipient.owner() };
            let result = operation(&mut backend);
            (result, backend.pending)
        };
        let delivered = pending.is_some_and(|callback| self.recipient.signal(callback));
        self.io.leave(saved);
        (result, delivered)
    }
    pub fn initialize(&self) -> Result<(), ServiceStatus> {
        self.with_backend(|backend| {
            if backend.state.initialized { return Ok(()); }
            detection::initialize_detection_output(backend);
            backend.state.initialized = true;
            Ok(())
        }).0
    }
    pub fn prepare_service(&self) -> Result<(), ServiceStatus> { self.set_mode(DetectionMode::Off) }
    pub fn set_mode(&self, mode: DetectionMode) -> Result<(), ServiceStatus> {
        self.with_backend(|backend| {
            if !backend.state.initialized { return Err(ServiceStatus(-30)); }
            if mode != DetectionMode::Off { backend.state.active = None; }
            detection::set_detection_mode(backend, mode)
        }).0
    }
    pub fn register_dma_callback(&self, callback: Option<CallbackId>) -> Result<(), ServiceStatus> {
        self.register(DetectionMode::Off, callback, DetectionKind::Continuous)
    }
    pub fn register(&self, mode: DetectionMode, callback: Option<CallbackId>, kind: DetectionKind) -> Result<(), ServiceStatus> {
        if callback.is_none() { self.recipient.cancel(); }
        self.with_backend(|backend| {
            if !backend.state.initialized || callback == Some(CallbackId(0)) { return Err(ServiceStatus(-30)); }
            if callback.is_none() && backend.state.owner != Some(backend.owner) { return Ok(()); }
            backend.state.owner = Some(backend.owner);
            backend.state.active = None;
            detection::continuous_detection_output(backend, mode, None)?;
            if callback.is_none() { return Ok(()); }
            backend.state.active = callback;
            let callback = callback.map(|callback| DetectionCallback(callback.0));
            let result = match kind {
                DetectionKind::Once => detection::detection_output(backend, mode, callback),
                DetectionKind::Continuous => detection::continuous_detection_output(backend, mode, callback),
            };
            if result.is_err() {
                backend.state.active = None;
                let _ = detection::continuous_detection_output(backend, mode, None);
            }
            result
        }).0
    }
    pub fn cancel(&self) -> Result<(), ServiceStatus> { self.register(DetectionMode::Off, None, DetectionKind::Continuous) }
    pub fn active_callback(&self) -> Option<CallbackId> {
        self.with_backend(|backend| if backend.state.owner == Some(backend.owner) { backend.state.active } else { None }).0
    }
    pub fn interrupt(&self, source: DetectionSource, callback: CallbackId) -> bool {
        self.with_backend(|backend| {
            if backend.state.owner != Some(backend.owner) || backend.state.active != Some(callback) || backend.state.source != Some(source) || backend.state.irq.is_none() { return; }
            detection::detection_interrupt(backend, source.mode());
        }).1
    }
    pub fn interrupt_current(&self, source: DetectionSource) -> bool {
        self.with_backend(|backend| {
            if backend.state.owner != Some(backend.owner) || backend.state.active.is_none() || backend.state.source != Some(source) || backend.state.irq.is_none() { return; }
            detection::detection_interrupt(backend, source.mode());
        }).1
    }
    pub fn write_dma_register(&self, register: LensDmaRegister, value: u32) {
        self.with_backend(|backend| backend.io.write_dma_register(register, value));
    }
}

pub struct Task99DetectionPower<'a, B: DetectionIo> {
    provider: DetectionProvider<'a, B>,
    events: &'a Task99DetectionEvents,
    failure: Option<ServiceStatus>,
}
impl<'a, B: DetectionIo> Task99DetectionPower<'a, B> {
    pub const fn new(state: &'a RefCell<DetectionState>, io: &'a B, events: &'a Task99DetectionEvents) -> Self {
        Self { provider: DetectionProvider::for_task99(state, io, events), events, failure: None }
    }
    pub fn take_failure(&mut self) -> Option<ServiceStatus> { self.failure.take() }
    fn output(&self, mode: DetectionMode, completion: Option<DetectionCompletion>) -> Result<(), ServiceStatus> {
        self.provider.cancel()?;
        let Some(completion) = completion else { return Ok(()); };
        let callback = self.events.arm(completion)?;
        let result = self.provider.register(mode, Some(callback), DetectionKind::Once);
        if result.is_err() { self.events.cancel(); }
        result
    }
}
fn task99_mode(mode: xm1_lens_transport::control_types::DetectionMode) -> DetectionMode {
    use xm1_lens_transport::control_types::DetectionMode as Mode;
    match mode { Mode::Off => DetectionMode::Off, Mode::On => DetectionMode::On, Mode::Probe => DetectionMode::Probe }
}
impl<B: DetectionIo> xm1_lens_transport::control_types::ControlPower for Task99DetectionPower<'_, B> {
    fn set_detection_mode(&mut self, mode: xm1_lens_transport::control_types::DetectionMode) {
        if let Err(error) = self.provider.set_mode(task99_mode(mode)) { self.failure = Some(error); }
    }
    fn detection_output(&mut self, mode: xm1_lens_transport::control_types::DetectionMode, completion: Option<DetectionCompletion>) {
        if let Err(error) = self.output(task99_mode(mode), completion) { self.failure = Some(error); }
    }
}
struct Backend<'a, B: DetectionIo> {
    state: &'a mut DetectionState,
    io: &'a B,
    pending: Option<CallbackId>,
    owner: DetectionOwner,
}
impl<B: DetectionIo> DetectionOutputBackend for Backend<'_, B> {
    type SavedCritical = B::SavedCritical;
    fn enter(&mut self) -> Self::SavedCritical { self.io.enter() }
    fn leave(&mut self, saved: Self::SavedCritical) { self.io.leave(saved); }
    fn load(&mut self, field: DetectionField) -> u32 {
        match field {
            DetectionField::Phase => self.state.phase.word(),
            DetectionField::Callback => self.state.callback.map_or(0, |callback| callback.0),
            DetectionField::IrqToken => self.state.irq.map_or(0, |token| token.0),
            DetectionField::Mode => self.state.mode as u32,
            DetectionField::ClockEnabled => u32::from(self.state.clock_enabled),
        }
    }
    fn store(&mut self, field: DetectionField, value: u32) {
        match field {
            DetectionField::Phase => self.state.phase = Phase::from_word(value),
            DetectionField::Callback => self.state.callback = if value == 0 { None } else { Some(DetectionCallback(value)) },
            DetectionField::IrqToken => self.state.irq = if value == 0 { None } else { Some(DetectionIrqToken(value)) },
            DetectionField::Mode => self.state.mode = DetectionMode::from_abi(value).unwrap(),
            DetectionField::ClockEnabled => self.state.clock_enabled = value != 0,
        }
    }
    fn write_register(&mut self, register: DetectionRegister, value: u32) { self.io.write_register(register, value); }
    fn read_register(&mut self, register: DetectionRegister) -> u32 { self.io.read_register(register) }
    fn release_irq(&mut self, token: DetectionIrqToken) -> Result<(), ServiceStatus> {
        match self.io.release_irq(token) {
            Ok(()) => { self.state.source = None; Ok(()) }
            Err(status) => { self.state.irq = Some(token); Err(status) }
        }
    }
    fn attach_irq(&mut self, mode: DetectionMode) -> Result<DetectionIrqToken, ServiceStatus> {
        if self.state.irq.is_some() { return Err(ServiceStatus(-255)); }
        let source = DetectionSource::from_mode(mode);
        let token = self.io.attach_irq(source)?;
        if token.0 == 0 { return Err(ServiceStatus(-255)); }
        self.state.source = Some(source);
        Ok(token)
    }
    fn invoke(&mut self, callback: DetectionCallback) {
        let callback = CallbackId(callback.0);
        if self.state.owner == Some(self.owner) && self.state.active == Some(callback) {
            self.pending = Some(callback);
        }
    }
}

#[cfg(target_arch = "arm")]
#[path = "eeprom_detection_native.rs"]
pub mod native;
#[cfg(test)]
#[path = "eeprom_detection_tests.rs"]
mod tests;
