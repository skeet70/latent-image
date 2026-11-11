//! X-M1 radio-mode composition at 0x012ac698 and 0x012aca24.
//! Source pin: 55436fc85c0560dd252b0410af6f2427fe198cd3.

use super::vendor_wireless_adapter::{Adapter, AdapterEvent, AddressConfiguration,
    CompletionDisposition, DescriptorId, FirmwareCommand, HardwareConfiguration, PhysicalWrite};
use super::vendor_wireless_lifecycle::{DriverAction, DriverAdmissionError, DriverReply, DriverStatus,
    RadioDriverRequest, SerializedRadioDriver, SynchronizationReply};
use super::vendor_wireless_parameter_driver::{SdioWriteOutcome, TransferIdentity, TransferReply, TransmitError};
use super::vendor_wireless_power::{PowerReply, RadioTick, RegisterTransfer, SdioReply, SdioToken};
use super::vendor_wireless_radio_program::RadioParameterProgram;
use super::vendor_wireless_station_driver::{StationDriver, StationDriverRequest, StationCompletion, StationReply};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RadioOperationId(u64);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RadioCompletion { pub operation: RadioOperationId, pub status: DriverStatus }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RadioPhase { Idle, EnteringRadioTest, ReinitializingAdapter, ConfiguringParameters, LeavingRadioTest, Station, Finished }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RadioPublication { ConnectionReset, RadioTestEntered, RadioTestExited }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MediaState { Zero, One, Other }

pub enum RadioRequest { Configure(RadioParameterProgram), ExitRadioTest, StopStation, Station(StationDriverRequest) }
pub enum RadioAction<'a> { Driver(DriverAction<'a>), Adapter(&'a mut FirmwareCommand) }

pub struct RadioRuntime<'a, const F: usize, const D: usize> {
    adapter: Adapter<'a, F, D>,
    driver: SerializedRadioDriver,
    hardware: HardwareConfiguration,
    address: AddressConfiguration,
    phase: RadioPhase,
    adapter_test_flag: bool,
    media: MediaState,
    next_operation: u64,
    operation: RadioOperationId,
    completion: Option<RadioCompletion>,
    parameters: Option<RadioParameterProgram>,
    publications: [Option<RadioPublication>; 3],
    publication_read: usize,
    publication_write: usize,
    adapter_owns_credits: bool,
    enter_prepared: bool,
}

impl<'a, const F: usize, const D: usize> RadioRuntime<'a, F, D> {
    pub fn new(mut adapter: Adapter<'a, F, D>, last_sequence: u8,
        hardware: HardwareConfiguration, address: AddressConfiguration) -> Self {
        adapter.set_last_sequence(last_sequence);
        Self { adapter, driver: SerializedRadioDriver::new(last_sequence), hardware, address,
            phase: RadioPhase::Idle, adapter_test_flag: false, media: MediaState::One, next_operation: 0,
            operation: RadioOperationId(0), completion: None, parameters: None,
            publications: [None; 3], publication_read: 0, publication_write: 0,
            adapter_owns_credits: true, enter_prepared: false }
    }

    pub fn submit(&mut self, request: RadioRequest, _now: RadioTick) -> Result<RadioOperationId, DriverAdmissionError> {
        if !matches!(self.phase, RadioPhase::Idle | RadioPhase::Finished)
            || self.publication_read != self.publication_write { return Err(DriverAdmissionError::Busy); }
        let Some(next) = self.next_operation.checked_add(1) else { return Err(DriverAdmissionError::Busy); };
        self.next_operation = next;
        self.operation = RadioOperationId(next);
        self.completion = None;
        self.publication_read = 0;
        self.publication_write = 0;
        match request {
            RadioRequest::Configure(program) => {
                if self.adapter_test_flag {
                    self.driver.submit(RadioDriverRequest::Parameters(program))?;
                    self.phase = RadioPhase::ConfiguringParameters;
                } else {
                    self.parameters = Some(program);
                    self.driver.submit(RadioDriverRequest::RegisterHandshake)?;
                    self.enter_prepared = false;
                    self.phase = RadioPhase::EnteringRadioTest;
                }
            }
            RadioRequest::ExitRadioTest => {
                if self.adapter_test_flag {
                    self.driver.submit(RadioDriverRequest::RegisterHandshake)?;
                    self.phase = RadioPhase::LeavingRadioTest;
                } else { self.finish(DriverStatus::SUCCESS); }
            }
            RadioRequest::StopStation => {
                self.select_transmitter(false);
                self.driver.submit(RadioDriverRequest::Station(StationDriverRequest::StopStation))?;
                self.phase = RadioPhase::Station;
            }
            RadioRequest::Station(request) => {
                self.select_transmitter(false);
                self.driver.submit(RadioDriverRequest::Station(request))?;
                self.phase = RadioPhase::Station;
            }
        }
        Ok(self.operation)
    }

    fn publish(&mut self, publication: RadioPublication) {
        self.publications[self.publication_write] = Some(publication);
        self.publication_write += 1;
    }

    pub fn pop_publication(&mut self) -> Option<RadioPublication> {
        if self.publication_read == self.publication_write { return None; }
        let publication = self.publications[self.publication_read].take();
        self.publication_read += 1;
        publication
    }

    fn finish(&mut self, status: DriverStatus) {
        self.parameters = None;
        self.completion = Some(RadioCompletion { operation: self.operation, status });
        self.phase = RadioPhase::Finished;
    }

    fn advance(&mut self, now: RadioTick) {
        let Some(status) = self.driver.outcome() else { return; };
        match self.phase {
            RadioPhase::EnteringRadioTest | RadioPhase::LeavingRadioTest => {
                if status != DriverStatus::SUCCESS { self.finish(status); return; }
                let entering = self.phase == RadioPhase::EnteringRadioTest;
                if !entering { self.adapter_test_flag = false; }
                if !entering && self.media != MediaState::One {
                    self.publish(RadioPublication::ConnectionReset);
                    self.address.timestamp = now.0;
                }
                if self.adapter.configure_address(self.address).is_err()
                    || (!entering && self.adapter.configure(&self.hardware).is_err()) {
                    self.finish(DriverStatus::FAILURE);
                    return;
                }
                self.phase = RadioPhase::ReinitializingAdapter;
            }
            RadioPhase::ReinitializingAdapter => {
                if self.adapter.has_pending_commands() { return; }
                self.publish(if self.parameters.is_some() {
                    RadioPublication::RadioTestEntered
                } else { RadioPublication::RadioTestExited });
                if let Some(program) = self.parameters.take() {
                    if self.driver.submit(RadioDriverRequest::Parameters(program)).is_err() {
                        self.finish(DriverStatus::FAILURE);
                    } else { self.phase = RadioPhase::ConfiguringParameters; }
                } else { self.finish(DriverStatus::SUCCESS); }
            }
            RadioPhase::ConfiguringParameters | RadioPhase::Station => self.finish(status),
            RadioPhase::Idle | RadioPhase::Finished => {}
        }
    }

    fn select_transmitter(&mut self, adapter: bool) {
        if adapter == self.adapter_owns_credits { return; }
        if adapter {
            let parameter = self.driver.parameter_driver();
            self.adapter.set_credits(parameter.take_credits());
            self.adapter.set_last_sequence(parameter.last_sequence());
        } else {
            let parameter = self.driver.parameter_driver();
            parameter.set_credits(self.adapter.take_credits());
            parameter.set_last_sequence(self.adapter.last_sequence());
        }
        self.adapter_owns_credits = adapter;
    }

    pub fn action(&mut self, now: RadioTick) -> Option<RadioAction<'_>> {
        self.advance(now);
        if self.phase == RadioPhase::EnteringRadioTest && !self.enter_prepared && self.driver.handshake_started() {
            self.enter_prepared = true;
            self.adapter_test_flag = true;
            if self.media != MediaState::One { self.publish(RadioPublication::ConnectionReset); }
            self.adapter.cancel_pending_frames();
            self.adapter.cancel_commands();
            self.adapter.cancel_received_descriptors();
            self.adapter.cancel_queued_frames();
            self.adapter.stop_protocols();
            self.adapter.reset_protocols();
        }
        if matches!(self.phase, RadioPhase::EnteringRadioTest | RadioPhase::LeavingRadioTest) {
            let action = self.driver.action(now);
            if matches!(&action, Some(DriverAction::Register(transfer))
                if transfer.operation == RegisterTransfer::WriteControl(1)) {
                self.adapter.reset_credits();
            }
            return action.map(RadioAction::Driver);
        }
        if matches!(self.phase, RadioPhase::ConfiguringParameters | RadioPhase::Station) {
            self.select_transmitter(false);
            return self.driver.action(now).map(RadioAction::Driver);
        }
        self.select_transmitter(true);
        self.adapter.command().map(RadioAction::Adapter)
    }

    pub fn synchronize(&mut self, reply: SynchronizationReply) -> DriverReply { self.driver.synchronize(reply) }
    pub fn parameter_reply(&mut self, identity: TransferIdentity, reply: SdioWriteOutcome) -> TransferReply {
        self.driver.parameter_reply(identity, reply)
    }
    pub fn station_reply(&mut self, identity: StationCompletion, reply: SdioWriteOutcome) -> StationReply {
        self.driver.station_reply(identity, reply)
    }
    pub fn station_driver(&mut self) -> &mut StationDriver { self.driver.station_driver() }
    pub fn register_reply(&mut self, token: SdioToken, reply: SdioReply, now: RadioTick) -> PowerReply {
        self.driver.register_reply(token, reply, now)
    }
    pub fn adapter_reply(&mut self, identity: DescriptorId, reply: PhysicalWrite) -> CompletionDisposition {
        self.adapter.finish_command(identity, reply)
    }
    pub fn observe_credit_returns(&mut self, bytes: [u8; 8]) -> Result<bool, TransmitError> {
        if self.adapter_owns_credits {
            self.adapter.observe_credit_returns(bytes)
        } else { self.driver.observe_credit_returns(bytes) }
    }
    pub fn cancel_handshake(&mut self) { self.driver.cancel_handshake(); }
    pub fn shutdown(&mut self) {
        self.driver.cancel_active();
        self.adapter.cancel();
        if self.phase == RadioPhase::ReinitializingAdapter { self.finish(DriverStatus::CANCELLED); }
    }
    pub fn is_quiescent(&self) -> bool {
        matches!(self.phase, RadioPhase::Idle | RadioPhase::Finished)
            && !self.adapter.has_pending_commands() && !self.adapter.has_pending_events()
    }
    pub fn set_stopping(&mut self, stopping: bool) { self.driver.set_stopping(stopping); }
    pub fn set_media_state(&mut self, media: MediaState) { self.media = media; }
    pub const fn phase(&self) -> RadioPhase { self.phase }
    pub const fn outcome(&self) -> Option<RadioCompletion> { self.completion }
    pub fn adapter(&mut self) -> &mut Adapter<'a, F, D> { &mut self.adapter }
    pub fn pop_adapter_event(&mut self) -> Option<AdapterEvent<'a>> { self.adapter.pop_event() }
    pub fn update_address(&mut self, address: AddressConfiguration) { self.address = address; }
    pub fn update_hardware(&mut self, hardware: HardwareConfiguration) -> Result<(), DriverAdmissionError> {
        if !matches!(self.phase, RadioPhase::Idle | RadioPhase::Finished) { return Err(DriverAdmissionError::Busy); }
        self.hardware = hardware;
        Ok(())
    }
}

#[cfg(test)]
#[path = "vendor_wireless_lifecycle_runtime_tests.rs"]
mod tests;
