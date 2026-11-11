use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

pub struct StatisticsSource { pub address: GuestAddress }

pub fn helper_ffcfd0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, item: u32, report: Option<GuestAddress>, metadata: Option<GuestAddress>) -> Result<Option<StatisticsSource>, MemoryError> {
    if u32::from(context.read_u8(GuestAddress::new(0x016f8158))?) != item { return Ok(None); }
    let address = GuestAddress::new(context.read_u32(GuestAddress::new(0x016f815c))?);
    if let Some(report) = report { context.copy_bytes(report, GuestAddress::new(0x016f8160), ByteCount(152))?; }
    if let Some(metadata) = metadata { context.copy_bytes(metadata, GuestAddress::new(0x016f81f8), ByteCount(216))?; }
    Ok(Some(StatisticsSource { address }))
}

pub fn helper_ffde8c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    context.read_u32(GuestAddress::new(0x016f8154))
}

pub fn helper_feacfc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    helper_ffde8c(context)
}

pub fn helper_ef67f0<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> { Ok(0) }

pub fn helper_fead0c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    Ok(u32::from(helper_ef67f0(context)? == 1))
}

pub fn helper_ffd244<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, source: GuestAddress, destination: Option<GuestAddress>) -> Result<i32, MemoryError> {
    let Some(destination) = destination else { return Ok(0); };
    for channel in 0..4 {
        let source_offset = crate::record_hardware_deep_banks::statistics_offset(3 + channel);
        context.copy_bytes(destination.checked_add(channel * 1024)?, source.checked_add(source_offset)?, ByteCount(1024))?;
    }
    for channel in 0..4 { context.write_u16(destination.checked_add(4096 + channel * 4)?, 0)?; }
    for channel in 0..4 { context.write_u16(destination.checked_add(4098 + channel * 4)?, 0)?; }
    for channel in 0..4 {
        let source_offset = crate::record_hardware_deep_banks::statistics_offset(13 + channel);
        let total = context.read_u32(source.checked_add(source_offset)?)?;
        context.write_u32(destination.checked_add(4112 + channel * 4)?, total)?;
    }
    Ok(1)
}

pub fn helper_feacd4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, item: u32, report: GuestAddress) -> Result<u32, MemoryError> {
    Ok(u32::from(helper_ffcfd0(context, item, Some(report), None)?.is_some()))
}
