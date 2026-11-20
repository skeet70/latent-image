use core::num::NonZeroU32;
use crate::nand::dma_read::{Deadline, Instant};

pub const COMMAND_COUNT: usize = 9;
pub const ENVELOPE_BYTES: usize = 24;

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandKind {
    Image5 = 0,
    FixedBlock = 1,
    Selected = 2,
    Plain = 3,
    Region = 4,
    Section3 = 5,
    Initialize = 6,
    RegisterCompletion = 7,
    Auxiliary = 8,
}
impl CommandKind {
    pub const fn parse(word: u32) -> Option<Self> {
        match word {
            0 => Some(Self::Image5), 1 => Some(Self::FixedBlock),
            2 => Some(Self::Selected), 3 => Some(Self::Plain),
            4 => Some(Self::Region), 5 => Some(Self::Section3),
            6 => Some(Self::Initialize), 7 => Some(Self::RegisterCompletion),
            8 => Some(Self::Auxiliary), _ => None,
        }
    }
    pub const fn index(self) -> usize { self as usize }
    pub const fn operation(self) -> u16 { 0x4c00 + self as u16 }
    pub const fn from_operation(operation: u16) -> Option<Self> {
        match operation.checked_sub(0x4c00) { Some(word) => Self::parse(word as u32), None => None }
    }
    pub const fn owns_executable(self) -> bool { matches!(self, Self::Image5 | Self::Plain) }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestId(NonZeroU32);
impl RequestId {
    pub const fn new(word: u32) -> Option<Self> {
        match NonZeroU32::new(word) { Some(word) => Some(Self(word)), None => None }
    }
    pub const fn word(self) -> u32 { self.0.get() }
    pub fn next(self) -> Option<Self> { self.word().checked_add(1).and_then(Self::new) }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallbackArgument(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallbackAddress(NonZeroU32);
impl CallbackAddress {
    pub const fn new(word: u32) -> Option<Self> {
        if word & 3 != 0 { return None; }
        match NonZeroU32::new(word) { Some(word) => Some(Self(word)), None => None }
    }
    pub const fn word(self) -> u32 { self.0.get() }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Callback { None, Address(CallbackAddress) }
impl Callback {
    pub const fn parse(word: u32) -> Option<Self> {
        if word == 0 { Some(Self::None) }
        else { match CallbackAddress::new(word) { Some(address) => Some(Self::Address(address)), None => None } }
    }
    pub const fn word(self) -> u32 { match self { Self::None => 0, Self::Address(address) => address.word() } }
}


#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolError {
    InvalidCommand, InvalidRequest, InvalidCallback, EnvelopeSize, EnvelopeVersion,
    NonFiniteDeadline, Expired, NotConfigured, Outstanding, RetainedOperation,
    Kernel(i32), Producer(i32), MissingCompletion,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Request {
    pub id: RequestId,
    pub kind: CommandKind,
    pub deadline: Deadline,
    pub callback: Callback,
    pub argument: CallbackArgument,
}
impl Request {
    pub fn encode(self) -> [u8; ENVELOPE_BYTES] {
        let mut bytes = [0; ENVELOPE_BYTES];
        let words = [1, self.id.word(), self.deadline.instant().count() as u32,
            (self.deadline.instant().count() >> 32) as u32, self.callback.word(), self.argument.0];
        for (chunk, word) in bytes.chunks_exact_mut(4).zip(words) { chunk.copy_from_slice(&word.to_le_bytes()); }
        bytes
    }
    pub fn decode(operation: u16, bytes: &[u8]) -> Result<Self, ProtocolError> {
        let kind = CommandKind::from_operation(operation).ok_or(ProtocolError::InvalidCommand)?;
        let bytes: &[u8; ENVELOPE_BYTES] = bytes.try_into().map_err(|_| ProtocolError::EnvelopeSize)?;
        let word = |offset: usize| u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]]);
        if word(0) != 1 { return Err(ProtocolError::EnvelopeVersion); }
        let id = RequestId::new(word(4)).ok_or(ProtocolError::InvalidRequest)?;
        let end = u64::from(word(8)) | (u64::from(word(12)) << 32);
        let deadline = Deadline::new(Instant::new(end)).ok_or(ProtocolError::NonFiniteDeadline)?;
        let callback = Callback::parse(word(16)).ok_or(ProtocolError::InvalidCallback)?;
        Ok(Self { id, kind, deadline, callback, argument: CallbackArgument(word(20)) })
    }
    pub fn live(self, now: Instant) -> bool { now.count() < self.deadline.instant().count() }
}
