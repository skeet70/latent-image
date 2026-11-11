//! Configuration key selection from a code word and a variant word.
//!
//! Image-5 bodies 0x00f6fb74 and 0x00f6fcb0.
use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_configuration::helper_69ff7c;
use crate::record_configuration_signed_byte_69ffe8::helper_69ffe8;

/// The image-5 body at 0x00f6fb74.
pub fn helper_f6fb74<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, source: GuestAddress) -> Result<i32, MemoryError> {
    let key = match context.read_u32(source)? {
        11 => 0x61bc,
        12 | 13 => 0x61bb,
        24 => 0x61ac,
        43 | 44 | 57 => 0x61ba,
        45 | 55 => 0x61b7,
        46 | 54 => {
            if context.read_u32(source.checked_add(0xc)?)? != 0 {
                0x61bd
            } else {
                0x61be
            }
        }
        47 | 56 => 0x61b8,
        _ => 0x61b9,
    };
    let value = helper_69ffe8(context, key)?;
    Ok((value.wrapping_mul(10) as u16) as i16 as i32)
}

fn alternate_pair(variant: u32) -> (u32, u32) {
    match variant {
        1 => (0x6a56, 0x6906),
        2 => (0x6a58, 0x6908),
        3 => (0x6a62, 0x6912),
        4 => (0x6a5a, 0x690a),
        5 => (0x6a5c, 0x690c),
        6 => (0x6a5e, 0x690e),
        7..=12 => (0x6a60, 0x6910),
        13 => (0x0178_9a, 0x0178_90),
        14 => (0x6a64, 0x6914),
        _ => (0x6a1a, 0x68ca),
    }
}

fn mode_pair(mode: u32) -> (u32, u32) {
    match mode {
        0 => (0x6a16, 0x68c6),
        1 => (0x6a18, 0x68c8),
        2 => (0x6a1a, 0x68ca),
        3 => (0x6a1c, 0x68cc),
        4 => (0x6a1e, 0x68ce),
        _ => (0x6a1a, 0x68ca),
    }
}

fn paired_mode_pair(mode: u32) -> (u32, u32) {
    match mode {
        0 => (0x6a20, 0x68d0),
        1 => (0x6a22, 0x68d2),
        2 => (0x6a24, 0x68d4),
        3 => (0x6a26, 0x68d6),
        4 => (0x6a28, 0x68d8),
        _ => (0x6a24, 0x68d4),
    }
}

fn code_pair(code: u32, mode: u32) -> (u32, u32) {
    match code {
        1..=4 => (0x6a2a, 0x68da),
        5..=7 => (0x6a2c, 0x68dc),
        8 | 9 => (0x6a2a, 0x68da),
        10 => (0x6a2e, 0x68de),
        11 => (0x6a32, 0x68e2),
        12 | 13 => (0x6a30, 0x68e0),
        14 | 15 => (0x6a2a, 0x68da),
        16 => (0x6a34, 0x68e4),
        17 => (0x6a36, 0x68e6),
        18 => (0x6a3e, 0x68ee),
        19 | 20 => (0x6a2a, 0x68da),
        21 => (0x6a3c, 0x68ec),
        22 | 23 => (0x6a2a, 0x68da),
        24 => (0x6a38, 0x68e8),
        25 => (0x6a3a, 0x68ea),
        26 | 27 => (0x6a2a, 0x68da),
        28 => (0x6a40, 0x68f0),
        29 => (0x6a46, 0x68f6),
        30 => (0x6a44, 0x68f4),
        31 => (0x6a42, 0x68f2),
        32 => (0x6a48, 0x68f8),
        33 => (0x6a34, 0x68e4),
        34 => (0x6a2c, 0x68dc),
        35 | 36 => (0x6a2a, 0x68da),
        37 => (0x6a2c, 0x68dc),
        38 | 39 => (0x6a2a, 0x68da),
        40 => (0x6a2c, 0x68dc),
        41 | 42 => (0x6a2a, 0x68da),
        43..=46 | 48 | 53..=55 => mode_pair(mode),
        47 | 56 => paired_mode_pair(mode),
        49 => (0x6a4a, 0x68fa),
        50 => (0x6a4c, 0x68fc),
        51 => (0x6a4e, 0x68fe),
        52 => (0x6a50, 0x6900),
        _ => mode_pair(mode),
    }
}

/// The image-5 body at 0x00f6fcb0.
pub fn helper_f6fcb0<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>,
    source: GuestAddress,
    mode: u32,
    select: u32,
    kind: u32,
    variant: u32,
) -> Result<i32, MemoryError> {
    let (mut first, mut second) = if variant != 0 { alternate_pair(variant) } else { code_pair(context.read_u32(source)?, mode) };
    if select != 0 {
        let code = context.read_u32(source)?;
        if code == 0 || code == 48 {
            first = 0x6a14;
            second = 0x68c4;
        }
    }
    if context.read_u16(source.checked_add(4)?)? & 2 != 0 {
        first = 0x6a4a;
        second = 0x68fa;
    }
    helper_69ff7c(context, if kind.wrapping_sub(3) <= 2 { first } else { second })
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 bodies at 0x00f6fb74 and 0x00f6fcb0.
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

    fn put(config: &mut [u8], key: u32, value: u16) {
        config[key as usize..key as usize + 2].copy_from_slice(&value.to_le_bytes());
    }

    #[test]
    fn the_code_selects_the_signed_byte_key() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        let mut memory = Memory { pointer: [0; 4], source: [0; 0x10] };
        put(&mut config, 0x61bc, 0x0005);
        put(&mut config, 0x61bd, 0x0006);
        put(&mut config, 0x61be, 0x0007);
        put(&mut config, 0x61b9, 0x00ff);
        with_config(&mut config, &mut memory, |context| {
            memory_source(context, 11, 0);
            assert_eq!(helper_f6fb74(context, GuestAddress::new(SOURCE)), Ok(50));
            memory_source(context, 46, 1);
            assert_eq!(helper_f6fb74(context, GuestAddress::new(SOURCE)), Ok(60));
            memory_source(context, 46, 0);
            assert_eq!(helper_f6fb74(context, GuestAddress::new(SOURCE)), Ok(70));
            memory_source(context, 99, 0);
            assert_eq!(helper_f6fb74(context, GuestAddress::new(SOURCE)), Ok(-10));
        });
    }

    fn memory_source<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, code: u32, word: u32) {
        context.write_u32(GuestAddress::new(SOURCE), code).unwrap();
        context.write_u32(GuestAddress::new(SOURCE + 0xc), word).unwrap();
    }

    #[test]
    fn the_variant_pair_wins_over_the_code_pair() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        let mut memory = Memory { pointer: [0; 4], source: [0; 0x10] };
        put(&mut config, 0x6a58, 0x1111);
        put(&mut config, 0x6908, 0x2222);
        put(&mut config, 0x1789a, 0x3333);
        with_config(&mut config, &mut memory, |context| {
            assert_eq!(helper_f6fcb0(context, GuestAddress::new(SOURCE), 0, 0, 3, 2), Ok(0x1111));
            assert_eq!(helper_f6fcb0(context, GuestAddress::new(SOURCE), 0, 0, 9, 2), Ok(0x2222));
            assert_eq!(helper_f6fcb0(context, GuestAddress::new(SOURCE), 0, 0, 3, 13), Ok(0x3333));
        });
    }

    #[test]
    fn the_code_and_kind_select_the_pair_member() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        let mut memory = Memory { pointer: [0; 4], source: [0; 0x10] };
        put(&mut config, 0x6a2a, 0x4444);
        put(&mut config, 0x68da, 0x5555);
        put(&mut config, 0x6a16, 0x6666);
        put(&mut config, 0x68c6, 0x7777);
        put(&mut config, 0x6a14, 0x0888);
        put(&mut config, 0x68c4, 0x0999);
        put(&mut config, 0x6a4a, 0x0aaa);
        put(&mut config, 0x68fa, 0x0bbb);
        with_config(&mut config, &mut memory, |context| {
            memory_source(context, 1, 0);
            assert_eq!(helper_f6fcb0(context, GuestAddress::new(SOURCE), 0, 0, 3, 0), Ok(0x4444));
            assert_eq!(helper_f6fcb0(context, GuestAddress::new(SOURCE), 0, 0, 9, 0), Ok(0x5555));
            memory_source(context, 43, 0);
            assert_eq!(helper_f6fcb0(context, GuestAddress::new(SOURCE), 0, 0, 3, 0), Ok(0x6666));
            assert_eq!(helper_f6fcb0(context, GuestAddress::new(SOURCE), 0, 0, 9, 0), Ok(0x7777));
            memory_source(context, 48, 0);
            assert_eq!(helper_f6fcb0(context, GuestAddress::new(SOURCE), 0, 1, 3, 0), Ok(0x0888));
            assert_eq!(helper_f6fcb0(context, GuestAddress::new(SOURCE), 0, 1, 9, 0), Ok(0x0999));
            context.write_u16(GuestAddress::new(SOURCE + 4), 2).unwrap();
            assert_eq!(helper_f6fcb0(context, GuestAddress::new(SOURCE), 0, 0, 3, 0), Ok(0x0aaa));
            assert_eq!(helper_f6fcb0(context, GuestAddress::new(SOURCE), 0, 0, 9, 0), Ok(0x0bbb));
        });
    }
}
