use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext, StackAlignment};
use crate::record_numeric_grid_engine as engine;
use crate::record_numeric_grid_frame::GridScratch;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum MotionClass { Still = 0, Moving = 1, Alternate = 2, Unknown = 3 }
impl MotionClass {
    pub const fn from_word(value: u32) -> Self {
        match value { 1 => Self::Moving, 2 => Self::Alternate, 3 => Self::Unknown, _ => Self::Still }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfileHandle(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingKey(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MotionChannel(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Centroid { pub x: i32, pub y: i32 }
#[repr(C)]
pub struct ProfileRecord { pub words: [u32; 10] }
impl ProfileRecord {
    pub fn half(&self, offset: usize) -> u16 { (self.words[offset / 4] >> ((offset % 4) * 8)) as u16 }
    pub fn set_half(&mut self, offset: usize, value: u16) {
        let shift = (offset % 4) * 8;
        self.words[offset / 4] = (self.words[offset / 4] & !(0xffff << shift)) | ((value as u32) << shift);
    }
}

pub fn displacement(previous: Centroid, current: Centroid) -> u16 {
    if previous.x == -1 { return 0; }
    let dx = current.x.wrapping_sub(previous.x);
    let dy = current.y.wrapping_sub(previous.y);
    let squared = dx.wrapping_mul(dx).wrapping_add(dy.wrapping_mul(dy));
    if squared == 0 { return 0; }
    let mut estimate = 1i32;
    let mut remaining = 51u16;
    loop {
        let quotient = squared.wrapping_div(estimate);
        estimate = estimate.wrapping_add(quotient) / 2;
        if estimate.wrapping_mul(estimate) <= squared && squared < estimate.wrapping_add(1).wrapping_mul(estimate.wrapping_add(1)) { break; }
        remaining -= 1;
        if remaining == 0 { break; }
        if estimate == 0 { return u16::MAX; }
    }
    estimate as u16
}

pub fn helper_fa898c<M: crate::record_address_space::GuestAddressSpace, E>(context: &mut crate::record_address_space::RecordContext<M, E>) -> Result<u32, crate::record_address_space::MemoryError> {
    crate::record_numeric_grid_engine::reset_classes(context)?;
    crate::record_numeric_grid_engine::reset_speeds(context)?;
    crate::record_numeric_grid_diagnostics::initialize_fields(context, 3, 0, 0, 0)?;
    crate::record_numeric_grid_diagnostics::reset_snapshots(context)
}

pub fn helper_fa8520<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: u32, channel: u32) -> Result<(), MemoryError> {
    context.with_guest_frame(ByteCount(124), StackAlignment::Doubleword, |context, frame| {
        let scratch = GridScratch::new(frame.base(), frame.length())?;
        engine::filter(context, ProfileHandle(profile), MotionChannel(channel), scratch)
    })
}

pub fn helper_fa95b8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: u32, input: GuestAddress, mode: u32, previous_x: GuestAddress, previous_y: GuestAddress) -> Result<(), MemoryError> {
    context.with_guest_frame(ByteCount(124), StackAlignment::Doubleword, |context, frame| {
        let scratch = GridScratch::new(frame.base(), frame.length())?;
        engine::grid(context, ProfileHandle(profile), input, mode == 1, previous_x, previous_y, scratch)
    })
}

pub fn helper_fa91b0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, input: GuestAddress, channel: u32) -> Result<(), MemoryError> {
    engine::snapshot(context, input, MotionChannel(channel))
}

pub fn helper_fa816c<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>) -> Result<GuestAddress, MemoryError> { Ok(engine::SPEEDS) }
pub fn helper_fa82c4<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>) -> Result<GuestAddress, MemoryError> { Ok(engine::CLASSES) }
pub fn helper_fa8188<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> { engine::reset_speeds(context) }
pub fn helper_fa82e0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> { engine::reset_classes(context) }
pub fn helper_fa81a4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<(), MemoryError> { engine::push_speed(context, value) }
pub fn helper_fa8334<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<(), MemoryError> { engine::push_class(context, MotionClass::from_word(value)) }
pub fn helper_fa81fc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, channel: u32) -> Result<u32, MemoryError> { engine::filtered_speed(context, MotionChannel(channel)).map(u32::from) }
pub fn helper_fa83c0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, channel: u32) -> Result<u32, MemoryError> { engine::filtered_class(context, MotionChannel(channel)).map(|class| class as u32) }
pub fn helper_fa82fc<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, value: u32) -> Result<u32, MemoryError> { Ok(MotionClass::from_word(value) as u32) }
pub fn helper_fa8388<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, value: u32) -> Result<u32, MemoryError> { Ok(MotionClass::from_word(value) as u32) }
pub fn helper_fa95a8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, key: u32, output: GuestAddress) -> Result<u32, MemoryError> { crate::record_task43_selection_clock::helper_1005f60(context, key, output) }

pub fn helper_f52440<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, kind: u32, speed: u32, exposure: u32, count: u32) -> Result<(), MemoryError> {
    crate::record_numeric_grid_diagnostics::initialize_fields(context, kind, speed, exposure, count)
}
pub fn helper_f5246c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: u32) -> Result<(), MemoryError> {
    context.with_guest_frame(ByteCount(124), StackAlignment::Doubleword, |context, frame| {
        let scratch = GridScratch::new(frame.base(), frame.length())?;
        crate::record_numeric_grid_diagnostics::publish_profile(context, ProfileHandle(profile), scratch)
    })
}
pub fn helper_f524e4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, channel: u32) -> Result<(), MemoryError> {
    crate::record_numeric_grid_diagnostics::copy_snapshot(context, MotionChannel(channel))
}
pub fn helper_f5251c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    crate::record_numeric_grid_diagnostics::reset_snapshots(context)
}

#[cfg(test)]
#[path = "record_numeric_grid_tests.rs"]
mod tests;
