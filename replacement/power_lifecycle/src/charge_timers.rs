use crate::primitives::ServiceStatus;
use crate::standby::SavedIrq;
use crate::osal::TimerReload;
use crate::serial_driver::StatusResidue;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct CallbackToken(pub i32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct InterfaceHandle(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct TimerSlot(u8);
impl TimerSlot { pub const fn index(self) -> usize { self.0 as usize } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimerStop { Complete, Failed }
impl TimerStop { pub const fn abi(self) -> i32 { match self { Self::Complete => 0, Self::Failed => -1 } } }

pub trait ChargeShutdown {
    fn callback_token(&mut self) -> CallbackToken;
    fn unregister_callback(&mut self, token: CallbackToken);
    fn lock_charge(&mut self);
    fn clear_charge_active(&mut self);
    fn unlock_charge(&mut self);
    fn send_charge_packet(&mut self, packet: [u8; 2]) -> ServiceStatus;
}

pub fn stop_charge(machine: &mut impl ChargeShutdown) -> ServiceStatus {
    let token = machine.callback_token();
    machine.unregister_callback(token);
    machine.lock_charge();
    machine.clear_charge_active();
    machine.unlock_charge();
    machine.send_charge_packet(charge_packet([0, 1, 0]))
}

pub const fn charge_packet(flags: [u8; 3]) -> [u8; 2] {
    [1, ((flags[0] & 1) << 2) | ((flags[1] & 1) << 1) | (flags[2] & 1)]
}

pub trait TimerShutdown {
    fn registered(&mut self) -> bool;
    fn send_unregister(&mut self) -> ServiceStatus;
    fn clear_registered(&mut self);
    fn lock_interrupts(&mut self) -> SavedIrq;
    fn slot_handle(&mut self, slot: TimerSlot) -> Option<InterfaceHandle>;
    fn release_handle(&mut self, handle: InterfaceHandle) -> ServiceStatus;
    fn clear_slot(&mut self, slot: TimerSlot);
    fn restore_interrupts(&mut self, saved: SavedIrq);
}

pub fn stop_timers(machine: &mut impl TimerShutdown) -> TimerStop {
    let mut errors = 0;
    if machine.registered() {
        let status = machine.send_unregister();
        errors |= status.0;
        if status.0 == 0 { machine.clear_registered(); }
    }
    for index in 0..5 {
        let slot = TimerSlot(index);
        let saved = machine.lock_interrupts();
        if let Some(handle) = machine.slot_handle(slot) {
            errors |= machine.release_handle(handle).0;
            machine.clear_slot(slot);
        }
        machine.restore_interrupts(saved);
    }
    if errors == 0 { TimerStop::Complete } else { TimerStop::Failed }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct SerialHandle(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SerialStatus { pub state: u32, pub available: u32 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct DelayMicros(pub u32);

pub trait ChargeTransport {
    fn open_charge_serial(&mut self) -> Result<SerialHandle, ServiceStatus>;
    fn write_charge_pin(&mut self, value: u32);
    fn configure(&mut self, handle: SerialHandle) -> (ServiceStatus, StatusResidue);
    fn send_byte(&mut self, handle: SerialHandle, byte: u8, residue: StatusResidue) -> ServiceStatus;
    fn status(&mut self, handle: SerialHandle) -> (ServiceStatus, SerialStatus);
    fn read_response(&mut self, handle: SerialHandle) -> ServiceStatus;
    fn delay(&mut self, duration: DelayMicros) -> TimerReload;
    fn stop(&mut self, handle: SerialHandle) -> ServiceStatus;
    fn close(&mut self, handle: SerialHandle) -> ServiceStatus;
}

pub fn transmit_charge(machine: &mut impl ChargeTransport, packet: &[u8]) -> ServiceStatus {
    let opened = machine.open_charge_serial();
    machine.write_charge_pin(0x0004_0004);
    let result = match opened {
        Err(status) => status,
        Ok(handle) => {
            let (configured, mut residue) = machine.configure(handle);
            let mut errors = configured.0;
            for (index, &byte) in packet.iter().enumerate() {
                errors |= machine.send_byte(handle, byte, residue).0;
                loop {
                    let (status, serial) = machine.status(handle);
                    errors |= status.0;
                    if serial.state == 1 && serial.available != 0 { break; }
                }
                errors |= machine.read_response(handle).0;
                if index + 1 < packet.len() {
                    for _ in 0..4 { residue = StatusResidue(machine.delay(DelayMicros(200)).0); }
                }
            }
            machine.delay(DelayMicros(40));
            let stop = machine.stop(handle);
            let close = machine.close(handle);
            ServiceStatus(errors | stop.0 | close.0)
        }
    };
    machine.write_charge_pin(0x0004_0000);
    result
}

pub trait TimerHandleRegistry {
    fn handle(&mut self, index: usize) -> InterfaceHandle;
    fn prepare_interface(&mut self, index: usize) -> ServiceStatus;
    fn invalidate_interface(&mut self, index: usize);
    fn clear_handle(&mut self, index: usize);
}

pub fn release_timer_handle(registry: &mut impl TimerHandleRegistry, handle: InterfaceHandle) -> ServiceStatus {
    if handle.0 == 0 { return ServiceStatus(-20); }
    for index in 0..128 {
        if registry.handle(index) == handle {
            let status = registry.prepare_interface(index);
            registry.invalidate_interface(index);
            registry.clear_handle(index);
            return ServiceStatus(if status.0 == 0 { 0 } else { -255 });
        }
    }
    ServiceStatus(-20)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct IrqSource(u8);
impl IrqSource {
    pub const fn from_abi(source: u32) -> Option<Self> { if source <= 122 { Some(Self(source as u8)) } else { None } }
    pub const fn index(self) -> usize { self.0 as usize }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct IrqHandler(u32);
impl IrqHandler {
    pub const fn from_abi(address: u32) -> Option<Self> { if address == 0 { None } else { Some(Self(address)) } }
    pub const fn address(self) -> u32 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IrqTrigger { Mode0, Mode1, Mode2, Mode3 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IrqRoute { slot: u8, priority: u8, trigger: IrqTrigger }
impl IrqRoute {
    pub const fn from_abi(slot: u32, priority: u32, trigger: u32) -> Option<Self> {
        if slot > 127 || priority > 15 { return None; }
        let trigger = match trigger { 0 => IrqTrigger::Mode0, 1 => IrqTrigger::Mode1, 2 => IrqTrigger::Mode2, 3 => IrqTrigger::Mode3, _ => return None };
        Some(Self { slot: slot as u8, priority: priority as u8, trigger })
    }
    pub const fn slot(self) -> usize { self.slot as usize }
    pub const fn priority(self) -> u32 { self.priority as u32 }
    pub const fn trigger(self) -> IrqTrigger { self.trigger }
}
pub trait TimerIrqRegistry: TimerHandleRegistry {
    fn route(&mut self, source: IrqSource) -> Option<IrqRoute>;
    fn lock_registry(&mut self) -> SavedIrq;
    fn unlock_registry(&mut self, saved: SavedIrq);
    fn token_counter(&mut self) -> u32;
    fn store_token_counter(&mut self, token: u32);
    fn store_handle(&mut self, slot: usize, handle: InterfaceHandle);
    fn install_irq(&mut self, route: IrqRoute, handler: IrqHandler) -> ServiceStatus;
    fn store_source(&mut self, slot: usize, source: IrqSource);
    fn registration_diagnostic(&mut self);
}

pub fn attach_timer_irq(registry: &mut impl TimerIrqRegistry, source: IrqSource, handler: IrqHandler) -> Result<InterfaceHandle, ServiceStatus> {
    let Some(route) = registry.route(source) else { return Err(ServiceStatus(-30)); };
    let saved = registry.lock_registry();
    let reservation = if registry.handle(route.slot()).0 != 0 {
        Err(ServiceStatus(-20))
    } else {
        loop {
            let mut candidate = registry.token_counter().wrapping_add(1);
            if candidate == 0 { candidate = 1; }
            registry.store_token_counter(candidate);
            let handle = InterfaceHandle(candidate);
            if !(0..128).any(|index| registry.handle(index) == handle) {
                registry.store_handle(route.slot(), handle);
                break Ok(handle);
            }
        }
    };
    registry.unlock_registry(saved);
    match reservation {
        Ok(_) => (),
        Err(status) => { registry.registration_diagnostic(); return Err(status); }
    };
    let saved = registry.lock_registry();
    let status = registry.install_irq(route, handler);
    registry.store_source(route.slot(), source);
    let handle = registry.handle(route.slot());
    registry.unlock_registry(saved);
    if status.0 == 0 { Ok(handle) } else { Err(ServiceStatus(-255)) }
}

#[cfg(target_arch = "arm")]
#[path = "charge_timers_native.rs"]
mod native;
#[cfg(test)]
#[path = "charge_timers_tests.rs"]
mod tests;
