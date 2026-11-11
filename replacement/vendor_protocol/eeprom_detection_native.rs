use super::*;
use core::cell::Cell;
use core::ptr;
use core::sync::atomic::{AtomicPtr, Ordering};

extern "C" {
    fn xm1_power_interrupt_lock() -> u32;
    fn xm1_power_interrupt_unlock(saved: u32);
    fn xm1_power_timer_irq_attach(source: u32, handler: u32, output: *mut u32) -> i32;
    fn xm1_power_timer_release_handle(handle: u32) -> i32;
}

static CONTEXT: AtomicPtr<NativeDetectionContext> = AtomicPtr::new(ptr::null_mut());

pub struct NativeDetectionIo { bound: Cell<bool> }
impl DetectionIo for NativeDetectionIo {
    type SavedCritical = u32;
    fn enter(&self) -> u32 { unsafe { xm1_power_interrupt_lock() } }
    fn leave(&self, saved: u32) { unsafe { xm1_power_interrupt_unlock(saved) }; }
    fn write_register(&self, register: DetectionRegister, value: u32) {
        unsafe { ptr::write_volatile(register.address() as *mut u32, value) };
    }
    fn read_register(&self, register: DetectionRegister) -> u32 {
        unsafe { ptr::read_volatile(register.address() as *const u32) }
    }
    fn write_dma_register(&self, register: LensDmaRegister, value: u32) {
        unsafe { ptr::write_volatile(register.address() as *mut u32, value) };
    }
    fn attach_irq(&self, source: DetectionSource) -> Result<DetectionIrqToken, ServiceStatus> {
        if !self.bound.get() { return Err(ServiceStatus(-30)); }
        let handler: unsafe extern "C" fn() = match source {
            DetectionSource::High => high_irq,
            DetectionSource::Low => low_irq,
            DetectionSource::Probe => probe_irq,
        };
        let handler = match register_handler(handler) { Ok(handler) => handler, Err(error) => return Err(ServiceStatus(error.status())) };
        let mut token = 0;
        let status = unsafe { xm1_power_timer_irq_attach(source.number(), handler.0 as u32, &mut token) };
        if status == 0 { Ok(DetectionIrqToken(token)) } else { Err(ServiceStatus(status)) }
    }
    fn release_irq(&self, token: DetectionIrqToken) -> Result<(), ServiceStatus> {
        let status = unsafe { xm1_power_timer_release_handle(token.0) };
        if status == 0 { Ok(()) } else { Err(ServiceStatus(status)) }
    }
}

pub struct NativeDetectionContext {
    state: RefCell<DetectionState>,
    io: NativeDetectionIo,
    events: EepromEvents,
    actuator_events: ActuatorEvents,
    task99_events: Task99DetectionEvents,
}
// X-M1 has one CPU; task and IRQ access is serialized by the IRQ lock.
unsafe impl Sync for NativeDetectionContext {}
impl NativeDetectionContext {
    pub const fn new(events: EepromEvents) -> Self {
        Self { state: RefCell::new(DetectionState::new()), io: NativeDetectionIo { bound: Cell::new(false) }, events, actuator_events: ActuatorEvents::new(), task99_events: Task99DetectionEvents::new() }
    }
    pub fn bind(&'static self) -> Result<(), ServiceStatus> {
        let saved = self.io.enter();
        let result = if CONTEXT.compare_exchange(ptr::null_mut(), self as *const Self as *mut Self, Ordering::AcqRel, Ordering::Acquire).is_err() {
            Err(ServiceStatus(-255))
        } else {
            let initialized = self.provider().initialize();
            self.io.bound.set(initialized.is_ok());
            initialized
        };
        self.io.leave(saved);
        result
    }
    pub fn provider(&self) -> DetectionProvider<'_, NativeDetectionIo> { DetectionProvider::new(&self.state, &self.io, &self.events) }
    pub fn events(&self) -> &EepromEvents { &self.events }
    pub fn actuator_provider(&self) -> DetectionProvider<'_, NativeDetectionIo> { DetectionProvider::for_actuator(&self.state, &self.io, &self.actuator_events) }
    pub fn actuator_events(&self) -> &ActuatorEvents { &self.actuator_events }
    pub fn task99_provider(&self) -> DetectionProvider<'_, NativeDetectionIo> { DetectionProvider::for_task99(&self.state, &self.io, &self.task99_events) }
    pub fn task99_power(&self) -> Task99DetectionPower<'_, NativeDetectionIo> { Task99DetectionPower::new(&self.state, &self.io, &self.task99_events) }
    pub fn task99_events(&self) -> &Task99DetectionEvents { &self.task99_events }
}
fn dispatch(source: DetectionSource) {
    let saved = unsafe { xm1_power_interrupt_lock() };
    let context = CONTEXT.load(Ordering::Acquire);
    if !context.is_null() {
        // bind installs only a permanent shared reference.
        let context = unsafe { &*context };
        let owner = context.state.borrow().owner;
        match owner {
            Some(DetectionOwner::Eeprom) => { let _ = context.provider().interrupt_current(source); }
            Some(DetectionOwner::Actuator) => { let _ = context.actuator_provider().interrupt_current(source); }
            Some(DetectionOwner::Task99) => { let _ = context.task99_provider().interrupt_current(source); }
            None => {}
        }
    }
    unsafe { xm1_power_interrupt_unlock(saved) };
}
unsafe extern "C" fn high_irq() { dispatch(DetectionSource::High); }
unsafe extern "C" fn low_irq() { dispatch(DetectionSource::Low); }
unsafe extern "C" fn probe_irq() { dispatch(DetectionSource::Probe); }
