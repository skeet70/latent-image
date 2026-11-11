use super::ptp_types::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtpReceiveWindow { pub offset: usize, pub bytes: usize }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PtpReceiveProgress { Header, HeaderContinuation, PayloadContinuation }

pub struct PtpReceiver {
    progress: PtpReceiveProgress,
    packet_size: u16,
    total: usize,
    remaining: usize,
    first_request: usize,
    final_window: bool,
}

impl PtpReceiver {
    pub const fn new(packet_size: u16) -> Self {
        Self { progress: PtpReceiveProgress::Header, packet_size, total: 0,
            remaining: 0, first_request: 0, final_window: true }
    }

    pub fn window(&mut self, requested: usize, capacity: usize) -> Result<PtpReceiveWindow, PtpPacketError> {
        match self.progress {
            PtpReceiveProgress::Header => {
                if requested > capacity { return Err(PtpPacketError::BufferTooSmall); }
                self.first_request = requested;
                Ok(PtpReceiveWindow { offset: 0, bytes: requested })
            }
            PtpReceiveProgress::HeaderContinuation => {
                let offset = usize::from(self.packet_size);
                let end = self.total.min(capacity);
                let bytes = end.checked_sub(offset).ok_or(PtpPacketError::BufferTooSmall)?;
                if self.total > capacity {
                    self.final_window = false;
                    self.remaining = self.total - capacity;
                }
                Ok(PtpReceiveWindow { offset, bytes })
            }
            PtpReceiveProgress::PayloadContinuation => {
                let bytes = self.remaining.min(capacity);
                if self.remaining <= capacity { self.final_window = true; }
                else { self.remaining -= capacity; }
                Ok(PtpReceiveWindow { offset: 0, bytes })
            }
        }
    }

    pub fn complete(&mut self, buffer: &[u8]) -> Result<bool, PtpPacketError> {
        match self.progress {
            PtpReceiveProgress::Header => {
                if buffer.len() < 12 { return Err(PtpPacketError::Truncated); }
                let total = u32::from_le_bytes(buffer[..4].try_into().unwrap()) as usize;
                if total < 12 { return Err(PtpPacketError::InvalidLength); }
                if self.first_request >= usize::from(self.packet_size) && total != usize::from(self.packet_size) {
                    self.total = total;
                    self.progress = PtpReceiveProgress::HeaderContinuation;
                    return Ok(false);
                }
                Ok(true)
            }
            PtpReceiveProgress::HeaderContinuation => {
                self.progress = if self.final_window { PtpReceiveProgress::Header }
                    else { PtpReceiveProgress::PayloadContinuation };
                Ok(true)
            }
            PtpReceiveProgress::PayloadContinuation => {
                if self.final_window { self.progress = PtpReceiveProgress::Header; }
                Ok(true)
            }
        }
    }

    pub fn reset(&mut self) {
        self.progress = PtpReceiveProgress::Header;
        self.total = 0;
        self.remaining = 0;
        self.first_request = 0;
        self.final_window = true;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtpTransferId(u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PtpTransferPhase { Ready, Pending, Complete, Terminated }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PtpPollResult { Idle, Pending, Complete }
impl PtpPollResult {
    pub const fn status(self) -> u32 {
        match self { Self::Idle => 0, Self::Pending => 0x100, Self::Complete => 0x220 }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtpTransfer {
    pub id: PtpTransferId,
    pub bytes: usize,
    pub zero_packet: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtpControlRequest {
    pub request_type: u8,
    pub request: u8,
    pub value: u16,
    pub index: u16,
    pub length: u16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PtpControlResult { Rejected, Complete, ReceiveCancel, Canceled }
impl PtpControlResult {
    pub const fn status(self) -> u32 {
        match self { Self::Rejected => 2, Self::Complete => 0, Self::ReceiveCancel => 0x1000000, Self::Canceled => 0x660 }
    }
}

pub struct PtpRuntime {
    packet_size: u16,
    transaction: PtpTransactionId,
    command: Option<PtpCommand>,
    generation: u64,
    active_transfer: Option<PtpTransfer>,
    reply_phase: PtpTransferPhase,
    data_phase: PtpTransferPhase,
    completion_initialized: bool,
    completion_status: PtpResponseCode,
    control_type: u8,
    session: PtpSessionId,
    event: Option<[u8; 16]>,
    battery: BatteryLevel,
}

impl Default for PtpRuntime {
    fn default() -> Self { Self::new() }
}

impl PtpRuntime {
    pub const fn new() -> Self {
        Self { packet_size: 0, transaction: PtpTransactionId(0), command: None,
            generation: 0, active_transfer: None, reply_phase: PtpTransferPhase::Ready,
            data_phase: PtpTransferPhase::Pending, completion_initialized: false,
            completion_status: PtpResponseCode::OK, control_type: 0, battery: BatteryLevel::Empty,
            session: PtpSessionId(0), event: None }
    }

    pub fn initialize(&mut self, packet_size: u16) {
        self.packet_size = packet_size;
        self.transaction = PtpTransactionId(0);
        self.control_type = 0;
        self.reset_reply();
    }

    fn reset_reply(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.active_transfer = None;
        self.command = None;
        self.reply_phase = PtpTransferPhase::Ready;
        self.data_phase = PtpTransferPhase::Pending;
    }

    pub fn reset(&mut self) {
        self.reset_reply();
        self.completion_initialized = true;
        self.completion_status = PtpResponseCode::OK;
        self.session = PtpSessionId(0);
        self.event = None;
    }

    pub const fn battery(&self) -> BatteryLevel { self.battery }
    pub fn set_battery(&mut self, value: BatteryLevel) { self.battery = value; }
    pub const fn completion_status(&self) -> PtpResponseCode { self.completion_status }
    pub const fn command(&self) -> Option<PtpCommand> { self.command }

    pub fn set_reply_phase(&mut self, phase: PtpTransferPhase) { self.reply_phase = phase; }
    pub fn set_data_phase(&mut self, phase: PtpTransferPhase) { self.data_phase = phase; }

    pub fn poll_data(&mut self) -> PtpPollResult {
        match self.data_phase {
            PtpTransferPhase::Pending => PtpPollResult::Pending,
            PtpTransferPhase::Complete => {
                self.reply_phase = PtpTransferPhase::Ready;
                self.data_phase = PtpTransferPhase::Ready;
                PtpPollResult::Complete
            }
            PtpTransferPhase::Ready => PtpPollResult::Idle,
            PtpTransferPhase::Terminated => PtpPollResult::Complete,
        }
    }

    pub fn notify(&mut self, _value: u32) {
        if self.session.0 == 0 { return; }
        let mut event = [0; 16];
        encode_header(&mut event, PtpContainerKind::Event, 0x4006,
            PtpTransactionId(u32::MAX), 4).unwrap();
        self.event = Some(event);
    }

    pub fn take_event(&mut self) -> Option<[u8; 16]> { self.event.take() }

    pub fn set_session(&mut self, session: Option<PtpSessionId>) {
        self.session = session.unwrap_or(PtpSessionId(0));
    }
    pub const fn session(&self) -> Option<PtpSessionId> {
        if self.session.0 == 0 { None } else { Some(self.session) }
    }

    pub fn receive<'a>(&mut self, bytes: &'a [u8]) -> Result<PtpContainer<'a>, PtpPacketError> {
        let container = PtpContainer::decode(bytes)?;

        match container.kind {
            PtpContainerKind::Command => {
                let command = container.command()?;
                self.transaction = command.transaction;
                self.command = Some(command);
                self.completion_status = PtpResponseCode::DEVICE_BUSY;
                self.reply_phase = PtpTransferPhase::Ready;
            }
            PtpContainerKind::Data if container.transaction == self.transaction => {}
            PtpContainerKind::Data => return Err(PtpPacketError::StaleTransaction),
            _ => {
                self.completion_status = PtpResponseCode::GENERAL_ERROR;
                self.reply_phase = PtpTransferPhase::Complete;
                self.data_phase = PtpTransferPhase::Complete;
                return Err(PtpPacketError::InvalidKind);
            }
        }
        Ok(container)
    }

    pub fn prepare_result(&mut self, result: PtpApplicationResult, output: &mut [u8]) -> Result<Option<PtpTransfer>, PtpPacketError> {
        let (bytes, zero_packet) = match result {
            PtpApplicationResult::Pending => {
                self.reply_phase = PtpTransferPhase::Pending;
                return Ok(None);
            }
            PtpApplicationResult::Response(response) => {
                self.completion_status = response.code;
                (response.encode(output)?, false)
            }
            PtpApplicationResult::Data { operation, transaction, bytes } => {
                if bytes.checked_add(12).is_none_or(|length| length > output.len()) {
                    return Err(PtpPacketError::BufferTooSmall);
                }
                encode_header(output, PtpContainerKind::Data, operation.0, transaction,
                    u32::try_from(bytes).map_err(|_| PtpPacketError::InvalidLength)?)?;
                let length = bytes + 12;
                let exact = self.packet_size != 0 && length % usize::from(self.packet_size) == 0;
                self.completion_initialized = !exact;
                (length, exact)
            }
        };
        self.generation = self.generation.wrapping_add(1);
        let transfer = PtpTransfer { id: PtpTransferId(self.generation), bytes, zero_packet };
        self.active_transfer = Some(transfer);
        self.data_phase = PtpTransferPhase::Ready;
        Ok(Some(transfer))
    }

    pub fn complete_transfer(&mut self, id: PtpTransferId) -> bool {
        if self.active_transfer.is_none_or(|transfer| transfer.id != id) { return false; }
        self.active_transfer = None;
        self.data_phase = PtpTransferPhase::Pending;
        true
    }

    pub fn poll_reply(&mut self) -> PtpPollResult {
        self.data_phase = PtpTransferPhase::Pending;
        match self.reply_phase {
            PtpTransferPhase::Ready => PtpPollResult::Idle,
            PtpTransferPhase::Pending => PtpPollResult::Pending,
            PtpTransferPhase::Complete => {
                self.reply_phase = PtpTransferPhase::Ready;
                self.data_phase = PtpTransferPhase::Ready;
                PtpPollResult::Complete
            }
            PtpTransferPhase::Terminated => PtpPollResult::Complete,
        }
    }

    pub fn begin_cancel(&mut self, request: PtpControlRequest) -> PtpControlResult {
        if request.request_type != 0x21 || request.value != 0 || request.index != 0 || request.length != 6 {
            return PtpControlResult::Rejected;
        }
        self.control_type = request.request;
        PtpControlResult::ReceiveCancel
    }

    pub fn finish_cancel(&mut self, bytes: &[u8; 6]) -> PtpControlResult {
        let transaction = PtpTransactionId(u32::from_le_bytes(bytes[2..6].try_into().unwrap()));
        if transaction != self.transaction { return PtpControlResult::Complete; }
        self.reset_reply();
        self.completion_initialized = true;
        self.completion_status = PtpResponseCode::OK;
        PtpControlResult::Canceled
    }

    pub fn status_packet(&self, input_endpoint: u8, input_active: bool, output_endpoint: u8,
        output_active: bool, requested: usize, output: &mut [u8; 12]) -> usize {
        *output = [0; 12];
        output[2..4].copy_from_slice(&self.completion_status.0.to_le_bytes());
        let length = match (input_active, output_active) {
            (false, false) => 4,
            (true, true) => { output[4] = output_endpoint; output[8] = input_endpoint; 12 }
            (true, false) => { output[4] = input_endpoint; 8 }
            (false, true) => { output[4] = output_endpoint; 8 }
        };
        output[0] = length as u8;
        if length == 4 { length } else { length.min(requested) }
    }
}
