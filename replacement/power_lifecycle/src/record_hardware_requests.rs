use crate::record_address_space::{ByteCount, CallbackId, GuestAddress, GuestAddressSpace, MemoryError, RecordContext, StackAlignment};
use crate::shutdown::KernelStatus;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureCommand(u16);
impl CaptureCommand {
    pub const fn new(value: u16) -> Self { Self(value) }
    pub const fn get(self) -> u16 { self.0 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureTask(u32);
impl CaptureTask {
    pub const fn new(value: u32) -> Self { Self(value) }
    pub const fn get(self) -> u32 { self.0 }
}

pub struct CaptureMessage<'callbacks> {
    pub command: CaptureCommand,
    pub source: CaptureTask,
    pub destination: CaptureTask,
    pub payload: GuestAddress,
    pub length: ByteCount,
    pub callbacks: &'callbacks [CallbackId],
}

pub trait CaptureQueue<M: GuestAddressSpace> {
    fn current_task(&mut self, memory: &mut M) -> Result<CaptureTask, MemoryError>;
    // The service consumes the guest payload before submit returns.
    fn submit(&mut self, memory: &mut M, message: CaptureMessage<'_>) -> KernelStatus;
}

pub fn helper_fe9580<M: GuestAddressSpace, E: CaptureQueue<M>>(
    context: &mut RecordContext<M, E>, command: CaptureCommand, payload: GuestAddress, length: ByteCount, callbacks: &[CallbackId],
) -> Result<KernelStatus, MemoryError> {
    let destination = CaptureTask::new(context.read_u32(GuestAddress::new(0x016f_0aa4))?);
    let source = context.effects.current_task(&mut context.memory)?;
    Ok(context.effects.submit(&mut context.memory, CaptureMessage { command, source, destination, payload, length, callbacks }))
}

fn send_words<M: GuestAddressSpace, E: CaptureQueue<M>, const N: usize>(
    context: &mut RecordContext<M, E>, code: u16, words: [u32; N], callbacks: &[CallbackId],
) -> Result<KernelStatus, MemoryError> {
    context.with_guest_frame(ByteCount(N as u32 * 4), StackAlignment::Word, |context, frame| {
        for (index, word) in words.into_iter().enumerate() {
            context.write_u32(frame.at(ByteCount(index as u32 * 4), ByteCount(4))?, word)?;
        }
        helper_fe9580(context, CaptureCommand::new(code), frame.base(), frame.length(), callbacks)
    })
}

pub fn helper_fea6c8<M: GuestAddressSpace, E: CaptureQueue<M>>(
    context: &mut RecordContext<M, E>, callback: CallbackId, a: u32, b: u32, c: u32, d: u32, e: u32,
) -> Result<KernelStatus, MemoryError> {
    send_words(context, 0x3f30, [callback.key().get(), a, b, (c & 0xffff) | (d << 16), e], &[callback])
}

pub fn helper_fea70c<M: GuestAddressSpace, E: CaptureQueue<M>>(context: &mut RecordContext<M, E>) -> Result<KernelStatus, MemoryError> {
    send_words(context, 0x3f31, [0], &[])
}

pub fn helper_fea738<M: GuestAddressSpace, E: CaptureQueue<M>>(context: &mut RecordContext<M, E>, value: u32) -> Result<KernelStatus, MemoryError> {
    send_words(context, 0x3f32, [value], &[])
}

pub fn helper_fea760<M: GuestAddressSpace, E: CaptureQueue<M>>(
    context: &mut RecordContext<M, E>, callback: CallbackId, a: u32, b: u32, c: u32, d: u32,
) -> Result<KernelStatus, MemoryError> {
    context.with_guest_frame(ByteCount(16), StackAlignment::Word, |context, frame| {
        context.write_u32(frame.base(), callback.key().get())?;
        context.write_u16(frame.at(ByteCount(4), ByteCount(2))?, a as u16)?;
        context.write_u16(frame.at(ByteCount(6), ByteCount(2))?, b as u16)?;
        context.write_u16(frame.at(ByteCount(8), ByteCount(2))?, c as u16)?;
        context.write_u32(frame.at(ByteCount(12), ByteCount(4))?, d)?;
        helper_fe9580(context, CaptureCommand::new(0x3f33), frame.base(), frame.length(), &[callback])
    })
}

pub fn helper_fea824<M: GuestAddressSpace, E: CaptureQueue<M>>(
    context: &mut RecordContext<M, E>, callback: CallbackId, profile: GuestAddress,
) -> Result<KernelStatus, MemoryError> {
    context.copy_bytes(GuestAddress::new(0x016f_0c90), profile, ByteCount(0xdc))?;
    send_words(context, 0x3f1a, [callback.key().get(), 0x016f_0c90], &[callback])
}

pub fn helper_fea868<M: GuestAddressSpace, E: CaptureQueue<M>>(context: &mut RecordContext<M, E>) -> Result<KernelStatus, MemoryError> {
    helper_fe9580(context, CaptureCommand::new(0x3f1b), GuestAddress::new(0), ByteCount(0), &[])
}

pub fn helper_fea884<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, request: GuestAddress) -> Result<u32, MemoryError> {
    let index = u32::from(context.read_u8(request)?);
    let mut pointers = [GuestAddress::new(0); 8];
    for (offset, pointer) in pointers.iter_mut().enumerate() {
        *pointer = GuestAddress::new(context.read_u32(request.checked_add(4 + offset as u32 * 4)?)?);
    }
    let adjusted = GuestAddress::new(context.read_u32(request.checked_add(40)?)?);
    crate::record_hardware_deep_entries::helper_fec494(
        context, index, pointers[0], pointers[1], pointers[2], pointers[3], pointers[4], pointers[5], pointers[6], pointers[7], request.checked_add(36)?, adjusted,
    )
}

fn copied_request<M: GuestAddressSpace, E: CaptureQueue<M>>(
    context: &mut RecordContext<M, E>, code: u16, callback: CallbackId, request: GuestAddress, payload_length: ByteCount,
) -> Result<KernelStatus, MemoryError> {
    context.with_guest_frame(ByteCount(payload_length.0 + 4), StackAlignment::Word, |context, frame| {
        context.write_u32(frame.at(payload_length, ByteCount(4))?, callback.key().get())?;
        context.copy_bytes(frame.base(), request, payload_length)?;
        helper_fe9580(context, CaptureCommand::new(code), frame.base(), frame.length(), &[callback])
    })
}

pub fn helper_fea8e8<M: GuestAddressSpace, E: CaptureQueue<M>>(context: &mut RecordContext<M, E>, callback: CallbackId, request: GuestAddress) -> Result<KernelStatus, MemoryError> {
    copied_request(context, 0x3f1d, callback, request, ByteCount(48))
}

pub fn helper_fea91c<M: GuestAddressSpace, E: CaptureQueue<M>>(context: &mut RecordContext<M, E>, callback: CallbackId, request: GuestAddress) -> Result<KernelStatus, MemoryError> {
    copied_request(context, 0x3f1e, callback, request, ByteCount(52))
}

pub fn helper_fea950<M: GuestAddressSpace, E: CaptureQueue<M>>(context: &mut RecordContext<M, E>, callback: CallbackId, value: u32, request: GuestAddress) -> Result<KernelStatus, MemoryError> {
    context.with_guest_frame(ByteCount(128), StackAlignment::Word, |context, frame| {
        context.write_u32(frame.at(ByteCount(120), ByteCount(4))?, value)?;
        context.write_u32(frame.at(ByteCount(124), ByteCount(4))?, callback.key().get())?;
        context.copy_bytes(frame.base(), request, ByteCount(120))?;
        helper_fe9580(context, CaptureCommand::new(0x3f1f), frame.base(), frame.length(), &[callback])
    })
}

pub fn helper_fea98c<M: GuestAddressSpace, E: CaptureQueue<M>>(context: &mut RecordContext<M, E>) -> Result<KernelStatus, MemoryError> {
    helper_fe9580(context, CaptureCommand::new(0x3f21), GuestAddress::new(0), ByteCount(0), &[])
}

pub fn helper_fea9a8<M: GuestAddressSpace, E: CaptureQueue<M>>(context: &mut RecordContext<M, E>, value: u32) -> Result<KernelStatus, MemoryError> {
    send_words(context, 0x3f22, [value], &[])
}

pub fn helper_fea9d0<M: GuestAddressSpace, E: CaptureQueue<M>>(context: &mut RecordContext<M, E>, value: u32) -> Result<KernelStatus, MemoryError> {
    send_words(context, 0x3f23, [value], &[])
}

pub fn helper_fea9f8<M: GuestAddressSpace, E: CaptureQueue<M>>(context: &mut RecordContext<M, E>, value: u32, flag: u32) -> Result<KernelStatus, MemoryError> {
    context.with_guest_frame(ByteCount(8), StackAlignment::Word, |context, frame| {
        context.write_u32(frame.base(), value)?;
        context.write_u8(frame.at(ByteCount(4), ByteCount(1))?, flag as u8)?;
        helper_fe9580(context, CaptureCommand::new(0x3f20), frame.base(), frame.length(), &[])
    })
}

pub fn helper_feaa34<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, output: GuestAddress) -> Result<GuestAddress, MemoryError> {
    let value = context.read_u16(GuestAddress::new(0x016f_102c))?;
    context.write_u16(output, value)?;
    Ok(GuestAddress::new(0x016f_111c))
}

pub fn helper_feaa54<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, output: GuestAddress) -> Result<GuestAddress, MemoryError> {
    context.copy_bytes(output, GuestAddress::new(0x016f_137c), ByteCount(0xa4))?;
    Ok(output)
}

pub fn helper_feaa24<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, request: GuestAddress) -> Result<u32, MemoryError> {
    crate::record_hardware_deep_samples::helper_fef5e8(context, request)
}
