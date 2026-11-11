#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PtpOperationCode(pub u16);
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PtpTransactionId(pub u32);
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PtpSessionId(pub u32);
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PtpObjectHandle(pub u32);
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PtpPropertyCode(pub u16);
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PtpStorageId(pub u32);
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PtpResponseCode(pub u16);

impl PtpResponseCode {
    pub const OK: Self = Self(0x2001);
    pub const GENERAL_ERROR: Self = Self(0x2002);
    pub const SESSION_NOT_OPEN: Self = Self(0x2003);
    pub const INVALID_TRANSACTION_ID: Self = Self(0x2004);
    pub const OPERATION_NOT_SUPPORTED: Self = Self(0x2005);
    pub const INVALID_STORAGE_ID: Self = Self(0x2008);
    pub const INVALID_OBJECT_HANDLE: Self = Self(0x2009);
    pub const DEVICE_PROP_NOT_SUPPORTED: Self = Self(0x200a);
    pub const INVALID_PARAMETER: Self = Self(0x201d);
    pub const SESSION_ALREADY_OPEN: Self = Self(0x201e);
    pub const DEVICE_BUSY: Self = Self(0x2019);
}

/// Stock byte 0x0146aefc, PTP property 0x5001.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum BatteryLevel { Empty = 0, Low = 1, Medium = 2, Full = 3 }
impl BatteryLevel {
    /// Stock 0x00d82178 over the flags word 0x006a0bb0 returns.
    pub const fn from_status_flags(flags: u32) -> Self {
        if flags & 1 != 0 { Self::Full }
        else if flags & 12 != 0 { Self::Low }
        else if flags & 16 != 0 { Self::Medium }
        else { Self::Full }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtpCommand {
    pub operation: PtpOperationCode,
    pub transaction: PtpTransactionId,
    pub parameters: [u32; 5],
    pub parameter_count: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtpResponse {
    pub code: PtpResponseCode,
    pub transaction: PtpTransactionId,
    pub parameters: [u32; 5],
    pub parameter_count: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PtpDispatchPhase { Command, Data, Poll }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PtpApplicationResult {
    Response(PtpResponse),
    Data { operation: PtpOperationCode, transaction: PtpTransactionId, bytes: usize },
    Pending,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum PtpContainerKind { Command = 1, Data = 2, Response = 3, Event = 4 }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PtpPacketError { Truncated, InvalidKind, InvalidLength, TooManyParameters, StaleTransaction, BufferTooSmall }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtpContainer<'a> {
    pub kind: PtpContainerKind,
    pub code: u16,
    pub transaction: PtpTransactionId,
    pub payload: &'a [u8],
}

impl<'a> PtpContainer<'a> {
    pub fn decode(bytes: &'a [u8]) -> Result<Self, PtpPacketError> {
        if bytes.len() < 12 { return Err(PtpPacketError::Truncated); }
        let length = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
        if length < 12 { return Err(PtpPacketError::InvalidLength); }
        if length > bytes.len() { return Err(PtpPacketError::Truncated); }
        let kind = match u16::from_le_bytes([bytes[4], bytes[5]]) {
            1 => PtpContainerKind::Command, 2 => PtpContainerKind::Data,
            3 => PtpContainerKind::Response, 4 => PtpContainerKind::Event,
            _ => return Err(PtpPacketError::InvalidKind),
        };
        Ok(Self { kind, code: u16::from_le_bytes([bytes[6], bytes[7]]),
            transaction: PtpTransactionId(u32::from_le_bytes(bytes[8..12].try_into().unwrap())),
            payload: &bytes[12..length] })
    }

    pub fn command(self) -> Result<PtpCommand, PtpPacketError> {
        if self.kind != PtpContainerKind::Command { return Err(PtpPacketError::InvalidKind); }
        if self.payload.len() % 4 != 0 { return Err(PtpPacketError::InvalidLength); }
        let count = self.payload.len() / 4;
        if count > 5 { return Err(PtpPacketError::TooManyParameters); }
        let mut parameters = [0; 5];
        for (destination, word) in parameters.iter_mut().zip(self.payload.chunks_exact(4)) {
            *destination = u32::from_le_bytes(word.try_into().unwrap());
        }
        Ok(PtpCommand { operation: PtpOperationCode(self.code), transaction: self.transaction,
            parameters, parameter_count: count as u8 })
    }
}

pub fn encode_header(destination: &mut [u8], kind: PtpContainerKind, code: u16,
    transaction: PtpTransactionId, payload_bytes: u32) -> Result<(), PtpPacketError> {
    if destination.len() < 12 { return Err(PtpPacketError::BufferTooSmall); }
    let length = payload_bytes.checked_add(12).ok_or(PtpPacketError::InvalidLength)?;
    destination[..4].copy_from_slice(&length.to_le_bytes());
    destination[4..6].copy_from_slice(&(kind as u16).to_le_bytes());
    destination[6..8].copy_from_slice(&code.to_le_bytes());
    destination[8..12].copy_from_slice(&transaction.0.to_le_bytes());
    Ok(())
}

impl PtpResponse {
    pub fn encode(&self, destination: &mut [u8]) -> Result<usize, PtpPacketError> {
        let count = self.parameter_count as usize;
        if count > 5 { return Err(PtpPacketError::TooManyParameters); }
        let length = 12 + count * 4;
        if destination.len() < length { return Err(PtpPacketError::BufferTooSmall); }
        encode_header(destination, PtpContainerKind::Response, self.code.0, self.transaction, (count * 4) as u32)?;
        for (word, parameter) in destination[12..length].chunks_exact_mut(4).zip(self.parameters) {
            word.copy_from_slice(&parameter.to_le_bytes());
        }
        Ok(length)
    }
}
