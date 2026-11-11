//! SDIO IRQ disable at 0x012d8df8 and function detach at 0x012d8be4.

use super::sd_controller::{Argument, Buffer, CardInterruptRoute, Completion, ControlError, ControlPlane, Execution, Host, InterruptRouter, Mmio, Operation, PhysicalResult, Request, VendorCode};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DetachError { Busy, Control(ControlError), Physical(Completion) }
pub struct InterruptsDisabled { _physical: Completion }
pub struct SdioDetached { _physical: Completion }
#[derive(Clone, Copy, Eq, PartialEq)]
enum InterruptPhase { Route, Write, Finished }
pub struct InterruptDisable { phase: InterruptPhase, issued: bool, outcome: Option<Result<InterruptsDisabled, DetachError>> }
impl Default for InterruptDisable { fn default() -> Self { Self::new() } }
impl InterruptDisable {
    pub const fn new() -> Self { Self { phase: InterruptPhase::Route, issued: false, outcome: None } }
    pub fn remove_route(&mut self, control: &mut ControlPlane, host: &mut Host, mmio: &mut impl Mmio, router: &mut impl InterruptRouter) -> Result<(), DetachError> {
        if self.phase != InterruptPhase::Route { return Err(DetachError::Busy); }
        control.set_card_interrupt(host, mmio, router, CardInterruptRoute::Removed).map_err(DetachError::Control)?;
        self.phase = InterruptPhase::Write;
        Ok(())
    }
    pub fn request(&mut self) -> Option<Request<'_>> {
        if self.phase != InterruptPhase::Write || self.issued { return None; }
        self.issued = true;
        Some(Request { operation: Operation::Direct52, argument: Argument::from_wire(0x8000_0800), execution: Execution::Polling, buffer: Buffer::None })
    }
    pub fn receive(&mut self, physical: Completion) -> bool {
        if !self.issued || self.phase != InterruptPhase::Write { return false; }
        self.issued = false;
        self.phase = InterruptPhase::Finished;
        self.outcome = Some(if confirmed(physical) { Ok(InterruptsDisabled { _physical: physical }) } else { Err(DetachError::Physical(physical)) });
        true
    }
    pub fn completion(&mut self) -> Option<Result<InterruptsDisabled, DetachError>> { self.outcome.take() }
}
#[derive(Clone, Copy, Eq, PartialEq)]
enum Phase { Read, Write(u8), Finished }
pub struct SdioDetach { phase: Phase, issued: bool, outcome: Option<Result<SdioDetached, DetachError>> }
impl Default for SdioDetach { fn default() -> Self { Self::new() } }
impl SdioDetach {
    pub const fn new() -> Self { Self { phase: Phase::Read, issued: false, outcome: None } }
    pub fn request(&mut self) -> Option<Request<'_>> {
        if self.issued { return None; }
        let argument = match self.phase { Phase::Read => 0x400, Phase::Write(value) => 0x8000_0400 | u32::from(value), Phase::Finished => return None };
        self.issued = true;
        Some(Request { operation: Operation::Direct52, argument: Argument::from_wire(argument), execution: Execution::Polling, buffer: Buffer::None })
    }
    pub fn receive(&mut self, physical: Completion) -> bool {
        if !self.issued || self.phase == Phase::Finished { return false; }
        self.issued = false;
        if !confirmed(physical) {
            self.phase = Phase::Finished;
            self.outcome = Some(Err(DetachError::Physical(physical)));
        } else {
            match self.phase {
                Phase::Read => self.phase = Phase::Write(physical.response & !2),
                Phase::Write(_) => { self.phase = Phase::Finished; self.outcome = Some(Ok(SdioDetached { _physical: physical })); }
                Phase::Finished => return false,
            }
        }
        true
    }
    pub fn completion(&mut self) -> Option<Result<SdioDetached, DetachError>> { self.outcome.take() }
}
fn confirmed(physical: Completion) -> bool { physical.code == VendorCode::Success && physical.physical == PhysicalResult::Completed }
