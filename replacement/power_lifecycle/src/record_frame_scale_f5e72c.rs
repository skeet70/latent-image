//! Stock-shaped body at 0x00f5eb34.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_numeric::helper_fa6590;

/// 0x00f5eb34
pub fn helper_f5eb34<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>, total: GuestAddress, tenth: GuestAddress, double: GuestAddress, copy: GuestAddress, first: u32, second: u32,
) -> Result<(), MemoryError> {
    if total.get() == 0 || tenth.get() == 0 { return Ok(()); }
    if double.get() == 0 || copy.get() == 0 { return Ok(()); }
    let value = helper_fa6590(context, first.wrapping_mul(second))?;
    context.write_u32(total, value)?;
    let tenth_value = value / 10;
    context.write_u32(tenth, tenth_value)?;
    context.write_u32(double, tenth_value.wrapping_mul(2))?;
    context.write_u32(copy, tenth_value)
}

use crate::record_native_exports::stock_export;

stock_export!(
    xm1_power_record_helper_f5eb34,
    helper_f5eb34_abi,
    (),
    (total: u32, tenth: u32, double: u32, copy: u32, first: u32, second: u32) -> (),
    (total: GuestAddress, tenth: GuestAddress, double: GuestAddress, copy: GuestAddress, first: u32, second: u32),
    (GuestAddress::new(total), GuestAddress::new(tenth), GuestAddress::new(double), GuestAddress::new(copy), first, second),
    crate::record_frame_scale_f5e72c::helper_f5eb34,
    |_| ()
);

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 body at 0x00f5eb34 and the
    //! configuration bytes it multiplies.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const OUTPUTS: u32 = 0x0020_0000;
    const CONFIGURATION: u32 = 0x0010_0000;
    const CONFIGURATION_POINTER: u32 = 0x0096_ca50;

    fn with_memory<R>(outputs: &mut [u8], configuration: &mut [u8], pointer: &mut [u8], body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(OUTPUTS, outputs.len() as u32), outputs).unwrap(),
            MemoryRegion::new(descriptor(CONFIGURATION, configuration.len() as u32), configuration).unwrap(),
            MemoryRegion::new(descriptor(CONFIGURATION_POINTER, 4), pointer).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    fn word(bytes: &[u8], index: usize) -> u32 { u32::from_le_bytes(bytes[index * 4..index * 4 + 4].try_into().unwrap()) }

    #[test]
    fn the_three_outputs_are_the_total_its_tenth_and_twice_the_tenth() {
        let mut outputs = [0u8; 16];
        let mut configuration = [0u8; 0x6400];
        configuration[0x6397] = 0x40;
        configuration[0x6398] = 0x02;
        configuration[0x6399] = 0x01;
        let mut pointer = CONFIGURATION.to_le_bytes();
        with_memory(&mut outputs, &mut configuration, &mut pointer, |context| {
            assert_eq!(helper_f5eb34(context, GuestAddress::new(OUTPUTS), GuestAddress::new(OUTPUTS + 4), GuestAddress::new(OUTPUTS + 8), GuestAddress::new(OUTPUTS + 12), 40, 25), Ok(()));
        });
        assert_eq!([word(&outputs, 0), word(&outputs, 1), word(&outputs, 2), word(&outputs, 3)], [66_112_000, 6_611_200, 13_222_400, 6_611_200]);
    }

    #[test]
    fn a_missing_output_pointer_leaves_every_output_alone() {
        let mut outputs = [0x5au8; 16];
        let mut configuration = [0u8; 0x6400];
        configuration[0x6397] = 0x40;
        let mut pointer = CONFIGURATION.to_le_bytes();
        with_memory(&mut outputs, &mut configuration, &mut pointer, |context| {
            assert_eq!(helper_f5eb34(context, GuestAddress::new(0), GuestAddress::new(OUTPUTS + 4), GuestAddress::new(OUTPUTS + 8), GuestAddress::new(OUTPUTS + 12), 40, 25), Ok(()));
            assert_eq!(helper_f5eb34(context, GuestAddress::new(OUTPUTS), GuestAddress::new(OUTPUTS + 4), GuestAddress::new(0), GuestAddress::new(OUTPUTS + 12), 40, 25), Ok(()));
            assert_eq!(helper_f5eb34(context, GuestAddress::new(OUTPUTS), GuestAddress::new(0), GuestAddress::new(OUTPUTS + 8), GuestAddress::new(OUTPUTS + 12), 40, 25), Ok(()));
            assert_eq!(helper_f5eb34(context, GuestAddress::new(OUTPUTS), GuestAddress::new(OUTPUTS + 4), GuestAddress::new(OUTPUTS + 8), GuestAddress::new(0), 40, 25), Ok(()));
        });
        assert_eq!(outputs, [0x5a; 16]);
    }

    #[test]
    fn a_zero_factor_still_publishes_the_zero_total() {
        let mut outputs = [0x5au8; 16];
        let mut configuration = [0u8; 0x6400];
        let mut pointer = CONFIGURATION.to_le_bytes();
        with_memory(&mut outputs, &mut configuration, &mut pointer, |context| {
            assert_eq!(helper_f5eb34(context, GuestAddress::new(OUTPUTS), GuestAddress::new(OUTPUTS + 4), GuestAddress::new(OUTPUTS + 8), GuestAddress::new(OUTPUTS + 12), 0, 25), Ok(()));
        });
        assert_eq!(outputs, [0; 16]);
    }
}
