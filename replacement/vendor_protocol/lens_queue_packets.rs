//! Queue 98 packet layouts from X-M1 1.50 image 4.
//! Source pin: 55436fc85c0560dd252b0410af6f2427fe198cd3.

use xm1_power_services::shutdown::RequestId;
use xm1_lens_transport::frame::{CommandWord, PageId, PayloadLength};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallbackId(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceAddress(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ControlOperation(RequestId);
impl ControlOperation {
    pub const fn parse(value: RequestId) -> Option<Self> {
        match value.0 { 0x7202..=0x7250 | 0x7256..=0x725c => Some(Self(value)), _ => None }
    }
    pub const fn request_id(self) -> RequestId { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PacketError { Operation, Length, CommandCount, PageLength, Boolean }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ControlPacket {
    pub operation: ControlOperation,
    pub callback: CallbackId,
    words: [CommandWord; 10],
    count: u8,
}
impl ControlPacket {
    pub fn new(operation: ControlOperation, callback: CallbackId, commands: &[CommandWord]) -> Result<Self, PacketError> {
        if commands.len() > 10 { return Err(PacketError::CommandCount); }
        let mut words = [CommandWord::new(0); 10];
        words[..commands.len()].copy_from_slice(commands);
        Ok(Self { operation, callback, words, count: commands.len() as u8 })
    }
    pub fn words(&self) -> &[CommandWord] { &self.words[..usize::from(self.count)] }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadRequest { pub page: PageId, pub length: PayloadLength, pub checksum: bool, pub marker: bool, pub callback: CallbackId }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WriteRequest { pub page: PageId, pub source: SourceAddress, pub length: PayloadLength, pub checksum: bool, pub callback: CallbackId }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueRequest {
    Initialize { callback: CallbackId, observer: CallbackId },
    Stop { callback: CallbackId },
    Control(ControlPacket),
    Read(ReadRequest),
    Write(WriteRequest),
}
fn word(bytes: &[u8], at: usize) -> u32 { u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) }
fn half(bytes: &[u8]) -> u16 { u16::from_le_bytes([bytes[0], bytes[1]]) }
fn boolean(value: u32) -> Result<bool, PacketError> { match value { 0 => Ok(false), 1 => Ok(true), _ => Err(PacketError::Boolean) } }
fn length(exponent: u32) -> Result<PayloadLength, PacketError> {
    let bytes = 1usize.checked_shl(exponent).ok_or(PacketError::PageLength)?;
    PayloadLength::new(bytes).map_err(|_| PacketError::PageLength)
}
impl QueueRequest {
    pub fn parse(operation: RequestId, bytes: &[u8]) -> Result<Self, PacketError> {
        match operation.0 {
            0x7200 | 0x7201 => {
                if bytes.len() != 8 { return Err(PacketError::Length); }
                let callback = CallbackId(word(bytes, 0));
                Ok(if operation.0 == 0x7200 { Self::Initialize { callback, observer: CallbackId(word(bytes, 4)) } } else { Self::Stop { callback } })
            }
            0x7252 | 0x7254 => {
                if bytes.len() != 0x14 { return Err(PacketError::Length); }
                let page = PageId::new(half(bytes));
                let callback = CallbackId(word(bytes, 16));
                if operation.0 == 0x7254 {
                    Ok(Self::Read(ReadRequest { page, length: length(word(bytes, 4))?, checksum: boolean(word(bytes, 8))?, marker: boolean(word(bytes, 12))?, callback }))
                } else {
                    Ok(Self::Write(WriteRequest { page, source: SourceAddress(word(bytes, 4)), length: length(word(bytes, 8))?, checksum: boolean(word(bytes, 12))?, callback }))
                }
            }
            _ => {
                let operation = ControlOperation::parse(operation).ok_or(PacketError::Operation)?;
                if bytes.len() != 0x30 { return Err(PacketError::Length); }
                let count = bytes[0x2c];
                if count > 10 { return Err(PacketError::CommandCount); }
                let mut words = [CommandWord::new(0); 10];
                for (index, slot) in words.iter_mut().take(usize::from(count)).enumerate() { *slot = CommandWord::new(word(bytes, 4 + index * 4)); }
                Ok(Self::Control(ControlPacket { operation, callback: CallbackId(word(bytes, 0)), words, count }))
            }
        }
    }
    pub const fn operation(self) -> RequestId {
        match self { Self::Initialize { .. } => RequestId(0x7200), Self::Stop { .. } => RequestId(0x7201), Self::Control(packet) => packet.operation.request_id(), Self::Read(_) => RequestId(0x7254), Self::Write(_) => RequestId(0x7252) }
    }
}
