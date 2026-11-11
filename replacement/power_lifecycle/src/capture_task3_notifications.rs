use crate::capture_task3::TaskQueue;
use crate::record_address_space::{ByteCount, CallbackError, CallbackHandler, CallbackKey, GuestAddress, GuestAddressSpace, RecordContext, RecordError, StackAlignment};
use crate::shutdown::KernelStatus;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct NotificationOperation(pub u16);

pub trait NotificationQueues {
    fn current_task(&mut self) -> u8;
    fn post_application_record(&mut self, destination: TaskQueue, record: &[u8; 76]) -> Result<(), KernelStatus>;
    fn save_interrupts(&mut self) -> u32;
    fn restore_interrupts(&mut self, saved: u32);
    fn notification_queue_full(&mut self, source: u8);
    fn notification_coalesced(&mut self);
}

pub trait ApplicationCallbacks<M: GuestAddressSpace>: Sized {
    fn application_handler(&self, key: CallbackKey) -> Result<CallbackHandler<RecordContext<M, Self>>, CallbackError>;
}

fn word(record: &[u8; 76], offset: usize) -> u32 {
    u32::from_le_bytes([record[offset], record[offset + 1], record[offset + 2], record[offset + 3]])
}

pub fn helper_72b210<M: GuestAddressSpace, E: NotificationQueues>(context: &mut RecordContext<M, E>, record: &[u8; 76]) -> Result<(), RecordError> {
    let operation = word(record, 0);
    let alternate = context.read_u32(GuestAddress::new(0x0091_34f8))? == 1;
    let destination = TaskQueue(if alternate && operation & 0x900 != 0 { 7 } else { 1 });
    if operation == 0x82e && destination.0 == 1 {
        let count = context.read_u32(GuestAddress::new(0x0091_21bc))?;
        if count != context.read_u32(GuestAddress::new(0x0091_34c4))? {
            context.effects.notification_coalesced();
            return Ok(());
        }
        context.write_u32(GuestAddress::new(0x0091_21bc), count.wrapping_add(1))?;
    }
    let result = context.effects.post_application_record(destination, record);
    if operation == 0x91b && context.read_u8(GuestAddress::new(0x0095_4884))? == 0 {
        let source = context.effects.current_task();
        context.write_u8(GuestAddress::new(0x0095_4884), source)?;
    }
    result.map_err(RecordError::from)
}

pub fn helper_72b318<M: GuestAddressSpace, E: NotificationQueues>(context: &mut RecordContext<M, E>, record: &[u8; 76]) -> Result<(), RecordError> {
    let saved = context.effects.save_interrupts();
    let result = (|| {
        let count = context.read_u32(GuestAddress::new(0x0091_34c0))?;
        if count > 63 {
            let source = context.effects.current_task();
            context.effects.notification_queue_full(source);
            return Err(KernelStatus(-63).into());
        }
        let destination = GuestAddress::new(0x0091_21c0 + count * 76);
        context.write_u32(destination, word(record, 0))?;
        context.write_u32(destination.checked_add(4)?, word(record, 4))?;
        context.write_u32(GuestAddress::new(0x0091_34c0), count + 1)?;
        Ok(())
    })();
    context.effects.restore_interrupts(saved);
    result
}

pub fn helper_72b680<M: GuestAddressSpace, E: ApplicationCallbacks<M>>(context: &mut RecordContext<M, E>, record: GuestAddress) -> Result<(), RecordError> {
    let address = context.read_u32(GuestAddress::new(0x0082_9a0c))?;
    let key = CallbackKey::new(address).ok_or(CallbackError::Unregistered)?;
    let handler = context.effects.application_handler(key)?;
    let CallbackHandler::ApplicationRecord(handler) = handler else { return Err(CallbackError::Signature.into()); };
    let mut payload = [0; 76];
    for (offset, byte) in payload.iter_mut().enumerate() { *byte = context.read_u8(record.checked_add(offset as u32)?)?; }
    handler(context, &payload)
}

pub fn helper_dcb6c0<M: GuestAddressSpace, E: ApplicationCallbacks<M>>(context: &mut RecordContext<M, E>, operation: NotificationOperation, value: u32) -> Result<(), RecordError> {
    context.with_guest_frame(ByteCount(84), StackAlignment::Word, |context, frame| {
        let record = frame.at(ByteCount(4), ByteCount(76))?;
        context.write_u32(record, u32::from(operation.0))?;
        context.write_u32(record.checked_add(4)?, value)?;
        helper_72b680(context, record)
    })
}

pub fn repeat_current<M: GuestAddressSpace, E: ApplicationCallbacks<M>>(context: &mut RecordContext<M, E>) -> Result<(), RecordError> {
    let operation = context.read_u16(GuestAddress::new(0x0152_6a78))?;
    helper_dcb6c0(context, NotificationOperation(0x91b), u32::from(operation))
}

pub fn helper_dcb764<M: GuestAddressSpace, E: ApplicationCallbacks<M>>(context: &mut RecordContext<M, E>) -> Result<(), RecordError> {
    let operation = context.read_u16(GuestAddress::new(0x0152_6a78))?;
    if operation & 0x7f00 != 0x200 { return repeat_current(context); }
    let offset = match operation {
        0x200 | 0x201 | 0x208 | 0x209 => Some(0x1c),
        0x202 | 0x20c | 0x20d | 0x20f | 0x215 => Some(0x10),
        0x203 | 0x204 | 0x20b => Some(0x18),
        0x205 | 0x214 => Some(0x0c),
        0x206 | 0x207 => Some(0x24),
        0x20e => Some(0x20),
        0x210 | 0x212 | 0x213 => Some(0x14),
        _ => None,
    };
    let value = match offset { Some(offset) => context.read_u32(GuestAddress::new(0x0152_6a78 + offset))?, None => 0xffff };
    helper_dcb6c0(context, NotificationOperation(0x838), value)
}

pub fn report_missing<M: GuestAddressSpace, E: ApplicationCallbacks<M>>(context: &mut RecordContext<M, E>) -> Result<(), RecordError> {
    if context.read_u32(GuestAddress::new(0x0091_34f8))? != 1 { repeat_current(context)?; }
    Ok(())
}
