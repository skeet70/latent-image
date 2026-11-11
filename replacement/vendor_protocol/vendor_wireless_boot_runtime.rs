//! Service startup and shutdown CPU at 0x012af120, 0x012af3ac and 0x012af80c.

use super::sd_controller::{Argument, BeginError, Buffer, Completion, Execution, Operation, PhysicalResult, Request, VendorCode};
use super::vendor_wireless_boot::{BootOutcome, BootReady, RadioBoot};
use super::vendor_wireless_boot_objects::{ObjectError, ObjectKernel, RadioObjects, SemaphoreHandles, TaskContext, TaskKind, TaskStack};
use super::vendor_wireless_boot_query::{Query, QueryReply};
use super::vendor_wireless_boot_query_driver::{QueryDriver, QueryFailure};
use super::vendor_wireless_lifecycle::{DriverSemaphore, Synchronization, SynchronizationReply};
use super::vendor_wireless_lifecycle_runtime::RadioRuntime;
use super::vendor_wireless_network::MacAddress;
use super::vendor_wireless_parameter_driver::{TrafficClass, TransmitError};
use super::vendor_wireless_power::RadioTick;
use super::vendor_wireless_service_host::{DriverStopped, PhysicalObservation, sync::{DriverSynchronization, SemaphoreStatus}};
use super::vendor_wireless_station_driver::receive::gate::StopFlags;
use super::wireless_association::station::{StationError, StationOperationId, StationRequest, StationRuntime, SupplicantStopped};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BootOperationId(u64);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BootRuntimeError { Busy, IdentityExhausted, Objects(ObjectError), Firmware, Query(QueryFailure), Physical, HostBegin(BeginError), Synchronization(SemaphoreStatus), Credits(TransmitError), CreditTimeout, Station(StationError), Stale, Cancelled }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BootPublication { Ready { address: MacAddress }, Stopped }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BootCompletion { pub operation: BootOperationId, pub status: Result<BootPublication, BootRuntimeError> }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BootPhase { Idle, Firmware, DriverStart, DriverStarted, Global, Adapter, Query, Releasing, SupplicantStart, SupplicantStarted, Ready, StopSupplicant, StopDriver, TeardownFirmware, DestroyObjects, Finished }
#[derive(Clone, Copy)]
enum PhysicalPhase { Reserve, Query, CreditLow, CreditHigh, Delay(RadioTick) }

pub struct BootRuntime {
    phase: BootPhase,
    physical_phase: PhysicalPhase,
    driver: RadioObjects,
    supplicant_objects: RadioObjects,
    station: Option<StationRuntime>,
    next_operation: u64,
    operation: BootOperationId,
    completion: Option<BootCompletion>,
    firmware: Option<BootReady>,
    address: Option<MacAddress>,
    query: Option<QueryDriver>,
    query_kind: Query,
    register: [u8; 4],
    packet: [u8; 28],
    credit_returns: [u8; 8],
    credit_polls: u16,
    issued: bool,
    holds_global: bool,
    holds_adapter: bool,
    failure: Option<BootRuntimeError>,
    leave_operation: Option<StationOperationId>,
    driver_stack: Option<TaskStack>,
    supplicant_stack: Option<TaskStack>,
}
impl Default for BootRuntime { fn default() -> Self { Self::new() } }
impl BootRuntime {
    pub const fn new() -> Self {
        Self { phase: BootPhase::Idle, physical_phase: PhysicalPhase::Reserve, driver: RadioObjects::new(), supplicant_objects: RadioObjects::supplicant(), station: None,
            next_operation: 0, operation: BootOperationId(0), completion: None, firmware: None, address: None, query: None, query_kind: Query::Address,
            register: [0; 4], packet: [0; 28], credit_returns: [0; 8], credit_polls: 0, issued: false, holds_global: false, holds_adapter: false,
            failure: None, leave_operation: None, driver_stack: None, supplicant_stack: None }
    }
    pub const fn phase(&self) -> BootPhase { self.phase }
    pub const fn handles(&self) -> Option<SemaphoreHandles> { self.driver.handles() }
    pub fn completion(&mut self) -> Option<BootCompletion> { self.completion.take() }
    pub fn station(&mut self) -> Option<&mut StationRuntime> { self.station.as_mut() }
    fn operation(&mut self) -> Result<BootOperationId, BootRuntimeError> {
        self.next_operation = self.next_operation.checked_add(1).ok_or(BootRuntimeError::IdentityExhausted)?;
        self.operation = BootOperationId(self.next_operation);
        self.completion = None;
        Ok(self.operation)
    }
    pub fn initialize(&mut self, kernel: &mut impl ObjectKernel) -> Result<BootOperationId, BootRuntimeError> {
        if !matches!(self.phase, BootPhase::Idle | BootPhase::Finished) || self.driver.task().is_some() || self.supplicant_objects.task().is_some() { return Err(BootRuntimeError::Busy); }
        let operation = self.operation()?;
        self.failure = None; self.address = None; self.firmware = None; self.query = None;
        if let Err(error) = self.driver.initialize(kernel) {
            self.failure = Some(BootRuntimeError::Objects(error));
            self.phase = BootPhase::DestroyObjects;
            return Ok(operation);
        }
        self.phase = BootPhase::Firmware;
        Ok(operation)
    }
    pub fn firmware_initialized<const F: usize, const D: usize>(&mut self, boot: &RadioBoot<'_, '_, '_, F, D>) -> Result<(), BootRuntimeError> {
        if self.phase != BootPhase::Firmware { return Err(BootRuntimeError::Busy); }
        match boot.outcome() {
            Some(BootOutcome::Ready(ready)) => { self.firmware = Some(ready); self.phase = BootPhase::DriverStart; Ok(()) }
            Some(BootOutcome::Failed(_)) => { self.failure = Some(BootRuntimeError::Firmware); self.phase = BootPhase::DestroyObjects; Ok(()) }
            _ => Err(BootRuntimeError::Busy),
        }
    }
    pub fn start_driver(&mut self, kernel: &mut impl ObjectKernel, context: TaskContext, stack: TaskStack) -> Result<(), super::vendor_wireless_boot_objects::TaskStartError> {
        if self.phase != BootPhase::DriverStart { return Err(super::vendor_wireless_boot_objects::TaskStartError { error: ObjectError::Busy, stack: Some(stack) }); }
        let result = self.driver.start_task(kernel, context, stack);
        match result {
            Ok(_) => { self.phase = BootPhase::DriverStarted; Ok(()) }
            Err(error) => { self.failure = Some(BootRuntimeError::Objects(error.error)); self.phase = if self.driver.task().is_some() { BootPhase::StopDriver } else { BootPhase::DestroyObjects }; Err(error) }
        }
    }
    pub fn observe_driver_started(&mut self, kernel: &impl ObjectKernel) -> Result<bool, BootRuntimeError> {
        if self.phase != BootPhase::DriverStarted { return Err(BootRuntimeError::Busy); }
        let Some(started) = self.driver.observe_task_started(kernel).map_err(BootRuntimeError::Objects)? else { return Ok(false); };
        if started.kind() != TaskKind::Driver || Some(started.task()) != self.driver.task() { return Err(BootRuntimeError::Stale); }
        self.query_kind = Query::Address;
        self.phase = BootPhase::Global;
        Ok(true)
    }
    pub fn advance<const F: usize, const D: usize>(&mut self, synchronization: &mut impl DriverSynchronization, radio: &mut RadioRuntime<'_, F, D>, now: RadioTick) {
        if self.phase == BootPhase::Releasing {
            self.release(synchronization);
            self.phase = BootPhase::StopDriver;
            return;
        }
        if self.phase == BootPhase::Global {
            let reply = synchronization.execute(Synchronization::Acquire { semaphore: DriverSemaphore::Global, timeout_ticks: 2000 });
            if reply.status.0 != 0 || reply.reply != SynchronizationReply::Acquired { self.abort_query(BootRuntimeError::Synchronization(reply.status)); return; }
            self.holds_global = true;
            self.phase = BootPhase::Adapter;
        }
        if self.phase == BootPhase::Adapter {
            let reply = synchronization.execute(Synchronization::Acquire { semaphore: DriverSemaphore::Adapter, timeout_ticks: 2000 });
            if reply.status.0 != 0 || reply.reply != SynchronizationReply::Acquired {
                self.release(synchronization); self.abort_query(BootRuntimeError::Synchronization(reply.status)); return;
            }
            self.holds_adapter = true;
            self.credit_polls = 0;
            self.physical_phase = PhysicalPhase::Reserve;
            self.query = None;
            self.phase = BootPhase::Query;
        }
        if self.phase != BootPhase::Query { return; }
        let Some(outcome) = self.query.as_ref().and_then(QueryDriver::outcome) else { return; };
        self.release(synchronization);
        if let Some(error) = self.failure { self.abort_query(error); return; }
        match outcome {
            Ok(QueryReply::Address(address)) if self.query_kind == Query::Address => {
                self.address = Some(address);
                self.query_kind = Query::PublishCapabilities;
                self.phase = BootPhase::Global;
            }
            Ok(QueryReply::Acknowledged) if self.query_kind == Query::PublishCapabilities => {
                self.station = Some(StationRuntime::new(self.address.unwrap()));
                self.phase = BootPhase::SupplicantStart;
            }
            Err(error) => self.abort_query(BootRuntimeError::Query(error)),
            _ => self.abort_query(BootRuntimeError::Firmware),
        }
        let _ = (radio, now);
    }
    fn release(&mut self, synchronization: &mut impl DriverSynchronization) {
        for semaphore in [DriverSemaphore::Adapter, DriverSemaphore::Global] {
            let held = match semaphore { DriverSemaphore::Adapter => &mut self.holds_adapter, DriverSemaphore::Global => &mut self.holds_global };
            if *held {
                let reply = synchronization.execute(Synchronization::Release(semaphore));
                if reply.status.0 != 0 && self.failure.is_none() { self.failure = Some(BootRuntimeError::Synchronization(reply.status)); }
                *held = false;
            }
        }
    }
    fn abort_query(&mut self, error: BootRuntimeError) { if self.failure.is_none() { self.failure = Some(error); } self.phase = BootPhase::Releasing; }
    pub fn abort_physical(&mut self, observation: PhysicalObservation) -> bool {
        if !self.issued { return false; }
        let error = match observation {
            PhysicalObservation::NotStarted(error) => BootRuntimeError::HostBegin(error),
            PhysicalObservation::Abandoned => BootRuntimeError::Cancelled,
            PhysicalObservation::Completed(_) => return false,
        };
        self.issued = false;
        self.abort_query(error);
        true
    }
    pub fn request<const F: usize, const D: usize>(&mut self, radio: &mut RadioRuntime<'_, F, D>, now: RadioTick) -> Option<Request<'_>> {
        if self.phase != BootPhase::Query || self.issued { return None; }
        if let PhysicalPhase::Delay(deadline) = self.physical_phase {
            if (now.0.wrapping_sub(deadline.0) as i32) < 0 { return None; }
            self.physical_phase = PhysicalPhase::Reserve;
        }
        if matches!(self.physical_phase, PhysicalPhase::Reserve) {
            let adapter = radio.adapter();
            let mut credits = adapter.take_credits();
            let reserved = credits.consume(TrafficClass::Command);
            adapter.set_credits(credits);
            if reserved {
                let sequence = adapter.last_sequence().wrapping_add(1);
                adapter.set_last_sequence(sequence);
                self.query = Some(QueryDriver::correlated(self.query_kind, sequence));
                self.physical_phase = PhysicalPhase::Query;
            } else if self.credit_polls == 256 { self.abort_query(BootRuntimeError::CreditTimeout); return None; }
            else { self.physical_phase = PhysicalPhase::CreditLow; }
        }
        let request = match self.physical_phase {
            PhysicalPhase::Query => self.query.as_mut()?.request(now, &mut self.register, &mut self.packet)?,
            PhysicalPhase::CreditLow | PhysicalPhase::CreditHigh => Request { operation: Operation::Extended53,
                argument: Argument::from_wire(if matches!(self.physical_phase, PhysicalPhase::CreditLow) { 0x1000_4004 } else { 0x1000_4804 }),
                execution: Execution::Polling, buffer: Buffer::Fifo(&mut self.register) },
            _ => return None,
        };
        self.issued = true;
        Some(request)
    }
    pub fn receive<const F: usize, const D: usize>(&mut self, radio: &mut RadioRuntime<'_, F, D>, physical: Completion, now: RadioTick, flags: StopFlags) -> bool {
        if self.phase != BootPhase::Query || !self.issued { return false; }
        self.issued = false;
        if flags.bus_removed || flags.service_stopped { self.abort_query(BootRuntimeError::Cancelled); return true; }
        if physical.code != VendorCode::Success || physical.physical != PhysicalResult::Completed { self.abort_query(BootRuntimeError::Physical); return true; }
        match self.physical_phase {
            PhysicalPhase::Query => { self.query.as_mut().unwrap().receive(physical, self.register, &self.packet, now, false); }
            PhysicalPhase::CreditLow => { self.credit_returns[..4].copy_from_slice(&self.register); self.physical_phase = PhysicalPhase::CreditHigh; }
            PhysicalPhase::CreditHigh => {
                self.credit_returns[4..].copy_from_slice(&self.register);
                if let Err(error) = radio.adapter().observe_credit_returns(self.credit_returns) { self.abort_query(BootRuntimeError::Credits(error)); }
                else { self.credit_polls += 1; self.physical_phase = PhysicalPhase::Delay(RadioTick(now.0.wrapping_add(50))); }
            }
            _ => return false,
        }
        true
    }
    pub fn start_supplicant(&mut self, kernel: &mut impl ObjectKernel, context: TaskContext, stack: TaskStack) -> Result<(), super::vendor_wireless_boot_objects::TaskStartError> {
        if self.phase != BootPhase::SupplicantStart { return Err(super::vendor_wireless_boot_objects::TaskStartError { error: ObjectError::Busy, stack: Some(stack) }); }
        if let Err(error) = self.supplicant_objects.initialize(kernel) {
            self.failure = Some(BootRuntimeError::Objects(error)); self.phase = BootPhase::StopDriver;
            return Err(super::vendor_wireless_boot_objects::TaskStartError { error, stack: Some(stack) });
        }
        match self.supplicant_objects.start_task(kernel, context, stack) {
            Ok(_) => { self.phase = BootPhase::SupplicantStarted; Ok(()) }
            Err(error) => { self.failure = Some(BootRuntimeError::Objects(error.error)); self.phase = if self.supplicant_objects.task().is_some() { BootPhase::StopSupplicant } else { BootPhase::StopDriver }; Err(error) }
        }
    }
    pub fn observe_supplicant_started(&mut self, kernel: &impl ObjectKernel) -> Result<bool, BootRuntimeError> {
        if self.phase != BootPhase::SupplicantStarted { return Err(BootRuntimeError::Busy); }
        let Some(started) = self.supplicant_objects.observe_task_started(kernel).map_err(BootRuntimeError::Objects)? else { return Ok(false); };
        if started.kind() != TaskKind::Supplicant || Some(started.task()) != self.supplicant_objects.task() { return Err(BootRuntimeError::Stale); }
        if let Some(error) = self.failure { self.abort_query(error); return Ok(false); }
        self.phase = BootPhase::Ready;
        self.completion = Some(BootCompletion { operation: self.operation, status: Ok(BootPublication::Ready { address: self.address.unwrap() }) });
        Ok(true)
    }
    pub fn deinitialize(&mut self, kernel: &mut impl ObjectKernel) -> Result<BootOperationId, BootRuntimeError> {
        if self.phase != BootPhase::Ready { return Err(BootRuntimeError::Busy); }
        let operation = self.operation()?;
        self.begin_supplicant_stop(kernel)?;
        Ok(operation)
    }
    fn begin_supplicant_stop(&mut self, kernel: &mut impl ObjectKernel) -> Result<(), BootRuntimeError> {
        let station = self.station.as_mut().ok_or(BootRuntimeError::Busy)?;
        let operation = if let Some(operation) = station.current_operation() { station.cancel(operation).map_err(BootRuntimeError::Station)?; operation }
            else { station.submit(StationRequest::Leave).map_err(BootRuntimeError::Station)? };
        self.leave_operation = Some(operation);
        self.supplicant_objects.request_shutdown(kernel).map_err(BootRuntimeError::Objects)?;
        self.phase = BootPhase::StopSupplicant;
        Ok(())
    }
    pub fn supplicant_stopped(&mut self, kernel: &mut impl ObjectKernel, receipt: SupplicantStopped) -> Result<(), BootRuntimeError> {
        if self.phase != BootPhase::StopSupplicant || Some(receipt.operation()) != self.leave_operation { return Err(BootRuntimeError::Stale); }
        self.supplicant_stack = self.supplicant_objects.delete_task(kernel).map_err(BootRuntimeError::Objects)?;
        self.supplicant_objects.destroy(kernel).map_err(BootRuntimeError::Objects)?;
        self.station = None;
        self.driver.request_shutdown(kernel).map_err(BootRuntimeError::Objects)?;
        self.phase = BootPhase::StopDriver;
        Ok(())
    }
    pub fn request_driver_stop(&mut self, kernel: &mut impl ObjectKernel) -> Result<(), BootRuntimeError> {
        if self.phase != BootPhase::StopDriver { return Err(BootRuntimeError::Busy); }
        self.driver.request_shutdown(kernel).map_err(BootRuntimeError::Objects)
    }
    pub fn driver_stopped(&mut self, kernel: &mut impl ObjectKernel, _receipt: DriverStopped) -> Result<(), BootRuntimeError> {
        if self.phase != BootPhase::StopDriver || !self.driver.shutdown_requested() { return Err(BootRuntimeError::Stale); }
        self.driver_stack = self.driver.delete_task(kernel).map_err(BootRuntimeError::Objects)?;
        self.phase = BootPhase::TeardownFirmware;
        Ok(())
    }
    pub fn firmware_stopped<const F: usize, const D: usize>(&mut self, boot: &RadioBoot<'_, '_, '_, F, D>) -> Result<(), BootRuntimeError> {
        if self.phase != BootPhase::TeardownFirmware { return Err(BootRuntimeError::Busy); }
        match boot.outcome() {
            Some(BootOutcome::Stopped) => { self.phase = BootPhase::DestroyObjects; Ok(()) }
            Some(BootOutcome::Failed(_)) => { if self.failure.is_none() { self.failure = Some(BootRuntimeError::Firmware); } self.phase = BootPhase::DestroyObjects; Ok(()) }
            _ => Err(BootRuntimeError::Busy),
        }
    }
    pub fn destroy_objects(&mut self, kernel: &mut impl ObjectKernel) -> Result<(), BootRuntimeError> {
        if self.phase != BootPhase::DestroyObjects { return Err(BootRuntimeError::Busy); }
        self.supplicant_objects.destroy(kernel).map_err(BootRuntimeError::Objects)?;
        self.driver.destroy(kernel).map_err(BootRuntimeError::Objects)?;
        self.phase = BootPhase::Finished;
        self.completion = Some(BootCompletion { operation: self.operation, status: match self.failure { Some(error) => Err(error), None => Ok(BootPublication::Stopped) } });
        Ok(())
    }
    pub fn take_driver_stack(&mut self) -> Option<TaskStack> { self.driver_stack.take() }
    pub fn take_supplicant_stack(&mut self) -> Option<TaskStack> { self.supplicant_stack.take() }
}
