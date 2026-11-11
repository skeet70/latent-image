//! Adjustment selection: feature-gated tables, level rows, the output record the
//! image-5 body at 0x00f624a8 fills, and the halfword table copy at 0x00f7a9ec.
//!
//! Image-5 bodies 0x00f408b4-0x00f7a9ec.
use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_configuration::{helper_69f8fc, helper_69fec8, helper_69ff68, helper_69ff7c};
use crate::record_native_exports::stock_export;
use crate::record_profile_globals::helper_772f28;
use crate::record_profile_mapping::helper_7469a0;

const FEATURE_FIRST_KEY: u32 = 0x8b16;
const FEATURE_SECOND_KEY: u32 = 0x8b1f;
const FEATURE_THRESHOLD: u32 = 0x016c_21de;
const PROFILE_MODE_KEY: u32 = 0x00a8;
const INITIAL_LIMIT_KEY: u32 = 0x6002;

const LEVEL_KEY_BASE: u32 = 0x631b;
const LEVEL_KEY_FEATURE_SHIFT: u32 = 0x38;

/// The image-5 body at 0x00f408b4.
pub fn helper_f408b4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, mode: u32, flag: u32) -> Result<u32, MemoryError> {
    let first = helper_69ff68(context, FEATURE_FIRST_KEY)?;
    let second = helper_69ff68(context, FEATURE_SECOND_KEY)?;
    let threshold = i32::from(context.read_u16(GuestAddress::new(FEATURE_THRESHOLD))? as i16);
    let result = if mode == 14 || mode == 16 || mode == 19 {
        let first = signed_15(first.wrapping_mul(5));
        if threshold <= first << 1 {
            0
        } else {
            let second = signed_15(second.wrapping_mul(5));
            if threshold <= second << 1 {
                2
            } else if flag != 0 {
                2
            } else {
                1
            }
        }
    } else {
        0
    };
    Ok(result.min(2))
}

fn signed_15(value: i32) -> i32 {
    ((value & 0x7fff) << 17) >> 17
}

/// The image-5 body at 0x00f5f324.
pub fn helper_f5f324<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, first: u32, second: u32, index: u32, fourth: u32) -> Result<i32, MemoryError> {
    let converted = helper_7469a0(context, index)?;
    let value = first.wrapping_sub(second).wrapping_add(fourth).wrapping_sub(converted) as u16 as i16 as i32;
    Ok(value.clamp(-200, 200))
}

fn level_row(level: i32) -> u32 {
    if level > 99 {
        0
    } else if level >= 0 {
        1
    } else if level >= -100 {
        2
    } else if level >= -200 {
        3
    } else if level >= -300 {
        4
    } else if level >= -400 {
        5
    } else {
        6
    }
}

fn level_column(index: u32) -> u32 {
    match index {
        0 => 3,
        1 => 2,
        2 => 1,
        _ => 0,
    }
}

/// The image-5 body at 0x00f5f3c0.
pub fn helper_f5f3c0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: u32, level: u32, index: u32) -> Result<i32, MemoryError> {
    let feature = crate::record_statistics_getters::helper_ef53d0(context, profile)?;
    let base = if feature == 3 { LEVEL_KEY_BASE + LEVEL_KEY_FEATURE_SHIFT } else { LEVEL_KEY_BASE };
    let key = base.wrapping_add(level_row(level as i32).wrapping_mul(8)).wrapping_add(level_column(index).wrapping_mul(2));
    helper_69ff7c(context, key)
}

/// The image-5 body at 0x00f624a8.
///
/// The image-5 body at 0x00ef6790 receives four arguments from this body and
/// reads none of them.
#[allow(clippy::too_many_arguments)]
pub fn helper_f624a8<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>,
    output: GuestAddress,
    first: u32,
    second: u32,
    _third: u32,
    _fourth: u32,
    _fifth: u32,
    _sixth: u32,
    _seventh: u32,
    eighth: u32,
) -> Result<(), MemoryError> {
    let initial = if eighth != 0 { helper_772f28(context, 3)? } else { helper_69fec8(context, INITIAL_LIMIT_KEY)? };
    context.write_u16(output, initial as u16)?;
    context.write_u32(output.checked_add(0x10)?, first)?;
    if helper_69f8fc(context, PROFILE_MODE_KEY)? & 1 == 0 {
        context.write_u16(output.checked_add(4)?, second as u16)?;
    } else {
        crate::record_statistics_getters::helper_eea030(context, None, None, Some(output.checked_add(4)?))?;
    }
    let mut threshold = u32::from(context.read_u16(output.checked_add(4)?)?);
    if threshold != 0 {
        context.write_u32(output.checked_add(0xc)?, 0)?;
    } else {
        context.write_u32(output.checked_add(0xc)?, 1)?;
        threshold = helper_772f28(context, 3)?;
    }
    let profile = if helper_69f8fc(context, PROFILE_MODE_KEY)? & 1 != 0 {
        crate::record_statistics_getters::helper_eea094(context)?
    } else {
        crate::record_frontend_ef6790::helper_ef6790(context)?
    };
    let conversion = crate::record_hardware_deep_calibration::helper_ef5310(context, profile)? as u32 & 0xffff;
    context.write_u32(output.checked_add(8)?, profile)?;
    let mut delta = threshold.wrapping_sub(conversion) & 0xffff;
    let limit = helper_772f28(context, 2)?;
    if i32::from(delta as u16 as i16) < limit as i32 {
        delta = limit;
    }
    context.write_u16(output.checked_add(2)?, conversion.wrapping_add(delta) as u16)
}

/// The image-5 body at 0x00f7a9ec.
///
/// The four blocks the source index selects start at the base word plus 172, 175,
/// 193 and 202 halfwords.
pub fn helper_f7a9ec<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>,
    first: u32,
    second: u32,
    third: u32,
    base: u32,
    destination: GuestAddress,
) -> Result<(), MemoryError> {
    let source = if first > 5 {
        if second <= 2 {
            base.wrapping_add(second.wrapping_mul(3).wrapping_add(third).wrapping_mul(2)).wrapping_add(386)
        } else {
            base.wrapping_add(third.wrapping_add(172).wrapping_mul(2))
        }
    } else if second > 2 {
        base.wrapping_add(first.wrapping_mul(3).wrapping_add(third).wrapping_mul(2)).wrapping_add(350)
    } else {
        base.wrapping_add(second.wrapping_mul(3).wrapping_add(first.wrapping_mul(9)).wrapping_add(third).wrapping_mul(2)).wrapping_add(404)
    };
    context.copy_bytes(destination, GuestAddress::new(source), ByteCount(2))
}

stock_export!(
    xm1_power_record_helper_f624a8,
    helper_f624a8_abi,
    (),
    (output: u32, first: u32, second: u32, third: u32, fourth: u32, fifth: u32, sixth: u32, seventh: u32, eighth: u32) -> (),
    (
        output: GuestAddress,
        first: u32,
        second: u32,
        third: u32,
        fourth: u32,
        fifth: u32,
        sixth: u32,
        seventh: u32,
        eighth: u32
    ),
    (GuestAddress::new(output), first, second, third, fourth, fifth, sixth, seventh, eighth),
    crate::record_adjustment_selection_f408b4::helper_f624a8,
    |_| ()
);

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 bodies at 0x00f408b4-0x00f7a9ec, with
    //! the mapped profile value 100 the configuration of the tests below yields.
    use super::*;
    use crate::record_address_space::{GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const CONFIG_POINTER: u32 = 0x0096_ca50;
    const CONFIG_DATA: u32 = 0x0020_0000;
    const CONFIG_LENGTH: usize = 0x40_0000;
    const MAPPED_WORD_KEY: u32 = 0x3f24_00;
    const FEATURE_TABLE: u32 = 0x00ef_6de4;
    const CALIBRATION: u32 = 0x00ef_66d0;
    const STATISTICS: u32 = 0x00ee_a284;
    const OUTPUT: u32 = 0x0080_0000;
    const TABLE: u32 = 0x0080_1000;

    struct Memory {
        pointer: [u8; 4],
        features: [u8; 0x40],
        calibration: [u8; 0x40],
        statistics: [u8; 0x30],
        threshold: [u8; 2],
        output: [u8; 0x20],
        table: [u8; 0x400],
    }

    fn memory() -> Memory {
        Memory {
            pointer: [0; 4],
            features: [0; 0x40],
            calibration: [0; 0x40],
            statistics: [0; 0x30],
            threshold: [0; 2],
            output: [0; 0x20],
            table: [0; 0x400],
        }
    }

    fn with_config<R>(config: &mut [u8], memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let mut pointer = CONFIG_DATA.to_le_bytes();
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(CONFIG_POINTER, 4), &mut pointer).unwrap(),
            MemoryRegion::new(descriptor(CONFIG_DATA, config.len() as u32), config).unwrap(),
            MemoryRegion::new(descriptor(FEATURE_TABLE, 0x40), &mut memory.features).unwrap(),
            MemoryRegion::new(descriptor(CALIBRATION, 0x40), &mut memory.calibration).unwrap(),
            MemoryRegion::new(descriptor(STATISTICS, 0x30), &mut memory.statistics).unwrap(),
            MemoryRegion::new(descriptor(FEATURE_THRESHOLD, 2), &mut memory.threshold).unwrap(),
            MemoryRegion::new(descriptor(OUTPUT, 0x20), &mut memory.output).unwrap(),
            MemoryRegion::new(descriptor(TABLE, 0x400), &mut memory.table).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    fn put(config: &mut [u8], key: u32, value: u16) {
        config[key as usize..key as usize + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn word(memory: &Memory, offset: usize) -> u32 {
        u32::from_le_bytes(memory.output[offset..offset + 4].try_into().unwrap())
    }

    fn half(memory: &Memory, offset: usize) -> u16 {
        u16::from_le_bytes(memory.output[offset..offset + 2].try_into().unwrap())
    }

    #[test]
    fn the_feature_threshold_gates_the_mode_flag() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        config[FEATURE_FIRST_KEY as usize] = 0x7f;
        config[FEATURE_SECOND_KEY as usize] = 0x7f;
        let mut memory = memory();
        memory.threshold = 0x0100u16.to_le_bytes();
        with_config(&mut config, &mut memory, |context| {
            assert_eq!(helper_f408b4(context, 14, 0), Ok(0));
            assert_eq!(helper_f408b4(context, 16, 0), Ok(0));
            assert_eq!(helper_f408b4(context, 19, 0), Ok(0));
            assert_eq!(helper_f408b4(context, 15, 0), Ok(0));
        });
        config[FEATURE_FIRST_KEY as usize] = 0x00;
        config[FEATURE_SECOND_KEY as usize] = 0x40;
        with_config(&mut config, &mut memory, |context| {
            assert_eq!(helper_f408b4(context, 19, 0), Ok(2));
            assert_eq!(helper_f408b4(context, 19, 1), Ok(2));
            assert_eq!(helper_f408b4(context, 20, 0), Ok(0));
        });
        config[FEATURE_SECOND_KEY as usize] = 0x00;
        with_config(&mut config, &mut memory, |context| {
            assert_eq!(helper_f408b4(context, 14, 0), Ok(1));
            assert_eq!(helper_f408b4(context, 14, 1), Ok(2));
        });
    }

    #[test]
    fn the_level_row_selects_a_signed_halfword() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        let mut memory = memory();
        memory.features[13] = 3;
        for row in 0..7u32 {
            for column in 0..4u32 {
                let key = LEVEL_KEY_BASE + LEVEL_KEY_FEATURE_SHIFT + row * 8 + column * 2;
                put(&mut config, key, 0x1000 + (row * 16 + column) as u16);
            }
        }
        with_config(&mut config, &mut memory, |context| {
            assert_eq!(helper_f5f3c0(context, 0, 100, 0), Ok(0x1003));
            assert_eq!(helper_f5f3c0(context, 0, 99, 1), Ok(0x1012));
            assert_eq!(helper_f5f3c0(context, 0, 0, 2), Ok(0x1011));
            assert_eq!(helper_f5f3c0(context, 0, 0xffff_ffff, 3), Ok(0x1020));
            assert_eq!(helper_f5f3c0(context, 0, -100i32 as u32, 0), Ok(0x1023));
            assert_eq!(helper_f5f3c0(context, 0, -101i32 as u32, 0), Ok(0x1033));
            assert_eq!(helper_f5f3c0(context, 0, -201i32 as u32, 0), Ok(0x1043));
            assert_eq!(helper_f5f3c0(context, 0, -301i32 as u32, 0), Ok(0x1053));
            assert_eq!(helper_f5f3c0(context, 0, -401i32 as u32, 0), Ok(0x1063));
        });
    }

    #[test]
    fn the_clamped_selection_subtracts_the_converted_index() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        put(&mut config, MAPPED_WORD_KEY, 1000);
        let mut memory = memory();
        with_config(&mut config, &mut memory, |context| {
            assert_eq!(helper_f5f324(context, 300, 100, 0, 50), Ok(150));
            assert_eq!(helper_f5f324(context, 1000, 0, 0, 0), Ok(200));
            assert_eq!(helper_f5f324(context, 0, 1000, 0, 0), Ok(-200));
        });
    }

    #[test]
    fn the_output_record_keeps_the_feature_mode_thresholds() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        config[PROFILE_MODE_KEY as usize] = 0;
        put(&mut config, INITIAL_LIMIT_KEY, 0x0500);
        let mut memory = memory();
        with_config(&mut config, &mut memory, |context| {
            assert_eq!(helper_f624a8(context, GuestAddress::new(OUTPUT), 0x0a, 0x0b, 0, 0, 0, 0, 0, 1), Ok(()));
        });
        assert_eq!(half(&memory, 0), 0);
        assert_eq!(half(&memory, 2), 0x0b);
        assert_eq!(half(&memory, 4), 0x0b);
        assert_eq!(word(&memory, 8), 1);
        assert_eq!(word(&memory, 0xc), 0);
        assert_eq!(word(&memory, 0x10), 0x0a);
    }

    #[test]
    fn the_threshold_shortfall_takes_the_published_limit() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        config[PROFILE_MODE_KEY as usize] = 0;
        put(&mut config, 0x1904, 5);
        let mut memory = memory();
        memory.calibration[0..2].copy_from_slice(&32u16.to_le_bytes());
        with_config(&mut config, &mut memory, |context| {
            assert_eq!(helper_f624a8(context, GuestAddress::new(OUTPUT), 0x0a, 0x0b, 0, 0, 0, 0, 0, 1), Ok(()));
        });
        assert_eq!(half(&memory, 2), 37);
    }

    #[test]
    fn a_zero_threshold_takes_the_published_limit() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        config[PROFILE_MODE_KEY as usize] = 1;
        config[0xbe] = 1;
        put(&mut config, INITIAL_LIMIT_KEY, 0x0007);
        let mut memory = memory();
        with_config(&mut config, &mut memory, |context| {
            assert_eq!(helper_f624a8(context, GuestAddress::new(OUTPUT), 1, 2, 3, 4, 5, 6, 7, 0), Ok(()));
        });
        assert_eq!(half(&memory, 0), 0x0007);
        assert_eq!(half(&memory, 4), 0);
        assert_eq!(word(&memory, 8), 1);
        assert_eq!(word(&memory, 0xc), 1);
    }

    #[test]
    fn the_halfword_copy_picks_one_of_four_blocks() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        let mut memory = memory();
        for index in 0..0x200usize {
            memory.table[index * 2..index * 2 + 2].copy_from_slice(&(0x8000u16 + index as u16).to_le_bytes());
        }
        with_config(&mut config, &mut memory, |context| {
            assert_eq!(helper_f7a9ec(context, 0, 0, 0, TABLE, GuestAddress::new(OUTPUT)), Ok(()));
            assert_eq!(helper_f7a9ec(context, 1, 0, 0, TABLE, GuestAddress::new(OUTPUT + 4)), Ok(()));
            assert_eq!(helper_f7a9ec(context, 6, 0, 0, TABLE, GuestAddress::new(OUTPUT + 8)), Ok(()));
            assert_eq!(helper_f7a9ec(context, 6, 3, 0, TABLE, GuestAddress::new(OUTPUT + 12)), Ok(()));
        });
        assert_eq!(half(&memory, 0), 0x8000 + 202);
        assert_eq!(half(&memory, 4), 0x8000 + 211);
        assert_eq!(half(&memory, 8), 0x8000 + 193);
        assert_eq!(half(&memory, 12), 0x8000 + 172);
    }
}
