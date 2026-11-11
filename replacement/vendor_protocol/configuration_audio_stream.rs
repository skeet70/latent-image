use super::{dma::{AudioRegisters, ControllerBank, DmaError, SamplePacking, TransferDirection, TransferLease}, packets::{PcmAddress, PcmByteLength, ServicePacketQueue}, pipeline::{PipelineError, TransferPipeline}};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StreamError { Dma(DmaError), NotConfigured, Stopping }
impl From<DmaError> for StreamError { fn from(value: DmaError) -> Self { Self::Dma(value) } }

pub struct AudioStream {
    pipeline: TransferPipeline,
    packets: ServicePacketQueue,
    lease: Option<TransferLease>,
    direction: TransferDirection,
    started: bool,
    marker: bool,
    command_active: bool,
    command_complete: bool,
    stopping: bool,
    cancel_started: bool,
}
impl AudioStream {
    pub const fn new() -> Self {
        Self { pipeline: TransferPipeline::new(), packets: ServicePacketQueue::new(), lease: None, direction: TransferDirection::Receive,
            started: false, marker: false, command_active: false, command_complete: false, stopping: false, cancel_started: false }
    }
    pub fn configure(&mut self, bank: ControllerBank, direction: TransferDirection, packing: SamplePacking, padding: PcmAddress, padding_length: PcmByteLength) -> Result<TransferLease, StreamError> {
        let lease = self.pipeline.acquire(bank, direction, packing, padding, padding_length)?;
        self.packets.clear();
        self.lease = Some(lease);
        self.direction = direction;
        self.started = false;
        self.marker = false;
        self.command_active = false;
        self.command_complete = false;
        self.stopping = false;
        self.cancel_started = false;
        Ok(lease)
    }
    pub fn enqueue(&mut self, registers: &mut impl AudioRegisters, position: PcmAddress, length: PcmByteLength) -> Result<(), StreamError> {
        let lease = self.lease.ok_or(StreamError::NotConfigured)?;
        if self.stopping { return Err(StreamError::Stopping); }
        let empty = self.packets.front().is_none();
        self.packets.submit(position, length);
        let Some(packet) = self.packets.front() else { return Ok(()); };
        if !self.started {
            if self.direction == TransferDirection::Receive {
                self.command_active = true;
                self.pipeline.submit(registers, lease, packet.position(), packet.length(), false)?;
            } else {
                self.marker = true;
                self.pipeline.submit(registers, lease, PcmAddress::from_abi(0), PcmByteLength::from_abi(0), false)?;
                self.pipeline.submit(registers, lease, PcmAddress::from_abi(0), PcmByteLength::from_abi(0), false)?;
            }
            self.started = true;
            self.advance_ready(registers)?;
        } else if empty && !self.command_active {
            self.command_active = true;
            self.pipeline.submit(registers, lease, packet.position(), packet.length(), true)?;
        }
        Ok(())
    }
    fn advance_ready(&mut self, registers: &mut impl AudioRegisters) -> Result<(), StreamError> {
        if !self.pipeline.take_buffer_ready() { return Ok(()); }
        let lease = self.lease.ok_or(StreamError::NotConfigured)?;
        if self.marker { self.marker = false; } else { self.packets.release_front(); }
        match self.packets.front() {
            Some(packet) => {
                if packet.first() {
                    if self.command_active { self.command_complete = true; }
                    self.command_active = true;
                }
                self.pipeline.submit(registers, lease, packet.position(), packet.length(), true)?;
            },
            None => {
                if self.command_active { self.command_complete = true; self.command_active = false; }
                if self.stopping && !self.cancel_started && !self.pipeline.completed() {
                    self.pipeline.cancel(registers, lease)?;
                    self.cancel_started = true;
                }
            },
        }
        Ok(())
    }
    pub fn physical_completion(&mut self, registers: &mut impl AudioRegisters, lease: TransferLease) -> Result<(), StreamError> {
        if self.lease != Some(lease) { return Ok(()); }
        self.pipeline.physical_completion(registers, lease)?;
        self.advance_ready(registers)?;
        if self.pipeline.completed() { self.lease = None; }
        Ok(())
    }
    pub fn stop_after_queued(&mut self, registers: &mut impl AudioRegisters) -> Result<(), StreamError> {
        let lease = self.lease.ok_or(StreamError::NotConfigured)?;
        self.stopping = true;
        if self.packets.front().is_none() {
            self.pipeline.cancel(registers, lease)?;
            self.cancel_started = true;
        }
        Ok(())
    }
    pub fn cancel(&mut self, registers: &mut impl AudioRegisters) -> Result<(), StreamError> {
        let lease = self.lease.ok_or(StreamError::NotConfigured)?;
        self.stopping = true;
        self.packets.clear();
        self.command_active = false;
        self.command_complete = false;
        self.pipeline.cancel(registers, lease)?;
        self.cancel_started = true;
        if self.pipeline.completed() { self.lease = None; }
        Ok(())
    }
    pub fn take_command_completed(&mut self) -> bool { core::mem::replace(&mut self.command_complete, false) }
    pub const fn completed(&self) -> bool { self.pipeline.completed() }
    pub const fn pipeline_error(&self) -> Option<PipelineError> { self.pipeline.error() }
}
impl Default for AudioStream { fn default() -> Self { Self::new() } }
