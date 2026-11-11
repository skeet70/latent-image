//! Device firmware download portion of X-M1 0x01292024, through 0x0129261c.
//! Source pin: 55436fc85c0560dd252b0410af6f2427fe198cd3.

use super::sd_controller::{Argument, Buffer, Completion, Execution, Operation, Request};
use super::vendor_wireless_lifecycle::DriverStatus;
use super::vendor_wireless_power::RadioTick;
use super::vendor_wireless_lifecycle_firmware::{ChunkLength, encode_chunk, write_chunk_request};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FirmwareAddress(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FirmwareDownloadError { ImageTooLarge }

#[derive(Clone, Copy)]
enum PollTarget { Reset, Start, Chunk, Complete }
#[derive(Clone, Copy)]
enum Phase { ResetHigh, ResetLow, Delay(RadioTick), Poll(PollTarget), Length, Start,
    EnableDownload, Address, Chunk, DisableDownload, Finished }

pub struct FirmwareDownload<'a> {
    image: &'a [u8],
    address: FirmwareAddress,
    phase: Phase,
    after_delay: PollTarget,
    offset: usize,
    polls: u16,
    status: DriverStatus,
    chunk_failure: bool,
    issued: bool,
    physical: Option<Completion>,
    start_after_delay: bool,
    chunk_length: Option<ChunkLength>,
}

impl<'a> FirmwareDownload<'a> {
    pub fn new(image: &'a [u8], address: FirmwareAddress) -> Result<Self, FirmwareDownloadError> {
        if u32::try_from(image.len()).is_err() { return Err(FirmwareDownloadError::ImageTooLarge); }
        Ok(Self { image, address, phase: Phase::ResetHigh, after_delay: PollTarget::Reset,
            offset: 0, polls: 0, status: DriverStatus::SUCCESS, chunk_failure: false,
            issued: false, physical: None, start_after_delay: false, chunk_length: None })
    }

    pub fn request<'b>(&mut self, now: RadioTick, register: &'b mut [u8; 4],
        chunk: &'b mut [u8; 2560]) -> Option<Request<'b>> {
        if let Phase::Delay(deadline) = self.phase {
            if (now.0.wrapping_sub(deadline.0) as i32) < 0 { return None; }
            if self.start_after_delay { self.phase = Phase::Start; self.start_after_delay = false; }
            else { self.phase = Phase::Poll(self.after_delay); }
        }
        if matches!(self.phase, Phase::Chunk) {
            let length = (self.image.len() - self.offset).min(2048);
            if !self.issued {
                self.chunk_length = Some(encode_chunk(&self.image[self.offset..self.offset + length], chunk));
            }
            self.issued = true;
            return Some(write_chunk_request(self.chunk_length?, chunk));
        }
        let (address, write) = match self.phase {
            Phase::ResetHigh => (0x8c, Some(0x8000u32)),
            Phase::ResetLow => (0x8c, Some(0)),
            Phase::Poll(PollTarget::Chunk) => (0x88, None),
            Phase::Poll(_) => (0x8c, None),
            Phase::Length => (0x90, Some((self.image.len() as u32) >> 6)),
            Phase::Start => (0x8c, Some(1)),
            Phase::EnableDownload => (0x88, Some(1)),
            Phase::Address => (0x84, Some(self.address.0)),
            Phase::DisableDownload => (0x88, Some(0)),
            Phase::Delay(_) | Phase::Finished | Phase::Chunk => return None,
        };
        if let Some(value) = write { *register = value.to_le_bytes(); }
        self.issued = true;
        Some(Request { operation: Operation::Extended53,
            argument: Argument::from_wire(0x1000_0004 | (address << 9) | if write.is_some() { 0x8000_0000 } else { 0 }),
            execution: Execution::Polling, buffer: Buffer::Fifo(register) })
    }

    fn begin_poll(&mut self, target: PollTarget) {
        self.polls = 0;
        self.phase = Phase::Poll(target);
    }

    pub fn receive(&mut self, physical: Completion, register: [u8; 4], now: RadioTick, cancelled: bool) -> bool {
        if !self.issued { return false; }
        self.issued = false;
        self.physical = Some(physical);
        match self.phase {
            Phase::ResetHigh => self.phase = Phase::ResetLow,
            Phase::ResetLow => {
                self.after_delay = PollTarget::Reset;
                self.phase = Phase::Delay(RadioTick(now.0.wrapping_add(120)));
            }
            Phase::Length => {
                self.start_after_delay = true;
                self.phase = Phase::Delay(RadioTick(now.0.wrapping_add(10)));
            }
            Phase::Start => self.begin_poll(PollTarget::Start),
            Phase::EnableDownload => self.phase = Phase::Address,
            Phase::Address => {
                self.begin_poll(if self.image.is_empty() { PollTarget::Complete } else { PollTarget::Chunk });
            }
            Phase::Poll(target) => {
                let value = u32::from_le_bytes(register);
                let ready = value & match target { PollTarget::Reset => 0x800, PollTarget::Start => 0x2000,
                    PollTarget::Chunk => 0x100, PollTarget::Complete => 0x8000 } != 0;
                if ready {
                    self.phase = match target { PollTarget::Reset => Phase::Length,
                        PollTarget::Start => Phase::EnableDownload, PollTarget::Chunk => Phase::Chunk,
                        PollTarget::Complete => Phase::DisableDownload };
                } else if cancelled || self.polls > 511
                    || (matches!(target, PollTarget::Complete) && value & 0x4000 != 0) {
                    self.status = DriverStatus::FAILURE;
                    if matches!(target, PollTarget::Chunk) {
                        self.chunk_failure = true;
                        self.phase = Phase::Chunk;
                    } else { self.phase = Phase::Finished; }
                } else {
                    self.polls += 1;
                    self.after_delay = target;
                    self.phase = Phase::Delay(RadioTick(now.0.wrapping_add(10)));
                }
            }
            Phase::Chunk => {
                if self.chunk_failure { self.phase = Phase::Finished; }
                else {
                    self.offset = (self.offset + 2048).min(self.image.len());
                    self.begin_poll(if self.offset == self.image.len() { PollTarget::Complete } else { PollTarget::Chunk });
                }
            }
            Phase::DisableDownload => self.phase = Phase::Finished,
            Phase::Delay(_) | Phase::Finished => return false,
        }
        true
    }

    pub const fn outcome(&self) -> Option<DriverStatus> {
        if matches!(self.phase, Phase::Finished) { Some(self.status) } else { None }
    }
    pub const fn last_physical(&self) -> Option<Completion> { self.physical }
}

#[cfg(test)]
#[path = "vendor_wireless_lifecycle_download_tests.rs"]
mod tests;
