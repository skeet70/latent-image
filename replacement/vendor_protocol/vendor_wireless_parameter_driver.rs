//! Radio parameter transmission through the SDIO command FIFO.
//!
//! Source: X-M1 overlay47 at x70view
//! 55436fc85c0560dd252b0410af6f2427fe198cd3, actual instructions at
//! 0x012997d0, 0x012938f0, 0x012a011c, 0x012a03a4, 0x012a0538,
//! 0x012a0fb4, and 0x012d9230. Class-four commands use FIFO 0x2c.
//! 0x012a0fb4 discards the SDIO write result. Physical observations are
//! retained separately from the logical service result.

use super::vendor_wireless_radio_program::{CommandSequence, RadioParameterProgram};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransmitError {
    Busy,
    CreditOverflow,
    Cancelled,
    InvalidChannel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransferIdentity(u64);

pub struct ParameterWrite {
    pub identity: TransferIdentity,
    bytes: [u8; 20],
}

impl ParameterWrite {
    pub const fn command(&self) -> u8 { 53 }
    pub const fn argument(&self) -> u32 { 0x9000_5814 }
    pub const fn bytes(&self) -> &[u8; 20] { &self.bytes }

    pub fn host_request(&mut self) -> super::sd_controller::Request<'_> {
        super::sd_controller::Request {
            operation: super::sd_controller::Operation::Extended53,
            argument: super::sd_controller::Argument::from_wire(self.argument()),
            execution: super::sd_controller::Execution::Interrupt,
            buffer: super::sd_controller::Buffer::Fifo(&mut self.bytes),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SdioWriteOutcome {
    Written,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferReply {
    Applied,
    Late,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(usize)]
pub enum TrafficClass {
    Zero,
    One,
    Two,
    Three,
    Command,
    Five,
}

pub struct TransmitCredits {
    available: [u8; 6],
    maximum: [u8; 6],
}

impl TransmitCredits {
    pub const fn new() -> Self {
        Self { available: [1, 14, 1, 1, 4, 1], maximum: [1, 14, 1, 1, 4, 1] }
    }

    pub fn reset(&mut self) { *self = Self::new(); }

    pub fn available(&self, class: TrafficClass) -> u8 { self.available[class as usize] }

    pub fn consume(&mut self, class: TrafficClass) -> bool {
        let credit = &mut self.available[class as usize];
        if *credit == 0 { false } else { *credit -= 1; true }
    }

    pub fn observe_returns(&mut self, bytes: [u8; 8]) -> Result<bool, TransmitError> {
        if bytes == [0; 8] { return Ok(false); }
        let mut updated = self.available;
        for ((credit, returned), maximum) in updated.iter_mut().zip(bytes).zip(self.maximum) {
            *credit = credit.wrapping_add(returned);
            if *credit > maximum { return Err(TransmitError::CreditOverflow); }
        }
        self.available = updated;
        Ok(true)
    }
}

impl Default for TransmitCredits {
    fn default() -> Self { Self::new() }
}

pub struct RadioParameterDriver {
    credits: TransmitCredits,
    sequence: CommandSequence,
    next_identity: u64,
    program: Option<RadioParameterProgram>,
    pending: Option<ParameterWrite>,
    outcome: Option<Result<(), TransmitError>>,
    last_physical: Option<SdioWriteOutcome>,
}

impl RadioParameterDriver {
    pub const fn new(last_sequence: u8) -> Self {
        Self {
            credits: TransmitCredits::new(),
            sequence: CommandSequence::from_last_issued(last_sequence),
            next_identity: 0,
            program: None,
            pending: None,
            outcome: None,
            last_physical: None,
        }
    }

    pub fn configure(&mut self, program: RadioParameterProgram) -> Result<(), TransmitError> {
        if self.program.is_some() || self.pending.is_some() { return Err(TransmitError::Busy); }
        self.program = Some(program);
        self.outcome = None;
        Ok(())
    }

    pub fn transfer(&mut self) -> Option<&ParameterWrite> {
        if self.pending.is_some() { return self.pending.as_ref(); }
        let program = self.program.as_mut()?;
        if program.remaining() == 0 {
            let result = if program.terminal_error().is_some() { Err(TransmitError::InvalidChannel) } else { Ok(()) };
            self.program = None;
            self.outcome = Some(result);
            return None;
        }
        if self.credits.available(TrafficClass::Command) == 0 { return None; }
        let parameter = program.next()?;
        self.credits.consume(TrafficClass::Command);
        let mut bytes = [0; 20];
        parameter.write_driver_command(self.sequence.issue_next(), &mut bytes);
        bytes[2] = 0;
        bytes[3] = 0x50;
        self.next_identity = self.next_identity.wrapping_add(1);
        self.pending = Some(ParameterWrite { identity: TransferIdentity(self.next_identity), bytes });
        self.pending.as_ref()
    }

    pub fn pending_mut(&mut self) -> Option<&mut ParameterWrite> { self.pending.as_mut() }

    pub fn reset_credits(&mut self) { self.credits.reset(); }

    pub fn take_credits(&mut self) -> TransmitCredits { core::mem::take(&mut self.credits) }
    pub fn set_credits(&mut self, credits: TransmitCredits) { self.credits = credits; }
    pub const fn last_sequence(&self) -> u8 { self.sequence.wire() }
    pub fn set_last_sequence(&mut self, sequence: u8) {
        self.sequence = CommandSequence::from_last_issued(sequence);
    }
    pub fn reserve_command_credit(&mut self) -> bool { self.credits.consume(TrafficClass::Command) }

    pub fn receive(&mut self, identity: TransferIdentity, written: SdioWriteOutcome) -> TransferReply {
        if self.pending.as_ref().map(|pending| pending.identity) != Some(identity) { return TransferReply::Late; }
        self.pending = None;
        self.last_physical = Some(written);
        if self.program.as_ref().map(RadioParameterProgram::remaining) == Some(0) {
            self.outcome = Some(if self.program.as_ref().and_then(RadioParameterProgram::terminal_error).is_some() {
                Err(TransmitError::InvalidChannel)
            } else { Ok(()) });
            self.program = None;
        }
        TransferReply::Applied
    }

    pub fn observe_credit_returns(&mut self, bytes: [u8; 8]) -> Result<bool, TransmitError> {
        self.credits.observe_returns(bytes)
    }

    pub fn cancel(&mut self) {
        if self.program.is_some() || self.pending.is_some() {
            self.program = None;
            self.pending = None;
            self.outcome = Some(Err(TransmitError::Cancelled));
        }
    }

    pub const fn outcome(&self) -> Option<Result<(), TransmitError>> { self.outcome }

    pub const fn last_physical_outcome(&self) -> Option<SdioWriteOutcome> { self.last_physical }
}

#[cfg(test)]
#[path = "vendor_wireless_parameter_driver_tests.rs"]
mod tests;
