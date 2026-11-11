//! Image-5 rectangle scaling helper.
use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_native_exports::stock_export;

const X: u32 = 0x0;
const Y: u32 = 0x4;
const WIDTH: u32 = 0x8;
const HEIGHT: u32 = 0xc;
const HORIZONTAL: u32 = 0x0;
const VERTICAL: u32 = 0x4;

/// `0xd1b7_1759 >> 45` is 1/10000.
fn divide_by_10000(product: u32) -> u32 {
    ((u64::from(product) * 0xd1b7_1759) >> 45) as u32
}

/// The dimensions pair is in hundredths: 10000 leaves a side unchanged.
pub fn helper_ee81bc<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>,
    source: GuestAddress,
    dimensions: GuestAddress,
    destination: GuestAddress,
) -> Result<(), MemoryError> {
    let height = context.read_u32(source.checked_add(HEIGHT)?)?;
    let vertical = context.read_u32(dimensions.checked_add(VERTICAL)?)?;
    let scaled_height = divide_by_10000(height.wrapping_mul(vertical));
    context.write_u32(destination.checked_add(HEIGHT)?, scaled_height)?;
    let horizontal = context.read_u32(dimensions.checked_add(HORIZONTAL)?)?;
    let width = context.read_u32(source.checked_add(WIDTH)?)?;
    let scaled_width = divide_by_10000(width.wrapping_mul(horizontal));
    context.write_u32(destination.checked_add(WIDTH)?, scaled_width)?;

    let height = context.read_u32(source.checked_add(HEIGHT)?)?;
    let offset = height.wrapping_sub(scaled_height) >> 1;
    context.write_u32(destination.checked_add(X)?, offset)?;
    let x = context.read_u32(source.checked_add(X)?)?;
    context.write_u32(destination.checked_add(X)?, offset.wrapping_add(x))?;

    let width = context.read_u32(source.checked_add(WIDTH)?)?;
    let offset = width.wrapping_sub(scaled_width) >> 1;
    context.write_u32(destination.checked_add(Y)?, offset)?;
    let y = context.read_u32(source.checked_add(Y)?)?;
    context.write_u32(destination.checked_add(Y)?, offset.wrapping_add(y))?;
    Ok(())
}

stock_export!(
    xm1_power_record_helper_ee81bc,
    helper_ee81bc_abi,
    (),
    (input: u32, dimensions: u32, output: u32) -> (),
    (input: GuestAddress, dimensions: GuestAddress, output: GuestAddress),
    (GuestAddress::new(input), GuestAddress::new(dimensions), GuestAddress::new(output)),
    crate::record_helper_rectangle_ee81bc::helper_ee81bc,
    |_| ()
);

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 body at 0x00ee_81bc.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const SOURCE: u32 = 0x0010_0000;
    const DIMENSIONS: u32 = 0x0010_1000;
    const DESTINATION: u32 = 0x0010_2000;

    struct Memory {
        source: [u8; 16],
        dimensions: [u8; 8],
        destination: [u8; 16],
    }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(SOURCE, 16), &mut memory.source).unwrap(),
            MemoryRegion::new(descriptor(DIMENSIONS, 8), &mut memory.dimensions).unwrap(),
            MemoryRegion::new(descriptor(DESTINATION, 16), &mut memory.destination).unwrap(),
        ];
        body(&mut RecordContext::new(HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap(), ()))
    }

    fn words(bytes: &[u8], count: usize) -> Vec<u32> {
        (0..count).map(|index| u32::from_le_bytes(bytes[index * 4..index * 4 + 4].try_into().unwrap())).collect()
    }

    fn set(bytes: &mut [u8], values: &[u32]) {
        for (index, value) in values.iter().enumerate() { bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes()); }
    }

    #[test]
    fn scales_and_centres_the_rectangle() {
        let mut memory = Memory { source: [0; 16], dimensions: [0; 8], destination: [0; 16] };
        set(&mut memory.source, &[10, 20, 1000, 500]);
        set(&mut memory.dimensions, &[5000, 5000]);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_ee81bc(context, GuestAddress::new(SOURCE), GuestAddress::new(DIMENSIONS), GuestAddress::new(DESTINATION)), Ok(()));
        });
        assert_eq!(words(&memory.destination, 4), vec![135, 270, 500, 250]);
    }

    #[test]
    fn a_scaled_size_above_the_original_wraps_before_halving() {
        let mut memory = Memory { source: [0; 16], dimensions: [0; 8], destination: [0; 16] };
        set(&mut memory.source, &[0, 0, 1, 1]);
        set(&mut memory.dimensions, &[20000, 20000]);
        with_memory(&mut memory, |context| {
            assert_eq!(helper_ee81bc(context, GuestAddress::new(SOURCE), GuestAddress::new(DIMENSIONS), GuestAddress::new(DESTINATION)), Ok(()));
        });
        assert_eq!(words(&memory.destination, 4), vec![0x7fff_ffff, 0x7fff_ffff, 2, 2]);
    }
}
