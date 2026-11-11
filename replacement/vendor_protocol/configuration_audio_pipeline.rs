use super::{dma::{self, AudioDma, AudioRegisters, ControllerBank, DmaError, SamplePacking, TransferDirection, TransferLease, TransferProgram}, packets::{PcmAddress, PcmByteLength}};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase { Closed, Acquired, Primed, Streaming, ReceiveCancel, Drain, Stopping, Failed }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PipelineError { Resource, Start, Update, Phase, Stop }
impl PipelineError {
    pub const fn firmware_code(self) -> u32 { match self { Self::Resource => 2, Self::Start => 3, Self::Update => 4, Self::Phase => 5, Self::Stop => 6 } }
}

pub struct TransferPipeline {
    dma: AudioDma,
    lease: Option<TransferLease>,
    active: bool,
    phase: Phase,
    bank: ControllerBank,
    direction: TransferDirection,
    packing: SamplePacking,
    padding: PcmAddress,
    padding_length: PcmByteLength,
    buffer_ready: bool,
    completed: bool,
    error: Option<PipelineError>,
}
impl TransferPipeline {
    pub const fn new() -> Self {
        Self { dma: AudioDma::new(), lease: None, active: false, phase: Phase::Closed, bank: ControllerBank::First, direction: TransferDirection::Receive,
            packing: SamplePacking::Packed32, padding: PcmAddress::from_abi(0x7d0c74), padding_length: PcmByteLength::from_abi(1920),
            buffer_ready: false, completed: false, error: None }
    }
    pub fn acquire(&mut self, bank: ControllerBank, direction: TransferDirection, packing: SamplePacking, padding: PcmAddress, padding_length: PcmByteLength) -> Result<TransferLease, DmaError> {
        if self.phase != Phase::Closed { self.error = Some(PipelineError::Resource); return Err(DmaError::Busy); }
        let lease = self.dma.acquire()?;
        self.lease = Some(lease);
        self.phase = Phase::Acquired;
        self.active = false;
        self.bank = bank;
        self.direction = direction;
        self.packing = packing;
        self.padding = padding;
        self.padding_length = padding_length;
        self.buffer_ready = false;
        self.completed = false;
        self.error = None;
        Ok(lease)
    }
    fn program_for(&self, position: PcmAddress, length: PcmByteLength) -> TransferProgram {
        match self.direction {
            TransferDirection::Receive => TransferProgram::receive(self.bank, position, length, self.packing),
            TransferDirection::Transmit => TransferProgram::transmit(self.bank, position, length, self.packing, self.padding, self.padding_length),
        }
    }
    pub fn submit(&mut self, registers: &mut impl AudioRegisters, lease: TransferLease, position: PcmAddress, length: PcmByteLength, continuation: bool) -> Result<(), DmaError> {
        if self.lease != Some(lease) { self.error = Some(PipelineError::Resource); return Err(DmaError::InvalidHandle); }
        let program = self.program_for(position, length);
        if !continuation && self.phase == Phase::Acquired {
            if let Err(error) = self.dma.program(registers, lease, program, false).and_then(|()| self.dma.start(registers, lease)) {
                self.phase = Phase::Failed;
                self.error = Some(PipelineError::Start);
                return Err(error);
            }
            self.active = true;
            while !self.dma.pending(registers, lease)? {}
            self.dma.program(registers, lease, program, true)?;
            if let Err(error) = dma::configure_controller(registers, self.bank, self.direction) {
                self.phase = Phase::Failed;
                self.error = Some(PipelineError::Start);
                return Err(error);
            }
            self.phase = Phase::Primed;
            if self.direction == TransferDirection::Receive { self.buffer_ready = true; }
            return Ok(());
        }
        if self.phase == Phase::Primed && (continuation || self.direction == TransferDirection::Transmit) {
            if continuation {
                if let Err(error) = self.dma.program(registers, lease, program, true) {
                    self.error = Some(PipelineError::Update);
                    self.phase = Phase::Failed;
                    return Err(error);
                }
            }
            self.phase = Phase::Streaming;
            return Ok(());
        }
        self.phase = Phase::Failed;
        self.error = Some(PipelineError::Phase);
        Err(DmaError::Verification)
    }
    fn idle_program(&self) -> TransferProgram {
        match self.direction {
            TransferDirection::Receive => self.program_for(PcmAddress::from_abi(0x93adf8), PcmByteLength::from_abi(2048)),
            TransferDirection::Transmit => self.program_for(self.padding, self.padding_length),
        }
    }
    fn terminate(&mut self, registers: &mut impl AudioRegisters, lease: TransferLease) -> Result<(), DmaError> {
        let result = if self.active {
            loop {
                match self.dma.finish(registers, lease) { Ok(true) => break Ok(()), Ok(false) => {}, Err(error) => break Err(error) }
            }
        } else { Ok(()) };
        dma::stop_controller(registers, self.bank);
        if result.is_err() { self.error = Some(PipelineError::Stop); }
        let release = self.dma.release(lease);
        if release.is_err() && self.error.is_none() { self.error = Some(PipelineError::Resource); }
        self.phase = Phase::Closed;
        self.lease = None;
        self.active = false;
        self.completed = true;
        result.and(release)
    }
    pub fn physical_completion(&mut self, registers: &mut impl AudioRegisters, lease: TransferLease) -> Result<(), DmaError> {
        if self.lease != Some(lease) || self.phase == Phase::Closed { return Ok(()); }
        match self.phase {
            Phase::Primed | Phase::Streaming => {
                let program = self.idle_program();
                self.dma.program(registers, lease, program, true)?;
                if self.phase == Phase::Streaming { self.phase = Phase::Primed; self.buffer_ready = true; }
            },
            Phase::ReceiveCancel => { self.phase = Phase::Drain; self.buffer_ready = true; },
            Phase::Drain => self.phase = Phase::Stopping,
            Phase::Stopping => return self.terminate(registers, lease),
            Phase::Closed | Phase::Acquired | Phase::Failed => {},
        }
        Ok(())
    }
    pub fn cancel(&mut self, registers: &mut impl AudioRegisters, lease: TransferLease) -> Result<(), DmaError> {
        if self.lease != Some(lease) { self.error = Some(PipelineError::Resource); return Err(DmaError::InvalidHandle); }
        match self.phase {
            Phase::Acquired => {
                let result = self.dma.release(lease);
                if result.is_err() { self.error = Some(PipelineError::Resource); }
                self.phase = Phase::Closed;
                self.lease = None;
                self.completed = true;
                result
            },
            Phase::Primed => {
                if self.direction == TransferDirection::Transmit {
                    let program = self.program_for(self.padding, PcmByteLength::from_abi(8));
                    self.dma.program(registers, lease, program, true)?;
                    self.phase = Phase::Stopping;
                } else { self.phase = Phase::Drain; }
                Ok(())
            },
            Phase::Streaming => {
                if self.direction == TransferDirection::Receive { self.phase = Phase::ReceiveCancel; }
                else {
                    let program = self.idle_program();
                    if let Err(error) = self.dma.program(registers, lease, program, true) {
                        self.error = Some(PipelineError::Update);
                        let _ = self.terminate(registers, lease);
                        return Err(error);
                    }
                    self.phase = Phase::Drain;
                }
                Ok(())
            },
            Phase::Failed => self.terminate(registers, lease),
            Phase::ReceiveCancel if self.direction == TransferDirection::Receive => self.terminate(registers, lease),
            _ => { if self.error.is_none() { self.error = Some(PipelineError::Phase); } Err(DmaError::Verification) },
        }
    }
    pub fn take_buffer_ready(&mut self) -> bool { core::mem::replace(&mut self.buffer_ready, false) }
    pub const fn completed(&self) -> bool { self.completed }
    pub const fn error(&self) -> Option<PipelineError> { self.error }
}
impl Default for TransferPipeline { fn default() -> Self { Self::new() } }
