use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext, RecordError, StackAlignment};

const RECORDING: GuestAddress = GuestAddress::new(0x0152_6a78);
const PROFILE_EXPOSURE: GuestAddress = GuestAddress::new(0x0091_682e);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct ExposureSelection(pub u32);

pub trait ExposureDisplay<M: GuestAddressSpace> {
    fn draw_exposure_text(&mut self, memory: &mut M, text: GuestAddress, column: u8, row: u8, foreground: u8, background: u8) -> Result<(), RecordError>;
}

pub fn helper_738d14<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, value: ExposureSelection) -> Result<u16, MemoryError> {
    Ok(value.0 as u16)
}

pub fn helper_72d9cc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u16) -> Result<(), MemoryError> {
    context.write_u16(PROFILE_EXPOSURE, value)
}

pub fn helper_dccbb0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, flags: u16) -> Result<(), MemoryError> {
    let address = RECORDING.checked_add(0x356c)?;
    let value = if flags == 0 { 0 } else { context.read_u16(address)? | flags };
    context.write_u16(address, value)
}

pub fn helper_dcbe68<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<bool, MemoryError> {
    const PROFILE_VALUE: GuestAddress = GuestAddress::new(0x0091_6898);
    if context.read_u32(PROFILE_VALUE)? == value { return Ok(false); }
    context.write_u32(PROFILE_VALUE, value)?;
    let work = GuestAddress::new(context.read_u32(RECORDING.checked_add(0x3540)?)?);
    let value = context.read_u32(PROFILE_VALUE)?;
    context.write_u32(work.checked_add(0x99c)?, value)?;
    Ok(true)
}

fn decimal_three<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress, value: u16) -> Result<(), MemoryError> {
    if value > 999 { return Ok(()); }
    for (offset, value) in [(0, b'0' + (value / 100) as u8), (1, b'0' + ((value / 10) % 10) as u8), (2, b'0' + (value % 10) as u8), (3, 0)] {
        context.write_u8(destination.checked_add(offset)?, value)?;
    }
    Ok(())
}

fn show_exposure<M: GuestAddressSpace, E: ExposureDisplay<M>>(context: &mut RecordContext<M, E>) -> Result<(), RecordError> {
    context.with_guest_frame(ByteCount(48), StackAlignment::Word, |context, frame| {
        let text = frame.at(ByteCount(8), ByteCount(40))?;
        for (offset, value) in b"OPT:\0".iter().enumerate() { context.write_u8(text.checked_add(offset as u32)?, *value)?; }
        let work = GuestAddress::new(context.read_u32(RECORDING.checked_add(0x3540)?)?);
        let optical = context.read_u8(work.checked_add(0xe8)?)?;
        decimal_three(context, text.checked_add(4)?, u16::from(optical))?;
        for (offset, value) in b"  ELE:\0".iter().enumerate() { context.write_u8(text.checked_add(7 + offset as u32)?, *value)?; }
        let work = GuestAddress::new(context.read_u32(RECORDING.checked_add(0x3540)?)?);
        let exposure = context.read_u16(work.checked_add(0xea)?)?;
        decimal_three(context, text.checked_add(13)?, exposure)?;
        context.effects.draw_exposure_text(&mut context.memory, text, 3, 8, 0xff, 7)
    })
}

pub fn helper_dcc01c<M: GuestAddressSpace, E: ExposureDisplay<M>>(context: &mut RecordContext<M, E>, selection: ExposureSelection) -> Result<bool, RecordError> {
    let handle = GuestAddress::new(context.read_u32(RECORDING.checked_add(0x3544)?)?);
    let resource = crate::recording_capture_resources::helper_eb2958(context, handle)?;
    let changed = context.read_u32(resource)? != selection.0;
    if changed {
        let narrowed = helper_738d14(context, selection)?;
        helper_72d9cc(context, narrowed)?;
        let handle = GuestAddress::new(context.read_u32(RECORDING.checked_add(0x3544)?)?);
        let resource = crate::recording_capture_resources::helper_eb2958(context, handle)?;
        context.write_u32(resource, selection.0)?;
        let work = GuestAddress::new(context.read_u32(RECORDING.checked_add(0x3540)?)?);
        let handle = GuestAddress::new(context.read_u32(RECORDING.checked_add(0x3544)?)?);
        let resource = crate::recording_capture_resources::helper_eb2958(context, handle)?;
        let cached = ExposureSelection(context.read_u32(resource)?);
        let narrowed = helper_738d14(context, cached)?;
        context.write_u16(work.checked_add(0xea)?, narrowed)?;
        let settings = GuestAddress::new(context.read_u32(RECORDING.checked_add(0x353c)?)?);
        let work = GuestAddress::new(context.read_u32(RECORDING.checked_add(0x3540)?)?);
        let value = context.read_u16(work.checked_add(0xea)?)?;
        context.write_u16(settings.checked_add(0x124)?, value)?;
    }
    if crate::record_configuration::helper_69f8fc(context, 0x16890)? != 0 { show_exposure(context)?; }
    Ok(changed)
}

#[cfg(test)]
#[path = "capture_task3_exposure_tests.rs"]
mod tests;
