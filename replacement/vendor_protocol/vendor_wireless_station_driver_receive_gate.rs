//! Firmware ownership handshake at X-M1 0x012a4c60.
use super::super::super::sd_controller::{Argument, Buffer, Execution, Operation, Request};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Tick(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FirmwareOwnership { Driver, Firmware }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GateError { Timeout, Removed, Cancelled, Stale }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PhysicalOutcome { Completed, Failed }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StopFlags { pub bus_removed: bool, pub service_stopped: bool }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase { Read, Write, Delay, Terminal }
pub struct OwnershipGate {
    ownership: FirmwareOwnership,
    phase: Phase,
    started: Tick,
    iterations: u32,
    register: [u8; 4],
    outcome: Option<Result<(), GateError>>,
    physical: Option<PhysicalOutcome>,
    issued: bool,
}
impl OwnershipGate {
    pub const fn new(ownership: FirmwareOwnership, now: Tick) -> Self {
        Self { ownership, phase: match ownership { FirmwareOwnership::Driver => Phase::Terminal, FirmwareOwnership::Firmware => Phase::Read },
            started: now, iterations: 0, register: [0; 4], outcome: match ownership { FirmwareOwnership::Driver => Some(Ok(())), FirmwareOwnership::Firmware => None }, physical: None, issued: false }
    }
    pub fn request(&mut self) -> Option<Request<'_>> {
        if self.issued { return None; }
        let argument = match self.phase {
            Phase::Read => 0x1000_0804,
            Phase::Write => { self.register = 0x200u32.to_le_bytes(); 0x9000_0804 },
            _ => return None,
        };
        self.issued = true;
        Some(Request { operation: Operation::Extended53, argument: Argument::from_wire(argument), execution: Execution::Polling, buffer: Buffer::Fifo(&mut self.register) })
    }
    pub fn finish(&mut self, physical: PhysicalOutcome, now: Tick, flags: StopFlags) -> Result<(), GateError> {
        if !self.issued { return Err(GateError::Stale); }
        self.issued = false;
        self.physical = Some(physical);
        match self.phase {
            Phase::Read => {
                if u32::from_le_bytes(self.register) & 0x100 != 0 {
                    self.ownership = FirmwareOwnership::Driver;
                    self.outcome = Some(Ok(()));
                    self.phase = Phase::Terminal;
                } else if flags.bus_removed || flags.service_stopped {
                    self.outcome = Some(Err(GateError::Removed));
                    self.phase = Phase::Terminal;
                } else if now.0.wrapping_sub(self.started.0) > 0x2000 {
                    self.outcome = Some(Err(GateError::Timeout));
                    self.phase = Phase::Terminal;
                } else {
                    self.phase = if self.iterations as u8 == 0 { Phase::Write } else { Phase::Delay };
                }
            }
            Phase::Write => self.phase = Phase::Delay,
            _ => return Err(GateError::Stale),
        }
        Ok(())
    }
    pub const fn delay_ticks(&self) -> Option<u32> { if matches!(self.phase, Phase::Delay) { Some(1) } else { None } }
    pub fn finish_delay(&mut self) -> Result<(), GateError> {
        if self.phase != Phase::Delay { return Err(GateError::Stale); }
        self.iterations = self.iterations.wrapping_add(1);
        self.phase = Phase::Read;
        Ok(())
    }
    pub const fn outcome(&self) -> Option<Result<(), GateError>> { self.outcome }
    pub const fn ownership(&self) -> FirmwareOwnership { self.ownership }
    pub const fn last_physical(&self) -> Option<PhysicalOutcome> { self.physical }
    pub fn cancel(&mut self) { self.issued = false; self.phase = Phase::Terminal; self.outcome = Some(Err(GateError::Cancelled)); }
}
