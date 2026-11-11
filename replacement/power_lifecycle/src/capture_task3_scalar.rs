use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use super::capture_task3_scalar_difference::helper_dd1254;

pub const RECORD: GuestAddress = GuestAddress::new(0x0152_6a78);
pub const STATE: GuestAddress = GuestAddress::new(0x0154_58cc);
pub const ROW: GuestAddress = GuestAddress::new(0x0154_580c);

pub(super) const fn field(offset: u32) -> GuestAddress { GuestAddress::new(0x0152_6a78 + offset) }
pub(super) fn pointer<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, offset: u32) -> Result<GuestAddress, MemoryError> { Ok(GuestAddress::new(context.read_u32(field(offset))?)) }
pub(super) fn clear<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress, size: ByteCount) -> Result<(), MemoryError> {
    for index in 0..size.0 { context.write_u8(destination.checked_add(index)?, 0)?; }
    Ok(())
}
pub(super) fn increment<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, offset: u32) -> Result<(), MemoryError> {
    let value = context.read_u32(field(offset))?;
    context.write_u32(field(offset), value.wrapping_add(1))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Operation(u8);
impl Operation {
    pub const fn new(value: u8) -> Option<Self> { if value < 70 { Some(Self(value)) } else { None } }
    pub const fn get(self) -> u8 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Bitmap(pub GuestAddress);
impl Bitmap {
    fn word(self, operation: Operation) -> Result<GuestAddress, MemoryError> { self.0.checked_add(u32::from(operation.0 / 32) * 4) }
    pub(super) fn contains<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>, operation: Operation) -> Result<bool, MemoryError> {
        Ok(context.read_u32(self.word(operation)?)? & (1 << (operation.0 % 32)) != 0)
    }
    pub(super) fn set<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>, operation: Operation) -> Result<(), MemoryError> {
        let address = self.word(operation)?;
        let value = context.read_u32(address)?;
        context.write_u32(address, value | (1 << (operation.0 % 32)))
    }
    fn unset<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>, operation: Operation) -> Result<(), MemoryError> {
        let address = self.word(operation)?;
        let value = context.read_u32(address)?;
        context.write_u32(address, value & !(1 << (operation.0 % 32)))
    }
}
pub(super) const REQUESTED: Bitmap = Bitmap(field(0x5cac));
pub(super) const STARTED: Bitmap = Bitmap(field(0x5cc0));
pub(super) const COMPLETED: Bitmap = Bitmap(field(0x5ccc));
pub(super) const GROUP: [Operation; 15] = [Operation(21), Operation(22), Operation(23), Operation(24), Operation(25), Operation(26), Operation(27), Operation(28), Operation(29), Operation(30), Operation(31), Operation(32), Operation(33), Operation(34), Operation(35)];
pub(super) const fn operation(value: u8) -> Operation { Operation(value) }
pub(super) fn pending<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, bitmap: Bitmap, operation: Operation) -> Result<bool, MemoryError> { Ok(REQUESTED.contains(context, operation)? && !bitmap.contains(context, operation)?) }
pub(super) fn mark_group<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, bitmap: Bitmap) -> Result<(), MemoryError> {
    for operation in GROUP { bitmap.set(context, operation)?; }
    Ok(())
}
pub(super) fn covered<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, actual: Bitmap, required: Bitmap, mut list: GuestAddress) -> Result<bool, MemoryError> {
    loop {
        let value = context.read_u32(list)?;
        if value == u32::MAX { return Ok(true); }
        let operation = Operation::new(value as u8).filter(|_| value < 70).ok_or(MemoryError::Unmapped(list))?;
        if required.contains(context, operation)? && !actual.contains(context, operation)? { return Ok(false); }
        list = list.checked_add(4)?;
    }
}
pub(super) fn response<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<(), MemoryError> { context.write_u32(field(0x5c84), value) }

pub fn helper_dde298<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, input: GuestAddress, output: Option<GuestAddress>) -> Result<u32, MemoryError> {
    context.copy_bytes(field(0x5c80), input, ByteCount(4))?;
    response(context, 0)?;
    if let Some(output) = output { context.copy_bytes(output, field(0x5c84), ByteCount(4))?; }
    context.read_u32(field(0x5c84))
}
pub fn helper_dde308<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<(), MemoryError> { response(context, value) }
pub fn helper_dde32c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    clear(context, field(0x5c88), ByteCount(0x7c))?;
    context.write_u32(field(0x5c9c), 0xa3)
}
pub fn helper_dde360<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>) -> Result<(), MemoryError> { Ok(()) }
pub fn helper_dde374<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    if context.read_u32(GuestAddress::new(0x9168f4))? == 1 {
        let value = context.read_u32(field(0xf264))?;
        context.write_u32(field(0x5cec), value)?;
        let value = context.read_u16(field(0xf268))?;
        context.write_u16(field(0x5cf0), value)?;
    }
    Ok(())
}
pub fn helper_dde3b4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    use super::capture_task3_scalar_configuration::{helper_72da80, helper_72dacc, helper_72db5c};
    let value = context.read_u32(GuestAddress::new(0x9168d8))?;
    context.write_u32(field(0x5ce4), value)?;
    helper_72da80(context, 0)?;
    let value = context.read_u32(GuestAddress::new(0x9168f4))?;
    context.write_u32(field(0x5ce8), value)?;
    let settings = pointer(context, 0x353c)?;
    let kind = context.read_u32(settings)?;
    helper_72dacc(context, kind, 0)?;
    helper_72db5c(context, 0)?;
    context.write_u32(field(0x5ca0), settings.get())?;
    context.write_u32(field(0x5ca4), field(0xc704).get())?;
    context.copy_bytes(field(0xc704), settings, ByteCount(1000))
}
pub fn helper_dde574<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), crate::capture_settings_predicates_runtime::CapturePredicateError> {
    context.with_guest_frame(ByteCount(40), crate::record_address_space::StackAlignment::Doubleword, |context, frame| {
        let before = pointer(context, 0x5ca0)?;
        let after = pointer(context, 0x5ca4)?;
        let configuration = context.read_u32(field(0x5c9c))?;
        helper_dd1254(context, before, after, configuration, frame.base())?;
        context.copy_bytes(REQUESTED.0, frame.base(), ByteCount(12))?;
        let category = context.read_u32(frame.at(ByteCount(12), ByteCount(4))?)?;
        context.write_u32(field(0x5cb8), category)?;
        context.copy_bytes(field(0x5cbc), frame.at(ByteCount(32), ByteCount(4))?, ByteCount(4))?;
        REQUESTED.unset(context, Operation(2))?;
        Ok(())
    })
}
pub fn helper_ddf32c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    for list in [0xe41c80, 0xe41c40, 0xe41bd0] {
        let mut list = GuestAddress::new(list);
        loop {
            let value = context.read_u32(list)?;
            if value == u32::MAX { break; }
            let operation = Operation::new(value as u8).filter(|_| value < 70).ok_or(MemoryError::Unmapped(list))?;
            if STARTED.contains(context, operation)? && !COMPLETED.contains(context, operation)? { return Ok(0); }
            list = list.checked_add(4)?;
        }
    }
    Ok(1)
}
pub(super) fn all_completed<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<bool, MemoryError> {
    for list in [0xe41c80, 0xe41c40, 0xe41bd0, 0xe41d10] {
        if !covered(context, COMPLETED, REQUESTED, GuestAddress::new(list))? { return Ok(false); }
    }
    Ok(true)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueRemoval { Removed, Missing }

pub fn helper_dce1f8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, request: u32) -> Result<QueueRemoval, MemoryError> {
    let count = context.read_u32(field(0x3534))?;
    let mut found = None;
    for index in 0..count {
        if context.read_u32(field(0x1ac).checked_add(index.wrapping_mul(0x38))?)? == request { found = Some(index); break; }
    }
    let Some(index) = found else {
        context.write_u32(field(0x3534), count.wrapping_sub(1))?;
        return Ok(QueueRemoval::Missing);
    };
    clear(context, field(0x188).checked_add(index.wrapping_mul(0x38))?, ByteCount(12))?;
    for cursor in index..count - 1 {
        context.copy_bytes(field(0x188).checked_add(cursor.wrapping_mul(0x38))?, field(0x188).checked_add(cursor.wrapping_add(1).wrapping_mul(0x38))?, ByteCount(12))?;
    }
    clear(context, field(0x188).checked_add((count - 1).wrapping_mul(0x38))?, ByteCount(12))?;
    clear(context, field(0x194).checked_add(index.wrapping_mul(0x38))?, ByteCount(44))?;
    for cursor in index..count - 1 {
        context.copy_bytes(field(0x194).checked_add(cursor.wrapping_mul(0x38))?, field(0x194).checked_add(cursor.wrapping_add(1).wrapping_mul(0x38))?, ByteCount(28))?;
    }
    clear(context, field(0x194).checked_add((count - 1).wrapping_mul(0x38))?, ByteCount(44))?;
    context.write_u32(field(0x3534), count - 1)?;
    Ok(QueueRemoval::Removed)
}

pub fn helper_dd0978<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>) -> Result<(), MemoryError> { Ok(()) }
pub fn helper_dd0bfc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, count: u32) -> Result<(), MemoryError> {
    let available = context.read_u8(field(0x355c))?;
    if count == 0 || count > u32::from(available) { return Ok(()); }
    for _ in 0..count {
        for offset in [0x3548, 0x354c, 0x3550, 0x3554] {
            let value = context.read_u32(field(offset + 4))?;
            context.write_u32(field(offset), value)?;
        }
        context.write_u32(field(0x3558), 0)?;
    }
    context.write_u8(field(0x355c), available.wrapping_sub(count as u8))
}
