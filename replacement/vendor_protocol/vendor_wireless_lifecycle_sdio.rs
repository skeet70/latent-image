//! Radio SDIO attachment and interrupt setup at 0x012d88d0 and 0x012d8cdc.
//! Source pin: 55436fc85c0560dd252b0410af6f2427fe198cd3.

use super::sd_controller::{AdmissionError, Argument, Buffer, CardInterruptRoute, ClockPower,
    ClockDelayTicks, ClockProgress, ClockSelection, ControlError, ControlPlane, Execution, Host, InterruptRouter,
    Mmio, Operation, Request, TransferUnit, VendorCode};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttachmentStep { Clock4, EnableRead, EnableWrite, EnableTransfer, BusRead, BusWrite,
    BlockLow, BlockHigh, Unit, Clock10, InterruptRead, InterruptWrite, InstallInterrupt }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttachmentError {
    Control { step: AttachmentStep, error: ControlError },
    Transfer { step: AttachmentStep, code: VendorCode },
    Unit(AdmissionError),
}
#[derive(Clone, Copy)]
enum Phase { Start4, Poll4, ReadEnable, WriteEnable, EnableTransfer, ReadBus, WriteBus,
    BlockLow, BlockHigh, Unit, Start10, Poll10, ReadInterrupt, WriteInterrupt, InstallInterrupt, Finished, Halted }

pub struct RadioSdioAttachment {
    phase: Phase,
    issued: bool,
    response: u8,
    failure: Option<AttachmentError>,
}

impl RadioSdioAttachment {
    pub const fn new() -> Self {
        Self { phase: Phase::Start4, issued: false, response: 0, failure: None }
    }

    pub const fn interrupt_registration() -> Self {
        Self { phase: Phase::ReadInterrupt, issued: false, response: 0, failure: None }
    }

    fn fail(&mut self, error: AttachmentError) -> AttachmentError {
        self.failure = Some(error);
        self.phase = Phase::Halted;
        error
    }

    pub fn advance(&mut self, control: &mut ControlPlane, host: &mut Host, mmio: &mut impl Mmio,
        power: &mut impl ClockPower, router: &mut impl InterruptRouter) -> Result<Option<ClockDelayTicks>, AttachmentError> {
        loop {
            let result = match self.phase {
                Phase::Start4 => control.start_clock(mmio, power, ClockSelection::Mode4)
                    .map(|progress| { self.phase = if progress == ClockProgress::Complete { Phase::ReadEnable } else { Phase::Poll4 }; })
                    .map_err(|error| AttachmentError::Control { step: AttachmentStep::Clock4, error }),
                Phase::Poll4 => {
                    let progress = control.poll_clock(mmio);
                    if progress != ClockProgress::Complete { return Ok(progress.delay_ticks()); }
                    self.phase = Phase::ReadEnable;
                    Ok(())
                }
                Phase::EnableTransfer => {
                    control.set_transfer_enabled(host, mmio, true);
                    self.phase = Phase::ReadBus;
                    Ok(())
                }
                Phase::Unit => {
                    control.set_transfer_unit(host, mmio, TransferUnit::BLOCK_512).map(|()| { self.phase = Phase::Start10; })
                        .map_err(AttachmentError::Unit)
                }
                Phase::Start10 => control.start_clock(mmio, power, ClockSelection::Mode10)
                    .map(|progress| { self.phase = if progress == ClockProgress::Complete { Phase::Finished } else { Phase::Poll10 }; })
                    .map_err(|error| AttachmentError::Control { step: AttachmentStep::Clock10, error }),
                Phase::Poll10 => {
                    let progress = control.poll_clock(mmio);
                    if progress != ClockProgress::Complete { return Ok(progress.delay_ticks()); }
                    self.phase = Phase::Finished;
                    Ok(())
                }
                Phase::InstallInterrupt => control.set_card_interrupt(host, mmio, router, CardInterruptRoute::Wireless)
                    .map(|()| { self.phase = Phase::Finished; })
                    .map_err(|error| AttachmentError::Control { step: AttachmentStep::InstallInterrupt, error }),
                Phase::Halted => return Err(self.failure.expect("halted attachment has a failure")),
                _ => return Ok(None),
            };
            if let Err(error) = result { return Err(self.fail(error)); }
        }
    }

    pub fn request(&mut self) -> Option<Request<'static>> {
        let argument = match self.phase {
            Phase::ReadEnable => 0x0000_0400,
            Phase::WriteEnable => 0x8000_0400 | u32::from(self.response | 2),
            Phase::ReadBus => 0x0000_0e00,
            Phase::WriteBus => 0x8000_0e00 | u32::from(self.response | 2),
            Phase::BlockLow => 0x8002_2000,
            Phase::BlockHigh => 0x8002_2202,
            Phase::ReadInterrupt => 0x0000_0800,
            Phase::WriteInterrupt => 0x8000_0800 | u32::from(self.response | 3),
            _ => return None,
        };
        self.issued = true;
        Some(Request { operation: Operation::Direct52, argument: Argument::from_wire(argument),
            execution: Execution::Polling, buffer: Buffer::None })
    }

    pub fn receive(&mut self, code: VendorCode, response: u8) -> Result<bool, AttachmentError> {
        if !self.issued { return Ok(false); }
        self.issued = false;
        let (step, next) = match self.phase {
            Phase::ReadEnable => (AttachmentStep::EnableRead, Phase::WriteEnable),
            Phase::WriteEnable => (AttachmentStep::EnableWrite, Phase::EnableTransfer),
            Phase::ReadBus => (AttachmentStep::BusRead, Phase::WriteBus),
            Phase::WriteBus => (AttachmentStep::BusWrite, Phase::BlockLow),
            Phase::BlockLow => (AttachmentStep::BlockLow, Phase::BlockHigh),
            Phase::BlockHigh => (AttachmentStep::BlockHigh, Phase::Unit),
            Phase::ReadInterrupt => (AttachmentStep::InterruptRead, Phase::WriteInterrupt),
            Phase::WriteInterrupt => (AttachmentStep::InterruptWrite, Phase::InstallInterrupt),
            _ => return Ok(false),
        };
        if code != VendorCode::Success && step != AttachmentStep::EnableRead {
            return Err(self.fail(AttachmentError::Transfer { step, code }));
        }
        self.response = response;
        self.phase = next;
        Ok(true)
    }

    pub const fn finished(&self) -> bool { matches!(self.phase, Phase::Finished) }
    pub const fn failure(&self) -> Option<AttachmentError> { self.failure }
}

impl Default for RadioSdioAttachment { fn default() -> Self { Self::new() } }

#[cfg(test)]
#[path = "vendor_wireless_lifecycle_sdio_tests.rs"]
mod tests;
