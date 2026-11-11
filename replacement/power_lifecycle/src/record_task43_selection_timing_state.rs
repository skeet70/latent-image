use crate::record_address_space::{ByteCount, CallbackKey, GuestAddress, GuestAddressSpace, MemoryError, RecordContext, RegisterWordCount};

pub const DRIVE: GuestAddress = GuestAddress::new(0x016a04d8);
pub const POINTS: GuestAddress = GuestAddress::new(0x00919860);
pub const SAVED_RECORD: GuestAddress = GuestAddress::new(0x009197ec);

pub fn helper_f17ee0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    context.write_u32(DRIVE.checked_add(0x20)?, 1)
}

pub fn helper_f17f04<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    helper_f17ee0(context)
}

pub fn helper_f17f14<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    if context.read_u32(DRIVE.checked_add(0x18)?)? == 0 { return Ok(1); }
    if context.read_u32(DRIVE.checked_add(0x1c)?)? == 0 { return Ok(1); }
    Ok(u32::from(context.read_u32(DRIVE.checked_add(0x20)?)? == 0))
}

pub fn helper_f1886c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    for offset in [0x18, 0x1c, 0x20] { context.write_u32(DRIVE.checked_add(offset)?, 0)?; }
    Ok(())
}

pub fn helper_f18898<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> { Ok(0) }

pub fn helper_ee8ba0<M, E, Error>(context: &mut RecordContext<M, E>, callback: fn(&mut RecordContext<M, E>) -> Result<(), Error>) -> Result<(), Error> {
    callback(context)
}

fn copy_points<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress, source: GuestAddress) -> Result<(), MemoryError> {
    context.copy_register_words(destination, source, RegisterWordCount::Four)?;
    context.copy_register_words(destination.checked_add(16)?, source.checked_add(16)?, RegisterWordCount::Four)?;
    context.copy_register_words(destination.checked_add(32)?, source.checked_add(32)?, RegisterWordCount::One)
}

pub fn helper_758940<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress) -> Result<GuestAddress, MemoryError> {
    copy_points(context, destination, POINTS)?;
    Ok(destination)
}

pub fn helper_758980<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, item: u32, source: GuestAddress) -> Result<(), MemoryError> {
    if crate::record_profile_accessors::helper_75a8c0(context, item, 34)? != 0 {
        copy_points(context, POINTS, source)?;
    }
    Ok(())
}

pub fn helper_75890c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, item: u32, source: GuestAddress) -> Result<(), MemoryError> {
    if crate::record_profile_accessors::helper_75a8c0(context, item, 33)? != 0 {
        context.copy_bytes(SAVED_RECORD, source, ByteCount(76))?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimingValue(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Position(pub i16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Channel(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimingCallbackArguments {
    pub parameter: TimingValue,
    pub aperture: TimingValue,
    pub target: TimingValue,
    pub timing: TimingValue,
    pub flags: u32,
    pub status: u32,
    pub low: Position,
    pub high: Position,
    pub step: Position,
    pub saved: Position,
    pub geometry: Channel,
}

impl TimingCallbackArguments {
    pub fn store(self, callback: CallbackKey, payload: &mut [u8; 48]) {
        fn word(payload: &mut [u8; 48], offset: usize, value: u32) { payload[offset..offset + 4].copy_from_slice(&value.to_le_bytes()); }
        fn half(payload: &mut [u8; 48], offset: usize, value: u16) { payload[offset..offset + 2].copy_from_slice(&value.to_le_bytes()); }
        word(payload, 0, callback.get());
        word(payload, 4, self.parameter.0);
        half(payload, 8, self.aperture.0 as u16);
        word(payload, 12, self.target.0);
        half(payload, 16, self.timing.0 as u16);
        word(payload, 20, self.flags);
        word(payload, 24, self.status);
        word(payload, 28, 0);
        word(payload, 32, 1);
        half(payload, 36, self.low.0 as u16);
        half(payload, 38, self.high.0 as u16);
        half(payload, 40, self.step.0 as u16);
        half(payload, 42, self.saved.0 as u16);
        word(payload, 44, self.geometry.0);
    }
}
