//! Opaque device-firmware chunk transfer at X-M1 0x012946c8.
//! Source pin: 55436fc85c0560dd252b0410af6f2427fe198cd3.

use super::sd_controller::{Argument, Buffer, Execution, Operation, Request};
use super::vendor_wireless_lifecycle::DriverStatus;
use super::vendor_wireless_power::RadioTick;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FirmwareAdmissionError { ChunkTooLarge }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FirmwareAction { ReadReady, WriteChunk }

#[derive(Clone, Copy)]
enum Phase { Read, Delay(RadioTick), Write, Finished }

#[derive(Clone, Copy)]
pub(crate) struct ChunkLength(usize);

pub(crate) fn encode_chunk(payload: &[u8], buffer: &mut [u8; 2560]) -> ChunkLength {
    let wire_length = (payload.len() + 7) & !3;
    buffer[..2].copy_from_slice(&(wire_length as u16).to_le_bytes());
    buffer[2..4].fill(0);
    buffer[4..4 + payload.len()].copy_from_slice(payload);
    let end = 4 + ((payload.len() + 3) & !3);
    buffer[end..end + 4].fill(0);
    ChunkLength(wire_length)
}

pub(crate) fn write_chunk_request(length: ChunkLength, buffer: &mut [u8; 2560]) -> Request<'_> {
    let (argument, transfer_length) = if length.0 >= 512 {
        let count = length.0.div_ceil(512);
        (0x9801_0000 | count as u32, count * 512)
    } else { (0x9001_0000 | length.0 as u32, length.0) };
    Request { operation: Operation::Extended53, argument: Argument::from_wire(argument),
        execution: Execution::Interrupt, buffer: Buffer::Fifo(&mut buffer[..transfer_length]) }
}

pub struct FirmwareChunk<'a> {
    buffer: &'a mut [u8; 2560],
    wire_length: ChunkLength,
    phase: Phase,
    polls: u16,
    status: DriverStatus,
}

impl<'a> FirmwareChunk<'a> {
    pub fn new(payload: &[u8], buffer: &'a mut [u8; 2560]) -> Result<Self, FirmwareAdmissionError> {
        if payload.len() > 2048 { return Err(FirmwareAdmissionError::ChunkTooLarge); }
        let wire_length = if payload.is_empty() { ChunkLength(0) } else { encode_chunk(payload, buffer) };
        Ok(Self { buffer, wire_length, phase: if payload.is_empty() { Phase::Finished } else { Phase::Read },
            polls: 0, status: DriverStatus::SUCCESS })
    }

    pub fn action(&mut self, now: RadioTick) -> Option<FirmwareAction> {
        if let Phase::Delay(deadline) = self.phase {
            if (now.0.wrapping_sub(deadline.0) as i32) < 0 { return None; }
            self.phase = Phase::Read;
        }
        match self.phase {
            Phase::Read => Some(FirmwareAction::ReadReady),
            Phase::Write => Some(FirmwareAction::WriteChunk),
            Phase::Delay(_) | Phase::Finished => None,
        }
    }

    pub fn ready_reply(&mut self, register: u32, cancelled: bool, now: RadioTick) -> bool {
        if !matches!(self.phase, Phase::Read) { return false; }
        if register & 0x100 != 0 { self.phase = Phase::Write; }
        else if cancelled || self.polls > 511 {
            self.status = DriverStatus::FAILURE;
            self.phase = Phase::Write;
        } else {
            self.polls += 1;
            self.phase = Phase::Delay(RadioTick(now.0.wrapping_add(10)));
        }
        true
    }

    pub fn ready_request<'b>(&self, buffer: &'b mut [u8; 4]) -> Request<'b> {
        Request { operation: Operation::Extended53,
            argument: Argument::from_wire(0x1001_1004), execution: Execution::Polling,
            buffer: Buffer::Fifo(buffer) }
    }

    pub fn write_request(&mut self) -> Option<Request<'_>> {
        if !matches!(self.phase, Phase::Write) { return None; }
        Some(write_chunk_request(self.wire_length, self.buffer))
    }

    pub fn written(&mut self) -> bool {
        if !matches!(self.phase, Phase::Write) { return false; }
        self.phase = Phase::Finished;
        true
    }

    pub const fn outcome(&self) -> Option<DriverStatus> {
        if matches!(self.phase, Phase::Finished) { Some(self.status) } else { None }
    }
}

#[cfg(test)]
#[path = "vendor_wireless_lifecycle_firmware_tests.rs"]
mod tests;
