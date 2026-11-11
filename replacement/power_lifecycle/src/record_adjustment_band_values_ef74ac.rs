//! Band values read from one configuration byte, and the selection gates that
//! pass them to the profile correction.
//!
//! Image-5 bodies 0x00ef74ac, 0x00f526fc, 0x00f5f368, 0x00f714d4, 0x00f72060,
//! 0x00f723b0 and 0x00ff5690.
use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_configuration::helper_69f8fc;
use crate::record_configuration_classification_69fffc::helper_69fffc;
use crate::record_configuration_search_746c60::helper_746cfc;
use crate::record_service_status_6b6ee8::helper_ff6fc0;
use crate::record_adjustment_mode_groups_f704c4::helper_f713bc;
use crate::record_adjustment_selection_f408b4::{helper_f5f324, helper_f7a9ec};

const BAND_KEY_BASE: u32 = 0x4a24;
const SELECTION_KEY_BASE: u32 = 0x61c0;
const BOUND_SENTINEL: u32 = 0x7fff;
const MODE_CORRECTION_MODES: [u32; 2] = [47, 56];
const MODE_THRESHOLD_OFFSET: u16 = 0x8000;

/// The image-5 body at 0x00ef74ac.
pub fn helper_ef74ac<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32) -> Result<i32, MemoryError> {
    Ok((helper_69fffc(context, index.wrapping_add(BAND_KEY_BASE))?.wrapping_mul(10) as u16) as i16 as i32)
}

/// The image-5 body at 0x00f723b0.
pub fn helper_f723b0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32) -> Result<i32, MemoryError> {
    Ok((helper_69fffc(context, index.wrapping_add(SELECTION_KEY_BASE))?.wrapping_mul(10) as u16) as i16 as i32)
}

/// The image-5 body at 0x00f714d4.
pub fn helper_f714d4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, mode: u32, bound: u32) -> Result<i32, MemoryError> {
    if bound == BOUND_SENTINEL {
        return Ok(0);
    }
    if matches!(mode, 53 | 54 | 57) {
        return Ok(0);
    }
    Ok((1 - helper_f713bc(context, mode)?).max(0))
}

/// The image-5 body at 0x00f72060.
pub fn helper_f72060<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, source: GuestAddress, bank: u32, destination: GuestAddress) -> Result<(), MemoryError> {
    let first = context.read_u16(source)?;
    let second = context.read_u16(source.checked_add(2)?)?;
    context.write_u16(destination.checked_add(0x12)?, first)?;
    context.write_u16(destination.checked_add(0x14)?, second)?;
    let bound = context.read_u16(source.checked_add(4)?)?;
    let found = helper_746cfc(context, bank, u32::from(bound))?;
    context.write_u32(destination.checked_add(0x18)?, found)
}

/// The image-5 body at 0x00f526fc.
pub fn helper_f526fc<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>,
    first: u32,
    second: u32,
    third: u32,
    base: u32,
    destination: GuestAddress,
) -> Result<(), MemoryError> {
    helper_f7a9ec(context, first, second, third, base, destination)
}

/// The image-5 body at 0x00f5f368.
#[allow(clippy::too_many_arguments)]
pub fn helper_f5f368<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>,
    mode: u32,
    _unused: u32,
    first: u32,
    second: u32,
    third: u32,
    fourth: u32,
) -> Result<i32, MemoryError> {
    if !MODE_CORRECTION_MODES.contains(&mode) {
        return Ok(0);
    }
    if fourth as u16 == MODE_THRESHOLD_OFFSET {
        return Ok(0);
    }
    let value = helper_f5f324(context, first, second, third, u32::from(fourth as u16))?;
    Ok(i32::from(value as i16))
}

/// The image-5 body at 0x00ff5690.
pub fn helper_ff5690<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress) -> Result<(), MemoryError> {
    helper_ff6fc0(context, destination)
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 bodies at 0x00ef74ac-0x00ff5690.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};
    use crate::record_service_status_6b6ee8::PIPELINE_SNAPSHOT_COUNTER;

    const CONFIG_POINTER: u32 = 0x0096_ca50;
    const CONFIG_DATA: u32 = 0x0020_0000;
    const CONFIG_LENGTH: usize = 0x40_0000;
    const MAPPED_WORD_KEY: usize = 0x3f24_00;
    const MAPPED_ENTRY_KEY: usize = 0x3f46_00;
    const SOURCE: u32 = 0x0080_0000;
    const DESTINATION: u32 = 0x0080_0100;
    const COUNTER: u32 = 0x0080_1000;

    struct Memory {
        pointer: [u8; 4],
        source: [u8; 8],
        destination: [u8; 0x400],
        counter: [u8; 4],
        pipeline: [u8; 4],
    }

    fn with_config<R>(config: &mut [u8], memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let mut pointer = CONFIG_DATA.to_le_bytes();
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(CONFIG_POINTER, 4), &mut pointer).unwrap(),
            MemoryRegion::new(descriptor(CONFIG_DATA, config.len() as u32), config).unwrap(),
            MemoryRegion::new(descriptor(SOURCE, 8), &mut memory.source).unwrap(),
            MemoryRegion::new(descriptor(DESTINATION, 0x400), &mut memory.destination).unwrap(),
            MemoryRegion::new(descriptor(COUNTER, 4), &mut memory.counter).unwrap(),
            MemoryRegion::new(descriptor(PIPELINE_SNAPSHOT_COUNTER, 4), &mut memory.pipeline).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    fn memory() -> Memory {
        Memory { pointer: [0; 4], source: [0; 8], destination: [0; 0x400], counter: [0; 4], pipeline: [0; 4] }
    }

    #[test]
    fn both_band_reads_scale_one_configuration_byte() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        config[(2 + BAND_KEY_BASE) as usize] = 5;
        config[(2 + SELECTION_KEY_BASE) as usize] = 0x80;
        let mut memory = memory();
        with_config(&mut config, &mut memory, |context| {
            assert_eq!(helper_ef74ac(context, 2), Ok(50));
            assert_eq!(helper_f723b0(context, 2), Ok(1280));
        });
    }

    #[test]
    fn the_threshold_gate_rejects_the_sentinel_and_three_modes() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        let mut memory = memory();
        with_config(&mut config, &mut memory, |context| {
            assert_eq!(helper_f714d4(context, 0x18, 0x7fff), Ok(0));
            assert_eq!(helper_f714d4(context, 53, 0), Ok(0));
            assert_eq!(helper_f714d4(context, 54, 0), Ok(0));
            assert_eq!(helper_f714d4(context, 57, 0), Ok(0));
            assert_eq!(helper_f714d4(context, 0x18, 0), Ok(0));
            assert_eq!(helper_f714d4(context, 0x2e, 0), Ok(1));
        });
    }

    #[test]
    fn the_bound_copy_writes_the_range_and_the_search_result() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        config[0x3e22c8] = 2;
        config[0x3e232d] = 5;
        config[MAPPED_ENTRY_KEY] = 0xff;
        let mut memory = memory();
        memory.source[0..2].copy_from_slice(&0x0101u16.to_le_bytes());
        memory.source[2..4].copy_from_slice(&0x0202u16.to_le_bytes());
        memory.source[4..6].copy_from_slice(&101u16.to_le_bytes());
        with_config(&mut config, &mut memory, |context| {
            assert_eq!(helper_f72060(context, GuestAddress::new(SOURCE), 0, GuestAddress::new(DESTINATION)), Ok(()));
        });
        assert_eq!(&memory.destination[0x12..0x14], &0x0101u16.to_le_bytes());
        assert_eq!(&memory.destination[0x14..0x16], &0x0202u16.to_le_bytes());
        assert_eq!(&memory.destination[0x18..0x1c], &5u32.to_le_bytes());
    }

    #[test]
    fn the_forwarding_bodies_keep_their_arguments() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        let mut memory = memory();
        for index in 0..0x40usize {
            memory.destination[index] = index as u8;
        }
        memory.pipeline = 0x0a0b_0c0du32.to_le_bytes();
        with_config(&mut config, &mut memory, |context| {
            assert_eq!(helper_ff5690(context, GuestAddress::new(SOURCE)), Ok(()));
            assert_eq!(helper_f526fc(context, 0, 0, 0, DESTINATION, GuestAddress::new(COUNTER)), Ok(()));
        });
        assert_eq!(&memory.source[0..4], &0x0a0b_0c0du32.to_le_bytes());
        assert_eq!(&memory.counter[0..2], &memory.destination[404..406]);
    }

    #[test]
    fn the_correction_gate_needs_one_of_two_modes_and_a_non_sentinel() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        config[MAPPED_WORD_KEY..MAPPED_WORD_KEY + 2].copy_from_slice(&1000u16.to_le_bytes());
        let mut memory = memory();
        with_config(&mut config, &mut memory, |context| {
            assert_eq!(helper_f5f368(context, 48, 0, 300, 100, 0, 50), Ok(0));
            assert_eq!(helper_f5f368(context, 47, 0, 300, 100, 0, 0x8000), Ok(0));
            assert_eq!(helper_f5f368(context, 47, 0, 300, 100, 0, 50), Ok(150));
            assert_eq!(helper_f5f368(context, 56, 0, 1000, 0, 0, 0), Ok(200));
        });
    }
}
