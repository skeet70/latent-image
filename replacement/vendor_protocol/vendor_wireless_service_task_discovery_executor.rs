use super::packet::{CommandPacket, OperationCode, TransactionId};
use super::super::super::vendor_wireless_network::transport::socket_task::SocketTaskError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionIdentifier(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResponseCode(pub u16);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PtpPhase { Command, Data, Cancel }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OperationRequest<'a> {
    pub phase: PtpPhase,
    pub command: CommandPacket,
    pub data: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OperationReply {
    pub response: ResponseCode,
    pub parameters: [u32; 5],
    pub parameter_count: u8,
}

impl OperationReply {
    pub const fn new(response: ResponseCode) -> Self {
        Self { response, parameters: [0; 5], parameter_count: 0 }
    }
}

pub struct PtpState {
    pub session_id: Option<SessionIdentifier>,
    pub transaction: TransactionId,
    pub response: ResponseCode,
}

impl PtpState {
    pub const fn new() -> Self {
        Self { session_id: None, transaction: TransactionId(0), response: ResponseCode(0x2001) }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PtpError {
    Transport(SocketTaskError),
    OutputFull,
    InvalidLength,
    InvalidState,
    NotConfigured,
}

pub fn validate_unused(command: CommandPacket, first_unused: usize) -> bool {
    command.parameters.get(first_unused..).is_some_and(|parameters| parameters.iter().all(|parameter| *parameter == 0))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputKind { Data, Response, Control }

pub struct PtpOutput {
    bytes: [u8; 2048],
    start: usize,
    end: usize,
    data_remaining: u32,
    kind: OutputKind,
}

impl PtpOutput {
    pub const fn new() -> Self { Self { bytes: [0; 2048], start: 0, end: 0, data_remaining: 0, kind: OutputKind::Control } }
    pub fn queued_bytes(&self) -> &[u8] { &self.bytes[self.start..self.end] }
    pub const fn is_empty(&self) -> bool { self.start == self.end }
    pub const fn data_remaining(&self) -> u32 { self.data_remaining }

    pub const fn kind(&self) -> OutputKind { self.kind }

    pub fn send_frame(&mut self, bytes: &[u8]) -> Result<(), PtpError> {
        if !self.is_empty() || self.data_remaining != 0 { return Err(PtpError::OutputFull); }
        if bytes.len() > self.bytes.len() { return Err(PtpError::InvalidLength); }
        self.bytes[..bytes.len()].copy_from_slice(bytes);
        self.start = 0;
        self.end = bytes.len();
        self.kind = OutputKind::Control;
        Ok(())
    }
    pub fn acknowledge(&mut self, count: usize) -> Result<(), PtpError> {
        if count > self.end - self.start { return Err(PtpError::InvalidLength); }
        self.start += count;
        if self.start == self.end { self.start = 0; self.end = 0; }
        Ok(())
    }

    pub fn discard(&mut self) { self.start = 0; self.end = 0; self.data_remaining = 0; }

    fn header(&mut self, length: u32, kind: u16, operation: u16, transaction: TransactionId) -> Result<(), PtpError> {
        if !self.is_empty() || self.data_remaining != 0 { return Err(PtpError::OutputFull); }
        self.bytes[..4].copy_from_slice(&length.to_le_bytes());
        self.bytes[4..6].copy_from_slice(&kind.to_le_bytes());
        self.bytes[6..8].copy_from_slice(&operation.to_le_bytes());
        self.bytes[8..12].copy_from_slice(&transaction.0.to_le_bytes());
        self.end = 12;
        Ok(())
    }

    pub fn begin_data(&mut self, operation: OperationCode, transaction: TransactionId, length: u32) -> Result<(), PtpError> {
        let frame_length = length.checked_add(12).ok_or(PtpError::InvalidLength)?;
        self.header(frame_length, 2, operation.0, transaction)?;
        self.data_remaining = length;
        self.kind = OutputKind::Data;
        Ok(())
    }

    pub fn write_data(&mut self, bytes: &[u8]) -> Result<usize, PtpError> {
        if bytes.len() > self.data_remaining as usize { return Err(PtpError::InvalidLength); }
        let count = bytes.len().min(self.bytes.len() - self.end);
        self.bytes[self.end..self.end + count].copy_from_slice(&bytes[..count]);
        self.end += count;
        self.data_remaining -= count as u32;
        Ok(count)
    }

    pub fn send_data(&mut self, operation: OperationCode, transaction: TransactionId, bytes: &[u8]) -> Result<(), PtpError> {
        if bytes.len() > self.bytes.len() - 12 { return Err(PtpError::OutputFull); }
        self.begin_data(operation, transaction, bytes.len() as u32)?;
        self.write_data(bytes)?;
        Ok(())
    }

    pub fn send_response(&mut self, reply: OperationReply, transaction: TransactionId) -> Result<(), PtpError> {
        if reply.parameter_count > 5 { return Err(PtpError::InvalidLength); }
        let length = 12 + u32::from(reply.parameter_count) * 4;
        self.header(length, 3, reply.response.0, transaction)?;
        for (chunk, parameter) in self.bytes[12..length as usize].chunks_exact_mut(4).zip(reply.parameters) {
            chunk.copy_from_slice(&parameter.to_le_bytes());
        }
        self.end = length as usize;
        self.kind = OutputKind::Response;
        Ok(())
    }
}
