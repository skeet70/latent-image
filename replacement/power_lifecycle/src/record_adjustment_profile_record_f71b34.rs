//! The profile record the image-5 adjustment bodies fill from a range.
//!
//! Image-5 bodies 0x00f71b34, 0x00f71d78 and 0x00f71e70.
use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext, StackAlignment};
use crate::record_adjustment_mode_groups_f704c4::{helper_f713bc, helper_f71470, helper_f71e54};
use crate::record_adjustment_range_ef7380::{helper_f71810, helper_f71874, helper_f719e8, helper_f71af4, AdjustmentRange};
use crate::record_profile_mapping::helper_7469a0;

/// The record the image-5 adjustment bodies 0x00f71b34-0x00f71e70 fill.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfileRecord(pub GuestAddress);

impl ProfileRecord {
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

    pub fn mode<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
        self.word(context, 0x04)
    }

    pub fn variant<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
        self.word(context, 0x0c)
    }

    pub fn selection<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>) -> Result<u16, MemoryError> {
        self.half(context, 0x20)
    }

    pub fn current_limit<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>) -> Result<i16, MemoryError> {
        self.signed(context, 0x22)
    }

    pub fn scale<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
        self.word(context, 0x24)
    }

    pub fn set_scale<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>, value: u32) -> Result<(), MemoryError> {
        self.set_word(context, 0x64, value)
    }

    pub fn paired<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>) -> Result<i16, MemoryError> {
        self.signed(context, 0x2a)
    }

    pub fn feature<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
        self.word(context, 0x34)
    }

    pub fn flag<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>) -> Result<u8, MemoryError> {
        context.read_u8(self.0.checked_add(0x38)?)
    }

    pub fn bound<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>) -> Result<u16, MemoryError> {
        self.half(context, 0x68)
    }

    pub fn set_bound<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>, value: u16) -> Result<(), MemoryError> {
        self.set_half(context, 0x68, value)
    }

    pub fn set_base<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>, value: u16) -> Result<(), MemoryError> {
        self.set_half(context, 0x6a, value)
    }

    pub fn set_correction<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>, value: u16) -> Result<(), MemoryError> {
        self.set_half(context, 0x6e, value)
    }
}

/// The image-5 body at 0x00f71b34.
#[allow(clippy::too_many_arguments)]
pub fn helper_f71b34<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>,
    value: u32,
    _second: u32,
    record: ProfileRecord,
    range: AdjustmentRange,
    flag: u32,
) -> Result<(), MemoryError> {
    context.with_guest_frame(ByteCount(0x2a), StackAlignment::Word, |context, frame| {
        let local = AdjustmentRange(frame.base());
        let bound = clamped_bound(context, record, range)?;
        context.copy_bytes(frame.base(), range.address(), ByteCount(0x2a))?;
        local.set_target(context, bound)?;
        local.set_paired_target(context, bound)?;
        let initial = range.current_limit(context)? as u16;
        local.set_initial_limit(context, initial)?;
        let kind = record.feature(context)?;
        let scale;
        let correction;
        if helper_f71af4(context, kind, value, local)? != 0 {
            scale = range.lower_scale(context)?;
            correction = helper_f719e8(context, value, local)?;
        } else {
            let gate = if flag == 1 { 0 } else { 1 };
            let gate = if kind != 1 { gate | 1 } else { gate };
            if gate == 0 {
                scale = helper_f71874(context, value, bound as i16 as u32, range)?;
                context.copy_bytes(frame.base(), range.address(), ByteCount(0x2a))?;
                local.set_lower_scale(context, scale)?;
                local.set_target(context, bound)?;
                local.set_paired_target(context, bound)?;
                let initial = range.current_limit(context)? as u16;
                local.set_initial_limit(context, initial)?;
                correction = helper_f719e8(context, value, local)?;
            } else {
                scale = helper_f71810(context, value, bound as i16 as u32, range)?;
                correction = 0;
            }
        }
        record.set_scale(context, scale)?;
        record.set_bound(context, bound)?;
        record.set_correction(context, correction as u16)?;
        let base = range.input_maximum(context)?;
        record.set_base(context, base)
    })
}

fn clamped_bound<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: ProfileRecord, range: AdjustmentRange) -> Result<u16, MemoryError> {
    let mut bound = i32::from(range.input_maximum(context)? as i16);
    let current = i32::from(record.current_limit(context)?);
    if bound >= current {
        bound = current;
    }
    let first = i32::from(range.first_maximum(context)?);
    if bound < first {
        bound = first;
    }
    Ok(bound as u16)
}

/// The image-5 body at 0x00f71d78.
pub fn helper_f71d78<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32, record: ProfileRecord, range: AdjustmentRange) -> Result<i32, MemoryError> {
    let mode = record.mode(context)?;
    let mut result = 0;
    if mode != 47 && mode != 56 {
        let (scale, paired) = if helper_f71470(context, mode)? != 0 {
            let scale = record.scale(context)?;
            (scale, scale)
        } else {
            (range.upper_scale(context)?, range.lower_scale(context)?)
        };
        let (first, second) = if helper_f713bc(context, mode)? != 0 {
            let bound = record.bound(context)?;
            (u32::from(bound), u32::from(bound))
        } else {
            (range.input_maximum(context)? as u16 as u32, range.first_maximum(context)? as u16 as u32)
        };
        let converted_scale = helper_7469a0(context, scale)?;
        let converted_paired = helper_7469a0(context, paired)?;
        let paired_total = converted_paired.wrapping_add(second).wrapping_sub(u32::from(range.current_limit(context)? as u16));
        let first_total = first.wrapping_add(converted_scale) as u16;
        if i32::from(first_total as i16) < value as i32 {
            result = if mode != 24 { 2 } else { 0 };
        }
        if i32::from(paired_total as u16 as i16) > value as i32 && record.flag(context)? == 0 && mode != 24 {
            result |= 1;
        }
    }
    if i32::from(record.bound(context)? as i16) < i32::from(range.second_maximum(context)?) {
        result |= 4;
    }
    Ok(result)
}

/// The image-5 body at 0x00f71e70.
pub fn helper_f71e70<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32, _second: u32, record: ProfileRecord, range: AdjustmentRange) -> Result<(), MemoryError> {
    context.with_guest_frame(ByteCount(0x2a), StackAlignment::Word, |context, frame| {
        let local = AdjustmentRange(frame.base());
        let scale = record.scale(context)?;
        let current = record.current_limit(context)?;
        let (bound, correction) = if current == i16::MIN {
            let _ = record.paired(context)?;
            (helper_f71e54(context)? as u16, 0u16)
        } else {
            let bound = clamped_bound(context, record, range)?;
            context.copy_bytes(frame.base(), range.address(), ByteCount(0x2a))?;
            local.set_lower_scale(context, scale)?;
            local.set_target(context, bound)?;
            local.set_paired_target(context, bound)?;
            let initial = range.current_limit(context)? as u16;
            local.set_initial_limit(context, initial)?;
            let kind = record.feature(context)?;
            let correction = if helper_f71af4(context, kind, value, local)? == 0 { 0 } else { helper_f719e8(context, value, local)? };
            let bound = if record.variant(context)? == 2 {
                let mut adjusted = u16::wrapping_add(bound, record.selection(context)?) as i16 as i32;
                let low = i32::from(range.input_maximum(context)? as i16);
                let high = i32::from(range.first_maximum(context)? as i16);
                if adjusted >= low {
                    adjusted = low;
                }
                if adjusted < high {
                    adjusted = high;
                }
                adjusted as u16
            } else {
                bound
            };
            (bound, correction as u16)
        };
        record.set_scale(context, scale)?;
        record.set_bound(context, bound)?;
        record.set_correction(context, correction)?;
        let base = range.input_maximum(context)?;
        record.set_base(context, base)
    })
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 bodies at 0x00f71b34-0x00f71e70, with
    //! the mapped profile value 100 the configuration below yields.
    use super::*;
    use crate::record_address_space::{GuestDomain, GuestStack, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const CONFIG_POINTER: u32 = 0x0096_ca50;
    const CONFIG_DATA: u32 = 0x0020_0000;
    const CONFIG_LENGTH: usize = 0x40_0000;
    const MAPPED_WORD_KEY: usize = 0x3f24_00;
    const RECORD: u32 = 0x0080_0000;
    const RANGE: u32 = 0x0080_0100;
    const STACK: u32 = 0x0080_0200;

    struct Memory {
        pointer: [u8; 4],
        record: [u8; 0x70],
        range: [u8; 0x40],
        stack: [u8; 0x80],
    }

    fn with_memory<R>(config: &mut [u8], memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let mut pointer = CONFIG_DATA.to_le_bytes();
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(CONFIG_POINTER, 4), &mut pointer).unwrap(),
            MemoryRegion::new(descriptor(CONFIG_DATA, config.len() as u32), config).unwrap(),
            MemoryRegion::new(descriptor(RECORD, 0x70), &mut memory.record).unwrap(),
            MemoryRegion::new(descriptor(RANGE, 0x40), &mut memory.range).unwrap(),
            MemoryRegion::new(descriptor(STACK, 0x80), &mut memory.stack).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let stack = GuestStack::new(GuestAddress::new(STACK), ByteCount(0x80)).unwrap();
        body(&mut RecordContext::with_stack(space, (), stack))
    }

    fn memory() -> Memory {
        Memory { pointer: [0; 4], record: [0; 0x70], range: [0; 0x40], stack: [0; 0x80] }
    }

    fn configure(memory: &mut Memory) {
        memory.range[0x00..0x02].copy_from_slice(&40u16.to_le_bytes());
        memory.range[0x02..0x04].copy_from_slice(&10u16.to_le_bytes());
        memory.range[0x04..0x08].copy_from_slice(&5u32.to_le_bytes());
        memory.range[0x08..0x0c].copy_from_slice(&1u32.to_le_bytes());
        memory.range[0x0c..0x0e].copy_from_slice(&20u16.to_le_bytes());
        memory.range[0x22..0x24].copy_from_slice(&5u16.to_le_bytes());
        memory.record[0x04..0x08].copy_from_slice(&0u32.to_le_bytes());
        memory.record[0x22..0x24].copy_from_slice(&5u16.to_le_bytes());
        memory.record[0x24..0x28].copy_from_slice(&7u32.to_le_bytes());
        memory.record[0x34..0x38].copy_from_slice(&1u32.to_le_bytes());
        memory.record[0x68..0x6a].copy_from_slice(&10u16.to_le_bytes());
    }

    fn record() -> ProfileRecord {
        ProfileRecord(GuestAddress::new(RECORD))
    }

    fn range() -> AdjustmentRange {
        AdjustmentRange(GuestAddress::new(RANGE))
    }

    fn half(memory: &Memory, offset: usize) -> u16 {
        u16::from_le_bytes(memory.record[offset..offset + 2].try_into().unwrap())
    }

    fn word(memory: &Memory, offset: usize) -> u32 {
        u32::from_le_bytes(memory.record[offset..offset + 4].try_into().unwrap())
    }

    #[test]
    fn the_correction_path_records_the_range_scale() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        config[MAPPED_WORD_KEY..MAPPED_WORD_KEY + 2].copy_from_slice(&1000u16.to_le_bytes());
        let mut memory = memory();
        configure(&mut memory);
        with_memory(&mut config, &mut memory, |context| {
            assert_eq!(helper_f71e70(context, 50, 0, record(), range()), Ok(()));
        });
        assert_eq!(word(&memory, 0x64), 7);
        assert_eq!(half(&memory, 0x68), 10);
        assert_eq!(half(&memory, 0x6a), 40);
        assert_eq!(half(&memory, 0x6e), 5);
    }

    #[test]
    fn the_minimum_limit_uses_the_constant_bound() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        let mut memory = memory();
        configure(&mut memory);
        memory.record[0x22..0x24].copy_from_slice(&0x8000u16.to_le_bytes());
        with_memory(&mut config, &mut memory, |context| {
            assert_eq!(helper_f71e70(context, 50, 0, record(), range()), Ok(()));
        });
        assert_eq!(half(&memory, 0x68), 0xfb62);
        assert_eq!(half(&memory, 0x6e), 0);
    }

    #[test]
    fn the_variant_two_bound_adds_the_selection() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        config[MAPPED_WORD_KEY..MAPPED_WORD_KEY + 2].copy_from_slice(&1000u16.to_le_bytes());
        let mut memory = memory();
        configure(&mut memory);
        memory.record[0x0c..0x10].copy_from_slice(&2u32.to_le_bytes());
        memory.record[0x20..0x22].copy_from_slice(&3u16.to_le_bytes());
        with_memory(&mut config, &mut memory, |context| {
            assert_eq!(helper_f71e70(context, 50, 0, record(), range()), Ok(()));
        });
        assert_eq!(half(&memory, 0x68), 13);
    }

    #[test]
    fn the_flag_body_reports_the_four_bits() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        config[MAPPED_WORD_KEY..MAPPED_WORD_KEY + 2].copy_from_slice(&1000u16.to_le_bytes());
        let mut memory = memory();
        configure(&mut memory);
        with_memory(&mut config, &mut memory, |context| {
            assert_eq!(helper_f71d78(context, 150, record(), range()), Ok(6));
            assert_eq!(helper_f71d78(context, 100, record(), range()), Ok(5));
        });
        memory.record[0x04..0x08].copy_from_slice(&47u32.to_le_bytes());
        with_memory(&mut config, &mut memory, |context| {
            assert_eq!(helper_f71d78(context, 150, record(), range()), Ok(4));
        });
    }

    #[test]
    fn the_range_body_records_the_search_scale_and_the_correction() {
        let mut config = std::vec![0u8; CONFIG_LENGTH];
        config[MAPPED_WORD_KEY..MAPPED_WORD_KEY + 2].copy_from_slice(&1000u16.to_le_bytes());
        let mut memory = memory();
        configure(&mut memory);
        with_memory(&mut config, &mut memory, |context| {
            assert_eq!(helper_f71b34(context, 50, 0, record(), range(), 0), Ok(()));
        });
        assert_eq!(word(&memory, 0x64), 1);
        assert_eq!(half(&memory, 0x68), 10);
        assert_eq!(half(&memory, 0x6a), 40);
        assert_eq!(half(&memory, 0x6e), 5);
        memory.record[0x34..0x38].copy_from_slice(&0u32.to_le_bytes());
        with_memory(&mut config, &mut memory, |context| {
            assert_eq!(helper_f71b34(context, 50, 0, record(), range(), 0), Ok(()));
        });
        assert_eq!(word(&memory, 0x64), 1);
        assert_eq!(half(&memory, 0x6e), 0);
    }
}
