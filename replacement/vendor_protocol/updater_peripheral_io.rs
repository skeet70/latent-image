use core::cell::Cell;
pub use super::gpio::GpioError;
#[cfg(target_arch = "arm")]
pub use super::gpio::GpioCompletion;
use xm1_power_services::primitives::ServiceStatus;
use xm1_power_services::software_timer_service::{self as detection, DetectionCallback, DetectionField, DetectionIrqToken, DetectionMode, DetectionOutputBackend, DetectionRegister};

mod sealed { pub trait Io {} }
pub trait PeripheralIo: sealed::Io {
    fn tick(&self) -> u32;
    fn output(&self, value: u32);
    fn arm(&mut self) -> Result<(), GpioError>;
    fn disarm(&mut self);
    fn take_interrupt(&mut self) -> bool;
}

pub struct HostSignals {
    tick: Cell<u32>,
    armed: Cell<Option<InterruptToken>>,
    pending: Cell<bool>,
    next: Cell<u32>,
    output: Cell<u32>,
    claimed: Cell<bool>,
    fields: [Cell<u32>; 5],
    registers: [Cell<u32>; 3],
    samples: Cell<[bool; 10]>,
    sample: Cell<usize>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InterruptToken(u32);
impl HostSignals {
    pub const fn new() -> Self {
        Self { tick: Cell::new(0), armed: Cell::new(None), pending: Cell::new(false), next: Cell::new(1), output: Cell::new(0), claimed: Cell::new(false), fields: [const { Cell::new(0) }; 5], registers: [const { Cell::new(0) }; 3], samples: Cell::new([false; 10]), sample: Cell::new(0) }
    }
    pub fn advance(&self, ticks: u32) { self.tick.set(self.tick.get().wrapping_add(ticks)); }
    pub fn armed(&self) -> Option<InterruptToken> { self.armed.get() }
    pub fn interrupt(&self, token: InterruptToken, samples: [bool; 10]) -> bool {
        if self.armed.get() != Some(token) { return false; }
        self.samples.set(samples);
        self.sample.set(0);
        detection::detection_interrupt(&mut HostBackend(self), DetectionMode::On);
        true
    }
    pub fn output(&self) -> u32 { self.output.get() }
    pub fn port(&self) -> Result<HostPeripheralIo<'_>, GpioError> {
        if self.claimed.replace(true) { return Err(GpioError::Busy); }
        detection::initialize_detection_output(&mut HostBackend(self));
        Ok(HostPeripheralIo(self))
    }
}
fn field_index(field: DetectionField) -> usize {
    match field { DetectionField::Phase => 0, DetectionField::Callback => 1, DetectionField::IrqToken => 2, DetectionField::Mode => 3, DetectionField::ClockEnabled => 4 }
}
fn register_index(register: DetectionRegister) -> usize {
    match register { DetectionRegister::SetClock => 0, DetectionRegister::ClearClock => 1, DetectionRegister::Input => 2 }
}
struct HostBackend<'a>(&'a HostSignals);
impl DetectionOutputBackend for HostBackend<'_> {
    type SavedCritical = ();
    fn enter(&mut self) {}
    fn leave(&mut self, _: ()) {}
    fn load(&mut self, field: DetectionField) -> u32 { self.0.fields[field_index(field)].get() }
    fn store(&mut self, field: DetectionField, value: u32) { self.0.fields[field_index(field)].set(value); }
    fn write_register(&mut self, register: DetectionRegister, value: u32) { self.0.registers[register_index(register)].set(value); }
    fn read_register(&mut self, register: DetectionRegister) -> u32 {
        if register != DetectionRegister::Input { return self.0.registers[register_index(register)].get(); }
        let index = self.0.sample.get();
        self.0.sample.set(index + 1);
        if self.0.samples.get().get(index).copied().unwrap_or(false) { 0x80 } else { 0 }
    }
    fn release_irq(&mut self, token: DetectionIrqToken) -> Result<(), ServiceStatus> {
        if self.0.armed.get() != Some(InterruptToken(token.0)) { return Err(ServiceStatus(-255)); }
        self.0.armed.set(None);
        Ok(())
    }
    fn attach_irq(&mut self, _: DetectionMode) -> Result<DetectionIrqToken, ServiceStatus> {
        let next = self.0.next.get();
        self.0.next.set(next.checked_add(1).ok_or(ServiceStatus(-255))?);
        self.0.armed.set(Some(InterruptToken(next)));
        Ok(DetectionIrqToken(next))
    }
    fn invoke(&mut self, _: DetectionCallback) { self.0.pending.set(true); }
}
pub struct HostPeripheralIo<'a>(&'a HostSignals);
impl sealed::Io for HostPeripheralIo<'_> {}
impl PeripheralIo for HostPeripheralIo<'_> {
    fn tick(&self) -> u32 { self.0.tick.get() }
    fn output(&self, value: u32) { self.0.output.set(value); }
    fn arm(&mut self) -> Result<(), GpioError> {
        self.disarm();
        let mut backend = HostBackend(self.0);
        detection::set_detection_mode(&mut backend, DetectionMode::Off).map_err(|status| GpioError::Attach(super::gpio::TimerStatus::new(status.0)))?;
        detection::continuous_detection_output(&mut backend, DetectionMode::On, Some(DetectionCallback(1))).map_err(|status| GpioError::Attach(super::gpio::TimerStatus::new(status.0)))
    }
    fn disarm(&mut self) {
        let _ = detection::continuous_detection_output(&mut HostBackend(self.0), DetectionMode::On, None);
        self.0.pending.set(false);
    }
    fn take_interrupt(&mut self) -> bool { self.0.pending.replace(false) }
}
impl Drop for HostPeripheralIo<'_> { fn drop(&mut self) { self.disarm(); self.0.claimed.set(false); } }

#[cfg(target_arch = "arm")]
impl sealed::Io for GpioCompletion {}
#[cfg(target_arch = "arm")]
impl PeripheralIo for GpioCompletion {
    fn tick(&self) -> u32 { unsafe { core::ptr::read_volatile(0x400c0800 as *const u32) } }
    fn output(&self, value: u32) { unsafe { core::ptr::write_volatile(0xfff89080 as *mut u32, value); } }
    fn arm(&mut self) -> Result<(), GpioError> { Self::arm(self) }
    fn disarm(&mut self) { Self::disarm(self); }
    fn take_interrupt(&mut self) -> bool { Self::take_interrupt(self).is_some() }
}
