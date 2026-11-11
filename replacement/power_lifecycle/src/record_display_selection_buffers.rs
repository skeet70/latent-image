//! Source: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; id5 f473a4..f49138.
use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_display_selection_memory::Address;
use crate::record_configuration::helper_69f8fc as configuration;

fn debug_flags<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    configuration(context, 0x24c)
}

fn clear_record<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: Address, sentinel: bool) -> Result<(), MemoryError> {
    if sentinel {
        destination.set_half(context, 0)?;
        for offset in [2, 4, 6] { destination.offset(offset).set_half(context, 0x7fff)?; }
        destination.offset(8).set_word(context, 0x7fff)?;
        for offset in [12, 14] { destination.offset(offset).set_half(context, 0x7fff)?; }
        destination.offset(16).clear(context, ByteCount(0x5c), 0)?;
    } else {
        destination.clear(context, ByteCount(0x6c), 0)?;
    }
    Ok(())
}

pub fn helper_f47468<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    if debug_flags(context)? & 2 == 0 { return Ok(()); }
    Address(0x016c_2228).set_word(context, 0)?;
    let source = Address(Address(0x00f4_9358).word(context)?);
    for index in 0..301 {
        if debug_flags(context)? & 2 != 0 { clear_record(context, source.offset(index * 0x6c), true)?; }
    }
    source.offset(0x7efc).clear(context, ByteCount(0xbc), 0)
}

pub fn helper_f48e74<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress) -> Result<(), MemoryError> {
    if debug_flags(context)? & 2 == 0 { return Ok(()); }
    let destination = Address(destination.get());
    let index = Address(0x016c_2228).word(context)?;
    let source = Address(Address(0x00f4_9358).word(context)?);
    destination.offset(0x79f90).set_word(context, 0x5354_4346)?;
    destination.offset(0x79f94).set_word(context, index)?;
    destination.offset(0x79f98).copy_from(context, source, ByteCount(0x7efc))?;
    destination.offset(0x81e94).copy_from(context, source.offset(0x7efc), ByteCount(0xbc))?;
    destination.offset(0x81f50).set_word(context, 0x4c41_4346)?;
    if debug_flags(context)? & 2 != 0 {
        let index = Address(0x016c_2228).word(context)?;
        clear_record(context, destination.offset(0x79f98u32.wrapping_add(index.wrapping_mul(0x6c))), false)?;
    }
    Ok(())
}

pub fn helper_f48f1c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress) -> Result<(), MemoryError> {
    if debug_flags(context)? & 1 != 0 { Address(destination.get()).offset(0x6cd60).copy_from(context, Address(0x5ffc_6354), ByteCount(0xd230))?; }
    Ok(())
}

fn mark_primary<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: Address) -> Result<(), MemoryError> {
    destination.offset(0x84f74).set_word(context, 0x5354_4d44)?;
    destination.offset(0x9cecc).set_word(context, 0x4c41_4d44)
}

pub fn helper_f48fd8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress, enabled: u32) -> Result<(), MemoryError> {
    let destination = Address(destination.get());
    let initialized = Address(0x016c_2220);
    if initialized.word(context)? == 0 && enabled == 1 {
        crate::record_numeric_configuration::helper_fa89fc(context, destination.offset(0x84f74).guest())?;
        mark_primary(context, destination)?;
        initialized.set_word(context, 1)?;
    }
    if configuration(context, 0x5256)? != 0 {
        if enabled == 1 {
            destination.offset(0x851fc).copy_from(context, Address(0x5ffd_b9ec), ByteCount(0xbe68))?;
        } else {
            destination.offset(0x84f74).clear(context, ByteCount(0x17f5c), 0)?;
            mark_primary(context, destination)?;
            initialized.set_word(context, 0)?;
        }
    } else {
        destination.offset(0x851fc).clear(context, ByteCount(0xbe68), 0)?;
        destination.offset(0x851fc).clear(context, ByteCount(0x2c00), 0xff)?;
        destination.offset(0x87e14).clear(context, ByteCount(0x8400), 0xff)?;
        destination.offset(0x90214).clear(context, ByteCount(0x168), 0xff)?;
    }
    Ok(())
}

pub fn helper_f4929c<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>) -> Result<(), MemoryError> { Ok(()) }

pub fn helper_105386c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    Address(0x0170_6a34).set_word(context, 0)
}
