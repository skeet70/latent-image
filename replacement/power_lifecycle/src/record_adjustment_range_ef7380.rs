//! The adjustment range block: its fields, its normalization and the profile
//! searches and corrections that read it.
//!
//! Image-5 bodies 0x00ef7380 and 0x00f71810-0x00f72a80.
use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_profile_mapping::helper_7469a0;

/// The image-5 body at 0x00ef7380.
pub fn helper_ef7380<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, levels: GuestAddress) -> Result<u32, MemoryError> {
    let first = context.read_u8(levels)?;
    let second = context.read_u8(levels.checked_add(1)?)?;
    Ok(match u32::from(first).wrapping_add(u32::from(second)).wrapping_sub(1) {
        0 => 2,
        1 => 3,
        2 => 4,
        3 => 5,
        _ => 1,
    })
}

/// The block the image-5 bodies 0x00f71810-0x00f72a80 read and write.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdjustmentRange(pub GuestAddress);

impl AdjustmentRange {
    pub const fn address(self) -> GuestAddress {
        self.0
    }

    fn word<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>, offset: u32) -> Result<u32, MemoryError> {
        context.read_u32(self.0.checked_add(offset)?)
    }

    fn set_word<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>, offset: u32, value: u32) -> Result<(), MemoryError> {
        context.write_u32(self.0.checked_add(offset)?, value)
    }

    fn half<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>, offset: u32) -> Result<u16, MemoryError> {
        context.read_u16(self.0.checked_add(offset)?)
    }

    fn set_half<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>, offset: u32, value: u16) -> Result<(), MemoryError> {
        context.write_u16(self.0.checked_add(offset)?, value)
    }

    fn signed<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>, offset: u32) -> Result<i16, MemoryError> {
        self.half(context, offset).map(|value| value as i16)
    }

    fn set_signed<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>, offset: u32, value: i16) -> Result<(), MemoryError> {
        self.set_half(context, offset, value as u16)
    }

    pub fn input_maximum<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>) -> Result<u16, MemoryError> {
        self.half(context, 0x00)
    }

    pub fn first_maximum<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>) -> Result<i16, MemoryError> {
        self.signed(context, 0x02)
    }

    pub fn set_first_maximum<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>, value: i16) -> Result<(), MemoryError> {
        self.set_signed(context, 0x02, value)
    }

    pub fn upper_scale<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
        self.word(context, 0x04)
    }

    pub fn set_upper_scale<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>, value: u32) -> Result<(), MemoryError> {
        self.set_word(context, 0x04, value)
    }

    pub fn lower_scale<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
        self.word(context, 0x08)
    }

    pub fn set_lower_scale<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>, value: u32) -> Result<(), MemoryError> {
        self.set_word(context, 0x08, value)
    }

    pub fn set_target<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>, value: u16) -> Result<(), MemoryError> {
        self.set_half(context, 0x1c, value)
    }

    pub fn set_paired_target<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>, value: u16) -> Result<(), MemoryError> {
        self.set_half(context, 0x1e, value)
    }

    pub fn second_maximum<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>) -> Result<i16, MemoryError> {
        self.signed(context, 0x0c)
    }

    pub fn set_second_maximum<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>, value: i16) -> Result<(), MemoryError> {
        self.set_signed(context, 0x0c, value)
    }

    pub fn lower_band<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>) -> Result<u16, MemoryError> {
        self.half(context, 0x12)
    }

    pub fn set_lower_band<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>, value: u16) -> Result<(), MemoryError> {
        self.set_half(context, 0x12, value)
    }

    pub fn upper_band<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>) -> Result<i16, MemoryError> {
        self.signed(context, 0x14)
    }

    pub fn set_upper_band<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>, value: i16) -> Result<(), MemoryError> {
        self.set_signed(context, 0x14, value)
    }

    pub fn selected_scale<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
        self.word(context, 0x18)
    }

    pub fn set_selected_scale<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>, value: u32) -> Result<(), MemoryError> {
        self.set_word(context, 0x18, value)
    }

    pub fn target<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>) -> Result<u16, MemoryError> {
        self.half(context, 0x1c)
    }

    pub fn paired_target<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>) -> Result<u16, MemoryError> {
        self.half(context, 0x1e)
    }

    pub fn initial_limit<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>) -> Result<u16, MemoryError> {
        self.half(context, 0x20)
    }

    pub fn set_initial_limit<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>, value: u16) -> Result<(), MemoryError> {
        self.set_half(context, 0x20, value)
    }

    pub fn current_limit<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>) -> Result<i16, MemoryError> {
        self.signed(context, 0x22)
    }

    pub fn set_current_limit<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>, value: i16) -> Result<(), MemoryError> {
        self.set_signed(context, 0x22, value)
    }

    pub fn final_limit<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>) -> Result<i16, MemoryError> {
        self.signed(context, 0x24)
    }

    pub fn set_final_limit<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>, value: i16) -> Result<(), MemoryError> {
        self.set_signed(context, 0x24, value)
    }
}

/// The image-5 body at 0x00f71810.
pub fn helper_f71810<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, level: u32, offset: u32, range: AdjustmentRange) -> Result<u32, MemoryError> {
    let difference = i32::from(level.wrapping_sub(offset) as u16 as i16);
    let mut profile = range.upper_scale(context)?;
    let lower = range.lower_scale(context)?;
    while lower < profile {
        let current = helper_7469a0(context, profile)? as i32;
        let previous = helper_7469a0(context, profile - 1)? as i32;
        let sum = current.wrapping_add(previous);
        let midpoint = sum.wrapping_add((sum as u32 >> 31) as i32) >> 1;
        if difference > midpoint {
            return Ok(profile);
        }
        profile -= 1;
    }
    Ok(profile)
}

/// The image-5 body at 0x00f71874.
pub fn helper_f71874<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, level: u32, offset: u32, range: AdjustmentRange) -> Result<u32, MemoryError> {
    let difference = i32::from(level.wrapping_sub(offset) as u16 as i16);
    let mut profile = range.upper_scale(context)?;
    let lower = range.lower_scale(context)?;
    while lower < profile {
        let previous = helper_7469a0(context, profile - 1)? as i32;
        if difference > previous {
            return Ok(profile);
        }
        profile -= 1;
    }
    Ok(profile)
}

/// The image-5 body at 0x00f719e8.
pub fn helper_f719e8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, level: u32, range: AdjustmentRange) -> Result<i32, MemoryError> {
    let level = level as i32;
    let current_limit = range.current_limit(context)?;
    if current_limit <= 0 {
        return Ok(0);
    }
    let lower_scale = range.lower_scale(context)?;
    let conversion = helper_7469a0(context, lower_scale)?;
    let target = u32::from(range.target(context)?);
    let initial = range.initial_limit(context)?;
    let first = conversion.wrapping_add(target) as u16;
    if i32::from(first as i16) <= level {
        return Ok(0);
    }
    let second = target.wrapping_sub(u32::from(initial)).wrapping_add(conversion) as u16;
    if level > i32::from(second as i16) {
        return Ok(i32::from(first.wrapping_sub(level as u16) as i16));
    }
    let paired = u32::from(range.paired_target(context)?);
    let third = paired.wrapping_sub(u32::from(initial)).wrapping_add(conversion) as u16;
    if i32::from(third as i16) <= level {
        return Ok(i32::from(initial as i16));
    }
    let last = paired.wrapping_sub(u32::from(current_limit as u16)).wrapping_add(conversion) as u16;
    if i32::from(last as i16) >= level {
        return Ok(i32::from(current_limit));
    }
    Ok(i32::from(third.wrapping_add(initial.wrapping_sub(level as u16)) as i16))
}

/// The image-5 body at 0x00f71af4.
pub fn helper_f71af4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, kind: u32, value: u32, range: AdjustmentRange) -> Result<i32, MemoryError> {
    if kind != 1 {
        return Ok(0);
    }
    let lower_scale = range.lower_scale(context)?;
    let conversion = helper_7469a0(context, lower_scale)?;
    let target = range.target(context)?;
    let boundary = conversion.wrapping_add(target as i16 as u32);
    Ok(i32::from((value as i32) < (boundary as i32)))
}

/// The image-5 body at 0x00f71c80.
pub fn helper_f71c80<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32, range: AdjustmentRange) -> Result<i32, MemoryError> {
    let selected = if range.selected_scale(context)? >= value { range.lower_band(context)? } else { range.upper_band(context)? as u16 };
    Ok(i32::from(selected as i16))
}

/// The image-5 body at 0x00f71d04.
pub fn helper_f71d04<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, reference: u32, fallback: u32, range: AdjustmentRange) -> Result<u32, MemoryError> {
    let selected = if reference != 0 { reference } else { fallback };
    let lower_scale = range.lower_scale(context)?;
    let conversion = helper_7469a0(context, lower_scale)?;
    let offset = crate::record_profile_globals::helper_772f28(context, 3)?;
    let boundary = i32::from(selected.wrapping_add(conversion).wrapping_sub(offset) as u16 as i16);
    let mut profile = range.upper_scale(context)?;
    let lower = lower_scale;
    while lower < profile {
        let previous = helper_7469a0(context, profile - 1)? as i32;
        if boundary > previous {
            return Ok(profile);
        }
        profile -= 1;
    }
    Ok(profile)
}

/// The image-5 body at 0x00f729d0.
pub fn helper_f729d0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, range: AdjustmentRange) -> Result<(), MemoryError> {
    let maximum = range.input_maximum(context)? as i16;
    if range.first_maximum(context)? > maximum {
        range.set_first_maximum(context, maximum)?;
    }
    if range.second_maximum(context)? > maximum {
        range.set_second_maximum(context, maximum)?;
    }
    if range.second_maximum(context)? < range.first_maximum(context)? {
        let first = range.first_maximum(context)?;
        range.set_second_maximum(context, first)?;
    }
    if range.upper_scale(context)? < range.lower_scale(context)? {
        let lower = range.lower_scale(context)?;
        range.set_upper_scale(context, lower)?;
    }
    let upper_bound = range.upper_band(context)?;
    let lower_band = range.lower_band(context)? as i16;
    if upper_bound > lower_band {
        let lower = range.lower_band(context)?;
        range.set_upper_band(context, lower as i16)?;
    }
    let lower_bound = range.lower_band(context)? as i16;
    if lower_bound < range.second_maximum(context)? {
        let second = range.second_maximum(context)?;
        range.set_lower_band(context, second as u16)?;
    }
    if range.upper_band(context)? < range.second_maximum(context)? {
        let second = range.second_maximum(context)?;
        range.set_upper_band(context, second)?;
    }
    if range.selected_scale(context)? < range.lower_scale(context)? {
        let lower = range.lower_scale(context)?;
        range.set_selected_scale(context, lower)?;
    }
    if range.upper_scale(context)? < range.selected_scale(context)? {
        let upper = range.upper_scale(context)?;
        range.set_selected_scale(context, upper)?;
    }
    Ok(())
}

/// The image-5 body at 0x00f72a80.
pub fn helper_f72a80<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>,
    strength: i32,
    first_limit: i32,
    final_limit: i32,
    range: AdjustmentRange,
) -> Result<(), MemoryError> {
    if strength > 100 {
        return Ok(());
    }
    let current = i32::from(range.current_limit(context)?);
    let first = if first_limit >= current { current } else { first_limit };
    range.set_current_limit(context, first as i16)?;
    range.set_initial_limit(context, first as u16)?;
    let final_value = i32::from(range.final_limit(context)?);
    let last = if final_limit >= final_value { final_value } else { final_limit };
    range.set_final_limit(context, last as i16)
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 bodies at 0x00ef7380 and
    //! 0x00f71810-0x00f72a80, with the mapped profile value 100 the
    //! configuration of the tests below yields.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const CONFIG_POINTER: u32 = 0x0096_ca50;
    const CONFIG_DATA: u32 = 0x0020_0000;
    const CONFIG_LENGTH: usize = 0x40_0000;
    const MAPPED_WORD_KEY: u32 = 0x3f24_00;
    const RANGE: u32 = 0x0080_0000;
    const LEVELS: u32 = 0x0080_0100;

    fn at(key: u32) -> usize {
        key as usize
    }

    fn put_half(config: &mut [u8], key: u32, value: u16) {
        let index = at(key);
        config[index..index + 2].copy_from_slice(&value.to_le_bytes());
    }

    struct Memory {
        pointer: [u8; 4],
        range: [u8; 0x28],
        levels: [u8; 4],
    }

    fn with_config<R>(
        config: &mut [u8],
        memory: &mut Memory,
        body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>, AdjustmentRange) -> R,
    ) -> R {
        let mut pointer = CONFIG_DATA.to_le_bytes();
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(CONFIG_POINTER, 4), &mut pointer).unwrap(),
            MemoryRegion::new(descriptor(CONFIG_DATA, config.len() as u32), config).unwrap(),
            MemoryRegion::new(descriptor(RANGE, 0x28), &mut memory.range).unwrap(),
            MemoryRegion::new(descriptor(LEVELS, 4), &mut memory.levels).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()), AdjustmentRange(GuestAddress::new(RANGE)))
    }

    fn set_word(memory: &mut Memory, offset: usize, value: u32) {
        memory.range[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn set_half(memory: &mut Memory, offset: usize, value: u16) {
        memory.range[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn word(memory: &Memory, offset: usize) -> u32 {
        u32::from_le_bytes(memory.range[offset..offset + 4].try_into().unwrap())
    }

    fn half(memory: &Memory, offset: usize) -> u16 {
        u16::from_le_bytes(memory.range[offset..offset + 2].try_into().unwrap())
    }

    fn scaled(memory: &mut Memory) {
        set_word(memory, 0x04, 5);
        set_word(memory, 0x08, 1);
    }

    #[test]
    fn the_level_pair_selects_the_encoded_value() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        put_half(&mut config, MAPPED_WORD_KEY, 1000);
        let mut memory = Memory { pointer: [0; 4], range: [0; 0x28], levels: [0; 4] };
        for (first, second, expected) in [(0u8, 0u8, 1u32), (1, 0, 2), (1, 1, 3), (1, 2, 4), (2, 2, 5), (2, 3, 1)] {
            memory.levels = [first, second, 0, 0];
            with_config(&mut config, &mut memory, |context, _| {
                assert_eq!(helper_ef7380(context, GuestAddress::new(LEVELS)), Ok(expected));
            });
        }
    }

    #[test]
    fn the_midpoint_search_stops_above_the_pair_average() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        put_half(&mut config, MAPPED_WORD_KEY, 1000);
        let mut memory = Memory { pointer: [0; 4], range: [0; 0x28], levels: [0; 4] };
        scaled(&mut memory);
        with_config(&mut config, &mut memory, |context, range| {
            assert_eq!(helper_f71810(context, 200, 0, range), Ok(5));
            assert_eq!(helper_f71810(context, 100, 0, range), Ok(1));
            assert_eq!(helper_f71810(context, 101, 0, range), Ok(5));
        });
    }

    #[test]
    fn the_lower_search_stops_above_the_previous_entry() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        put_half(&mut config, MAPPED_WORD_KEY, 1000);
        let mut memory = Memory { pointer: [0; 4], range: [0; 0x28], levels: [0; 4] };
        scaled(&mut memory);
        with_config(&mut config, &mut memory, |context, range| {
            assert_eq!(helper_f71874(context, 200, 0, range), Ok(5));
            assert_eq!(helper_f71874(context, 100, 0, range), Ok(1));
        });
    }

    #[test]
    fn the_reference_search_uses_the_reference_or_the_fallback() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        put_half(&mut config, MAPPED_WORD_KEY, 1000);
        let mut memory = Memory { pointer: [0; 4], range: [0; 0x28], levels: [0; 4] };
        scaled(&mut memory);
        with_config(&mut config, &mut memory, |context, range| {
            assert_eq!(helper_f71d04(context, 200, 0, range), Ok(5));
            assert_eq!(helper_f71d04(context, 0, 0, range), Ok(1));
            assert_eq!(helper_f71d04(context, 0, 200, range), Ok(5));
        });
    }

    #[test]
    fn the_correction_follows_each_range_boundary() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        put_half(&mut config, MAPPED_WORD_KEY, 1000);
        let mut memory = Memory { pointer: [0; 4], range: [0; 0x28], levels: [0; 4] };
        scaled(&mut memory);
        set_half(&mut memory, 0x1c, 20);
        set_half(&mut memory, 0x20, 10);
        set_half(&mut memory, 0x22, 20);
        with_config(&mut config, &mut memory, |context, range| {
            assert_eq!(helper_f719e8(context, 125, range), Ok(0));
            assert_eq!(helper_f719e8(context, 115, range), Ok(5));
            assert_eq!(helper_f719e8(context, 95, range), Ok(10));
            assert_eq!(helper_f719e8(context, 75, range), Ok(20));
            assert_eq!(helper_f719e8(context, 85, range), Ok(15));
        });
        set_half(&mut memory, 0x22, 0);
        with_config(&mut config, &mut memory, |context, range| assert_eq!(helper_f719e8(context, 50, range), Ok(0)));
    }

    #[test]
    fn the_above_level_predicate_needs_kind_one() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        put_half(&mut config, MAPPED_WORD_KEY, 1000);
        let mut memory = Memory { pointer: [0; 4], range: [0; 0x28], levels: [0; 4] };
        scaled(&mut memory);
        set_half(&mut memory, 0x1c, 20);
        with_config(&mut config, &mut memory, |context, range| {
            assert_eq!(helper_f71af4(context, 0, 0, range), Ok(0));
            assert_eq!(helper_f71af4(context, 1, 119, range), Ok(1));
            assert_eq!(helper_f71af4(context, 1, 120, range), Ok(0));
        });
    }

    #[test]
    fn the_threshold_selection_reads_one_of_the_two_bands() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        put_half(&mut config, MAPPED_WORD_KEY, 1000);
        let mut memory = Memory { pointer: [0; 4], range: [0; 0x28], levels: [0; 4] };
        set_word(&mut memory, 0x18, 0x10);
        set_half(&mut memory, 0x12, 0x2222);
        set_half(&mut memory, 0x14, 0x3333);
        with_config(&mut config, &mut memory, |context, range| {
            assert_eq!(helper_f71c80(context, 0x10, range), Ok(0x2222));
            assert_eq!(helper_f71c80(context, 0x11, range), Ok(0x3333));
        });
    }

    #[test]
    fn normalization_clamps_the_scale_and_band_order() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        let mut memory = Memory { pointer: [0; 4], range: [0; 0x28], levels: [0; 4] };
        set_half(&mut memory, 0x00, 40);
        set_half(&mut memory, 0x02, 50);
        set_half(&mut memory, 0x0c, 60);
        set_word(&mut memory, 0x04, 1);
        set_word(&mut memory, 0x08, 9);
        set_half(&mut memory, 0x12, 70);
        set_half(&mut memory, 0x14, 80);
        set_word(&mut memory, 0x18, 2);
        with_config(&mut config, &mut memory, |context, range| {
            assert_eq!(helper_f729d0(context, range), Ok(()));
        });
        assert_eq!(half(&memory, 0x02), 40);
        assert_eq!(half(&memory, 0x0c), 40);
        assert_eq!(word(&memory, 0x04), 9);
        assert_eq!(half(&memory, 0x12), 70);
        assert_eq!(half(&memory, 0x14), 70);
        assert_eq!(word(&memory, 0x18), 9);
    }

    #[test]
    fn normalization_raises_the_bands_to_the_second_maximum() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        let mut memory = Memory { pointer: [0; 4], range: [0; 0x28], levels: [0; 4] };
        set_half(&mut memory, 0x00, 100);
        set_half(&mut memory, 0x02, 50);
        set_half(&mut memory, 0x0c, 40);
        set_word(&mut memory, 0x04, 9);
        set_word(&mut memory, 0x08, 1);
        set_half(&mut memory, 0x12, 10);
        set_half(&mut memory, 0x14, 5);
        set_word(&mut memory, 0x18, 9);
        with_config(&mut config, &mut memory, |context, range| {
            assert_eq!(helper_f729d0(context, range), Ok(()));
        });
        assert_eq!(half(&memory, 0x02), 50);
        assert_eq!(half(&memory, 0x0c), 50);
        assert_eq!(half(&memory, 0x12), 50);
        assert_eq!(half(&memory, 0x14), 50);
        assert_eq!(word(&memory, 0x18), 9);
    }

    #[test]
    fn the_range_limit_only_applies_below_one_hundred() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        let mut memory = Memory { pointer: [0; 4], range: [0; 0x28], levels: [0; 4] };
        set_half(&mut memory, 0x20, 11);
        set_half(&mut memory, 0x22, 50);
        set_half(&mut memory, 0x24, 70);
        with_config(&mut config, &mut memory, |context, range| {
            assert_eq!(helper_f72a80(context, 101, 30, 90, range), Ok(()));
        });
        assert_eq!(half(&memory, 0x20), 11);
        assert_eq!(half(&memory, 0x22), 50);
        assert_eq!(half(&memory, 0x24), 70);
        with_config(&mut config, &mut memory, |context, range| {
            assert_eq!(helper_f72a80(context, 100, 30, 90, range), Ok(()));
        });
        assert_eq!(half(&memory, 0x20), 30);
        assert_eq!(half(&memory, 0x22), 30);
        assert_eq!(half(&memory, 0x24), 70);
    }
}
