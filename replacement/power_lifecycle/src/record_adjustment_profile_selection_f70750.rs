//! Profile selection over the table at 0x00f83484: the gated key lookup and the
//! band search that stops at the first converted band above the value.
//!
//! Image-5 bodies 0x00f70750, 0x00f71ca8 and 0x00f780d0.
use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_adjustment_mode_groups_f704c4::helper_f704c4;
use crate::record_adjustment_profile_table_f77d90::{PROFILE_STRIDE, PROFILE_TABLE};
use crate::record_adjustment_range_ef7380::AdjustmentRange;
use crate::record_configuration::helper_69f8fc;
use crate::record_configuration_classification_69fffc::helper_69fffc;
use crate::record_indexed_state_f77db4::helper_f77db4;
use crate::record_profile_mapping::helper_7469a0;

const MODE_KEY: u32 = 0x62a8;
const MODE_RANGE_KEY: u32 = 0x62ab;
const MODE_FLAG_KEY: u32 = 0x62ac;

/// The words the image-5 body at 0x00f780d0 selects between.
pub const SELECTION_WORDS: u32 = 0x00f8_1114;

/// The image-5 body at 0x00f70750.
pub fn helper_f70750<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, source: GuestAddress) -> Result<i32, MemoryError> {
    let code = context.read_u32(source)?;
    let key = if code == 50 {
        MODE_KEY
    } else if code == 53 || code == 54 {
        MODE_RANGE_KEY
    } else {
        let flags = u32::from(context.read_u16(source.checked_add(4)?)?);
        let enabled = context.read_u32(source.checked_add(8)?)?;
        if helper_f704c4(context, code, flags, enabled)? == 0 {
            return Ok(0);
        }
        MODE_FLAG_KEY
    };
    Ok((helper_69fffc(context, key)?.wrapping_mul(10) as u16) as i16 as i32)
}

/// The image-5 body at 0x00f71ca8.
pub fn helper_f71ca8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32, range: AdjustmentRange) -> Result<u32, MemoryError> {
    let mut profile = range.upper_scale(context)?;
    let lower = range.lower_scale(context)?;
    while lower < profile {
        let band = crate::record_adjustment_range_ef7380::helper_f71c80(context, profile, range)?;
        let converted = helper_7469a0(context, profile)?;
        let total = converted.wrapping_add(band as u32);
        if (value as i32) >= (total as i32) {
            break;
        }
        profile -= 1;
    }
    Ok(profile)
}

/// The image-5 body at 0x00f780d0.
pub fn helper_f780d0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32) -> Result<u32, MemoryError> {
    let row = GuestAddress::new(PROFILE_TABLE.wrapping_add(index.wrapping_mul(PROFILE_STRIDE)));
    if context.read_u32(row.checked_add(0x0c)?)? == 1 {
        return context.read_u32(row.checked_add(4)?);
    }
    let key = context.read_u32(row.checked_add(4)?)?;
    let value = helper_69f8fc(context, key)?;
    let extra = helper_f77db4(context, index)?;
    let combined = extra.wrapping_add(value.wrapping_mul(6));
    context.read_u32(GuestAddress::new(SELECTION_WORDS.wrapping_add(combined.wrapping_mul(4))))
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 bodies at 0x00f70750, 0x00f71ca8 and
    //! 0x00f780d0, with the mapped profile value 100 the configuration below yields.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const CONFIG_POINTER: u32 = 0x0096_ca50;
    const CONFIG_DATA: u32 = 0x0020_0000;
    const CONFIG_LENGTH: usize = 0x40_0000;
    const MAPPED_WORD_KEY: usize = 0x3f24_00;
    const RANGE: u32 = 0x0080_0000;
    const SOURCE: u32 = 0x0080_0100;

    struct Memory {
        pointer: [u8; 4],
        range: [u8; 0x28],
        source: [u8; 0x10],
        table: [u8; 0x100],
        words: [u8; 0x100],
    }

    fn with_config<R>(config: &mut [u8], memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let mut pointer = CONFIG_DATA.to_le_bytes();
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(CONFIG_POINTER, 4), &mut pointer).unwrap(),
            MemoryRegion::new(descriptor(CONFIG_DATA, config.len() as u32), config).unwrap(),
            MemoryRegion::new(descriptor(RANGE, 0x28), &mut memory.range).unwrap(),
            MemoryRegion::new(descriptor(SOURCE, 0x10), &mut memory.source).unwrap(),
            MemoryRegion::new(descriptor(PROFILE_TABLE, 0x100), &mut memory.table).unwrap(),
            MemoryRegion::new(descriptor(SELECTION_WORDS, 0x100), &mut memory.words).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    fn memory() -> Memory {
        Memory { pointer: [0; 4], range: [0; 0x28], source: [0; 0x10], table: [0; 0x100], words: [0; 0x100] }
    }

    #[test]
    fn the_mode_keys_split_on_the_code_and_the_flag_gate() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        config[MODE_KEY as usize] = 5;
        config[MODE_RANGE_KEY as usize] = 6;
        config[MODE_FLAG_KEY as usize] = 7;
        let mut memory = memory();
        with_config(&mut config, &mut memory, |context| {
            context.write_u32(GuestAddress::new(SOURCE), 50).unwrap();
            assert_eq!(helper_f70750(context, GuestAddress::new(SOURCE)), Ok(50));
            context.write_u32(GuestAddress::new(SOURCE), 53).unwrap();
            assert_eq!(helper_f70750(context, GuestAddress::new(SOURCE)), Ok(60));
            context.write_u32(GuestAddress::new(SOURCE), 54).unwrap();
            assert_eq!(helper_f70750(context, GuestAddress::new(SOURCE)), Ok(60));
            context.write_u32(GuestAddress::new(SOURCE), 48).unwrap();
            context.write_u16(GuestAddress::new(SOURCE + 4), 0).unwrap();
            context.write_u32(GuestAddress::new(SOURCE + 8), 1).unwrap();
            assert_eq!(helper_f70750(context, GuestAddress::new(SOURCE)), Ok(70));
            context.write_u32(GuestAddress::new(SOURCE), 15).unwrap();
            assert_eq!(helper_f70750(context, GuestAddress::new(SOURCE)), Ok(0));
            context.write_u32(GuestAddress::new(SOURCE), 48).unwrap();
            context.write_u32(GuestAddress::new(SOURCE + 8), 0).unwrap();
            assert_eq!(helper_f70750(context, GuestAddress::new(SOURCE)), Ok(0));
        });
    }

    #[test]
    fn the_band_search_stops_at_the_first_band_above_the_value() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        config[MAPPED_WORD_KEY..MAPPED_WORD_KEY + 2].copy_from_slice(&1000u16.to_le_bytes());
        let mut memory = memory();
        memory.range[0x04..0x08].copy_from_slice(&5u32.to_le_bytes());
        memory.range[0x08..0x0c].copy_from_slice(&1u32.to_le_bytes());
        memory.range[0x14..0x16].copy_from_slice(&16u16.to_le_bytes());
        with_config(&mut config, &mut memory, |context| {
            let range = AdjustmentRange(GuestAddress::new(RANGE));
            assert_eq!(helper_f71ca8(context, 116, range), Ok(5));
            assert_eq!(helper_f71ca8(context, 115, range), Ok(1));
        });
    }

    #[test]
    fn the_gated_table_row_reads_its_key_or_its_selection_word() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        config[0x1234] = 2;
        let mut memory = memory();
        memory.table[0x04..0x08].copy_from_slice(&0x1234u32.to_le_bytes());
        memory.table[0x1c..0x20].copy_from_slice(&3u32.to_le_bytes());
        memory.words[60..64].copy_from_slice(&0xbeefu32.to_le_bytes());
        with_config(&mut config, &mut memory, |context| {
            assert_eq!(helper_f780d0(context, 0), Ok(0xbeef));
        });
        memory.table[0x0c..0x10].copy_from_slice(&1u32.to_le_bytes());
        with_config(&mut config, &mut memory, |context| {
            assert_eq!(helper_f780d0(context, 0), Ok(0x1234));
        });
    }
}
