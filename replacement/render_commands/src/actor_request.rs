use super::{commands::DrawList, context::OutputMode};

pub const TASK_ID: u8 = 73;
pub const QUEUE_CAPACITY: usize = 48;
pub const MESSAGE_BYTES: usize = 56;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation { Open, Close, Render }
impl Operation {
    pub const fn number(self) -> u16 { match self { Self::Open => 0x5900, Self::Close => 0x5901, Self::Render => 0x5902 } }
    pub const fn parse(value: u16) -> Result<Self, RequestError> {
        match value { 0x5900 => Ok(Self::Open), 0x5901 => Ok(Self::Close), 0x5902 => Ok(Self::Render), _ => Err(RequestError::Operation) }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestId(pub core::num::NonZeroU64);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WaiterId(pub core::num::NonZeroU32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallbackAddress(core::num::NonZeroU32);
impl CallbackAddress {
    pub const fn new(value: u32) -> Option<Self> { match core::num::NonZeroU32::new(value) { Some(value) => Some(Self(value)), None => None } }
    pub const fn value(self) -> u32 { self.0.get() }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InitializationWord(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallbackBinding { pub address: CallbackAddress, pub waiter: WaiterId }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestError { Operation, PayloadLength, UnresolvedCallback(CallbackAddress), AmbiguousCallback(CallbackAddress), DescriptorMismatch }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueRequest {
    Open { initialization: InitializationWord, waiter: Option<WaiterId> },
    Close { waiter: Option<WaiterId> },
    Render { waiters: [Option<WaiterId>; 6] },
}
impl QueueRequest {
    pub const fn operation(self) -> Operation { match self { Self::Open { .. } => Operation::Open, Self::Close { .. } => Operation::Close, Self::Render { .. } => Operation::Render } }
    // 6d75c4/6d7620 select one waiter; 6d7684 copies six display waiters.
    pub fn decode(operation: u16, payload: &[u8], bindings: &[CallbackBinding]) -> Result<Self, RequestError> {
        let operation = Operation::parse(operation)?;
        let expected = match operation { Operation::Open => 8, Operation::Close => 4, Operation::Render => 48 };
        if payload.len() != expected { return Err(RequestError::PayloadLength); }
        let word = |offset| u32::from_le_bytes([payload[offset], payload[offset + 1], payload[offset + 2], payload[offset + 3]]);
        let resolve = |value| -> Result<Option<WaiterId>, RequestError> {
            let Some(address) = CallbackAddress::new(value) else { return Ok(None); };
            let mut matches = bindings.iter().filter(|binding| binding.address == address);
            let waiter = matches.next().ok_or(RequestError::UnresolvedCallback(address))?.waiter;
            if matches.next().is_some() { return Err(RequestError::AmbiguousCallback(address)); }
            Ok(Some(waiter))
        };
        Ok(match operation {
            Operation::Open => Self::Open { initialization: InitializationWord(word(0)), waiter: resolve(word(4))? },
            Operation::Close => Self::Close { waiter: resolve(word(0))? },
            Operation::Render => {
                let mut waiters = [None; 6];
                for (index, waiter) in waiters.iter_mut().enumerate() { *waiter = resolve(word(index * 4))?; }
                Self::Render { waiters }
            }
        })
    }
}

pub struct RenderComposition<'a> {
    pub mode: OutputMode,
    pub first: DrawList<'a>,
    pub second: Option<DrawList<'a>>,
}
impl<'a> RenderComposition<'a> {
    pub fn resolve(self, request: QueueRequest) -> Result<RenderRequest<'a>, RequestError> {
        match request {
            QueueRequest::Render { waiters, .. } => Ok(RenderRequest { composition: self, waiters }),
            _ => Err(RequestError::DescriptorMismatch),
        }
    }
}
pub struct RenderRequest<'a> { pub composition: RenderComposition<'a>, pub waiters: [Option<WaiterId>; 6] }
