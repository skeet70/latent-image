//! Register programming prefix of action 16, 0x00dbb3a4.
//! Source: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3, authenticated image 5.

use crate::configuration_i2c_actor::{RegisterAddress, RegisterIo};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DiagnosticTick(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramError { Busy }

pub struct DiagnosticRegisters { started: Option<DiagnosticTick> }

impl DiagnosticRegisters {
    pub const fn new() -> Self { Self { started: None } }

    pub fn disable(&mut self, io: &mut impl RegisterIo) {
        self.started = None;
        io.write(RegisterAddress(0xfff8_9080), 0x8000_0000);
        io.write(RegisterAddress(0xfff8_9080), 0x4000_0000);
    }

    pub fn enable(&mut self, io: &mut impl RegisterIo, now: DiagnosticTick) -> Result<(), ProgramError> {
        if self.started.is_some() { return Err(ProgramError::Busy); }
        io.write(RegisterAddress(0xfff8_9080), 0x4000_4000);
        self.started = Some(now);
        Ok(())
    }

    pub fn poll(&mut self, io: &mut impl RegisterIo, now: DiagnosticTick) -> bool {
        let Some(started) = self.started else { return false; };
        if now.0.wrapping_sub(started.0) < 20 { return false; }
        self.started = None;
        for (address, value) in [
            (0xfff8_9080, 0x8000_8000),
            (0xfff8_9350, 0x0003_0001),
            (0xfff8_9344, 0x0100_0100),
            (0xfff8_934c, 0x0030_0010),
            (0xfff8_9344, 0x0004_0004),
            (0xfff8_934c, 0x0c00_0400),
            (0xfff8_9344, 0x0020_0000),
        ] { io.write(RegisterAddress(address), value); }
        true
    }
}
