use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext, StackAlignment};
use crate::record_configuration::helper_69f8fc as configuration;
use crate::record_profile_accessors::{helper_74e5c0, helper_750474};
use crate::record_profile_shared_accessors::{helper_7569a4, helper_757ff0, helper_758028};
use crate::record_profile_slot_accessors::{helper_75502c, helper_75516c};
use crate::record_task43_selection_clock::helper_1005f60;

const ADJUSTMENT: u32 = 0x016b_e044;
#[derive(Clone, Copy)]
struct SelectionMask(u8);
#[derive(Clone, Copy)]
struct ProfileHandle(u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum SelectionStage { Stage0, Stage1, Stage2, Stage3, Stage4, Stage5, Stage6, Complete }
impl SelectionStage {
    pub const fn get(self) -> u32 { self as u32 }
    fn from_word(value: u32) -> Self { match value { 0 => Self::Stage0, 1 => Self::Stage1, 2 => Self::Stage2, 3 => Self::Stage3, 4 => Self::Stage4, 5 => Self::Stage5, 6 => Self::Stage6, _ => Self::Complete } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelectionPair { pub first: SelectionStage, pub second: SelectionStage }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelectionMasks { pub changed: bool, pub first: u8, pub second: u8 }

fn address(value: u32) -> GuestAddress { GuestAddress::new(value) }
fn byte<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<u8, MemoryError> { context.read_u8(address(value)) }
fn word<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<u32, MemoryError> { context.read_u32(address(value)) }
fn handle<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<ProfileHandle, MemoryError> { word(context, ADJUSTMENT + 16).map(ProfileHandle) }
fn mask_for_stage(stage: u32) -> u8 { match stage { 0 | 1 => 1, 2 => 2, 3 => 4, 4 => 8, 5 => 0x20, 6 => 0x40, _ => 0 } }

pub fn helper_f32530<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, mask: u32) -> Result<(), MemoryError> {
    let value = byte(context, ADJUSTMENT + 1)?; context.write_u8(address(ADJUSTMENT + 1), value | mask as u8)
}
pub fn helper_f32558<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, mask: u32) -> Result<(), MemoryError> {
    let value = byte(context, ADJUSTMENT + 1)?; context.write_u8(address(ADJUSTMENT + 1), value & !(mask as u8))
}
pub fn helper_f32580<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, mask: u32) -> Result<(), MemoryError> {
    let value = byte(context, ADJUSTMENT + 2)?; context.write_u8(address(ADJUSTMENT + 2), value | mask as u8)
}
pub fn helper_f325a8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, mask: u32) -> Result<u32, MemoryError> {
    let value = byte(context, ADJUSTMENT + 2)?; context.write_u8(address(ADJUSTMENT + 2), value & !(mask as u8))?; Ok(mask)
}
pub fn helper_f32dd8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, status: u32, stage: u32, output: GuestAddress) -> Result<(), MemoryError> {
    if stage == 7 || status == 2 { context.write_u8(output, mask_for_stage(stage))?; }
    Ok(())
}
pub fn helper_f32e60<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, category: u32) -> Result<GuestAddress, MemoryError> {
    if category != 7 {
        for index in 0..8 {
            let entry = word(context, 0x00fa_b030 + index * 4)?;
            if entry == category { return Ok(address(ADJUSTMENT + 4)); }
            if entry == 7 { break; }
        }
    }
    Ok(address(ADJUSTMENT + 3))
}
pub fn helper_f330e4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    let flags = configuration(context, 0x88ef)?;
    let profile = handle(context)?;
    let mode = helper_74e5c0(context, profile.0)?;
    Ok(if flags & 2 != 0 { u32::from(mode == 1) } else if flags & 4 != 0 { u32::from(mode == 0) } else { (flags >> 3) & 1 })
}

fn candidate<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, mask: SelectionMask, pass: u8, mode: u32, stage: &mut u32, changed: GuestAddress) -> Result<bool, MemoryError> {
    let key = word(context, 0x00fa_b110u32.wrapping_add(stage.wrapping_mul(7).wrapping_add(mode).wrapping_mul(4)))?;
    let configuration = configuration(context, key)?;
    let enabled = |bit| mask.0 & bit != 0 && pass < 32 && (configuration >> pass) & 1 != 0;
    Ok(match *stage {
        0 => { context.write_u32(changed, 1)?; enabled(1) }
        1 => pass == 3 && mask.0 & 1 != 0,
        2 => enabled(2), 3 => enabled(4), 4 => enabled(8), 5 => mask.0 & 1 != 0, 6 => enabled(0x20), 7 => true,
        _ => { *stage = 7; true }
    })
}
fn next_stage<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, mask: SelectionMask, cursor: u32, table: u32, changed: GuestAddress) -> Result<u32, MemoryError> {
    loop {
        let index = byte(context, cursor)?;
        let mut stage = word(context, table + u32::from(index) * 4)?;
        let pass = byte(context, 0x016d_8ff0)?;
        let mode = word(context, 0x016d_877c)?;
        let accepted = candidate(context, mask, pass, mode, &mut stage, changed)?;
        if stage != 7 { let index = byte(context, cursor)?; context.write_u8(address(cursor), index.wrapping_add(1))?; }
        if accepted { return Ok(stage); }
    }
}
fn select_policy<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: ProfileHandle, mask: SelectionMask, first_stage: u8, second_stage: u8, first: GuestAddress, second: GuestAddress, changed: GuestAddress) -> Result<(), MemoryError> {
    context.with_guest_frame(ByteCount(64), StackAlignment::Doubleword, |context, frame| {
        let first_local = frame.at(ByteCount(20), ByteCount(4))?;
        let second_local = frame.at(ByteCount(16), ByteCount(4))?;
        context.write_u32(first_local, 8)?; context.write_u32(second_local, 8)?;
        if byte(context, 0x016d_8778)? == 0 && byte(context, 0x016d_8ff1)? == 0 {
            let mode = if helper_74e5c0(context, profile.0)? == 1 { 1 }
            else if word(context, 0x016d_8708)? == 1 { 2 }
            else if helper_75502c(context, profile.0)? == 1 { 3 }
            else if helper_75516c(context, profile.0)? == 1 {
                let selector = word(context, 0x016d_8748)?;
                let output = frame.at(ByteCount(12), ByteCount(4))?;
                helper_1005f60(context, selector, output)?;
                let period = context.read_u32(output)?;
                let rate = if period == 0 { 0 } else { (1_000_000 / period) as u16 as u32 };
                if rate < configuration(context, 0x4bd8)? { 5 } else { 4 }
            } else { 0 };
            context.write_u32(address(0x016d_877c), mode)?;
        }
        context.write_u32(first, 7)?; context.write_u32(second, 7)?;
        if word(context, 0x016d_877c)? == 6 { return Ok(()); }
        if first_stage == 0 {
            let selected = next_stage(context, mask, 0x016d_8778, 0x00fa_afb0, changed)?;
            context.write_u32(first_local, selected)?; context.write_u32(first, selected)?;
        }
        if second_stage == 0 {
            let selected = next_stage(context, mask, 0x016d_8ff1, 0x00fa_b030, changed)?;
            context.write_u32(second_local, selected)?; context.write_u32(second, selected)?;
            if context.read_u32(first_local)? == 7 && selected == 7 {
                context.write_u8(address(0x016d_8778), 0)?; context.write_u8(address(0x016d_8ff1), 0)?;
                let next = byte(context, 0x016d_8ff0)?.wrapping_add(1);
                context.write_u8(address(0x016d_8ff0), if next > 3 { 0 } else { next })?;
            }
        }
        Ok(())
    })
}

pub fn helper_f32bc4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    context.with_guest_frame(ByteCount(64), StackAlignment::Doubleword, |context, frame| {
        let counter = word(context, 0x016f_672c)?;
        let record = frame.at(ByteCount(8), ByteCount(48))?;
        let output = frame.at(ByteCount(4), ByteCount(4))?;
        let profile = handle(context)?; helper_757ff0(context, profile.0, record)?;
        let count_address = record.checked_add(40)?;
        let count = context.read_u16(count_address)?;
        if count < 10 {
            helper_1005f60(context, counter, output)?;
            let value = context.read_u32(output)?;
            context.write_u32(record.checked_add(u32::from(count) * 4)?, value)?;
            context.write_u16(count_address, count + 1)?;
        } else {
            for index in 0..9 { let value = context.read_u32(record.checked_add((index + 1) * 4)?)?; context.write_u32(record.checked_add(index * 4)?, value)?; }
            helper_1005f60(context, counter, output)?;
            let value = context.read_u32(output)?; context.write_u32(record.checked_add(36)?, value)?; context.write_u16(count_address, 10)?;
        }
        let profile = handle(context)?; helper_758028(context, profile.0, record)?;
        Ok(())
    })
}
pub fn helper_f32c74<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    context.with_guest_frame(ByteCount(64), StackAlignment::Doubleword, |context, frame| {
        let record = frame.at(ByteCount(0), ByteCount(48))?;
        let profile = handle(context)?; helper_757ff0(context, profile.0, record)?;
        for offset in 0..40 { context.write_u8(record.checked_add(offset)?, 0)?; }
        context.write_u16(record.checked_add(40)?, 0)?;
        let profile = handle(context)?; helper_758028(context, profile.0, record)?;
        Ok(())
    })
}
pub fn select_adjustment_policy_into<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, flags: u32, first: GuestAddress, second: GuestAddress) -> Result<(), MemoryError> {
    context.with_guest_frame(ByteCount(48), StackAlignment::Doubleword, |context, frame| {
        let changed = frame.at(ByteCount(20), ByteCount(4))?; context.write_u32(changed, 0)?;
        let profile = handle(context)?;
        let initial = helper_750474(context, profile.0)?;
        let mask = SelectionMask((flags as u8 & 15) | if flags & 0x20 != 0 { 1 } else { 0 } | if flags & 0x40 != 0 { 0x20 } else { 0 });
        let profile = handle(context)?; let first_stage = byte(context, ADJUSTMENT + 3)?; let second_stage = byte(context, ADJUSTMENT + 4)?;
        select_policy(context, profile, mask, first_stage, second_stage, first, second, changed)?;
        if configuration(context, 0x5256)? & 2 != 0 && context.read_u32(changed)? == 1 {
            if initial == 1 { helper_f32bc4(context)?; } else if initial == 0 { helper_f32c74(context)?; }
        }
        Ok(())
    })
}
pub fn helper_f32cc0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, flags: u32) -> Result<SelectionPair, MemoryError> {
    context.with_guest_frame(ByteCount(8), StackAlignment::Doubleword, |context, frame| {
        let first = frame.at(ByteCount(0), ByteCount(4))?; let second = frame.at(ByteCount(4), ByteCount(4))?;
        select_adjustment_policy_into(context, flags, first, second)?;
        let first = SelectionStage::from_word(context.read_u32(first)?); let second = SelectionStage::from_word(context.read_u32(second)?);
        Ok(SelectionPair { first, second })
    })
}
pub fn helper_f32f1c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    let profile = handle(context)?; let mode = helper_750474(context, profile.0)?;
    if configuration(context, 0x5256)? & 2 != 0 { if mode == 1 { helper_f32bc4(context)?; } else { helper_f32c74(context)?; } }
    Ok(())
}
pub fn helper_f32e88(first: u32, second: u32) -> u32 {
    let difference = first & !second;
    (if difference & 1 != 0 { 0x21 } else { 0 }) | (difference & 14) | if difference & 0x20 != 0 { 0x40 } else { 0 }
}
pub fn helper_f32f60<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> { Ok(u32::from(configuration(context, 0x003e_0009)? != 1)) }
pub fn helper_f32f80<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, first: u32, second: u32) -> Result<u32, MemoryError> {
    let intersection = first & second;
    let mut result = if intersection & 1 != 0 { 0x21 } else { 0 };
    if intersection & 2 != 0 && helper_f32f60(context)? == 1 { result |= 2; }
    result |= intersection & 4;
    if intersection & 8 != 0 && helper_f32f60(context)? == 1 { result |= 8; }
    if intersection & 0x20 != 0 { result |= 0x40; }
    Ok(result)
}
pub fn changed_masks_into<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, first: GuestAddress, second: GuestAddress) -> Result<i32, MemoryError> {
    let requested = word(context, 0x0169_d230)?; let enabled = word(context, 0x0169_d234)?;
    let common = helper_f32f80(context, requested, enabled)? as u8; context.write_u8(first, common)?;
    let excluded = byte(context, ADJUSTMENT)?; context.write_u8(first, common & !excluded)?;
    let requested = word(context, 0x0169_d230)?; let enabled = word(context, 0x0169_d234)?;
    context.write_u8(second, helper_f32e88(requested, enabled) as u8)?;
    let profile = handle(context)?;
    if helper_7569a4(context, profile.0)? == 0 { let flags = context.read_u8(second)?; context.write_u8(second, flags | 0x10)?; }
    Ok(i32::from(context.read_u8(first)? != 0 || context.read_u8(second)? != 0))
}
pub fn helper_f3300c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<SelectionMasks, MemoryError> {
    context.with_guest_frame(ByteCount(8), StackAlignment::Doubleword, |context, frame| {
        let first = frame.at(ByteCount(0), ByteCount(1))?; let second = frame.at(ByteCount(1), ByteCount(1))?;
        let changed = changed_masks_into(context, first, second)? != 0;
        Ok(SelectionMasks { changed, first: context.read_u8(first)?, second: context.read_u8(second)? })
    })
}
