//! Correlated wireless execution through the canonical SD controller.

use core::task::Poll;
use super::sd_controller::{Argument, BeginError, Buffer, Completion, ControlError, ControlPlane,
    ControlledTransaction, DevicePort, Execution, Host, InterruptRouter, Milliseconds, Mmio,
    Notifications, Operation, OperationLock, PhysicalResult, Request, VendorCode};
use super::transport::cpu::CacheInstructions;
use super::vendor_wireless_adapter::{DescriptorId, PhysicalWrite};
use super::vendor_wireless_boot::{RadioBoot, BootError};
use super::vendor_wireless_boot_runtime::BootRuntime;
use super::vendor_wireless_lifecycle::{DriverAction, DriverReply};
use super::vendor_wireless_lifecycle_runtime::{RadioAction, RadioRuntime};
use super::vendor_wireless_parameter_driver::{SdioWriteOutcome, TransferIdentity};
use super::vendor_wireless_power::{RadioTick, RegisterTransfer, SdioReply, SdioTransfer};
use super::vendor_wireless_station_driver::{StationCompletion};
use super::vendor_wireless_station_driver::receive::{ReceiveError, ReceivePlan};
use super::vendor_wireless_station_driver::receive::gate::StopFlags;

#[path = "vendor_wireless_service_host_sync.rs"]
pub mod sync;
#[path = "vendor_wireless_service_host_pool.rs"]
pub mod pool;
use sync::{DriverSynchronization, SynchronizationObservation};
use pool::{AggregateIdentity, AggregateWriteLease};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostOperationId(u64);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PhysicalObservation { Completed(Completion), NotStarted(BeginError), Abandoned }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostObservation {
    pub operation: HostOperationId,
    pub physical: PhysicalObservation,
    pub release_error: Option<ControlError>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PumpError { Busy, IdentityExhausted, WrongConsumer, Stale, Receive(ReceiveError), Interrupt(ControlError), Boot(BootError) }
#[derive(Clone, Copy)]
enum Target {
    Parameter(TransferIdentity), Register(SdioTransfer), Station(StationCompletion), Adapter(DescriptorId),
    Boot, ServiceBoot, Receive { aggregate: AggregateIdentity, plan: ReceivePlan },
}
#[derive(Clone, Copy)]
enum Receipt {
    Synchronization(SynchronizationObservation),
    Host { target: Target, observation: HostObservation },
}
struct PumpState {
    next: u64,
    active: bool,
    receipt: Option<Receipt>,
    observation: Option<HostObservation>,
    synchronization: Option<SynchronizationObservation>,
    notifications: Notifications,
    reaping: bool,
    shutdown_receipt_issued: bool,
}
pub struct HostPump { state: PumpState, register: [u8; 4] }

pub struct ReceiveRead {
    aggregate: AggregateIdentity,
    plan: ReceivePlan,
    cancelled: bool,
    observation: HostObservation,
}
impl ReceiveRead { pub const fn observation(&self) -> HostObservation { self.observation } }

pub struct DriverStopped { operation: HostOperationId }
impl DriverStopped { pub const fn operation(&self) -> HostOperationId { self.operation } }

impl HostPump {
    pub const fn new() -> Self {
        Self { state: PumpState { next: 0, active: false, receipt: None, observation: None,
            synchronization: None, notifications: Notifications { inserted: false, removed: false, card_interrupt: false }, reaping: false, shutdown_receipt_issued: false },
            register: [0; 4] }
    }
    fn prepare(&mut self, host: &mut Host, mmio: &mut impl Mmio) -> Result<bool, PumpError> {
        if self.state.active || self.state.receipt.is_some() { return Err(PumpError::Busy); }
        if self.state.reaping {
            if host.reap_cancelled(mmio).is_pending() { return Ok(false); }
            self.state.reaping = false;
        }
        Ok(true)
    }
    pub fn begin_radio<'a, 'frames, const F: usize, const D: usize, M: Mmio, L: OperationLock>(
        &'a mut self, radio: &'a mut RadioRuntime<'frames, F, D>, control: &ControlPlane,
        host: &'a mut Host, mmio: &'a mut M, cache: &mut impl CacheInstructions,
        lock: &'a mut L, synchronization: &mut impl DriverSynchronization, now: RadioTick,
    ) -> Result<Option<HostTransfer<'a, M, L>>, PumpError> {
        if !self.prepare(host, mmio)? { return Ok(None); }
        let Some(action) = radio.action(now) else { return Ok(None); };
        let (target, request) = match action {
            RadioAction::Driver(DriverAction::Synchronize(operation)) => {
                self.state.receipt = Some(Receipt::Synchronization(synchronization.execute(operation)));
                self.state.shutdown_receipt_issued = false;
                return Ok(None);
            }
            RadioAction::Driver(DriverAction::Parameter(command)) => {
                (Target::Parameter(command.identity), command.host_request())
            }
            RadioAction::Driver(DriverAction::Register(transfer)) => {
                (Target::Register(transfer), transfer.host_request(&mut self.register))
            }
            RadioAction::Driver(DriverAction::Station(command)) => {
                let identity = command.identity;
                let argument = command.argument();
                (Target::Station(identity), Request { operation: Operation::Extended53,
                    argument: Argument::from_wire(argument), execution: Execution::Interrupt,
                    buffer: Buffer::Fifo(command.bytes_mut()) })
            }
            RadioAction::Adapter(command) => {
                let identity = command.identity();
                let argument = command.argument();
                (Target::Adapter(identity), Request { operation: Operation::Extended53,
                    argument: Argument::from_wire(argument), execution: Execution::Interrupt,
                    buffer: Buffer::Fifo(command.bytes_mut()) })
            }
        };
        start(&mut self.state, target, request, control, host, mmio, cache, lock, now)
    }
    pub fn begin_boot<'a, 'image, 'adapter, 'frames, const F: usize, const D: usize, M: Mmio, L: OperationLock>(
        &'a mut self, boot: &'a mut RadioBoot<'image, 'adapter, 'frames, F, D>, control: &mut ControlPlane,
        host: &'a mut Host, mmio: &'a mut M, cache: &mut impl CacheInstructions,
        lock: &'a mut L, router: &mut impl InterruptRouter, now: RadioTick,
    ) -> Result<Option<HostTransfer<'a, M, L>>, PumpError> {
        if !self.prepare(host, mmio)? { return Ok(None); }
        if boot.awaiting_interrupt_binding() {
            boot.bind_interrupt(control, host, mmio, router).map_err(PumpError::Boot)?;
            return Ok(None);
        }
        let Some(request) = boot.request(now) else { return Ok(None); };
        start(&mut self.state, Target::Boot, request, control, host, mmio, cache, lock, now)
    }
    pub fn begin_service_boot<'a, const F: usize, const D: usize, M: Mmio, L: OperationLock>(
        &'a mut self, boot: &'a mut BootRuntime, radio: &'a mut RadioRuntime<'_, F, D>,
        control: &ControlPlane, host: &'a mut Host, mmio: &'a mut M,
        cache: &mut impl CacheInstructions, lock: &'a mut L,
        synchronization: &mut impl DriverSynchronization, now: RadioTick,
    ) -> Result<Option<HostTransfer<'a, M, L>>, PumpError> {
        if !self.prepare(host, mmio)? { return Ok(None); }
        boot.advance(synchronization, radio, now);
        let Some(request) = boot.request(radio, now) else { return Ok(None); };
        start(&mut self.state, Target::ServiceBoot, request, control, host, mmio, cache, lock, now)
    }
    pub fn begin_receive<'a, 'pool, 'memory, const A: usize, M: Mmio, L: OperationLock>(
        &'a mut self, writer: &'a mut AggregateWriteLease<'pool, 'memory, A>, plan: ReceivePlan,
        control: &ControlPlane, host: &'a mut Host, mmio: &'a mut M,
        cache: &mut impl CacheInstructions, lock: &'a mut L, now: RadioTick,
    ) -> Result<Option<HostTransfer<'a, M, L>>, PumpError> {
        if !self.prepare(host, mmio)? { return Ok(None); }
        if writer.capacity() < plan.buffer_length() { return Err(PumpError::Receive(ReceiveError::Capacity)); }
        let target = Target::Receive { aggregate: writer.identity(), plan };
        let request = Request { operation: Operation::Extended53, argument: Argument::from_wire(plan.argument()),
            execution: Execution::Interrupt, buffer: Buffer::Fifo(&mut writer.buffer_mut()[..plan.buffer_length()]) };
        start(&mut self.state, target, request, control, host, mmio, cache, lock, now)
    }
    pub fn has_receipt(&self) -> bool { self.state.receipt.is_some() }
    pub const fn last_observation(&self) -> Option<HostObservation> { self.state.observation }
    pub const fn last_synchronization(&self) -> Option<SynchronizationObservation> { self.state.synchronization }
    pub fn take_notifications(&mut self) -> Notifications {
        core::mem::replace(&mut self.state.notifications, Notifications { inserted: false, removed: false, card_interrupt: false })
    }
    pub fn finish_shutdown<const F: usize, const D: usize>(&mut self,
        radio: &mut RadioRuntime<'_, F, D>, host: &mut Host, mmio: &mut impl Mmio) -> Option<DriverStopped> {
        radio.shutdown();
        if self.state.shutdown_receipt_issued || !self.prepare(host, mmio).ok()? || !radio.is_quiescent() { return None; }
        self.state.shutdown_receipt_issued = true;
        Some(DriverStopped { operation: HostOperationId(self.state.next) })
    }
    pub fn apply_radio<const F: usize, const D: usize>(&mut self, radio: &mut RadioRuntime<'_, F, D>,
        control: &mut ControlPlane, host: &mut Host, mmio: &mut impl Mmio,
        router: &mut impl InterruptRouter, now: RadioTick) -> Result<bool, PumpError> {
        let Some(receipt) = self.state.receipt else { return Ok(false); };
        match receipt {
            Receipt::Synchronization(observation) => {
                self.state.receipt = None;
                self.state.synchronization = Some(observation);
                if radio.synchronize(observation.reply) != DriverReply::Applied { return Err(PumpError::Stale); }
            }
            Receipt::Host { target: Target::Boot | Target::ServiceBoot | Target::Receive { .. }, .. } => return Err(PumpError::WrongConsumer),
            Receipt::Host { target, observation } => {
                self.state.receipt = None;
                self.state.observation = Some(observation);
                if cancelled(observation.physical) { radio.shutdown(); return Ok(true); }
                let written = physical_write(observation);
                match target {
                    Target::Parameter(identity) => { radio.parameter_reply(identity, written); }
                    Target::Station(identity) => { radio.station_reply(identity, written); }
                    Target::Adapter(identity) => { radio.adapter_reply(identity, if written == SdioWriteOutcome::Written { PhysicalWrite::Written } else { PhysicalWrite::Failed }); }
                    Target::Register(transfer) => {
                        if matches!(transfer.operation, RegisterTransfer::WriteControl(value) if value & 1 != 0) {
                            control.set_card_interrupt(host, mmio, router, super::sd_controller::CardInterruptRoute::Wireless).map_err(PumpError::Interrupt)?;
                        }
                        let reply = match transfer.operation { RegisterTransfer::Read(_) => SdioReply::Read(self.register), RegisterTransfer::WriteControl(_) => SdioReply::Written };
                        radio.register_reply(transfer.token, reply, now);
                    }
                    Target::Boot | Target::ServiceBoot | Target::Receive { .. } => return Err(PumpError::WrongConsumer),
                }
            }
        }
        Ok(true)
    }
    pub fn apply_boot<const F: usize, const D: usize>(&mut self, boot: &mut RadioBoot<'_, '_, '_, F, D>,
        now: RadioTick, stopping: StopFlags) -> Result<bool, PumpError> {
        let Some(Receipt::Host { target: Target::Boot, observation }) = self.state.receipt else {
            return if self.state.receipt.is_none() { Ok(false) } else { Err(PumpError::WrongConsumer) };
        };
        self.state.receipt = None;
        self.state.observation = Some(observation);
        match observation.physical {
            PhysicalObservation::Completed(completion) => {
                if !boot.receive(completion, now, stopping) { return Err(PumpError::Stale); }
            }
            PhysicalObservation::NotStarted(_) | PhysicalObservation::Abandoned => boot.cancel(),
        }
        Ok(true)
    }
    pub fn apply_service_boot<const F: usize, const D: usize>(&mut self, boot: &mut BootRuntime,
        radio: &mut RadioRuntime<'_, F, D>, now: RadioTick, stopping: StopFlags) -> Result<bool, PumpError> {
        let Some(Receipt::Host { target: Target::ServiceBoot, observation }) = self.state.receipt else {
            return if self.state.receipt.is_none() { Ok(false) } else { Err(PumpError::WrongConsumer) };
        };
        self.state.receipt = None;
        self.state.observation = Some(observation);
        let applied = match observation.physical {
            PhysicalObservation::Completed(completion) => boot.receive(radio, completion, now, stopping),
            PhysicalObservation::NotStarted(_) | PhysicalObservation::Abandoned => boot.abort_physical(observation.physical),
        };
        if !applied { return Err(PumpError::Stale); }
        Ok(true)
    }
    pub fn take_receive(&mut self) -> Result<Option<ReceiveRead>, PumpError> {
        let Some(Receipt::Host { target: Target::Receive { aggregate, plan }, observation }) = self.state.receipt else {
            return if self.state.receipt.is_none() { Ok(None) } else { Err(PumpError::WrongConsumer) };
        };
        self.state.receipt = None;
        self.state.observation = Some(observation);
        Ok(Some(ReceiveRead { aggregate, plan, cancelled: cancelled(observation.physical), observation }))
    }
}
impl Default for HostPump { fn default() -> Self { Self::new() } }

fn cancelled(observation: PhysicalObservation) -> bool {
    matches!(observation, PhysicalObservation::Abandoned
        | PhysicalObservation::Completed(Completion { physical: PhysicalResult::Cancelled, .. }))
}
fn physical_write(observation: HostObservation) -> SdioWriteOutcome {
    if matches!(observation.physical, PhysicalObservation::Completed(Completion {
        physical: PhysicalResult::Completed, code: VendorCode::Success, .. })) && observation.release_error.is_none() {
        SdioWriteOutcome::Written
    } else { SdioWriteOutcome::Failed }
}
fn start<'a, M: Mmio, L: OperationLock>(state: &'a mut PumpState, target: Target, request: Request<'a>,
    control: &ControlPlane, host: &'a mut Host, mmio: &'a mut M, cache: &mut impl CacheInstructions,
    lock: &'a mut L, now: RadioTick) -> Result<Option<HostTransfer<'a, M, L>>, PumpError> {
    let Some(next) = state.next.checked_add(1) else { return Err(PumpError::IdentityExhausted); };
    state.next = next;
    state.shutdown_receipt_issued = false;
    let identity = HostOperationId(next);
    match control.begin(DevicePort::Zero, host, mmio, cache, lock, request, Milliseconds::new(now.0)) {
        Ok(transaction) => {
            state.active = true;
            Ok(Some(HostTransfer { state, transaction: Some(transaction), target, identity, completed: None }))
        }
        Err(error) => {
            state.receipt = Some(Receipt::Host { target, observation: HostObservation {
                operation: identity, physical: PhysicalObservation::NotStarted(error), release_error: None } });
            Ok(None)
        }
    }
}

pub struct HostTransfer<'a, M: Mmio, L: OperationLock> {
    state: &'a mut PumpState,
    transaction: Option<ControlledTransaction<'a, M, L>>,
    target: Target,
    identity: HostOperationId,
    completed: Option<Completion>,
}
impl<M: Mmio, L: OperationLock> HostTransfer<'_, M, L> {
    pub const fn identity(&self) -> HostOperationId { self.identity }
    pub fn poll(&mut self, now: RadioTick) -> Poll<Completion> {
        let Some(transaction) = self.transaction.as_mut() else { return Poll::Pending; };
        let result = transaction.poll(Milliseconds::new(now.0));
        if let Poll::Ready(completion) = result { self.completed = Some(completion); }
        result
    }
    pub fn interrupt(&mut self) -> Notifications {
        let notifications = if let Some(transaction) = self.transaction.as_mut() { transaction.interrupt() }
            else { Notifications { inserted: false, removed: false, card_interrupt: false } };
        self.state.notifications.inserted |= notifications.inserted;
        self.state.notifications.removed |= notifications.removed;
        self.state.notifications.card_interrupt |= notifications.card_interrupt;
        notifications
    }
    pub fn cancel(&mut self) {
        if let Some(transaction) = self.transaction.as_mut() { transaction.cancel(); }
    }
    pub fn finish(mut self) -> Result<(), Self> {
        let Some(completion) = self.completed else { return Err(self); };
        let Some(transaction) = self.transaction.take() else { return Err(self); };
        let release_error = transaction.finish().err();
        self.state.active = false;
        self.state.receipt = Some(Receipt::Host { target: self.target, observation: HostObservation {
            operation: self.identity, physical: PhysicalObservation::Completed(completion), release_error } });
        Ok(())
    }
}
impl<M: Mmio, L: OperationLock> Drop for HostTransfer<'_, M, L> {
    fn drop(&mut self) {
        if let Some(transaction) = self.transaction.take() {
            drop(transaction);
            self.state.active = false;
            self.state.reaping = true;
            self.state.receipt = Some(Receipt::Host { target: self.target, observation: HostObservation {
                operation: self.identity, physical: PhysicalObservation::Abandoned, release_error: None } });
        }
    }
}

#[cfg(target_arch = "arm")]
pub fn advance_attachment_native(attachment: &mut super::vendor_wireless_lifecycle_sdio::RadioSdioAttachment,
    control: &mut ControlPlane, host: &mut Host, mmio: &mut impl Mmio,
    power: &mut impl super::sd_controller::ClockPower, router: &mut impl InterruptRouter,
) -> Result<(), super::vendor_wireless_lifecycle_sdio::AttachmentError> {
    if let Some(delay) = attachment.advance(control, host, mmio, power, router)? {
        unsafe { xm1_power_task_delay(delay.get()); }
    }
    Ok(())
}

#[cfg(target_arch = "arm")]
extern "C" { fn xm1_power_task_delay(ticks: u32) -> i32; }

#[cfg(test)]
#[path = "vendor_wireless_service_host_tests.rs"]
mod tests;
