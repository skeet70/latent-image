//! Image-5 selection byte pairs, bodies 0x00f1_aa70-0x00f6_efc8.
use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

fn configuration_byte<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, key: u32) -> Result<u8, MemoryError> {
    crate::record_configuration::helper_69f8fc(context, key).map(|value| value as u8)
}

fn split<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>, key: u32, low: GuestAddress, high: GuestAddress,
) -> Result<(), MemoryError> {
    let value = configuration_byte(context, key)?;
    context.write_u8(low, value & 15)?;
    context.write_u8(high, value >> 4)
}

/// The image-5 body at 0x00f1_aa70.
pub fn helper_f1aa70<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>, first: u32, second: u32, third: u32, fourth: u32, low: GuestAddress, high: GuestAddress,
) -> Result<(), MemoryError> {
    let (low_value, high_value) = if first < fourth {
        (first, first)
    } else if second > third {
        (second, second)
    } else {
        (first.min(third), second.max(fourth))
    };
    context.write_u8(low, low_value as u8)?;
    context.write_u8(high, high_value as u8)
}

/// The image-5 body at 0x00f5_f604.
pub fn helper_f5f604<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>, value: u32, low: GuestAddress, high: GuestAddress,
) -> Result<(), MemoryError> {
    let mapped = crate::record_profile_globals::helper_772ff8(context, value)?;
    let key = match mapped { 1..=10 => 0x4a3f + mapped, _ => 0x4a41 };
    split(context, key, low, high)
}

/// The image-5 body at 0x00f5_f700.
pub fn helper_f5f700<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, low: GuestAddress, high: GuestAddress) -> Result<(), MemoryError> {
    split(context, 0x4a67, low, high)
}

/// The image-5 body at 0x00f6_ec88.
pub fn helper_f6ec88<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>, first: u32, second: u32, third: u32, fourth: u32,
    flag: u32, mode: u32, kind: u32, low: GuestAddress, high: GuestAddress,
) -> Result<(), MemoryError> {
    let mut key = if kind != 0 {
        match kind - 1 {
            0 => 0x4a6a,
            1 => 0x4a6b,
            2 => 0x4a70,
            3 => 0x4a6c,
            4 => 0x4a6d,
            5 => 0x4a6e,
            6..=11 => 0x4a6f,
            12 => 0x1_787c,
            13 => 0x4a71,
            _ => 0x4a4c,
        }
    } else {
        match first {
            0 | 5 | 6 | 7 | 34 | 37 | 40 | 43..=48 | 54..=57 => {
                if third != 0 {
                    if first == 56 || first == 47 { 0x4a50 } else { 0x4a4f }
                } else {
                    match fourth { 0 => 0x4a4e, 1 => 0x4a4d, 2 => 0x4a4c, 3 => 0x4a4b, 4 => 0x4a4a, _ => 0x4a4c }
                }
            }
            3 => 0x4a54,
            10 => 0x4a52,
            11 => 0x4a56,
            12 | 13 | 33 => 0x4a55,
            16 => 0x4a57,
            17 => 0x4a58,
            18 => 0x4a5c,
            21 => 0x4a5b,
            24 => 0x4a59,
            25 => 0x4a5a,
            28 => 0x4a5d,
            29 => 0x4a61,
            30 => 0x4a5f,
            31 => 0x4a5e,
            32 => 0x4a60,
            49 => 0x4a63,
            50 => 0x4a62,
            51 => 0x4a64,
            52 => 0x4a65,
            53 => 0x4a66,
            _ => 0x4a53,
        }
    };
    if flag != 0 && (first == 48 || first == 0) { key = 0x4a68; }
    if mode == 4 { key = 0x4a69; }
    if second & 2 != 0 { key = 0x4a63; }
    split(context, key, low, high)
}

/// The image-5 body at 0x00f5_f6c0.
pub fn helper_f5f6c0<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>, first: u32, second: u32, third: u32, fourth: u32,
    flag: u32, mode: u32, kind: u32, low: GuestAddress, high: GuestAddress,
) -> Result<(), MemoryError> {
    helper_f6ec88(context, first, second, third, fourth, flag, mode, kind, low, high)
}

#[cfg(test)]
mod tests {
    //! Expected keys and nibbles are the image-5 literals at 0x00f1_aa70-0x00f6_efc8.
    use super::*;
    use crate::record_address_space::{GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const CONFIGURATION: u32 = 0x0160_0000;
    const CONFIGURATION_POINTER: u32 = 0x0096_ca50;

    struct Memory {
        pointer: [u8; 4],
        configuration: [u8; 0x5000],
        output: [u8; 4],
    }

    impl Memory {
        fn new() -> Self { Memory { pointer: CONFIGURATION.to_le_bytes(), configuration: [0; 0x5000], output: [0; 4] } }

        fn with_context<R>(&mut self, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
            let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
            let mut regions = [
                MemoryRegion::new(descriptor(CONFIGURATION_POINTER, 4), &mut self.pointer).unwrap(),
                MemoryRegion::new(descriptor(CONFIGURATION, 0x5000), &mut self.configuration).unwrap(),
                MemoryRegion::new(descriptor(0x0010_0000, 4), &mut self.output).unwrap(),
            ];
            let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
            body(&mut RecordContext::new(space, ()))
        }
    }

    #[test]
    fn the_limit_pairs_collapse_to_one_value_or_keep_the_inner_bounds() {
        let mut memory = Memory::new();
        memory.with_context(|context| {
            assert_eq!(helper_f1aa70(context, 5, 9, 7, 6, GuestAddress::new(0x0010_0000), GuestAddress::new(0x0010_0001)), Ok(()));
        });
        assert_eq!(&memory.output[..2], &[5, 5]);
        memory.with_context(|context| {
            assert_eq!(helper_f1aa70(context, 5, 9, 7, 3, GuestAddress::new(0x0010_0000), GuestAddress::new(0x0010_0001)), Ok(()));
        });
        assert_eq!(&memory.output[..2], &[9, 9]);
        memory.with_context(|context| {
            assert_eq!(helper_f1aa70(context, 5, 6, 7, 3, GuestAddress::new(0x0010_0000), GuestAddress::new(0x0010_0001)), Ok(()));
        });
        assert_eq!(&memory.output[..2], &[5, 6]);
        memory.with_context(|context| {
            assert_eq!(helper_f1aa70(context, 9, 2, 7, 6, GuestAddress::new(0x0010_0000), GuestAddress::new(0x0010_0001)), Ok(()));
        });
        assert_eq!(&memory.output[..2], &[7, 6]);
    }

    #[test]
    fn the_mapped_value_selects_its_key_and_both_nibbles() {
        let mut memory = Memory::new();
        memory.configuration[0x4a40] = 0x21;
        memory.configuration[0x4a49] = 0x5a;
        memory.with_context(|context| {
            assert_eq!(helper_f5f604(context, 0, GuestAddress::new(0x0010_0000), GuestAddress::new(0x0010_0001)), Ok(()));
        });
        assert_eq!(&memory.output[..2], &[0, 0]);
        for index in 2..=10u32 {
            let at = (0x1900 + index * 2) as usize;
            memory.configuration[at..at + 2].copy_from_slice(&0x0100u16.to_le_bytes());
        }
        memory.with_context(|context| {
            assert_eq!(helper_f5f604(context, 5, GuestAddress::new(0x0010_0000), GuestAddress::new(0x0010_0001)), Ok(()));
        });
        assert_eq!(&memory.output[..2], &[0x01, 0x02]);
        memory.with_context(|context| {
            assert_eq!(helper_f5f604(context, 300, GuestAddress::new(0x0010_0000), GuestAddress::new(0x0010_0001)), Ok(()));
        });
        assert_eq!(&memory.output[..2], &[0x0a, 0x05]);
    }

    #[test]
    fn the_fixed_key_pair_splits_the_byte_at_zero_4a67() {
        let mut memory = Memory::new();
        memory.configuration[0x4a67] = 0x3c;
        memory.with_context(|context| {
            assert_eq!(helper_f5f700(context, GuestAddress::new(0x0010_0000), GuestAddress::new(0x0010_0001)), Ok(()));
        });
        assert_eq!(&memory.output[..2], &[0x0c, 0x03]);
    }

    #[test]
    fn the_kind_switch_selects_the_high_keys() {
        let mut memory = Memory::new();
        memory.configuration[0x4a6a] = 0x12;
        memory.configuration[0x4a6b] = 0x34;
        memory.configuration[0x4a71] = 0x56;
        memory.configuration[0x4a4c] = 0x9a;
        memory.with_context(|context| {
            assert_eq!(helper_f6ec88(context, 0, 0, 0, 0, 0, 0, 1, GuestAddress::new(0x0010_0000), GuestAddress::new(0x0010_0001)), Ok(()));
        });
        assert_eq!(&memory.output[..2], &[0x02, 0x01]);
        memory.with_context(|context| {
            assert_eq!(helper_f6ec88(context, 0, 0, 0, 0, 0, 0, 2, GuestAddress::new(0x0010_0000), GuestAddress::new(0x0010_0001)), Ok(()));
        });
        assert_eq!(&memory.output[..2], &[0x04, 0x03]);
        memory.with_context(|context| {
            assert_eq!(helper_f6ec88(context, 0, 0, 0, 0, 0, 0, 14, GuestAddress::new(0x0010_0000), GuestAddress::new(0x0010_0001)), Ok(()));
        });
        assert_eq!(&memory.output[..2], &[0x06, 0x05]);
        memory.with_context(|context| {
            assert_eq!(helper_f6ec88(context, 0, 0, 0, 0, 0, 0, 15, GuestAddress::new(0x0010_0000), GuestAddress::new(0x0010_0001)), Ok(()));
        });
        assert_eq!(&memory.output[..2], &[0x0a, 0x09]);
    }

    #[test]
    fn the_first_argument_switch_selects_the_low_keys() {
        let mut memory = Memory::new();
        memory.configuration[0x4a53] = 0x11;
        memory.configuration[0x4a54] = 0x22;
        memory.configuration[0x4a52] = 0x33;
        memory.configuration[0x4a66] = 0x44;
        memory.with_context(|context| {
            assert_eq!(helper_f6ec88(context, 1, 0, 0, 0, 0, 0, 0, GuestAddress::new(0x0010_0000), GuestAddress::new(0x0010_0001)), Ok(()));
        });
        assert_eq!(&memory.output[..2], &[0x01, 0x01]);
        memory.with_context(|context| {
            assert_eq!(helper_f6ec88(context, 3, 0, 0, 0, 0, 0, 0, GuestAddress::new(0x0010_0000), GuestAddress::new(0x0010_0001)), Ok(()));
        });
        assert_eq!(&memory.output[..2], &[0x02, 0x02]);
        memory.with_context(|context| {
            assert_eq!(helper_f6ec88(context, 10, 0, 0, 0, 0, 0, 0, GuestAddress::new(0x0010_0000), GuestAddress::new(0x0010_0001)), Ok(()));
        });
        assert_eq!(&memory.output[..2], &[0x03, 0x03]);
        memory.with_context(|context| {
            assert_eq!(helper_f6ec88(context, 53, 0, 0, 0, 0, 0, 0, GuestAddress::new(0x0010_0000), GuestAddress::new(0x0010_0001)), Ok(()));
        });
        assert_eq!(&memory.output[..2], &[0x04, 0x04]);
    }

    #[test]
    fn the_shared_target_branches_on_the_third_and_fourth_arguments() {
        let mut memory = Memory::new();
        memory.configuration[0x4a50] = 0x12;
        memory.configuration[0x4a4f] = 0x34;
        memory.configuration[0x4a4e] = 0x56;
        memory.configuration[0x4a4d] = 0x78;
        memory.configuration[0x4a4c] = 0x9a;
        memory.configuration[0x4a4b] = 0xbc;
        memory.configuration[0x4a4a] = 0xde;
        memory.with_context(|context| {
            assert_eq!(helper_f6ec88(context, 47, 0, 1, 0, 0, 0, 0, GuestAddress::new(0x0010_0000), GuestAddress::new(0x0010_0001)), Ok(()));
        });
        assert_eq!(&memory.output[..2], &[0x02, 0x01]);
        memory.with_context(|context| {
            assert_eq!(helper_f6ec88(context, 0, 0, 1, 0, 0, 0, 0, GuestAddress::new(0x0010_0000), GuestAddress::new(0x0010_0001)), Ok(()));
        });
        assert_eq!(&memory.output[..2], &[0x04, 0x03]);
        for (fourth, expected) in [(0u32, 0x56u8), (1, 0x78), (2, 0x9a), (3, 0xbc), (4, 0xde), (5, 0x9a)] {
            memory.with_context(|context| {
                assert_eq!(helper_f6ec88(context, 0, 0, 0, fourth, 0, 0, 0, GuestAddress::new(0x0010_0000), GuestAddress::new(0x0010_0001)), Ok(()));
            });
            assert_eq!(memory.output[0], expected & 15);
            assert_eq!(memory.output[1], expected >> 4);
        }
    }

    #[test]
    fn the_overrides_replace_the_selected_key_in_order() {
        let mut memory = Memory::new();
        memory.configuration[0x4a68] = 0x21;
        memory.configuration[0x4a69] = 0x43;
        memory.configuration[0x4a63] = 0x65;
        memory.with_context(|context| {
            assert_eq!(helper_f6ec88(context, 48, 0, 0, 0, 1, 0, 0, GuestAddress::new(0x0010_0000), GuestAddress::new(0x0010_0001)), Ok(()));
        });
        assert_eq!(&memory.output[..2], &[0x01, 0x02]);
        memory.with_context(|context| {
            assert_eq!(helper_f6ec88(context, 1, 0, 0, 0, 0, 4, 0, GuestAddress::new(0x0010_0000), GuestAddress::new(0x0010_0001)), Ok(()));
        });
        assert_eq!(&memory.output[..2], &[0x03, 0x04]);
        memory.with_context(|context| {
            assert_eq!(helper_f6ec88(context, 1, 2, 0, 0, 1, 4, 0, GuestAddress::new(0x0010_0000), GuestAddress::new(0x0010_0001)), Ok(()));
        });
        assert_eq!(&memory.output[..2], &[0x05, 0x06]);
    }

    #[test]
    fn the_forwarder_passes_every_argument_through() {
        let mut memory = Memory::new();
        memory.configuration[0x4a6a] = 0x12;
        memory.with_context(|context| {
            assert_eq!(helper_f5f6c0(context, 0, 0, 0, 0, 0, 0, 1, GuestAddress::new(0x0010_0000), GuestAddress::new(0x0010_0001)), Ok(()));
        });
        assert_eq!(&memory.output[..2], &[0x02, 0x01]);
    }
}
