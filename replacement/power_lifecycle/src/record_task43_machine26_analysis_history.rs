use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext, RegisterWordCount, StackAlignment};
use crate::record_configuration::{helper_69fec8 as configuration_halfword, helper_69ff68 as configuration_signed_byte};
use crate::record_task43_machine26_state::*;
use crate::record_profile_shared_cpu::helper_7567ec;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct SelectionHistoryRecord {
    pub score: u32,
    pub metric1: u32,
    pub metric2: u32,
    pub measurement: i16,
    pub position: i16,
    pub mode: u32,
}
#[derive(Clone, Copy)]
enum HistoryBank { Recent, Peaks, Secondary }
impl HistoryBank {
    fn address(self) -> GuestAddress { GuestAddress::new(STATE + match self { Self::Recent => 0x1000, Self::Peaks => 0x108c, Self::Secondary => 0x1190 }) }
    fn capacity(self) -> u8 { match self { Self::Peaks => 13, _ => 7 } }
    fn entry(self, index: u32) -> GuestAddress { GuestAddress::new(self.address().get().wrapping_add(index.wrapping_mul(20))) }
}
fn read_record<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, address: GuestAddress) -> Result<SelectionHistoryRecord, MemoryError> {
    let score = context.read_u32(address)?;
    let metric1 = context.read_u32(address.checked_add(4)?)?;
    let metric2 = context.read_u32(address.checked_add(8)?)?;
    let pair = context.read_u32(address.checked_add(12)?)?;
    let mode = context.read_u32(address.checked_add(16)?)?;
    Ok(SelectionHistoryRecord { score, metric1, metric2, measurement: pair as i16, position: (pair >> 16) as i16, mode })
}
fn copy_record<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress, source: GuestAddress) -> Result<(), MemoryError> {
    context.copy_register_words(destination, source, RegisterWordCount::Four)?;
    context.copy_register_words(destination.checked_add(16)?, source.checked_add(16)?, RegisterWordCount::One)
}
fn transfer<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, bank: HistoryBank, index: u32, record: GuestAddress, write: bool) -> Result<i32, MemoryError> {
    if index as u8 >= bank.capacity() { return Ok(0); }
    let entry = bank.entry(index);
    let (source, destination) = if write { (record, entry) } else { (entry, record) };
    copy_record(context, destination, source)?;
    Ok(1)
}
fn load<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, bank: HistoryBank, index: u32) -> Result<Option<SelectionHistoryRecord>, MemoryError> {
    if index as u8 >= bank.capacity() { return Ok(None); }
    read_record(context, bank.entry(index)).map(Some)
}
pub fn load_first_history<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32) -> Result<Option<SelectionHistoryRecord>, MemoryError> { load(context, HistoryBank::Recent, index) }
pub fn load_second_history<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32) -> Result<Option<SelectionHistoryRecord>, MemoryError> { load(context, HistoryBank::Peaks, index) }
pub fn load_secondary_history<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32) -> Result<Option<SelectionHistoryRecord>, MemoryError> { load(context, HistoryBank::Secondary, index) }
fn signed_index<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, offset: u32) -> Result<u32, MemoryError> { Ok(b(context, offset)? as i8 as i32 as u32) }
fn clear_bytes<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, address: GuestAddress, count: u32) -> Result<(), MemoryError> {
    for offset in 0..count { context.write_u8(address.checked_add(offset)?, 0)?; }
    Ok(())
}
fn copy_records<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress, source: GuestAddress, count: u32) -> Result<(), MemoryError> {
    for index in 0..count { copy_record(context, destination.checked_add(index * 20)?, source.checked_add(index * 20)?)?; }
    Ok(())
}
macro_rules! history_transfer {
    ($name:ident, $bank:ident, $write:expr) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress, index: u32) -> Result<i32, MemoryError> {
            transfer(context, HistoryBank::$bank, index, record, $write)
        }
    };
}
history_transfer!(helper_f42a6c, Recent, false);
history_transfer!(helper_f43af8, Recent, true);
history_transfer!(helper_f43b48, Peaks, false);
history_transfer!(helper_f43b9c, Peaks, true);
history_transfer!(helper_f43bf0, Secondary, false);
history_transfer!(helper_f43c44, Secondary, true);

pub fn helper_f40f88<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    clear_bytes(context, HistoryBank::Peaks.address(), 260)?;
    pb(context, 0x121d, 0xff)?; pb(context, 0x121f, 0xff)
}
pub fn helper_f40fc4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    clear_bytes(context, HistoryBank::Secondary.address(), 140)?;
    pb(context, 0x121e, 0xff)?; pb(context, 0x1220, 0xff)
}

pub fn helper_f43c98<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    context.with_guest_frame(ByteCount(48), StackAlignment::Doubleword, |context, frame| {
        let next = b(context, 0x121c)?.wrapping_add(1); pb(context, 0x121c, next)?;
        if next as i8 > 6 {
            pb(context, 0x121c, 6)?;
            copy_records(context, HistoryBank::Recent.address(), HistoryBank::Recent.address().checked_add(20)?, 6)?;
        }
        let descriptor = frame.at(ByteCount(20), ByteCount(12))?;
        let channel = word(context, CHANNEL)?;
        helper_7567ec(context, channel, descriptor)?;
        let record = frame.at(ByteCount(0), ByteCount(20))?;
        let score = w(context, 0xf8c)?; context.write_u32(record, score)?;
        let sample_index = context.read_u8(descriptor.checked_add(4)?)?;
        let sample_base = context.read_u32(descriptor.checked_add(8)?)?;
        let sample = GuestAddress::new(sample_base.wrapping_add(u32::from(sample_index).wrapping_mul(0x228)));
        let first = context.read_u16(sample.checked_add(0x204)?)?; context.write_u32(record.checked_add(4)?, u32::from(first))?;
        let second = context.read_u16(sample.checked_add(0x206)?)?; context.write_u32(record.checked_add(8)?, u32::from(second))?;
        let measurement = h(context, 0x1222)?; context.write_u16(record.checked_add(12)?, measurement)?;
        let position = h(context, 0x32)?; context.write_u16(record.checked_add(14)?, position)?;
        let mode = w(context, 0x4c)?; context.write_u32(record.checked_add(16)?, mode)?;
        let index = signed_index(context, 0x121c)?;
        helper_f43af8(context, record, index)?;
        Ok(())
    })
}

pub fn helper_f43e90<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    context.with_guest_frame(ByteCount(56), StackAlignment::Doubleword, |context, frame| {
        let recent = frame.at(ByteCount(20), ByteCount(20))?;
        let secondary = frame.at(ByteCount(0), ByteCount(20))?;
        let index = signed_index(context, 0x121c)?; helper_f42a6c(context, recent, index)?;
        if b(context, 0x121e)? != 0xff {
            let index = signed_index(context, 0x1220)?; helper_f43bf0(context, secondary, index)?;
            let recent_score = context.read_u32(recent)?;
            if context.read_u32(secondary)? >= recent_score {
                let current = b(context, 0x121e)?;
                if current & 0x80 == 0 && i32::from(current as i8) + 1 <= 6 {
                    pb(context, 0x121e, current.wrapping_add(1))?;
                    let index = signed_index(context, 0x121e)?; helper_f43c44(context, recent, index)?;
                }
                return Ok(());
            }
        }
        if b(context, 0x121e)? == 0xff && b(context, 0x1239)? == 0 {
            clear_bytes(context, secondary, 20)?;
            let score = w(context, 0xef4)?; context.write_u32(secondary, score)?;
            let position = h(context, 0xef8)?; context.write_u16(secondary.checked_add(14)?, position)?;
            let mode = w(context, 0xf30)?; context.write_u32(secondary.checked_add(16)?, mode)?;
            helper_f40fc4(context)?;
            let next = b(context, 0x121e)?.wrapping_add(1); pb(context, 0x121e, next)?;
            helper_f43c44(context, secondary, next as i8 as i32 as u32)?;
            let current = b(context, 0x121e)?; pb(context, 0x1220, current)?;
            return Ok(());
        }
        helper_f40fc4(context)?;
        let current = b(context, 0x121c)?;
        let count = (i32::from(current as i8) + 1).min(4) as u8;
        let start = current.wrapping_add(1).wrapping_sub(count);
        copy_records(context, HistoryBank::Secondary.address(), HistoryBank::Recent.address().checked_add(u32::from(start) * 20)?, u32::from(count))?;
        pb(context, 0x1220, count.wrapping_sub(1))?;
        let current = b(context, 0x1220)?; pb(context, 0x121e, current)
    })
}

pub fn helper_f4403c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<i32, MemoryError> {
    context.with_guest_frame(ByteCount(80), StackAlignment::Doubleword, |context, frame| {
        let last = b(context, 0x121d)? as i8;
        let current = b(context, 0x121f)?;
        if last <= 1 || current == 0 || last == current as i8 { return Ok(0); }
        let previous = frame.at(ByteCount(44), ByteCount(20))?;
        let middle = frame.at(ByteCount(24), ByteCount(20))?;
        let next = frame.at(ByteCount(4), ByteCount(20))?;
        helper_f43b48(context, previous, current.wrapping_sub(1) as i8 as i32 as u32)?;
        let index = signed_index(context, 0x121f)?; helper_f43b48(context, middle, index)?;
        let index = b(context, 0x121f)?.wrapping_add(1); helper_f43b48(context, next, index as i8 as i32 as u32)?;
        let first = context.read_u16(previous.checked_add(12)?)? as i16;
        let second = context.read_u16(middle.checked_add(12)?)? as i16;
        let third = context.read_u16(next.checked_add(12)?)? as i16;
        if first == second && third == first { return Ok(0); }
        put(context, 0xfc0, first as i32 as u32)?; put(context, 0xfc8, second as i32 as u32)?; put(context, 0xfd0, third as i32 as u32)?;
        let value = context.read_u16(previous.checked_add(14)?)?; ph(context, 0xfc4, value)?;
        let value = context.read_u16(middle.checked_add(14)?)?; ph(context, 0xfcc, value)?;
        let value = context.read_u16(next.checked_add(14)?)?; ph(context, 0xfd4, value)?;
        Ok(1)
    })
}

pub fn helper_f44d60<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<i32, MemoryError> {
    context.with_guest_frame(ByteCount(56), StackAlignment::Doubleword, |context, frame| {
        let minimum = setting(context, 0x944c)?;
        if i32::from(b(context, 0x121c)? as i8) + 1 < minimum as i32 { return Ok(0); }
        let threshold = u32::from(configuration_halfword(context, 0x944f)?);
        if b(context, 0x121c)? == 0xff { return Ok(0); }
        let record = frame.at(ByteCount(4), ByteCount(20))?;
        let mut hits = 0u8;
        let mut index = 0u8;
        loop {
            helper_f42a6c(context, record, index as i8 as i32 as u32)?;
            let first = context.read_u32(record.checked_add(4)?)?;
            if first >= threshold || context.read_u32(record.checked_add(8)?)? >= threshold {
                hits = hits.wrapping_add(1);
                if minimum <= u32::from(hits) { return Ok(1); }
            }
            index = index.wrapping_add(1);
            if index >= b(context, 0x121c)?.wrapping_add(1) { return Ok(0); }
        }
    })
}

pub fn helper_f44e38<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<i32, MemoryError> {
    context.with_guest_frame(ByteCount(56), StackAlignment::Doubleword, |context, frame| {
        let recent = frame.at(ByteCount(20), ByteCount(20))?;
        let peak = frame.at(ByteCount(0), ByteCount(20))?;
        let index = signed_index(context, 0x121c)?; helper_f42a6c(context, recent, index)?;
        let should_check = if b(context, 0x121d)? == 0xff { true } else {
            let index = signed_index(context, 0x121f)?; helper_f43b48(context, peak, index)?;
            let first = context.read_u16(peak.checked_add(12)?)? as i16;
            first < context.read_u16(recent.checked_add(12)?)? as i16
        };
        if should_check && helper_f44d60(context)? == 1 {
            helper_f40f88(context)?;
            let count = (i32::from(b(context, 0x121c)? as i8) + 1) as u32;
            copy_records(context, HistoryBank::Peaks.address(), HistoryBank::Recent.address(), count)?;
            let current = b(context, 0x121c)?; pb(context, 0x121d, current)?; pb(context, 0x121f, current)?;
            return Ok(1);
        }
        let current = b(context, 0x121d)?;
        if current & 0x80 == 0 && i32::from(current as i8) + 1 <= 12 {
            pb(context, 0x121d, current.wrapping_add(1))?;
            let index = signed_index(context, 0x121d)?; helper_f43b9c(context, recent, index)?;
        }
        Ok(0)
    })
}

pub fn helper_f43d94<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    let source = if word(context, 0x016c_0f88)? == 1 && w(context, 8)? == 14 { STATE + 0xfe0 } else { STATE + 0x1a8 };
    let second = if b(context, 0xf5d)? != 0 { true } else if w(context, 0xf9c)? != 0 { false } else {
        let key = if w(context, 8)? == 13 { 0x8805 } else { 0x8804 };
        setting(context, key)? == 1
    };
    let first = word(context, source + if second { 4 } else { 0 })?; put(context, 0xf8c, first)?;
    let next = word(context, source + if second { 12 } else { 8 })?; put(context, 0xf90, next)?;
    helper_f43c98(context)
}

pub fn helper_f44afc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, mut value: u32) -> Result<u32, MemoryError> {
    context.with_guest_frame(ByteCount(72), StackAlignment::Doubleword, |context, frame| {
        let threshold = u32::from(configuration_halfword(context, 0x944f)?);
        let enter = setting(context, 0x8b13)?;
        let leave = setting(context, 0x8b14)?;
        let lower = configuration_signed_byte(context, 0x8b15)?;
        let upper = configuration_signed_byte(context, 0x8b16)?;
        let selection = setting(context, 0x944b)?;
        let recent = frame.at(ByteCount(12), ByteCount(20))?;
        let index = signed_index(context, 0x121c)?; helper_f42a6c(context, recent, index)?;
        let first = context.read_u32(recent.checked_add(4)?)? >= threshold;
        let second = context.read_u32(recent.checked_add(8)?)? >= threshold;
        let normal = w(context, 0xf9c)? == 0;
        let measurement = s(context, 0x1226)?;
        let position = measurement >= lower * 10 && measurement <= upper * 10;
        let count = if normal && first && second && position { h(context, 0x1230)?.wrapping_add(1) } else { 0 }; ph(context, 0x1230, count)?;
        let count = if normal && (first || second) && position { h(context, 0x1234)?.wrapping_add(1) } else { 0 }; ph(context, 0x1234, count)?;
        if enter <= u32::from(half(context, 0x016c_21b8 + if selection == 0 { 0x30 } else { 0x34 })?) { value = 1; }
        let count = if !normal || (!first && !second) || !position { h(context, 0x1232)?.wrapping_add(1) } else { 0 }; ph(context, 0x1232, count)?;
        let count = if !normal || !first || !second || !position { h(context, 0x1236)?.wrapping_add(1) } else { 0 }; ph(context, 0x1236, count)?;
        if leave <= u32::from(half(context, 0x016c_21b8 + if selection == 0 { 0x32 } else { 0x36 })?) { value = 0; }
        Ok(value)
    })
}
