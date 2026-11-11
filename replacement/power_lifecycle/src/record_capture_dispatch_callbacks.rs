use crate::record_address_space::{ByteCount, CallbackId, GuestAddress, GuestAddressSpace, MemoryError, RecordContext, RegisterWordCount, StackAlignment};
use crate::record_hardware_requests::{helper_fe9580, CaptureCommand, CaptureQueue};

fn empty<M: GuestAddressSpace, E: CaptureQueue<M>>(context: &mut RecordContext<M, E>, command: u16, callbacks: &[CallbackId]) -> Result<(), MemoryError> {
    let _ = helper_fe9580(context, CaptureCommand::new(command), GuestAddress::new(0), ByteCount(0), callbacks)?;
    Ok(())
}

pub fn helper_fe95c8<M: GuestAddressSpace, E: CaptureQueue<M>>(context: &mut RecordContext<M, E>, status: u32, value: u32, image: GuestAddress, tail: u32, callbacks: &[CallbackId]) -> Result<(), MemoryError> {
    if status != 0 { return empty(context, 0x3f0a, callbacks); }
    context.with_guest_frame(ByteCount(224), StackAlignment::Word, |context, frame| {
        context.write_u32(frame.base(), value)?;
        context.copy_bytes(frame.at(ByteCount(4), ByteCount(216))?, image, ByteCount(216))?;
        context.write_u32(frame.at(ByteCount(220), ByteCount(4))?, tail)?;
        let _ = helper_fe9580(context, CaptureCommand::new(0x3f07), frame.base(), ByteCount(224), callbacks)?;
        Ok(())
    })
}

pub fn helper_fe9624<M: GuestAddressSpace, E: CaptureQueue<M>>(context: &mut RecordContext<M, E>, status: u32, value: u32, pair: GuestAddress, short: GuestAddress, tail: u32, callbacks: &[CallbackId]) -> Result<(), MemoryError> {
    if status != 0 { return empty(context, 0x3f0a, callbacks); }
    context.with_guest_frame(ByteCount(24), StackAlignment::Word, |context, frame| {
        context.write_u32(frame.base(), value)?;
        context.copy_register_words(frame.at(ByteCount(4), ByteCount(8))?, pair, RegisterWordCount::Two)?;
        context.copy_bytes(frame.at(ByteCount(12), ByteCount(2))?, short, ByteCount(2))?;
        context.write_u32(frame.at(ByteCount(16), ByteCount(4))?, tail)?;
        let _ = helper_fe9580(context, CaptureCommand::new(0x3f08), frame.base(), ByteCount(24), callbacks)?;
        Ok(())
    })
}

pub fn helper_fe968c<M: GuestAddressSpace, E: CaptureQueue<M>>(context: &mut RecordContext<M, E>, callbacks: &[CallbackId]) -> Result<(), MemoryError> { empty(context, 0x3f01, callbacks) }
pub fn helper_fe96a8<M: GuestAddressSpace, E: CaptureQueue<M>>(context: &mut RecordContext<M, E>, callbacks: &[CallbackId]) -> Result<(), MemoryError> { empty(context, 0x3f00, callbacks) }
pub fn helper_fe96c4<M: GuestAddressSpace, E: CaptureQueue<M>>(context: &mut RecordContext<M, E>, callbacks: &[CallbackId]) -> Result<(), MemoryError> { empty(context, 0x3f0b, callbacks) }
