//! The interpolation of one record's spans: image-5 body 0x00f8_dd3c.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

pub const FIRST: u32 = 0x08;
pub const MIDDLE: u32 = 0x0c;
pub const LAST: u32 = 0x10;
pub const START: u32 = 0x1c;
pub const CENTRE: u32 = 0x20;
pub const END: u32 = 0x24;

fn read<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress, at: u32) -> Result<i32, MemoryError> {
    context.read_u32(record.checked_add(at)?).map(|value| value as i32)
}

/// The image-5 body at 0x00f8_dd3c.
pub fn helper_f8dd3c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress) -> Result<i32, MemoryError> {
    let x = read(context, record, START)?;
    let y = read(context, record, CENTRE)?;
    let z = read(context, record, END)?;
    let a = read(context, record, MIDDLE)?;
    let b = read(context, record, FIRST)?;
    let c = read(context, record, LAST)?;
    let leading = (y.wrapping_sub(x).unsigned_abs() & 0xffff) as i32;
    let trailing = (z.wrapping_sub(y).unsigned_abs() & 0xffff) as i32;
    let first = a.wrapping_sub(b);
    let second = c.wrapping_sub(a);
    let cross = i64::from(first) * i64::from(trailing) - i64::from(leading) * i64::from(second);
    let slope = if first >= 0 {
        if cross <= 0 { second } else { first }
    } else if cross > 0 { second } else { first };
    let divisor = i64::from(slope) * 2;
    if divisor == 0 { return Ok(i32::from((y as u16) as i16)); }
    let value = (cross * 10 / divisor) as i32;
    let mut rounded = value;
    if value.unsigned_abs() % 10 > 4 {
        rounded = if value >= 0 { value.wrapping_add(10) } else { value.wrapping_sub(10) };
    }
    if x > z { rounded = rounded.wrapping_neg(); }
    Ok(i32::from((y.wrapping_add(rounded / 10) as u16) as i16))
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 body at 0x00f8_dd3c.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const RECORD: u32 = 0x0010_0000;

    fn with_record<R>(record: &mut [u8; 0x40], body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = RegionDescriptor::new(GuestAddress::new(RECORD), ByteCount(0x40), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [MemoryRegion::new(descriptor, record).unwrap()];
        body(&mut RecordContext::new(HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap(), ()))
    }

    fn put(record: &mut [u8; 0x40], at: u32, value: i32) { record[at as usize..at as usize + 4].copy_from_slice(&value.to_le_bytes()); }

    #[test]
    fn the_centre_answers_unchanged_when_the_slope_cancels() {
        let mut record = [0u8; 0x40];
        put(&mut record, START, 0);
        put(&mut record, CENTRE, 100);
        put(&mut record, END, 200);
        put(&mut record, FIRST, 0);
        put(&mut record, MIDDLE, 0);
        put(&mut record, LAST, 0);
        with_record(&mut record, |context| assert_eq!(helper_f8dd3c(context, GuestAddress::new(RECORD)), Ok(100)));
    }

    #[test]
    fn the_cross_product_selects_the_first_slope() {
        let mut record = [0u8; 0x40];
        put(&mut record, START, 0);
        put(&mut record, CENTRE, 100);
        put(&mut record, END, 200);
        put(&mut record, FIRST, 0);
        put(&mut record, MIDDLE, 1000);
        put(&mut record, LAST, 2000);
        with_record(&mut record, |context| assert_eq!(helper_f8dd3c(context, GuestAddress::new(RECORD)), Ok(100)));
    }

    #[test]
    fn the_cross_product_selects_the_second_slope() {
        let mut record = [0u8; 0x40];
        put(&mut record, START, 0);
        put(&mut record, CENTRE, 100);
        put(&mut record, END, 200);
        put(&mut record, FIRST, 1000);
        put(&mut record, MIDDLE, 0);
        put(&mut record, LAST, 1000);
        with_record(&mut record, |context| assert_eq!(helper_f8dd3c(context, GuestAddress::new(RECORD)), Ok(200)));
    }

    #[test]
    fn a_reversed_record_subtracts_the_scaled_cross_product() {
        let mut record = [0u8; 0x40];
        put(&mut record, START, 200);
        put(&mut record, CENTRE, 100);
        put(&mut record, END, 0);
        put(&mut record, FIRST, 0);
        put(&mut record, MIDDLE, 1000);
        put(&mut record, LAST, 2000);
        with_record(&mut record, |context| assert_eq!(helper_f8dd3c(context, GuestAddress::new(RECORD)), Ok(100)));
    }
}
