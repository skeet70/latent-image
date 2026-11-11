//! Exposure measurement arithmetic: image-5 bodies 0x00ee_97f0 and
//! 0x00f8_a390-0x00f8_cbd0.

use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_numeric::helper_741fa0;

/// The hundred log2(mantissa) bytes the image-5 body at 0x00ee_97f0 reads at
/// 0x00ee_9d60.
const LOG2_TABLE: [u8; 100] = [
    0, 1, 3, 4, 6, 7, 8, 10, 11, 12, 14, 15, 16, 18, 19, 20, 21, 23, 24, 25,
    26, 28, 29, 30, 31, 32, 33, 34, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 48,
    49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67,
    68, 69, 70, 70, 71, 72, 73, 74, 75, 76, 77, 77, 78, 79, 80, 81, 82, 82, 83, 84,
    85, 86, 86, 87, 88, 89, 90, 90, 91, 92, 93, 93, 94, 95, 96, 96, 97, 98, 99, 99,
];
pub const ZERO_ANSWER: u32 = 132;
pub const SMALL_ANSWER: i32 = 100;

/// The image-5 body at 0x00ee_97f0: a hundred times the base-two logarithm of
/// `value`, stored through `output`.
pub fn helper_ee97f0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u64, output: GuestAddress) -> Result<u32, MemoryError> {
    if value == 0 { return Ok(ZERO_ANSWER); }
    let mut bit = 0u32;
    if value > 1 {
        while (value >> bit) > 1 { bit += 1; }
    }
    let product = value.wrapping_mul(100);
    let quotient = product / (1u64 << bit);
    let index = usize::try_from(quotient.wrapping_sub(100)).unwrap_or(0).min(LOG2_TABLE.len() - 1);
    context.write_u32(output, bit * 100 + u32::from(LOG2_TABLE[index]))?;
    Ok(0)
}

/// The image-5 body at 0x00f8_a390: copies eleven words to `destination` and
/// scales the five words at 0x18-0x28 in place.
pub fn helper_f8a390<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, source: GuestAddress, destination: GuestAddress, mode: u32) -> Result<i32, MemoryError> {
    for index in 0..11u32 {
        let value = context.read_u32(source.checked_add(index * 4)?)?;
        context.write_u32(destination.checked_add(index * 4)?, value)?;
    }
    let (start, samples) = match mode {
        0 | 1 => return Ok(SMALL_ANSWER),
        4 => {
            let first = context.read_u32(destination.checked_add(8)?)?;
            let second = context.read_u32(destination.checked_add(0x10)?)?;
            if first > second { (0, 3) } else { (1, 4) }
        }
        other => (1, other),
    };
    let mut step = 0u32;
    for index in start..samples {
        let low = context.read_u32(destination.checked_add(0x18 + index * 4)?)? as i32;
        let high = context.read_u32(destination.checked_add(0x1c + index * 4)?)? as i32;
        let difference = high.wrapping_sub(low).unsigned_abs();
        step = step.max(difference);
    }
    if step <= 500 { return Ok(SMALL_ANSWER); }
    let factor = 50_000u32.wrapping_div(step) as i32;
    for index in 0..5u32 {
        let value = context.read_u32(destination.checked_add(0x18 + index * 4)?)? as i32;
        context.write_u32(destination.checked_add(0x18 + index * 4)?, value.wrapping_mul(factor).wrapping_div(100) as u32)?;
    }
    Ok(factor)
}

/// The image-5 body at 0x00f8_cae8: the rounded ratio of the record's two
/// spans.
pub fn helper_f8cae8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress) -> Result<i32, MemoryError> {
    let mut field = [0i32; 10];
    for (index, value) in field.iter_mut().enumerate() {
        *value = context.read_u32(record.checked_add(index as u32 * 4)?)? as i32;
    }
    let (b, a, c, x, y, z) = (field[2], field[3], field[4], field[7], field[8], field[9]);
    if y == x || y == z || a == 0 { return Ok(0); }
    let first = divide(a.wrapping_sub(b), y.wrapping_sub(x));
    let second = divide(c.wrapping_sub(a), z.wrapping_sub(y));
    let value = divide(first.wrapping_sub(second).wrapping_mul(5).wrapping_mul(10), z.wrapping_sub(x));
    let quotient = (i64::from(value) * 100 * i64::from(y.wrapping_sub(z).unsigned_abs())) / i64::from(a);
    Ok(helper_741fa0(context, quotient as i32)?.unsigned_abs() as i32)
}

/// The image-5 body at 0x00f8_cbd0.
pub fn helper_f8cbd0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress) -> Result<u32, MemoryError> {
    let value = helper_f8cae8(context, record)? as u32;
    let limit = crate::record_configuration::helper_69f8fc(context, 0x927c)?;
    Ok(u32::from(value >= limit))
}

fn divide(dividend: i32, divisor: i32) -> i32 {
    if divisor == 0 { 0 } else { dividend.wrapping_div(divisor) }
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 bodies at 0x00ee_97f0-0x00f8_cbd0.
    use super::*;
    use crate::record_address_space::{GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    struct Memory {
        record: [u8; 0x40],
        other: [u8; 0x40],
        pointer: [u8; 4],
        configuration: [u8; 0x1_0000],
    }

    fn with_context<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(0x0010_0000, 0x40), &mut memory.record).unwrap(),
            MemoryRegion::new(descriptor(0x0010_1000, 0x40), &mut memory.other).unwrap(),
            MemoryRegion::new(descriptor(0x0096_ca50, 4), &mut memory.pointer).unwrap(),
            MemoryRegion::new(descriptor(0x0010_2000, 0x1_0000), &mut memory.configuration).unwrap(),
        ];
        body(&mut RecordContext::new(HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap(), ()))
    }

    #[test]
    fn the_logarithm_answers_hundredths_and_the_zero_code() {
        let mut memory = Memory { record: [0; 0x40], other: [0; 0x40], pointer: [0; 4], configuration: [0; 0x1_0000] };
        let output = GuestAddress::new(0x0010_1000);
        let cases = [(0u64, ZERO_ANSWER, 0u32), (1, 0, 0), (2, 0, 100), (3, 0, 158), (10, 0, 332), (1024, 0, 1000)];
        for (value, status, expected) in cases {
            with_context(&mut memory, |context| assert_eq!(helper_ee97f0(context, value, output), Ok(status)));
            assert_eq!(u32::from_le_bytes(memory.other[..4].try_into().unwrap()), expected);
        }
    }

    #[test]
    fn the_scale_copies_eleven_words_and_answers_one_hundred_for_a_short_step() {
        let mut memory = Memory { record: [0; 0x40], other: [0; 0x40], pointer: [0; 4], configuration: [0; 0x1_0000] };
        for index in 0..11u32 { memory.record[index as usize * 4..index as usize * 4 + 4].copy_from_slice(&(0x100 + index).to_le_bytes()); }
        with_context(&mut memory, |context| {
            assert_eq!(helper_f8a390(context, GuestAddress::new(0x0010_0000), GuestAddress::new(0x0010_1000), 0), Ok(SMALL_ANSWER));
            assert_eq!(helper_f8a390(context, GuestAddress::new(0x0010_0000), GuestAddress::new(0x0010_1000), 1), Ok(SMALL_ANSWER));
        });
        assert_eq!(&memory.other[..44], &memory.record[..44]);
        memory.record[0x1c..0x20].copy_from_slice(&100u32.to_le_bytes());
        with_context(&mut memory, |context| {
            assert_eq!(helper_f8a390(context, GuestAddress::new(0x0010_0000), GuestAddress::new(0x0010_1000), 2), Ok(SMALL_ANSWER));
        });
        assert_eq!(u32::from_le_bytes(memory.other[0x1c..0x20].try_into().unwrap()), 100);
    }

    #[test]
    fn the_scale_divides_the_five_words_by_the_largest_step() {
        let mut memory = Memory { record: [0; 0x40], other: [0; 0x40], pointer: [0; 4], configuration: [0; 0x1_0000] };
        let words = [0u32, 1000, 3000, 6000, 10000, 0];
        for (index, value) in words.iter().enumerate() { memory.record[0x18 + index * 4..0x1c + index * 4].copy_from_slice(&value.to_le_bytes()); }
        with_context(&mut memory, |context| {
            assert_eq!(helper_f8a390(context, GuestAddress::new(0x0010_0000), GuestAddress::new(0x0010_1000), 4), Ok(12));
        });
        let expected = [0u32, 1000, 3000, 6000, 10000];
        for (index, value) in expected.iter().enumerate() {
            let at = 0x18 + index * 4;
            assert_eq!(u32::from_le_bytes(memory.other[at..at + 4].try_into().unwrap()), value * 12 / 100);
        }
    }

    #[test]
    fn the_ratio_is_zero_for_a_degenerate_record() {
        let mut memory = Memory { record: [0; 0x40], other: [0; 0x40], pointer: [0; 4], configuration: [0; 0x1_0000] };
        with_context(&mut memory, |context| assert_eq!(helper_f8cae8(context, GuestAddress::new(0x0010_0000)), Ok(0)));
        memory.record[0x1c..0x20].copy_from_slice(&4u32.to_le_bytes());
        memory.record[0x20..0x24].copy_from_slice(&4u32.to_le_bytes());
        with_context(&mut memory, |context| assert_eq!(helper_f8cae8(context, GuestAddress::new(0x0010_0000)), Ok(0)));
        memory.record[0x20..0x24].copy_from_slice(&8u32.to_le_bytes());
        memory.record[0x24..0x28].copy_from_slice(&8u32.to_le_bytes());
        with_context(&mut memory, |context| assert_eq!(helper_f8cae8(context, GuestAddress::new(0x0010_0000)), Ok(0)));
        memory.record[0x24..0x28].copy_from_slice(&12u32.to_le_bytes());
        with_context(&mut memory, |context| assert_eq!(helper_f8cae8(context, GuestAddress::new(0x0010_0000)), Ok(0)));
    }

    #[test]
    fn the_ratio_follows_the_two_spans() {
        let mut memory = Memory { record: [0; 0x40], other: [0; 0x40], pointer: [0; 4], configuration: [0; 0x1_0000] };
        memory.record[0x08..0x0c].copy_from_slice(&10u32.to_le_bytes());
        memory.record[0x0c..0x10].copy_from_slice(&110u32.to_le_bytes());
        memory.record[0x10..0x14].copy_from_slice(&210u32.to_le_bytes());
        memory.record[0x1c..0x20].copy_from_slice(&0u32.to_le_bytes());
        memory.record[0x20..0x24].copy_from_slice(&2u32.to_le_bytes());
        memory.record[0x24..0x28].copy_from_slice(&4u32.to_le_bytes());
        with_context(&mut memory, |context| assert_eq!(helper_f8cae8(context, GuestAddress::new(0x0010_0000)), Ok(0)));
        memory.record[0x0c..0x10].copy_from_slice(&210u32.to_le_bytes());
        memory.record[0x10..0x14].copy_from_slice(&10u32.to_le_bytes());
        memory.record[0x08..0x0c].copy_from_slice(&10u32.to_le_bytes());
        with_context(&mut memory, |context| assert_eq!(helper_f8cae8(context, GuestAddress::new(0x0010_0000)), Ok(238)));
    }

    #[test]
    fn the_ratio_limit_compares_against_the_configuration_word() {
        let mut memory = Memory { record: [0; 0x40], other: [0; 0x40], pointer: [0; 4], configuration: [0; 0x1_0000] };
        memory.record[0x0c..0x10].copy_from_slice(&210u32.to_le_bytes());
        memory.record[0x10..0x14].copy_from_slice(&10u32.to_le_bytes());
        memory.record[0x08..0x0c].copy_from_slice(&10u32.to_le_bytes());
        memory.record[0x1c..0x20].copy_from_slice(&0u32.to_le_bytes());
        memory.record[0x20..0x24].copy_from_slice(&2u32.to_le_bytes());
        memory.record[0x24..0x28].copy_from_slice(&4u32.to_le_bytes());
        memory.pointer.copy_from_slice(&0x0010_2000u32.to_le_bytes());
        memory.pointer.copy_from_slice(&0x0010_2000u32.to_le_bytes());
        memory.configuration[0x927c] = 238;
        with_context(&mut memory, |context| assert_eq!(helper_f8cbd0(context, GuestAddress::new(0x0010_0000)), Ok(1)));
        memory.configuration[0x927c] = 239;
        with_context(&mut memory, |context| assert_eq!(helper_f8cbd0(context, GuestAddress::new(0x0010_0000)), Ok(0)));
    }
}
