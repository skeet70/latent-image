//! X-M1 1.50 conditional header merge, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.
//! Installed 759250 bytes define destination-handle, source-header argument order.

use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext, RegisterWordCount, StackAlignment};
use crate::record_profile_accessors::{self as access, ProfileField, ProfileHandle};
use crate::record_profile_merge_accessors as tail;

pub fn helper_759250<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: u32, source: GuestAddress) -> Result<(), MemoryError> {
    let bank = access::read_bank(context, ProfileHandle(destination), ProfileField(0))?;
    let resource = access::resource_address(bank);
    for offset in [0x80, 0x84, 0x88, 0x8c] {
        context.write_u32(resource.checked_add(offset)?, 0)?;
    }

    macro_rules! value {
        (bytes30, $index:expr) => { u32::from(context.read_u8(source.checked_add(0x30 + $index)?)?) };
        (halfwords26, $index:expr) => { u32::from(context.read_u16(source.checked_add(0x26 + 2 * $index)?)?) };
        (signed18, $index:expr) => { context.read_u16(source.checked_add(0x18 + 2 * $index)?)? as i16 as i32 as u32 };
        (words00, $index:expr) => { context.read_u32(source.checked_add(4 * $index)?)? };
    }
    macro_rules! scalar {
        ($low:expr, $high:expr, $member:ident, $index:expr, $setter:ident) => {
            scalar!($low, $high, $member, $index, access, $setter);
        };
        ($low:expr, $high:expr, $member:ident, $index:expr, $provider:ident, $setter:ident) => {
            if access::header_contains_mask(context, source, $low, $high)? {
                let value = value!($member, $index);
                $provider::$setter(context, destination, value)?;
            }
        };
    }
    macro_rules! block {
        ($low:expr, $high:expr, $offset:expr, $length:expr, $setter:ident) => {
            block!($low, $high, $offset, $length, access, $setter);
        };
        ($low:expr, $high:expr, $offset:expr, $length:expr, $provider:ident, $setter:ident) => {
            if access::header_contains_mask(context, source, $low, $high)? {
                context.with_guest_frame(ByteCount($length), StackAlignment::Word, |context, frame| {
                    let input = source.checked_add($offset)?;
                    match ($offset, $length) {
                        (0, 4) => context.copy_register_words(frame.base(), input, RegisterWordCount::One)?,
                        (4, 8) => context.copy_register_words(frame.base(), input, RegisterWordCount::Two)?,
                        (12, 2) => {
                            let value = context.read_u16(input)?;
                            context.write_u16(frame.base(), value)?;
                        }
                        _ => context.copy_bytes(frame.base(), input, ByteCount($length))?,
                    }
                    $provider::$setter(context, destination, frame.base())
                })?;
            }
        };
    }

    scalar!(1, 0, bytes30, 0, helper_74e604);
    scalar!(2, 0, bytes30, 1, helper_74e6d4);
    scalar!(0x8000, 0x80000000, halfwords26, 4, helper_74e75c);
    scalar!(4, 0, bytes30, 2, helper_74e834);
    scalar!(0, 0x8000, bytes30, 3, helper_74e8bc);
    scalar!(0, 0x10000, halfwords26, 0, helper_74e944);
    scalar!(0, 0x20000, signed18, 0, helper_74e9cc);
    scalar!(8, 0, bytes30, 43, helper_74ea54);
    scalar!(16, 0, bytes30, 44, helper_74eadc);
    scalar!(32, 0, bytes30, 45, helper_74eb64);
    scalar!(64, 0, bytes30, 4, helper_74ebec);
    block!(128, 0, 0, 4, helper_74eca0);
    scalar!(0, 0x4000000, bytes30, 5, helper_74ed28);
    scalar!(0x100, 0, bytes30, 6, helper_74edb0);
    scalar!(0x200, 0, bytes30, 7, helper_74ee38);
    scalar!(0x400, 0, bytes30, 8, helper_74eec0);
    scalar!(0x800, 0, bytes30, 9, helper_74ef48);
    scalar!(0, 0x40000, bytes30, 10, helper_74efd0);
    scalar!(0, 0x80000, bytes30, 11, helper_74f058);
    scalar!(0x1000, 0, bytes30, 12, helper_74f0e0);
    scalar!(0x2000, 0, bytes30, 13, helper_74f168);
    scalar!(0x200, 0x80000000, bytes30, 14, helper_74f234);
    scalar!(0x4000, 0, bytes30, 15, helper_74f278);
    scalar!(0x8000, 0, signed18, 1, helper_74f498);
    scalar!(0x10000, 0, bytes30, 46, helper_74f520);
    scalar!(0, 0x200000, bytes30, 16, helper_74f300);
    scalar!(0, 0x400000, bytes30, 17, helper_74f388);
    scalar!(64, 0x80000000, bytes30, 18, helper_74f410);
    scalar!(0x20000, 0, bytes30, 19, helper_74f5a8);
    scalar!(0x40000, 0, signed18, 2, helper_74f630);
    scalar!(0x80000, 0, words00, 5, helper_74f6b8);
    scalar!(0x100000, 0, signed18, 3, helper_74f740);
    scalar!(0x200000, 0, bytes30, 20, helper_74f7c8);
    scalar!(0, 0x800000, signed18, 4, helper_74f850);
    scalar!(0x400000, 0, signed18, 5, helper_74f8d8);
    scalar!(0x800000, 0, signed18, 6, helper_74f960);
    scalar!(0x1000000, 0, bytes30, 22, helper_74f9e8);
    scalar!(0x2000000, 0, bytes30, 23, helper_74fa70);
    block!(0x4000000, 0, 4, 8, helper_74fb10);
    scalar!(0x8000000, 0, bytes30, 47, helper_74fb98);
    scalar!(0x10000000, 0, bytes30, 48, helper_74fc20);
    scalar!(0, 0x400, bytes30, 49, helper_74fca8);
    scalar!(0, 0x2000, halfwords26, 1, helper_74fd30);
    scalar!(0x20000000, 0, bytes30, 24, helper_74fdb8);
    scalar!(0x40000000, 0, bytes30, 25, helper_74fe40);
    scalar!(0x80000000, 0, bytes30, 26, helper_74fec8);
    block!(0, 1, 14, 6, helper_74ff68);
    scalar!(0, 0x1000, halfwords26, 2, helper_74fff0);
    scalar!(0, 2, bytes30, 21, helper_750078);
    scalar!(0, 4, bytes30, 27, helper_750100);
    scalar!(0, 32, bytes30, 28, helper_750188);
    scalar!(0, 64, bytes30, 29, helper_750210);
    scalar!(0, 8, bytes30, 30, helper_750298);
    scalar!(0, 16, bytes30, 31, helper_750320);
    scalar!(0, 0x800, bytes30, 32, helper_7503a8);
    scalar!(0, 128, bytes30, 51, helper_750430);
    scalar!(0, 0x2000000, bytes30, 53, helper_7505c8);
    scalar!(0, 0x100, bytes30, 33, helper_750650);
    scalar!(0, 0x200, bytes30, 34, helper_7506d8);
    scalar!(0, 0x4000, bytes30, 52, helper_750980);
    scalar!(0, 0x8000000, bytes30, 54, helper_750a08);
    scalar!(0, 0x100000, bytes30, 36, helper_750760);
    scalar!(0, 0x1000000, bytes30, 35, helper_7507e8);
    scalar!(2, 0x80000000, bytes30, 37, helper_7508f8);
    scalar!(0, 0x10000000, bytes30, 55, helper_750b04);
    block!(0, 0x40000000, 12, 2, helper_750bbc);
    scalar!(16, 0x80000000, bytes30, 57, helper_750ccc);
    scalar!(32, 0x80000000, halfwords26, 3, helper_750d10);
    scalar!(0, 0x20000000, bytes30, 56, helper_750c44);
    scalar!(4, 0x80000000, bytes30, 59, helper_750870);
    scalar!(1, 0x80000000, bytes30, 50, helper_7504b8);
    scalar!(8, 0x80000000, bytes30, 58, helper_750540);
    scalar!(128, 0x80000000, bytes30, 60, helper_750e20);
    scalar!(0x100, 0x80000000, bytes30, 61, helper_750ddc);
    scalar!(0x10000, 0x80000000, bytes30, 64, tail, helper_758fec);
    scalar!(0x20000, 0x80000000, bytes30, 62, tail, helper_759074);
    scalar!(0x40000, 0x80000000, bytes30, 40, tail, helper_7590fc);
    scalar!(0x80000, 0x80000000, bytes30, 41, tail, helper_759184);
    scalar!(0x400, 0x80000000, bytes30, 38, tail, helper_758d34);
    scalar!(0x800, 0x80000000, bytes30, 39, tail, helper_758dbc);
    block!(0x1000, 0x80000000, 0x72, 4, tail, helper_758e4c);
    block!(0x2000, 0x80000000, 0x76, 4, tail, helper_758edc);
    scalar!(0x4000, 0x80000000, bytes30, 65, tail, helper_758f64);
    scalar!(0x100000, 0x80000000, bytes30, 42, tail, helper_75920c);
    Ok(())
}

#[cfg(test)]
#[path = "record_profile_merge_tests.rs"]
mod tests;
