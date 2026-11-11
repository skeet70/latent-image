//! Serialized radio execution at X-M1 0x012b0bb8.
//! Source pin: 55436fc85c0560dd252b0410af6f2427fe198cd3.
//! The handshake request covers registers only, not adapter mode entry or exit.

use super::vendor_wireless_parameter_driver::{
    ParameterWrite, RadioParameterDriver, SdioWriteOutcome, TransferIdentity, TransferReply,
    TransmitError,
};
use super::vendor_wireless_power::{
    PowerOutcome, PowerReply, RadioPowerHandshake, RadioTick, SdioReply, SdioToken, SdioTransfer,
};
use super::vendor_wireless_radio_program::RadioParameterProgram;
use super::vendor_wireless_station_driver::{StationDriver, StationDriverRequest, StationDriverError,
    FirmwareCommand as StationFirmwareCommand, StationCompletion, StationReply};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverStatus(pub u32);

impl DriverStatus {
    pub const SUCCESS: Self = Self(0);
    pub const FAILURE: Self = Self(0xc000_0001);
    pub const STOPPED: Self = Self(0xc001_0011);
    pub const CANCELLED: Self = Self(0xc000_00bb);
}

pub enum RadioDriverRequest {
    Parameters(RadioParameterProgram),
    RegisterHandshake,
    Station(StationDriverRequest),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DriverSemaphore { Global, Adapter }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Synchronization {
    Acquire { semaphore: DriverSemaphore, timeout_ticks: u32 },
    Release(DriverSemaphore),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SynchronizationReply { Acquired, TimedOut, Released }

pub enum DriverAction<'a> {
    Synchronize(Synchronization),
    Parameter(&'a mut ParameterWrite),
    Register(SdioTransfer),
    Station(&'a mut StationFirmwareCommand),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DriverAdmissionError { Busy }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DriverReply { Applied, Late, WrongOperation }

#[derive(Clone, Copy)]
enum Phase { Idle, Global, Adapter, Parameters, Handshake, Station, ReleaseAdapter, ReleaseGlobal, Finished }

pub struct SerializedRadioDriver {
    phase: Phase,
    request: Option<RadioDriverRequest>,
    parameter: RadioParameterDriver,
    power: RadioPowerHandshake,
    station: StationDriver,
    station_issued: Option<StationCompletion>,
    stopping: bool,
    result: DriverStatus,
}

impl SerializedRadioDriver {
    pub const fn new(last_sequence: u8) -> Self {
        Self { phase: Phase::Idle, request: None,
            parameter: RadioParameterDriver::new(last_sequence), power: RadioPowerHandshake::new(),
            station: StationDriver::new(last_sequence), station_issued: None,
            stopping: false, result: DriverStatus::FAILURE }
    }

    pub fn submit(&mut self, request: RadioDriverRequest) -> Result<(), DriverAdmissionError> {
        if !matches!(self.phase, Phase::Idle | Phase::Finished) { return Err(DriverAdmissionError::Busy); }
        self.request = Some(request);
        self.result = DriverStatus::FAILURE;
        self.phase = Phase::Global;
        Ok(())
    }

    pub fn set_stopping(&mut self, stopping: bool) { self.stopping = stopping; }

    pub fn cancel_handshake(&mut self) {
        if matches!(self.phase, Phase::Handshake) { self.power.cancel(); }
    }

    pub fn cancel_active(&mut self) {
        self.stopping = true;
        match self.phase {
            Phase::Parameters => self.parameter.cancel(),
            Phase::Station => self.station.cancel(),
            Phase::Handshake => self.power.cancel(),
            _ => {}
        }
    }

    pub fn action(&mut self, now: RadioTick) -> Option<DriverAction<'_>> {
        match self.phase {
            Phase::Parameters => {
                self.parameter.transfer();
                if let Some(result) = self.parameter.outcome() {
                    self.result = match result {
                        Ok(()) => DriverStatus::SUCCESS,
                        Err(TransmitError::Cancelled) => DriverStatus::CANCELLED,
                        Err(_) => DriverStatus::FAILURE,
                    };
                    self.phase = Phase::ReleaseAdapter;
                }
            }
            Phase::Handshake => {
                if let Some(outcome) = self.power.outcome() {
                    self.result = match outcome {
                        PowerOutcome::Ready => DriverStatus::SUCCESS,
                        PowerOutcome::Cancelled => DriverStatus::CANCELLED,
                    };
                    self.phase = Phase::ReleaseAdapter;
                }
            }
            Phase::Station => {
                if let Some(outcome) = self.station.outcome() {
                    self.result = match outcome {
                        Ok(()) => DriverStatus::SUCCESS,
                        Err(StationDriverError::Cancelled) => DriverStatus::CANCELLED,
                        Err(StationDriverError::Stopped) => DriverStatus::STOPPED,
                        Err(_) => DriverStatus::FAILURE,
                    };
                    self.phase = Phase::ReleaseAdapter;
                }
            }
            _ => {}
        }
        match self.phase {
            Phase::Global => Some(DriverAction::Synchronize(Synchronization::Acquire {
                semaphore: DriverSemaphore::Global, timeout_ticks: 2000,
            })),
            Phase::Adapter => Some(DriverAction::Synchronize(Synchronization::Acquire {
                semaphore: DriverSemaphore::Adapter, timeout_ticks: 2000,
            })),
            Phase::ReleaseAdapter => Some(DriverAction::Synchronize(Synchronization::Release(DriverSemaphore::Adapter))),
            Phase::ReleaseGlobal => Some(DriverAction::Synchronize(Synchronization::Release(DriverSemaphore::Global))),
            Phase::Parameters => self.parameter.pending_mut().map(DriverAction::Parameter),
            Phase::Handshake => {
                let transfer = self.power.transfer(now)?;
                if transfer.operation == super::vendor_wireless_power::RegisterTransfer::WriteControl(1) {
                    self.parameter.reset_credits();
                }
                Some(DriverAction::Register(transfer))
            }
            Phase::Station => {
                let identity = self.station.command()?.identity;
                if self.station_issued != Some(identity) {
                    if !self.parameter.reserve_command_credit() { return None; }
                    self.station_issued = Some(identity);
                }
                self.station.command_mut().map(DriverAction::Station)
            }
            Phase::Idle | Phase::Finished => None,
        }
    }

    pub fn synchronize(&mut self, reply: SynchronizationReply) -> DriverReply {
        match (self.phase, reply) {
            (Phase::Global, SynchronizationReply::Acquired) => {
                if self.stopping {
                    self.result = DriverStatus::STOPPED;
                    self.request = None;
                    self.phase = Phase::ReleaseGlobal;
                } else { self.phase = Phase::Adapter; }
            }
            (Phase::Global, SynchronizationReply::TimedOut) => {
                self.request = None;
                self.phase = Phase::Finished;
            }
            (Phase::Adapter, SynchronizationReply::TimedOut) => {
                self.request = None;
                self.phase = Phase::ReleaseGlobal;
            }
            (Phase::Adapter, SynchronizationReply::Acquired) => {
                match self.request.take() {
                    Some(RadioDriverRequest::Parameters(program)) => {
                        match self.parameter.configure(program) {
                            Ok(()) => self.phase = Phase::Parameters,
                            Err(_) => self.phase = Phase::ReleaseAdapter,
                        }
                    }
                    Some(RadioDriverRequest::RegisterHandshake) => {
                        self.power = RadioPowerHandshake::new();
                        self.phase = Phase::Handshake;
                    }
                    Some(RadioDriverRequest::Station(request)) => {
                        self.station.set_last_sequence(self.parameter.last_sequence());
                        match self.station.submit(request) {
                            Ok(()) => {
                                self.parameter.set_last_sequence(self.station.last_sequence());
                                self.station_issued = None;
                                self.phase = Phase::Station;
                            }
                            Err(StationDriverError::Stopped) => {
                                self.result = DriverStatus::STOPPED;
                                self.phase = Phase::ReleaseAdapter;
                            }
                            Err(_) => self.phase = Phase::ReleaseAdapter,
                        }
                    }
                    None => return DriverReply::WrongOperation,
                }
            }
            (Phase::ReleaseAdapter, SynchronizationReply::Released) => self.phase = Phase::ReleaseGlobal,
            (Phase::ReleaseGlobal, SynchronizationReply::Released) => self.phase = Phase::Finished,
            (Phase::Idle | Phase::Finished, _) => return DriverReply::Late,
            _ => return DriverReply::WrongOperation,
        }
        DriverReply::Applied
    }

    pub fn parameter_reply(&mut self, identity: TransferIdentity, outcome: SdioWriteOutcome) -> TransferReply {
        if !matches!(self.phase, Phase::Parameters) { return TransferReply::Late; }
        self.parameter.receive(identity, outcome)
    }
    pub fn station_reply(&mut self, identity: StationCompletion, outcome: SdioWriteOutcome) -> StationReply {
        if !matches!(self.phase, Phase::Station) { return StationReply::Late; }
        self.station.receive(identity, outcome)
    }

    pub fn station_driver(&mut self) -> &mut StationDriver { &mut self.station }


    pub fn register_reply(&mut self, token: SdioToken, reply: SdioReply, now: RadioTick) -> PowerReply {
        if !matches!(self.phase, Phase::Handshake) { return PowerReply::Late; }
        self.power.receive(token, reply, now)
    }

    pub fn observe_credit_returns(&mut self, bytes: [u8; 8]) -> Result<bool, TransmitError> {
        self.parameter.observe_credit_returns(bytes)
    }

    pub fn parameter_driver(&mut self) -> &mut RadioParameterDriver { &mut self.parameter }
    pub const fn handshake_started(&self) -> bool { matches!(self.phase, Phase::Handshake) }

    pub const fn outcome(&self) -> Option<DriverStatus> {
        if matches!(self.phase, Phase::Finished) { Some(self.result) } else { None }
    }
}

#[cfg(test)]
#[path = "vendor_wireless_lifecycle_tests.rs"]
mod tests;
