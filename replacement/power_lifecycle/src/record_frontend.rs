use crate::record_address_space::{ByteCount, CallbackId, GuestAddress, GuestAddressSpace, RecordContext, RecordError, StackAlignment};
use crate::record_hardware_requests::{CaptureCommand, helper_fe9580};
pub use crate::record_hardware_requests::CaptureQueue as FrontendEffects;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct GeometryInput {
    pub profile: u32, pub enabled: u32, pub dimensions: u32, pub mode: u32, pub layout: u32, pub grid_mode: u32,
    pub format: u16, pub format_padding: u16, pub curve: u32, pub scale_first: u32, pub scale_second: u32,
    pub reserved: u32, pub horizontal_enable: u32, pub vertical_enable: u32, pub selection: u32,
    pub magnification: u16, pub radius: u16, pub adjustment: u16, pub adjustment_padding: u16,
    pub output_mode: u32, pub curve_limit: u32, pub cell_size: u16, pub grid_size: u16,
    pub active: u32, pub first_present: u32, pub second_present: u32, pub first_mode: u32, pub second_mode: u32,
    pub count: u32, pub records: u32, pub shift: u16, pub shift_padding: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PeakPositionRequest {
    pub kind: u32, pub transition: i32, pub width: u16, pub height: u16, pub step: i16,
    pub padding: u16, pub focus: u32, pub enabled: u32, pub mode: u32, pub reserved: u32, pub time: u32,
}

fn pair(first: u16, second: u16) -> u32 { u32::from(first) | (u32::from(second) << 16) }

fn words<M: GuestAddressSpace, E: FrontendEffects<M>, const N: usize>(context: &mut RecordContext<M, E>, command: u16, words: [u32; N], callbacks: &[CallbackId]) -> Result<(), RecordError> {
    context.with_guest_frame(ByteCount(N as u32 * 4), StackAlignment::Word, |context, frame| {
        for (index, word) in words.into_iter().enumerate() { context.write_u32(frame.at(ByteCount(index as u32 * 4), ByteCount(4))?, word)?; }
        helper_fe9580(context, CaptureCommand::new(command), frame.base(), frame.length(), callbacks)?;
        Ok(())
    })
}

pub fn helper_feab6c<M: GuestAddressSpace, E: FrontendEffects<M>>(context: &mut RecordContext<M, E>, first: CallbackId, second: CallbackId, input: &GeometryInput) -> Result<(), RecordError> {
    words(context, 0x3f46, [first.key().get(), second.key().get(),
        input.profile, input.enabled, input.dimensions, input.mode, input.layout, input.grid_mode,
        pair(input.format, input.format_padding), input.curve, input.scale_first, input.scale_second,
        input.reserved, input.horizontal_enable, input.vertical_enable, input.selection,
        pair(input.magnification, input.radius), pair(input.adjustment, input.adjustment_padding),
        input.output_mode, input.curve_limit, pair(input.cell_size, input.grid_size), input.active,
        input.first_present, input.second_present, input.first_mode, input.second_mode,
        input.count, input.records, pair(input.shift, input.shift_padding)], &[first, second])
}

pub fn helper_fea634<M: GuestAddressSpace, E: FrontendEffects<M>>(context: &mut RecordContext<M, E>, callback: CallbackId, input: GuestAddress) -> Result<(), RecordError> {
    context.with_guest_frame(ByteCount(44), StackAlignment::Word, |context, frame| {
        let packet = frame.base();
        context.write_u32(packet, callback.key().get())?;
        for offset in [0, 4] {
            let value = context.read_u32(input.checked_add(offset)?)?;
            context.write_u32(packet.checked_add(4 + offset)?, value)?;
        }
        for offset in [8, 10, 12] {
            let value = context.read_u16(input.checked_add(offset)?)?;
            context.write_u16(packet.checked_add(4 + offset)?, value)?;
        }
        for offset in [16, 20, 24, 28, 32] {
            let value = context.read_u32(input.checked_add(offset)?)?;
            context.write_u32(packet.checked_add(4 + offset)?, value)?;
        }
        helper_fe9580(context, CaptureCommand::new(0x3f2e), packet, ByteCount(44), &[callback])?;
        Ok(())
    })
}

pub fn helper_fea6ac<M: GuestAddressSpace, E: FrontendEffects<M>>(context: &mut RecordContext<M, E>) -> Result<(), RecordError> {
    helper_fe9580(context, CaptureCommand::new(0x3f2f), GuestAddress::new(0), ByteCount(0), &[])?;
    Ok(())
}

pub fn helper_fea468<M: GuestAddressSpace, E: FrontendEffects<M>>(context: &mut RecordContext<M, E>, callback: CallbackId, first: u32, second: u32, third: u32) -> Result<(), RecordError> {
    words(context, 0x3f16, [callback.key().get(), first, second, third], &[callback])
}

pub fn helper_feabb8<M: GuestAddressSpace, E: FrontendEffects<M>>(context: &mut RecordContext<M, E>, callback: CallbackId, input: GuestAddress) -> Result<(), RecordError> {
    let value = context.read_u32(input)?;
    words(context, 0x3f47, [callback.key().get(), value], &[callback])
}

pub fn helper_feac54<M: GuestAddressSpace, E: FrontendEffects<M>>(context: &mut RecordContext<M, E>, callback: CallbackId, value: i32, bound: i16) -> Result<(), RecordError> {
    context.with_guest_frame(ByteCount(12), StackAlignment::Word, |context, frame| {
        context.write_u32(frame.base(), callback.key().get())?;
        context.write_u32(frame.base().checked_add(4)?, value as u32)?;
        context.write_u16(frame.base().checked_add(8)?, bound as u16)?;
        helper_fe9580(context, CaptureCommand::new(0x3f4a), frame.base(), frame.length(), &[callback])?;
        Ok(())
    })
}

pub fn helper_feac84<M: GuestAddressSpace, E: FrontendEffects<M>>(context: &mut RecordContext<M, E>, callback: CallbackId) -> Result<(), RecordError> {
    words(context, 0x3f4f, [callback.key().get()], &[callback])
}

pub fn helper_fead28<M: GuestAddressSpace, E: FrontendEffects<M>>(context: &mut RecordContext<M, E>, input: GuestAddress) -> Result<(), RecordError> {
    context.with_guest_frame(ByteCount(20), StackAlignment::Word, |context, frame| {
        context.copy_bytes(frame.base(), input, ByteCount(20))?;
        helper_fe9580(context, CaptureCommand::new(0x3f4e), frame.base(), frame.length(), &[])?;
        Ok(())
    })
}

#[cfg(test)]
#[path = "record_frontend_tests.rs"]
mod tests;
