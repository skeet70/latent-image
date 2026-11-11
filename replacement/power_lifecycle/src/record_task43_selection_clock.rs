use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_configuration::helper_69f8fc as configuration;

#[derive(Clone, Copy)]
struct ClockSelector(u32);
#[derive(Clone, Copy)]
struct ClockRecord(GuestAddress);

fn word<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, address: u32) -> Result<u32, MemoryError> {
    context.read_u32(GuestAddress::new(address))
}
fn rounded_even_tenths(value: u32) -> u32 {
    let rounded = value / 10 + u32::from(value % 10 >= 5);
    rounded & !1
}
fn special_sensor<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<bool, MemoryError> {
    if word(context, 0x0091_34f8)? == 0 { return Ok(false); }
    if configuration(context, 0x80)? == 0x83 { return Ok(true); }
    if configuration(context, 0x80)? == 0x8a { return Ok(true); }
    Ok(configuration(context, 0x80)? == 0x8b)
}
fn sensor_rate<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: ClockRecord) -> Result<u32, MemoryError> {
    let high = configuration(context, 0xcb)?;
    let low = configuration(context, 0xca)?;
    let measured = (((high << 8) | low) as u16 as u32).wrapping_mul(1000);
    let maximum = context.read_u32(record.0.checked_add(16)?)?;
    Ok(measured.min(maximum).max(1000))
}

pub fn period<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, selector: u32) -> Result<Option<u32>, MemoryError> {
    let selector = ClockSelector(selector);
    let index = word(context, 0x0101_e014u32.wrapping_add(selector.0.wrapping_mul(8)))?;
    let kind = context.read_u8(GuestAddress::new(0x00ef_6decu32.wrapping_add(selector.0.wrapping_mul(20))))?;
    if !matches!(kind, 2 | 3 | 5) { return Ok(None); }
    let record = ClockRecord(GuestAddress::new(0x0101_de8cu32.wrapping_add(index.wrapping_mul(28))));
    let ticks = if special_sensor(context)? {
        let rate = sensor_rate(context, record)?;
        let maximum = context.read_u32(record.0.checked_add(16)?)?;
        let scale = context.read_u32(record.0.checked_add(4)?)?;
        rounded_even_tenths(maximum.wrapping_mul(scale.wrapping_mul(10)) / rate).wrapping_mul(8)
    } else if kind == 5 {
        context.read_u32(record.0.checked_add(4)?)?.wrapping_mul(8)
    } else {
        u32::from(context.read_u16(GuestAddress::new(0x016f_8f26))?).wrapping_mul(8)
    };
    let scale = context.read_u32(record.0.checked_add(8)?)?;
    Ok(Some(scale.wrapping_mul(ticks >> 3) / 54))
}

pub fn helper_1005f60<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, selector: u32, output: GuestAddress) -> Result<u32, MemoryError> {
    match period(context, selector)? {
        Some(value) => { context.write_u32(output, value)?; Ok(0) }
        None => Ok(133),
    }
}
