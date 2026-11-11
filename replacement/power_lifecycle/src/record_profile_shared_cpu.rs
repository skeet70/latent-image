//! X-M1 1.50 shared profile access, installed image-4 offsets.

use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_profile_accessors::{can_write, copy_words, read_bank, write_bank, resource_address, ProfileField, ProfileHandle};

const SHARED: u32 = 0x0091_904c;
#[derive(Clone, Copy)]
#[repr(transparent)]
struct ConfigurationIndex(u32);

fn address(base: u32, offset: u32) -> GuestAddress { GuestAddress::new(base.wrapping_add(offset)) }

fn resource<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<GuestAddress, MemoryError> {
    let bank = read_bank(context, ProfileHandle(handle), ProfileField(30))?;
    Ok(resource_address(bank))
}

fn writable<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<Option<GuestAddress>, MemoryError> {
    Ok(write_bank(context, ProfileHandle(handle), ProfileField(30))?.map(resource_address))
}


pub fn helper_7560b8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, value: u32) -> Result<(), MemoryError> {
    let base = resource(context, handle)?;
    context.write_u32(address(base.get(), 0x20a4), value)
}
pub fn helper_756158<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, value: u32) -> Result<(), MemoryError> {
    let base = resource(context, handle)?;
    context.write_u32(address(base.get(), 0x20a8), value)
}
pub fn helper_75624c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<u32, MemoryError> {
    let base = resource(context, handle)?;
    context.read_u32(address(base.get(), 0x7a0))
}
pub fn helper_756298<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, count: u32) -> Result<(), MemoryError> {
    if let Some(base) = writable(context, handle)? { context.write_u32(address(base.get(), 0x7a0), count)?; }
    Ok(())
}
fn configuration(base: GuestAddress, index: ConfigurationIndex) -> GuestAddress {
    address(base.get(), 0x7a4u32.wrapping_add(index.0.wrapping_mul(0x68)))
}
pub fn helper_7562e4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, index: u32) -> Result<GuestAddress, MemoryError> {
    Ok(configuration(resource(context, handle)?, ConfigurationIndex(index)))
}
pub fn helper_756338<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, _unused: u32, index: u32, source: GuestAddress) -> Result<(), MemoryError> {
    if let Some(base) = writable(context, handle)? { context.copy_bytes(configuration(base, ConfigurationIndex(index)), source, ByteCount(0x68))?; }
    Ok(())
}

macro_rules! configuration_word {
    ($get:ident, $set:ident, $offset:expr) => {
        pub fn $get<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<u32, MemoryError> {
            let base = resource(context, handle)?;
            context.read_u32(address(base.get(), $offset))
        }
        pub fn $set<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, value: u32) -> Result<(), MemoryError> {
            if let Some(base) = writable(context, handle)? { context.write_u32(address(base.get(), $offset), value)?; }
            Ok(())
        }
    };
}
configuration_word!(helper_7563d4, helper_75638c, 0x17f8);
configuration_word!(helper_7564c0, helper_756508, 0x1878);
macro_rules! configuration_half {
    ($get:ident, $set:ident, $offset:expr) => {
        pub fn $get<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<u32, MemoryError> {
            let base = resource(context, handle)?;
            Ok(context.read_u16(address(base.get(), $offset))? as i16 as i32 as u32)
        }
        pub fn $set<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, value: u32) -> Result<(), MemoryError> {
            if let Some(base) = writable(context, handle)? { context.write_u16(address(base.get(), $offset), value as u16)?; }
            Ok(())
        }
    };
}
configuration_half!(helper_756550, helper_756598, 0x17f0);
configuration_half!(helper_7565e0, helper_756628, 0x17f2);
configuration_half!(helper_756670, helper_7566b8, 0x17f4);
pub fn helper_75641c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, destination: GuestAddress) -> Result<GuestAddress, MemoryError> {
    let base = resource(context, handle)?;
    context.copy_bytes(destination, address(base.get(), 0x17fc), ByteCount(0x68))?;
    Ok(destination)
}
pub fn helper_756478<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, source: GuestAddress) -> Result<(), MemoryError> {
    if let Some(base) = writable(context, handle)? { context.copy_bytes(address(base.get(), 0x17fc), source, ByteCount(0x68))?; }
    Ok(())
}

macro_rules! shared_getter {
    ($name:ident, $offset:expr, $words:expr, handle) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, _handle: u32, destination: GuestAddress) -> Result<(), MemoryError> {
            copy_words::<$words>(context, destination, address(SHARED, $offset))
        }
    };
    ($name:ident, $offset:expr, $words:expr) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress) -> Result<GuestAddress, MemoryError> {
            copy_words::<$words>(context, destination, address(SHARED, $offset))?;
            Ok(destination)
        }
    };
}
pub fn helper_756734<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, _handle: u32, destination: GuestAddress) -> Result<GuestAddress, MemoryError> {
    context.copy_bytes(destination, address(SHARED, 0), ByteCount(0x84))?;
    Ok(destination)
}
shared_getter!(helper_756788, 0x84, 8, handle);
shared_getter!(helper_7567ec, 0xa4, 3, handle);
shared_getter!(helper_756840, 0xb0, 12, handle);
shared_getter!(helper_7568b8, 0xe0, 16, handle);
shared_getter!(helper_756e24, 0x180, 5);
pub fn helper_756f3c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress) -> Result<GuestAddress, MemoryError> {
    context.copy_bytes(destination, address(SHARED, 0x1ac), ByteCount(0x50))?;
    Ok(destination)
}
shared_getter!(helper_7582c0, 0x7ec, 10);
pub fn helper_7588e8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress) -> Result<GuestAddress, MemoryError> {
    context.copy_bytes(destination, address(SHARED, 0x7a0), ByteCount(0x4c))?;
    Ok(destination)
}
shared_getter!(helper_758c68, 0x840, 7, handle);
shared_getter!(helper_758c98, 0x85c, 2);

macro_rules! shared_setter {
    ($name:ident, $field:expr, $offset:expr, $words:expr) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, source: GuestAddress) -> Result<(), MemoryError> {
            if can_write(context, ProfileHandle(handle), ProfileField($field))? { copy_words::<$words>(context, address(SHARED, $offset), source)?; }
            Ok(())
        }
    };
}
shared_setter!(helper_756878, 17, 0xb0, 12);
shared_setter!(helper_7568f8, 18, 0xe0, 16);
shared_setter!(helper_758300, 27, 0x7ec, 10);
shared_setter!(helper_7589e0, 35, 0x83c, 1);
shared_setter!(helper_758c34, 36, 0x840, 7);
shared_setter!(helper_758cc4, 37, 0x85c, 2);

pub fn helper_756754<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, source: GuestAddress) -> Result<u32, MemoryError> {
    if !can_write(context, ProfileHandle(handle), ProfileField(14))? { return Ok(0); }
    context.copy_bytes(address(SHARED, 0), source, ByteCount(0x84))?;
    Ok(SHARED)
}
pub fn helper_756f60<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, source: GuestAddress) -> Result<u32, MemoryError> {
    if !can_write(context, ProfileHandle(handle), ProfileField(25))? { return Ok(0); }
    context.copy_bytes(address(SHARED, 0x1ac), source, ByteCount(0x50))?;
    Ok(SHARED + 0x1ac)
}
pub fn helper_7567b8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, source: GuestAddress) -> Result<u32, MemoryError> {
    if !can_write(context, ProfileHandle(handle), ProfileField(15))? { return Ok(0); }
    let destination = address(SHARED, 0x84);
    copy_words::<4>(context, destination, source)?;
    let second = [context.read_u32(address(source.get(), 16))?, context.read_u32(address(source.get(), 20))?, context.read_u32(address(source.get(), 24))?, context.read_u32(address(source.get(), 28))?];
    for (index, value) in second.into_iter().enumerate() { context.write_u32(address(destination.get(), 16 + index as u32 * 4), value)?; }
    Ok(second[0])
}
pub fn helper_756e5c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, source: GuestAddress) -> Result<u32, MemoryError> {
    if !can_write(context, ProfileHandle(handle), ProfileField(22))? { return Ok(0); }
    let first = [context.read_u32(source)?, context.read_u32(address(source.get(), 4))?, context.read_u32(address(source.get(), 8))?, context.read_u32(address(source.get(), 12))?];
    for (index, value) in first.into_iter().enumerate() { context.write_u32(address(SHARED, 0x180 + index as u32 * 4), value)?; }
    let last = context.read_u32(address(source.get(), 16))?;
    context.write_u32(address(SHARED, 0x190), last)?;
    Ok(first[0])
}

macro_rules! shared_scalar {
    ($name:ident, $field:expr, $offset:expr, $write:ident, $ty:ty) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, value: u32) -> Result<(), MemoryError> {
            if can_write(context, ProfileHandle(handle), ProfileField($field))? { context.$write(address(SHARED, $offset), value as $ty)?; }
            Ok(())
        }
    };
}
shared_scalar!(helper_756fb8, 26, 0x1fc, write_u16, u16);
shared_scalar!(helper_757004, 26, 0x200, write_u32, u32);
shared_scalar!(helper_757050, 26, 0x206, write_u16, u16);
shared_scalar!(helper_7570a0, 26, 0x208, write_u16, u16);
shared_scalar!(helper_7570f0, 26, 0x204, write_u16, u16);
shared_scalar!(helper_757140, 26, 0x20c, write_u16, u16);
shared_scalar!(helper_757190, 26, 0x212, write_u16, u16);
shared_scalar!(helper_7571e0, 26, 0x216, write_u16, u16);
shared_scalar!(helper_75722c, 26, 0x218, write_u32, u32);
shared_scalar!(helper_757274, 26, 0x230, write_u32, u32);
shared_scalar!(helper_7572c0, 26, 0x238, write_u16, u16);
shared_scalar!(helper_757310, 26, 0x23a, write_u16, u16);
shared_scalar!(helper_757360, 26, 0x234, write_u16, u16);
shared_scalar!(helper_7573ac, 26, 0x236, write_u8, u8);
shared_scalar!(helper_7573f4, 26, 0x23c, write_u32, u32);
shared_scalar!(helper_758208, 27, 0x742, write_u8, u8);
shared_scalar!(helper_758250, 27, 0x744, write_u32, u32);
shared_scalar!(helper_758360, 14, 0x890, write_u32, u32);

pub fn helper_7585dc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, source: GuestAddress) -> Result<(), MemoryError> {
    if let Some(bank) = write_bank(context, ProfileHandle(handle), ProfileField(32))? {
        copy_words::<5>(context, address(resource_address(bank).get(), 0x2140), source)?;
    }
    Ok(())
}

macro_rules! configuration_getter {
    ($name:ident, $offset:expr) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<u32, MemoryError> {
            let base = resource(context, handle)?;
            context.read_u32(address(base.get(), $offset))
        }
    };
}
configuration_getter!(helper_75606c, 0x20a4);
configuration_getter!(helper_75610c, 0x20a8);
configuration_getter!(helper_7561ac, 0x20ac);

shared_scalar!(helper_7569c4, 20, 0x13c, write_u32, u32);
shared_scalar!(helper_756ae4, 20, 0x14c, write_u32, u32);
shared_scalar!(helper_756b74, 20, 0x154, write_u32, u32);
shared_scalar!(helper_756bbc, 20, 0x158, write_u32, u32);
shared_scalar!(helper_756c04, 20, 0x15c, write_u32, u32);
shared_scalar!(helper_756c4c, 20, 0x160, write_u32, u32);
shared_scalar!(helper_756c94, 20, 0x164, write_u32, u32);
shared_scalar!(helper_756d24, 20, 0x16c, write_u32, u32);
shared_scalar!(helper_756d6c, 20, 0x170, write_u32, u32);
shared_scalar!(helper_756db4, 20, 0x174, write_u32, u32);
shared_setter!(helper_756814, 16, 0xa4, 3);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_address_space::{GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    #[test]
    fn snapshot_fault_preserves_completed_output_groups() {
        let mut source = [0x31; 24];
        let mut destination = [0xa7; 48];
        let source_base = address(SHARED, 0xb0);
        let destination_base = GuestAddress::new(0x1000);
        let descriptor = |base, length| RegionDescriptor::new(base, ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(source_base, 24), &mut source).unwrap(),
            MemoryRegion::new(descriptor(destination_base, 48), &mut destination).unwrap(),
        ];
        let memory = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let mut context = RecordContext::new(memory, ());
        assert_eq!(helper_756840(&mut context, u32::MAX, destination_base), Err(MemoryError::Unmapped(address(source_base.get(), 24))));
        for index in 0..48 {
            assert_eq!(context.read_u8(address(destination_base.get(), index)), Ok(if index < 16 { 0x31 } else { 0xa7 }));
        }
    }

    #[test]
    fn shared_snapshot_is_read_back_without_handle_permission() {
        let mut source = [0; 40];
        source[0..4].copy_from_slice(&0x1234_5678u32.to_le_bytes());
        source[36..40].copy_from_slice(&0xfedc_ba98u32.to_le_bytes());
        let mut destination = [0xa7; 40];
        let descriptor = |base, length| RegionDescriptor::new(base, ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(address(SHARED, 0x7ec), 40), &mut source).unwrap(),
            MemoryRegion::new(descriptor(GuestAddress::new(0x1000), 40), &mut destination).unwrap(),
        ];
        let memory = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let mut context = RecordContext::new(memory, ());
        assert_eq!(helper_7582c0(&mut context, GuestAddress::new(0x1000)), Ok(GuestAddress::new(0x1000)));
        assert_eq!(context.read_u32(GuestAddress::new(0x1000)), Ok(0x1234_5678));
        assert_eq!(context.read_u32(GuestAddress::new(0x1024)), Ok(0xfedc_ba98));
    }
}
