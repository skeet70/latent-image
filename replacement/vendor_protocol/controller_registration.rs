use core::cell::RefCell;
use core::sync::atomic::{AtomicBool, Ordering};
use crate::{controller::RequestKind, controller_runtime::DeferredMessages, transport_connection_cpu::ConnectionState};
use xm1_power_services::{
    device_timer_release::TimerPeriod,
    primitives::ServiceStatus,
    shutdown::{KernelStatus, QueueId, RequestId, ServiceMessage, ShutdownRtos},
    shutdown_task7_8::RegistryHandle,
    shutdown_task7_8_transport::{RegistrationRegistry, UsbCompletionPolicy, UsbRegistrationSlot, UsbRegistrationStorage, UsbSecondaryMode},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PollPeriod { Connection, Vendor }
impl PollPeriod {
    pub const fn period(self) -> TimerPeriod { TimerPeriod(match self { Self::Connection => 25, Self::Vendor => 500 }) }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimerEvent { ConnectionPoll, SessionPoll, ShortTransfer, LongTransfer }
impl TimerEvent {
    pub const fn opcode(self) -> u16 {
        match self { Self::ConnectionPoll => 0x340d, Self::SessionPoll => 0x340f, Self::ShortTransfer => 0x3410, Self::LongTransfer => 0x3411 }
    }
    pub fn publish(self, rtos: &mut impl ShutdownRtos) -> Result<(), KernelStatus> {
        let payload = [0; 24];
        let sender = rtos.current_task();
        let result = rtos.post(ServiceMessage { operation: RequestId(self.opcode()), sender, destination: QueueId(36), payload: &payload });
        if self == Self::ConnectionPoll { let _ = rtos.set_event_flag(36, 8); }
        result
    }
}

pub trait TimerRegistry: RegistrationRegistry {
    fn start_periodic(&mut self, period: TimerPeriod, event: TimerEvent) -> Result<RegistryHandle, ServiceStatus>;
}

pub struct TimerProducers { primary: AtomicBool, secondary: AtomicBool }
impl TimerProducers {
    pub const fn new() -> Self { Self { primary: AtomicBool::new(false), secondary: AtomicBool::new(false) } }
    pub fn fire(&self, event: TimerEvent, rtos: &mut impl ShutdownRtos) -> Result<(), KernelStatus> {
        let pending = match event {
            TimerEvent::ConnectionPoll => Some(&self.primary),
            TimerEvent::SessionPoll => Some(&self.secondary),
            TimerEvent::ShortTransfer | TimerEvent::LongTransfer => None,
        };
        if pending.is_some_and(|pending| pending.swap(true, Ordering::AcqRel)) { return Ok(()); }
        event.publish(rtos)
    }
    pub fn acknowledge_primary(&self) { self.primary.store(false, Ordering::Release); }
    pub fn acknowledge_secondary(&self) { self.secondary.store(false, Ordering::Release); }
}

pub struct ControllerRegistrations<'a> {
    primary: Option<RegistryHandle>,
    secondary: Option<RegistryHandle>,
    short: Option<RegistryHandle>,
    long: Option<RegistryHandle>,
    producers: &'a TimerProducers,
}
impl<'state> ControllerRegistrations<'state> {
    pub const fn new(producers: &'state TimerProducers) -> Self {
        Self { primary: None, secondary: None, short: None, long: None, producers }
    }
    pub const fn primary(&self) -> Option<RegistryHandle> { self.primary }
    pub const fn secondary(&self) -> Option<RegistryHandle> { self.secondary }
    pub const fn short(&self) -> Option<RegistryHandle> { self.short }
    pub const fn long(&self) -> Option<RegistryHandle> { self.long }
    pub fn fire(&self, event: TimerEvent, rtos: &mut impl ShutdownRtos) -> Result<(), KernelStatus> {
        self.producers.fire(event, rtos)
    }
    pub fn primary_pending(&self) -> bool { self.producers.primary.load(Ordering::Acquire) }
    pub fn secondary_pending(&self) -> bool { self.producers.secondary.load(Ordering::Acquire) }
    pub fn start_primary(&mut self, period: PollPeriod, registry: &mut impl TimerRegistry) -> Result<(), ServiceStatus> {
        self.primary = Some(registry.start_periodic(period.period(), TimerEvent::ConnectionPoll)?);
        Ok(())
    }
    pub fn stop_primary(&mut self, registry: &mut impl RegistrationRegistry) -> ServiceStatus {
        let result = registry.release(self.primary.unwrap_or(RegistryHandle(0)));
        self.primary = None;
        self.acknowledge_primary_poll();
        result
    }
    /// Stock 0x00d81e40.
    pub fn start_secondary(&mut self, kind: RequestKind, connection: &mut ConnectionState, registry: &mut impl TimerRegistry) -> Result<(), ServiceStatus> {
        if kind != RequestKind::Protocol { return Ok(()); }
        connection.clear_secondary_activity();
        self.acknowledge_secondary_poll();
        self.secondary = Some(registry.start_periodic(TimerPeriod(50), TimerEvent::SessionPoll)?);
        Ok(())
    }
    pub fn stop_secondary(&mut self, registry: &mut impl RegistrationRegistry) -> ServiceStatus {
        let result = registry.release(self.secondary.unwrap_or(RegistryHandle(0)));
        self.secondary = None;
        result
    }
    pub fn start_short(&mut self, registry: &mut impl TimerRegistry) -> Result<(), ServiceStatus> {
        self.short = Some(registry.start_periodic(TimerPeriod(250), TimerEvent::ShortTransfer)?);
        Ok(())
    }
    pub fn start_long(&mut self, registry: &mut impl TimerRegistry) -> Result<(), ServiceStatus> {
        self.long = Some(registry.start_periodic(TimerPeriod(60_000), TimerEvent::LongTransfer)?);
        Ok(())
    }
    pub fn stop_short(&mut self, registry: &mut impl RegistrationRegistry) -> ServiceStatus {
        crate::transport::cancel_transfer_timer(&mut self.short, registry)
    }
    pub fn stop_long(&mut self, registry: &mut impl RegistrationRegistry) -> ServiceStatus {
        crate::transport::cancel_transfer_timer(&mut self.long, registry)
    }
    pub fn acknowledge_primary_poll(&mut self) { self.producers.acknowledge_primary(); }
    pub fn acknowledge_secondary_poll(&mut self) { self.producers.acknowledge_secondary(); }
    pub fn shutdown_parts<'a, O: ShutdownRtos>(&'a mut self, kind: RequestKind, connection: &'a mut ConnectionState,
        deferred: &'a mut DeferredMessages, rtos: &'a RefCell<O>) -> ShutdownRegistrationParts<'a, O> {
        ShutdownRegistrationParts {
            storage: RegistrationStorage { primary: &mut self.primary, secondary: &mut self.secondary,
                producers: self.producers, kind, connection, deferred, rtos },
            short: &mut self.short, long: &mut self.long,
        }
    }
}

pub struct SharedRtos<'a, O>(pub &'a RefCell<O>);
impl<O: ShutdownRtos> ShutdownRtos for SharedRtos<'_, O> {
    fn current_task(&mut self) -> xm1_power_services::shutdown::TaskId { self.0.borrow_mut().current_task() }
    fn post(&mut self, message: ServiceMessage<'_>) -> Result<(), KernelStatus> { self.0.borrow_mut().post(message) }
    fn set_event_flag(&mut self, flag: u32, bits: u32) -> Result<(), KernelStatus> { self.0.borrow_mut().set_event_flag(flag, bits) }
}

pub struct ShutdownRegistrationParts<'a, O> {
    pub storage: RegistrationStorage<'a, O>,
    pub short: &'a mut Option<RegistryHandle>,
    pub long: &'a mut Option<RegistryHandle>,
}
pub struct RegistrationStorage<'a, O> {
    primary: &'a mut Option<RegistryHandle>,
    secondary: &'a mut Option<RegistryHandle>,
    producers: &'a TimerProducers,
    kind: RequestKind,
    connection: &'a mut ConnectionState,
    deferred: &'a mut DeferredMessages,
    rtos: &'a RefCell<O>,
}
impl<O: ShutdownRtos> UsbRegistrationStorage for RegistrationStorage<'_, O> {
    fn handle(&mut self, slot: UsbRegistrationSlot) -> RegistryHandle {
        match slot { UsbRegistrationSlot::Primary => *self.primary, UsbRegistrationSlot::Secondary => *self.secondary }.unwrap_or(RegistryHandle(0))
    }
    fn clear_handle(&mut self, slot: UsbRegistrationSlot) {
        match slot { UsbRegistrationSlot::Primary => *self.primary = None, UsbRegistrationSlot::Secondary => *self.secondary = None }
    }
    fn clear_primary_activity(&mut self) { self.producers.acknowledge_primary(); }
    fn secondary_mode(&mut self) -> UsbSecondaryMode {
        if self.kind == RequestKind::Protocol { UsbSecondaryMode::Mode2 } else { UsbSecondaryMode::Other }
    }
    fn secondary_active(&mut self) -> bool { self.connection.secondary_active() }
    fn complete_secondary(&mut self, status: ServiceStatus) {
        let _ = crate::controller::publish_secondary_status(status, &mut SharedRtos(self.rtos));
    }
    fn clear_secondary_activity(&mut self) { self.connection.clear_secondary_activity(); }
    fn set_completion_policy(&mut self, policy: UsbCompletionPolicy) {
        self.deferred.set_blocked(policy == UsbCompletionPolicy::Enabled);
    }
}

#[cfg(target_arch = "arm")]
pub struct TimerCallbackEntries {
    pub connection_poll: unsafe extern "C" fn(),
    pub session_poll: unsafe extern "C" fn(),
    pub short_transfer: unsafe extern "C" fn(),
    pub long_transfer: unsafe extern "C" fn(),
}
#[cfg(target_arch = "arm")]
pub struct NativeTimerRegistry<'a> {
    pub registry: &'a mut xm1_power_services::shutdown_task7_8_transport::NativeRegistrationRegistry,
    pub entries: &'a TimerCallbackEntries,
}
#[cfg(target_arch = "arm")]
impl RegistrationRegistry for NativeTimerRegistry<'_> {
    fn release(&mut self, handle: RegistryHandle) -> ServiceStatus { self.registry.release(handle) }
}
#[cfg(target_arch = "arm")]
impl TimerRegistry for NativeTimerRegistry<'_> {
    fn start_periodic(&mut self, period: TimerPeriod, event: TimerEvent) -> Result<RegistryHandle, ServiceStatus> {
        extern "C" { fn xm1_power_software_timer_start(period: u32, mode: u32, callback: u32, token: *mut u32) -> i32; }
        let callback = match event {
            TimerEvent::ConnectionPoll => self.entries.connection_poll,
            TimerEvent::SessionPoll => self.entries.session_poll,
            TimerEvent::ShortTransfer => self.entries.short_transfer,
            TimerEvent::LongTransfer => self.entries.long_transfer,
        };
        let mut handle = 0;
        let status = unsafe { xm1_power_software_timer_start(period.0, 1, callback as usize as u32, &mut handle) };
        if status == 0 { Ok(RegistryHandle(handle)) } else { Err(ServiceStatus(status)) }
    }
}
