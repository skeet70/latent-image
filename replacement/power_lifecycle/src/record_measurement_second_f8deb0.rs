//! The second-order interpolation of one record's spans: image-5 body
//! 0x00f8_deb0.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

/// The configuration byte 0x00f8_deb0 compares the record's ratio against.
pub const RATIO_LIMIT: u32 = 0x927b;

fn read<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress, at: u32) -> Result<i64, MemoryError> {
    context.read_u32(record.checked_add(at)?).map(|value| i64::from(value as i32))
}

/// The image-5 body at 0x00f8_deb0.
pub fn helper_f8deb0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress) -> Result<i32, MemoryError> {
    let ratio = crate::record_measurement_gain_f8a390::helper_f8cae8(context, record)? as u32;
    let limit = crate::record_configuration::helper_69f8fc(context, RATIO_LIMIT)?;
    if ratio >= limit { return crate::record_measurement_interpolation_f8dd3c::helper_f8dd3c(context, record); }
    let b = read(context, record, crate::record_measurement_interpolation_f8dd3c::FIRST)?;
    let a = read(context, record, crate::record_measurement_interpolation_f8dd3c::MIDDLE)?;
    let c = read(context, record, crate::record_measurement_interpolation_f8dd3c::LAST)?;
    let x = read(context, record, crate::record_measurement_interpolation_f8dd3c::START)?;
    let y = read(context, record, crate::record_measurement_interpolation_f8dd3c::CENTRE)?;
    let z = read(context, record, crate::record_measurement_interpolation_f8dd3c::END)?;
    let leading = c.wrapping_sub(b);
    let middle = a.wrapping_sub(c);
    let trailing = b.wrapping_sub(a);
    let sum = x.wrapping_mul(leading).wrapping_add(y.wrapping_mul(middle)).wrapping_add(z.wrapping_mul(trailing));
    let doubled = sum.wrapping_mul(2);
    if doubled == 0 { return crate::record_measurement_interpolation_f8dd3c::helper_f8dd3c(context, record); }
    let squares = x.wrapping_mul(x).wrapping_mul(leading)
        .wrapping_add(y.wrapping_mul(y).wrapping_mul(middle))
        .wrapping_add(z.wrapping_mul(z).wrapping_mul(trailing));
    let quotient = squares.wrapping_mul(10).wrapping_div(doubled) as i32;
    let mut rounded = quotient;
    if quotient.unsigned_abs() % 10 > 4 {
        rounded = if quotient >= 0 { quotient.wrapping_add(10) } else { quotient.wrapping_sub(10) };
    }
    Ok(i32::from((rounded / 10) as u16 as i16))
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 body at 0x00f8_deb0.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const RECORD: u32 = 0x0010_0000;
    const CONFIG: u32 = 0x0010_2000;

    struct Memory {
        record: [u8; 0x40],
        pointer: [u8; 4],
        configuration: [u8; 0x1_0000],
    }

    fn with_context<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(RECORD, 0x40), &mut memory.record).unwrap(),
            MemoryRegion::new(descriptor(0x0096_ca50, 4), &mut memory.pointer).unwrap(),
            MemoryRegion::new(descriptor(CONFIG, 0x1_0000), &mut memory.configuration).unwrap(),
        ];
        body(&mut RecordContext::new(HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap(), ()))
    }

    fn put(record: &mut [u8; 0x40], at: u32, value: i32) { record[at as usize..at as usize + 4].copy_from_slice(&value.to_le_bytes()); }

    fn memory() -> Memory {
        let mut memory = Memory { record: [0; 0x40], pointer: [0; 4], configuration: [0; 0x1_0000] };
        memory.pointer.copy_from_slice(&CONFIG.to_le_bytes());
        memory
    }

    #[test]
    fn a_zero_second_difference_falls_back_to_the_first_order_body() {
        let mut memory = memory();
        memory.configuration[RATIO_LIMIT as usize] = 1;
        put(&mut memory.record, crate::record_measurement_interpolation_f8dd3c::FIRST, 0);
        put(&mut memory.record, crate::record_measurement_interpolation_f8dd3c::MIDDLE, 0);
        put(&mut memory.record, crate::record_measurement_interpolation_f8dd3c::LAST, 0);
        put(&mut memory.record, crate::record_measurement_interpolation_f8dd3c::START, 0);
        put(&mut memory.record, crate::record_measurement_interpolation_f8dd3c::CENTRE, 10);
        put(&mut memory.record, crate::record_measurement_interpolation_f8dd3c::END, 20);
        let expected = with_context(&mut memory, |context| crate::record_measurement_interpolation_f8dd3c::helper_f8dd3c(context, GuestAddress::new(RECORD)));
        with_context(&mut memory, |context| assert_eq!(helper_f8deb0(context, GuestAddress::new(RECORD)), expected));
    }

    #[test]
    fn a_ratio_at_the_limit_falls_back_to_the_first_order_body() {
        let mut memory = memory();
        memory.configuration[RATIO_LIMIT as usize] = 50;
        put(&mut memory.record, crate::record_measurement_interpolation_f8dd3c::FIRST, 0);
        put(&mut memory.record, crate::record_measurement_interpolation_f8dd3c::MIDDLE, 1000);
        put(&mut memory.record, crate::record_measurement_interpolation_f8dd3c::LAST, 0);
        put(&mut memory.record, crate::record_measurement_interpolation_f8dd3c::START, 0);
        put(&mut memory.record, crate::record_measurement_interpolation_f8dd3c::CENTRE, 10);
        put(&mut memory.record, crate::record_measurement_interpolation_f8dd3c::END, 20);
        let expected = with_context(&mut memory, |context| crate::record_measurement_interpolation_f8dd3c::helper_f8dd3c(context, GuestAddress::new(RECORD)));
        with_context(&mut memory, |context| assert_eq!(helper_f8deb0(context, GuestAddress::new(RECORD)), expected));
    }

    #[test]
    fn the_second_difference_is_ten_times_the_weighted_squares() {
        let mut memory = memory();
        memory.configuration[RATIO_LIMIT as usize] = 100;
        put(&mut memory.record, crate::record_measurement_interpolation_f8dd3c::FIRST, 0);
        put(&mut memory.record, crate::record_measurement_interpolation_f8dd3c::MIDDLE, 1000);
        put(&mut memory.record, crate::record_measurement_interpolation_f8dd3c::LAST, 0);
        put(&mut memory.record, crate::record_measurement_interpolation_f8dd3c::START, 0);
        put(&mut memory.record, crate::record_measurement_interpolation_f8dd3c::CENTRE, 10);
        put(&mut memory.record, crate::record_measurement_interpolation_f8dd3c::END, 20);
        with_context(&mut memory, |context| assert_eq!(helper_f8deb0(context, GuestAddress::new(RECORD)), Ok(15)));
    }
}
