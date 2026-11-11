//! Stock-shaped bodies over the measurement block at 0x016c_0fb8.
//!
//! Each body is the image-5 instruction sequence at the address in its name.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

pub const STATE: u32 = 0x016c_0fb8;
const HISTORY_NEWEST: u32 = 0x0efc;
const HISTORY_MIDDLE: u32 = 0x0f04;
const HISTORY_OLDEST: u32 = 0x0f08;
const VALUE: u32 = 0x0f0c;
const VALUE_PREVIOUS: u32 = 0x0f10;
/// Written by 0x00f3f7a8.
const SCALED_BYTE: u32 = 0x0f14;
const CHOICE_BYTE: u32 = 0x0f15;
const SELECTOR: u32 = 0x0f20;
const MODE: u32 = 0x0f94;
const FLAG: u32 = 0x0f9c;
const COUNTER: u32 = 0x0ef4;

/// The levels 0x00f3f7a8 treats as selected.
const SELECTED_LEVELS: u32 = 0x03c1_8071;

fn at(offset: u32) -> GuestAddress {
    GuestAddress::new(STATE.wrapping_add(offset))
}

/// 0x00f3ee60
pub fn helper_f3ee60<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    let sample = crate::record_configuration::helper_69f8fc(context, 0x8876)?;
    let first = crate::record_configuration::helper_69f8fc(context, 0x8873)?;
    let second = crate::record_configuration::helper_69f8fc(context, 0x8874)?;
    let third = crate::record_configuration::helper_69f8fc(context, 0x8875)?;
    let newest = context.read_u32(at(HISTORY_NEWEST))?;
    let middle = context.read_u32(at(HISTORY_MIDDLE))?;
    let oldest = context.read_u32(at(HISTORY_OLDEST))?;
    let value = context.read_u32(at(VALUE))?;
    let previous = context.read_u32(at(VALUE_PREVIOUS))?;
    let weighted = newest.wrapping_add(oldest).wrapping_add(middle.wrapping_shl(1));
    let scaled = ((u64::from(weighted) * u64::from(third as u8)) as i64) / 100;
    let delta = (i64::from(value as i32) * i64::from(first as u8) - i64::from(previous as i32) * i64::from(second as u8)) / 100;
    let measured = (scaled as i32).wrapping_add(delta as i32);
    let held = if (newest as i32) > 0 { newest } else { 0 };
    let updated = if (held as i32) < measured { measured as u32 } else { held };
    context.write_u32(at(HISTORY_OLDEST), middle)?;
    context.write_u32(at(HISTORY_MIDDLE), newest)?;
    context.write_u32(at(VALUE_PREVIOUS), value)?;
    context.write_u32(at(VALUE), updated)?;
    let selector = context.read_u32(at(SELECTOR))?;
    let flag = context.read_u32(at(FLAG))?;
    let factor = if selector == 0 {
        if flag != 1 { 100 } else { 100u32.wrapping_mul(u32::from(sample as u8)) & 0xffff }
    } else if selector == 1 {
        if flag != 0 { 100 } else { u32::from(100u32.checked_div(u32::from(sample as u8)).unwrap_or(0) as u8) }
    } else {
        100
    };
    Ok((updated.wrapping_mul(factor) as i32 / 100) as u32)
}

/// 0x00f3f7a8
pub fn helper_f3f7a8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, level: u32) -> Result<(), MemoryError> {
    let selected = level <= 25 && SELECTED_LEVELS & (1 << level) != 0;
    let both = context.read_u32(at(MODE))? == 1 && context.read_u32(at(FLAG))? == 1;
    let first = match (selected, both) {
        (true, true) => 0x8865,
        (true, false) => 0x8862,
        (false, true) => 0x8863,
        (false, false) => 0x8860,
    };
    let value = crate::record_configuration::helper_69f8fc(context, first)?;
    context.write_u8(at(SCALED_BYTE), value as u8)?;
    let counter = context.read_u32(at(COUNTER))?;
    let limit = crate::record_configuration::helper_69ff7c(context, 0x885d)? as u32;
    let reached = counter >= limit;
    let second = match (selected, both, reached) {
        (true, true, true) => 0x885a,
        (true, true, false) => 0x885c,
        (true, false, true) => 0x8855,
        (true, false, false) => 0x8857,
        (false, true, true) => 0x8858,
        (false, true, false) => 0x885b,
        (false, false, true) => 0x8853,
        (false, false, false) => 0x8856,
    };
    let value = crate::record_configuration::helper_69f8fc(context, second)?;
    context.write_u8(at(CHOICE_BYTE), value as u8)
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 bodies at 0x00f3ee60 and 0x00f3f7a8.
    use super::*;
    use crate::record_address_space::{
        ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission,
    };

    const CONFIG: u32 = 0x0030_0000;

    struct Memory {
        config_pointer: [u8; 4],
        config: [u8; 0x9000],
        state: [u8; 0x1000],
    }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(0x0096_ca50, 4), &mut memory.config_pointer).unwrap(),
            MemoryRegion::new(descriptor(CONFIG, 0x9000), &mut memory.config).unwrap(),
            MemoryRegion::new(descriptor(STATE, 0x1000), &mut memory.state).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    fn sample() -> Memory {
        let mut memory = Memory { config_pointer: CONFIG.to_le_bytes(), config: [0; 0x9000], state: [0; 0x1000] };
        memory.config[0x8873] = 5;
        memory.config[0x8874] = 3;
        memory.config[0x8875] = 50;
        memory.config[0x8876] = 10;
        memory.state[HISTORY_NEWEST as usize..HISTORY_NEWEST as usize + 4].copy_from_slice(&100u32.to_le_bytes());
        memory.state[HISTORY_MIDDLE as usize..HISTORY_MIDDLE as usize + 4].copy_from_slice(&200u32.to_le_bytes());
        memory.state[HISTORY_OLDEST as usize..HISTORY_OLDEST as usize + 4].copy_from_slice(&300u32.to_le_bytes());
        memory.state[VALUE as usize..VALUE as usize + 4].copy_from_slice(&400u32.to_le_bytes());
        memory.state[VALUE_PREVIOUS as usize..VALUE_PREVIOUS as usize + 4].copy_from_slice(&350u32.to_le_bytes());
        memory
    }

    fn word(memory: &Memory, offset: u32) -> u32 {
        u32::from_le_bytes(memory.state[offset as usize..offset as usize + 4].try_into().unwrap())
    }

    #[test]
    fn the_weighted_measurement_shifts_the_history_and_scales_by_the_selected_mode() {
        let mut memory = sample();
        memory.state[FLAG as usize..FLAG as usize + 4].copy_from_slice(&1u32.to_le_bytes());
        with_memory(&mut memory, |context| assert_eq!(helper_f3ee60(context), Ok(4090)));
        assert_eq!(word(&memory, HISTORY_NEWEST), 100);
        assert_eq!(word(&memory, HISTORY_MIDDLE), 100);
        assert_eq!(word(&memory, HISTORY_OLDEST), 200);
        assert_eq!(word(&memory, VALUE), 409);
        assert_eq!(word(&memory, VALUE_PREVIOUS), 400);
    }

    fn scaled(selector: u32, flag: u32) -> u32 {
        let mut memory = sample();
        memory.state[SELECTOR as usize..SELECTOR as usize + 4].copy_from_slice(&selector.to_le_bytes());
        memory.state[FLAG as usize..FLAG as usize + 4].copy_from_slice(&flag.to_le_bytes());
        with_memory(&mut memory, |context| helper_f3ee60(context).unwrap())
    }

    #[test]
    fn each_selector_and_flag_pair_scales_the_updated_value() {
        assert_eq!(scaled(0, 0), 409);
        assert_eq!(scaled(1, 0), 40);
        assert_eq!(scaled(1, 1), 409);
        assert_eq!(scaled(2, 0), 409);
    }

    #[test]
    fn a_negative_measurement_is_held_at_zero() {
        let mut memory = sample();
        memory.state[HISTORY_NEWEST as usize..HISTORY_NEWEST as usize + 4].copy_from_slice(&0u32.to_le_bytes());
        memory.state[HISTORY_MIDDLE as usize..HISTORY_MIDDLE as usize + 4].copy_from_slice(&0u32.to_le_bytes());
        memory.state[HISTORY_OLDEST as usize..HISTORY_OLDEST as usize + 4].copy_from_slice(&0u32.to_le_bytes());
        memory.state[VALUE as usize..VALUE as usize + 4].copy_from_slice(&0u32.to_le_bytes());
        memory.state[VALUE_PREVIOUS as usize..VALUE_PREVIOUS as usize + 4].copy_from_slice(&1000u32.to_le_bytes());
        with_memory(&mut memory, |context| assert_eq!(helper_f3ee60(context), Ok(0)));
        assert_eq!(word(&memory, VALUE), 0);
    }

    fn sample_selection() -> Memory {
        let mut memory = Memory { config_pointer: CONFIG.to_le_bytes(), config: [0; 0x9000], state: [0; 0x1000] };
        for offset in 0x8853u32..0x8866 {
            memory.config[offset as usize] = offset as u8;
        }
        memory.config[0x885d] = 0x00;
        memory.config[0x885e] = 0x01;
        memory.state[MODE as usize..MODE as usize + 4].copy_from_slice(&1u32.to_le_bytes());
        memory.state[FLAG as usize..FLAG as usize + 4].copy_from_slice(&1u32.to_le_bytes());
        memory.state[COUNTER as usize..COUNTER as usize + 4].copy_from_slice(&300u32.to_le_bytes());
        memory
    }

    fn bytes(memory: &Memory) -> (u8, u8) {
        (memory.state[SCALED_BYTE as usize], memory.state[CHOICE_BYTE as usize])
    }

    #[test]
    fn the_level_and_flags_pick_one_of_four_key_pairs() {
        let mut memory = sample_selection();
        with_memory(&mut memory, |context| assert_eq!(helper_f3f7a8(context, 0), Ok(())));
        assert_eq!(bytes(&memory), (0x65, 0x5a));

        let mut memory = sample_selection();
        memory.state[COUNTER as usize..COUNTER as usize + 4].copy_from_slice(&100u32.to_le_bytes());
        with_memory(&mut memory, |context| assert_eq!(helper_f3f7a8(context, 4), Ok(())));
        assert_eq!(bytes(&memory), (0x65, 0x5c));

        let mut memory = sample_selection();
        memory.state[FLAG as usize..FLAG as usize + 4].copy_from_slice(&0u32.to_le_bytes());
        with_memory(&mut memory, |context| assert_eq!(helper_f3f7a8(context, 6), Ok(())));
        assert_eq!(bytes(&memory), (0x62, 0x55));
        with_memory(&mut memory, |context| assert_eq!(helper_f3f7a8(context, 26), Ok(())));
        assert_eq!(bytes(&memory), (0x60, 0x53));

        let mut memory = sample_selection();
        memory.state[MODE as usize..MODE as usize + 4].copy_from_slice(&0u32.to_le_bytes());
        with_memory(&mut memory, |context| assert_eq!(helper_f3f7a8(context, 26), Ok(())));
        assert_eq!(bytes(&memory), (0x60, 0x53));

        let mut memory = sample_selection();
        memory.state[COUNTER as usize..COUNTER as usize + 4].copy_from_slice(&100u32.to_le_bytes());
        with_memory(&mut memory, |context| assert_eq!(helper_f3f7a8(context, 1), Ok(())));
        assert_eq!(bytes(&memory), (0x63, 0x5b));

        let mut memory = sample_selection();
        memory.state[FLAG as usize..FLAG as usize + 4].copy_from_slice(&0u32.to_le_bytes());
        memory.state[COUNTER as usize..COUNTER as usize + 4].copy_from_slice(&100u32.to_le_bytes());
        with_memory(&mut memory, |context| assert_eq!(helper_f3f7a8(context, 1), Ok(())));
        assert_eq!(bytes(&memory), (0x60, 0x56));
    }
}
