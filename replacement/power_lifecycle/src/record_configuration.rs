use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct ConfigurationKey(pub u32);

pub fn read_byte<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, key: ConfigurationKey) -> Result<u8, MemoryError> {
    let base = context.read_u32(GuestAddress::new(0x0096_ca50))?;
    context.read_u8(GuestAddress::new(base.wrapping_add(key.0)))
}

pub fn read_halfword<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, key: ConfigurationKey) -> Result<u16, MemoryError> {
    let low = read_byte(context, key)?;
    let high = read_byte(context, ConfigurationKey(key.0.wrapping_add(1)))?;
    Ok(u16::from_le_bytes([low, high]))
}

pub fn helper_69f8fc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, key: u32) -> Result<u32, MemoryError> {
    read_byte(context, ConfigurationKey(key)).map(u32::from)
}

pub fn helper_69fec8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, key: u32) -> Result<u32, MemoryError> {
    read_halfword(context, ConfigurationKey(key)).map(u32::from)
}

pub fn helper_69fef0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, key: u32) -> Result<u32, MemoryError> {
    let low = u32::from(read_byte(context, ConfigurationKey(key))?);
    let middle = u32::from(read_byte(context, ConfigurationKey(key.wrapping_add(1)))?);
    let high = u32::from(read_byte(context, ConfigurationKey(key.wrapping_add(2)))?);
    Ok(low | middle << 8 | high << 16)
}

pub fn helper_69ff68<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, key: u32) -> Result<i32, MemoryError> {
    read_byte(context, ConfigurationKey(key)).map(|value| i32::from(value as i8))
}

pub fn helper_69ff7c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, key: u32) -> Result<i32, MemoryError> {
    read_halfword(context, ConfigurationKey(key)).map(|value| i32::from(value as i16))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    #[test]
    fn configuration_reads_follow_published_pointer_and_byte_order() {
        let mut pointer = 0x1000u32.to_le_bytes();
        let mut first = [0xfe, 0x81, 0x72, 0x63];
        let mut second = [0x14, 0x25, 0x36, 0x47];
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(0x0096_ca50, 4), &mut pointer).unwrap(),
            MemoryRegion::new(descriptor(0x1000, 4), &mut first).unwrap(),
            MemoryRegion::new(descriptor(0x2000, 4), &mut second).unwrap(),
        ];
        let memory = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let mut context = RecordContext::new(memory, ());
        assert_eq!(helper_69f8fc(&mut context, 0), Ok(254));
        assert_eq!(helper_69fec8(&mut context, 0), Ok(0x81fe));
        assert_eq!(helper_69fef0(&mut context, 0), Ok(0x7281fe));
        assert_eq!(helper_69ff68(&mut context, 0), Ok(-2));
        assert_eq!(helper_69ff7c(&mut context, 0), Ok(-32258));
        context.write_u32(GuestAddress::new(0x0096_ca50), 0x2000).unwrap();
        assert_eq!(helper_69fef0(&mut context, 0), Ok(0x362514));
    }
}
