//! The configuration key a code word selects, read as a signed byte and scaled.
//!
//! Image-5 body 0x00f71204.
use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_adjustment_mode_groups_f704c4::helper_f704c4;
use crate::record_configuration_signed_byte_69ffe8::helper_69ffe8;

const FLAGGED_KEY: u32 = 0x61ae;
const ACCEPTED_KEY: u32 = 0x61b4;
const REJECTED_KEY: u32 = 0x61a8;

/// The image-5 body at 0x00f71204.
pub fn helper_f71204<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, source: GuestAddress) -> Result<i32, MemoryError> {
    let flags = u32::from(context.read_u16(source.checked_add(4)?)?);
    let key = if flags & 2 != 0 {
        FLAGGED_KEY
    } else {
        match context.read_u32(source)? {
            11 => 0x61ab,
            12 | 13 => 0x61aa,
            17 => 0x61ad,
            24 => 0x61ac,
            43 | 44 | 57 => 0x61a9,
            45 | 55 => 0x61b7,
            46 => {
                if context.read_u32(source.checked_add(0xc)?)? != 0 {
                    0x61b6
                } else {
                    0x61b5
                }
            }
            47 | 56 => 0x61b8,
            49 => 0x61ae,
            50 => 0x61af,
            51 => 0x61b0,
            52 => 0x61b1,
            53 => 0x61b2,
            54 => 0x61b3,
            code => {
                let enabled = context.read_u32(source.checked_add(8)?)?;
                if helper_f704c4(context, code, flags, enabled)? != 0 { ACCEPTED_KEY } else { REJECTED_KEY }
            }
        }
    };
    Ok((helper_69ffe8(context, key)?.wrapping_mul(10) as u16) as i16 as i32)
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 body at 0x00f71204.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const CONFIG_POINTER: u32 = 0x0096_ca50;
    const CONFIG_DATA: u32 = 0x0020_0000;
    const CONFIG_LENGTH: usize = 0x40_0000;
    const SOURCE: u32 = 0x0080_0000;

    struct Memory {
        pointer: [u8; 4],
        source: [u8; 0x10],
    }

    fn with_config<R>(config: &mut [u8], memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let mut pointer = CONFIG_DATA.to_le_bytes();
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(CONFIG_POINTER, 4), &mut pointer).unwrap(),
            MemoryRegion::new(descriptor(CONFIG_DATA, config.len() as u32), config).unwrap(),
            MemoryRegion::new(descriptor(SOURCE, 0x10), &mut memory.source).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    #[test]
    fn each_code_selects_its_configuration_key() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        for key in 0x61a8..=0x61b8u32 {
            config[key as usize] = (key - 0x61a8) as u8 + 1;
        }
        let mut memory = Memory { pointer: [0; 4], source: [0; 0x10] };
        let cases = [
            (11u32, 4i32),
            (12, 3),
            (13, 3),
            (17, 6),
            (24, 5),
            (43, 2),
            (44, 2),
            (57, 2),
            (45, 16),
            (55, 16),
            (46, 14),
            (47, 17),
            (56, 17),
            (49, 7),
            (50, 8),
            (51, 9),
            (52, 10),
            (53, 11),
            (54, 12),
        ];
        with_config(&mut config, &mut memory, |context| {
            for (code, expected) in cases {
                context.write_u32(GuestAddress::new(SOURCE), code).unwrap();
                context.write_u16(GuestAddress::new(SOURCE + 4), 0).unwrap();
                assert_eq!(helper_f71204(context, GuestAddress::new(SOURCE)), Ok(expected * 10), "code {code}");
            }
        });
    }

    #[test]
    fn the_dynamic_and_gated_codes_read_their_own_word() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        for key in 0x61a8..=0x61b8u32 {
            config[key as usize] = (key - 0x61a8) as u8 + 1;
        }
        let mut memory = Memory { pointer: [0; 4], source: [0; 0x10] };
        with_config(&mut config, &mut memory, |context| {
            context.write_u32(GuestAddress::new(SOURCE), 46).unwrap();
            context.write_u32(GuestAddress::new(SOURCE + 0xc), 1).unwrap();
            assert_eq!(helper_f71204(context, GuestAddress::new(SOURCE)), Ok(150));
            context.write_u16(GuestAddress::new(SOURCE + 4), 2).unwrap();
            assert_eq!(helper_f71204(context, GuestAddress::new(SOURCE)), Ok(70));
            context.write_u16(GuestAddress::new(SOURCE + 4), 0).unwrap();
            context.write_u32(GuestAddress::new(SOURCE), 48).unwrap();
            context.write_u32(GuestAddress::new(SOURCE + 8), 1).unwrap();
            assert_eq!(helper_f71204(context, GuestAddress::new(SOURCE)), Ok(130));
            context.write_u32(GuestAddress::new(SOURCE + 8), 0).unwrap();
            assert_eq!(helper_f71204(context, GuestAddress::new(SOURCE)), Ok(10));
        });
    }
}
