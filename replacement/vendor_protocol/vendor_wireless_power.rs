//! Register handshake within X-M1 0x012ac698 and 0x012aca24.
//!
//! Authenticated overlay47 instructions at x70view
//! 55436fc85c0560dd252b0410af6f2427fe198cd3 establish the 500-tick
//! initial delay, 10-tick status polling, status bit 21, and optional
//! interrupt-status reads. Software teardown precedes this handshake;
//! adapter reinitialization and host interrupt registration follow it.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RadioTick(pub u32);

impl RadioTick {
    fn reached(self, deadline: Self) -> bool { (self.0.wrapping_sub(deadline.0) as i32) >= 0 }
    fn after(self, ticks: u32) -> Self { Self(self.0.wrapping_add(ticks)) }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SdioToken(u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RadioRegister {
    Status,
    Control,
    InterruptStatus,
    InterruptLow,
    InterruptHigh,
}

impl RadioRegister {
    pub const fn address(self) -> u32 {
        match self {
            Self::Status => 0,
            Self::Control => 4,
            Self::InterruptStatus => 16,
            Self::InterruptLow => 32,
            Self::InterruptHigh => 36,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegisterTransfer {
    Read(RadioRegister),
    WriteControl(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SdioTransfer {
    pub token: SdioToken,
    pub operation: RegisterTransfer,
}

impl SdioTransfer {
    pub const fn command(self) -> u8 { 53 }

    pub const fn argument(self) -> u32 {
        let (register, write) = match self.operation {
            RegisterTransfer::Read(register) => (register, 0),
            RegisterTransfer::WriteControl(_) => (RadioRegister::Control, 0x8000_0000),
        };
        write | 0x1000_0004 | (register.address() << 9)
    }

    pub const fn write_data(self) -> Option<[u8; 4]> {
        match self.operation {
            RegisterTransfer::Read(_) => None,
            RegisterTransfer::WriteControl(value) => Some(value.to_le_bytes()),
        }
    }

    pub fn host_request<'a>(self, buffer: &'a mut [u8; 4]) -> super::sd_controller::Request<'a> {
        if let Some(bytes) = self.write_data() { *buffer = bytes; }
        super::sd_controller::Request {
            operation: super::sd_controller::Operation::Extended53,
            argument: super::sd_controller::Argument::from_wire(self.argument()),
            execution: super::sd_controller::Execution::Polling,
            buffer: super::sd_controller::Buffer::Fifo(buffer),
        }
    }

    pub fn host_completion(self, control: &mut super::sd_controller::ControlPlane,
        host: &mut super::sd_controller::Host, mmio: &mut impl super::sd_controller::Mmio,
        router: &mut impl super::sd_controller::InterruptRouter,
        physical: super::sd_controller::Completion, buffer: [u8; 4],
    ) -> Result<RadioRegisterCompletion, super::sd_controller::ControlError> {
        if matches!(self.operation, RegisterTransfer::WriteControl(value) if value & 1 != 0) {
            control.set_card_interrupt(host, mmio, router, super::sd_controller::CardInterruptRoute::Wireless)?;
        }
        let logical = match self.operation {
            RegisterTransfer::Read(_) => SdioReply::Read(buffer),
            RegisterTransfer::WriteControl(_) => SdioReply::Written,
        };
        Ok(RadioRegisterCompletion { physical, logical })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SdioReply {
    Read([u8; 4]),
    Written,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RadioRegisterCompletion {
    pub physical: super::sd_controller::Completion,
    pub logical: SdioReply,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PowerOutcome {
    Ready,
    Cancelled,
}

impl PowerOutcome {
    pub const fn driver_status(self) -> u32 {
        match self { Self::Ready => 0, Self::Cancelled => 0xc000_00bb }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PowerReply {
    Applied,
    Late,
    WrongTransfer,
}

#[derive(Clone, Copy)]
enum Phase {
    DisableInterrupts,
    InitialDelay(RadioTick),
    ReadStatus,
    PollDelay(RadioTick),
    ReadInterruptStatus,
    ReadInterruptLow,
    ReadInterruptHigh,
    EnableInterrupts,
    Finished(PowerOutcome),
}

pub struct RadioPowerHandshake {
    phase: Phase,
    issued: Option<SdioTransfer>,
    next_token: u32,
    cancelled: bool,
    status: u32,
    interrupt_status: u32,
    interrupt_words: [u32; 2],
}

impl RadioPowerHandshake {
    pub const fn new() -> Self {
        Self {
            phase: Phase::DisableInterrupts,
            issued: None,
            next_token: 0,
            cancelled: false,
            status: 0,
            interrupt_status: 0,
            interrupt_words: [0; 2],
        }
    }

    pub fn cancel(&mut self) { self.cancelled = true; }

    pub fn transfer(&mut self, now: RadioTick) -> Option<SdioTransfer> {
        if let Some(transfer) = self.issued { return Some(transfer); }
        match self.phase {
            Phase::InitialDelay(deadline) | Phase::PollDelay(deadline) => {
                if !now.reached(deadline) { return None; }
                self.phase = Phase::ReadStatus;
            }
            _ => {}
        }
        let operation = match self.phase {
            Phase::DisableInterrupts => RegisterTransfer::WriteControl(2),
            Phase::ReadStatus => RegisterTransfer::Read(RadioRegister::Status),
            Phase::ReadInterruptStatus => RegisterTransfer::Read(RadioRegister::InterruptStatus),
            Phase::ReadInterruptLow => RegisterTransfer::Read(RadioRegister::InterruptLow),
            Phase::ReadInterruptHigh => RegisterTransfer::Read(RadioRegister::InterruptHigh),
            Phase::EnableInterrupts => RegisterTransfer::WriteControl(1),
            Phase::InitialDelay(_) | Phase::PollDelay(_) | Phase::Finished(_) => return None,
        };
        self.next_token = self.next_token.wrapping_add(1);
        let transfer = SdioTransfer { token: SdioToken(self.next_token), operation };
        self.issued = Some(transfer);
        Some(transfer)
    }

    pub fn receive(&mut self, token: SdioToken, reply: SdioReply, now: RadioTick) -> PowerReply {
        let Some(transfer) = self.issued else { return PowerReply::Late; };
        if token != transfer.token { return PowerReply::Late; }
        let value = match (transfer.operation, reply) {
            (RegisterTransfer::Read(_), SdioReply::Read(bytes)) => Some(u32::from_le_bytes(bytes)),
            (RegisterTransfer::WriteControl(_), SdioReply::Written) | (_, SdioReply::Failed) => None,
            _ => return PowerReply::WrongTransfer,
        };
        self.issued = None;
        self.phase = match self.phase {
            Phase::DisableInterrupts => Phase::InitialDelay(now.after(500)),
            Phase::ReadStatus => {
                if let Some(value) = value { self.status = value; }
                if self.status & 0x0020_0000 != 0 {
                    Phase::ReadInterruptStatus
                } else if self.cancelled {
                    Phase::Finished(PowerOutcome::Cancelled)
                } else {
                    Phase::PollDelay(now.after(10))
                }
            }
            Phase::ReadInterruptStatus => {
                if let Some(value) = value { self.interrupt_status = value; }
                if self.interrupt_status & 1 != 0 { Phase::ReadInterruptLow } else { Phase::EnableInterrupts }
            }
            Phase::ReadInterruptLow => {
                if let Some(value) = value { self.interrupt_words[0] = value; }
                Phase::ReadInterruptHigh
            }
            Phase::ReadInterruptHigh => {
                if let Some(value) = value { self.interrupt_words[1] = value; }
                Phase::EnableInterrupts
            }
            Phase::EnableInterrupts => Phase::Finished(PowerOutcome::Ready),
            Phase::InitialDelay(_) | Phase::PollDelay(_) | Phase::Finished(_) => return PowerReply::WrongTransfer,
        };
        PowerReply::Applied
    }

    pub const fn outcome(&self) -> Option<PowerOutcome> {
        match self.phase { Phase::Finished(outcome) => Some(outcome), _ => None }
    }

    pub const fn interrupt_words(&self) -> [u32; 2] { self.interrupt_words }
}

impl Default for RadioPowerHandshake {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
#[path = "vendor_wireless_power_tests.rs"]
mod tests;
