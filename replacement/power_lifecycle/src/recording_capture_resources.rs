#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapturePoolCount(u16);
impl CapturePoolCount {
    pub const fn new(count: u16) -> Option<Self> {
        if count <= 320 { Some(Self(count)) } else { None }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapturePoolIndex(pub u16);

pub fn initialize_bitmap(count: CapturePoolCount, words: &mut [u32; 10]) {
    for (index, word) in words.iter_mut().enumerate() {
        let first = index as u32 * 32;
        let available = u32::from(count.0).saturating_sub(first).min(32);
        *word = if available == 32 { 0 } else { u32::MAX << available };
    }
}

pub fn acquire_bitmap(count: CapturePoolCount, words: &mut [u32; 10]) -> Option<CapturePoolIndex> {
    for index in 0..u32::from(count.0) {
        let slot = &mut words[(index / 32) as usize];
        let bit = 1 << (index % 32);
        if *slot & bit == 0 {
            *slot |= bit;
            return Some(CapturePoolIndex(index as u16));
        }
    }
    None
}

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

const POOLS: GuestAddress = GuestAddress::new(0x015b_7428);
const ROOT: GuestAddress = GuestAddress::new(0x015b_5be8);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureResourceError {
    Memory(MemoryError),
    PoolCapacity(u16),
    RootSlot(i16),
}

impl From<MemoryError> for CaptureResourceError {
    fn from(error: MemoryError) -> Self { Self::Memory(error) }
}

fn acquire<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, pool: GuestAddress) -> Result<i16, CaptureResourceError> {
    let count = context.read_u16(pool)?;
    if count > 320 { return Err(CaptureResourceError::PoolCapacity(count)); }
    for index in 0..u32::from(count) {
        let address = pool.checked_add(8 + index / 32 * 4)?;
        let value = context.read_u32(address)?;
        let bit = 1 << (index % 32);
        if value & bit == 0 {
            context.write_u32(address, value | bit)?;
            return Ok(index as i16);
        }
    }
    Ok(-1)
}

fn resolve<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: i16, offset: u32) -> Result<GuestAddress, MemoryError> {
    if index == -1 { return Ok(GuestAddress::new(0)); }
    if index == 0 { return ROOT.checked_add(offset); }
    let base = context.read_u32(ROOT.checked_add(0x17d0)?)?;
    Ok(GuestAddress::new(base.wrapping_add((i32::from(index) - 1).wrapping_mul(0x17d0) as u32).wrapping_add(offset)))
}

pub fn helper_eb25d8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    for pool in 0..13 {
        let address = POOLS.checked_add(pool * 0x30)?;
        let count = if pool == 12 { 1 } else { 202 };
        context.write_u16(address, count)?;
        for index in 0..10 {
            let available = u32::from(count).saturating_sub(index * 32).min(32);
            let mask = if available == 32 { 0 } else { u32::MAX << available };
            context.write_u32(address.checked_add(8 + index * 4)?, mask)?;
        }
    }
    Ok(())
}

pub fn helper_eb26ac<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, secondary: bool) -> Result<GuestAddress, CaptureResourceError> {
    let index = acquire(context, POOLS)?;
    if !secondary && index != 0 || secondary && index == 0 { return Err(CaptureResourceError::RootSlot(index)); }
    let address = if secondary { resolve(context, index, 0)? } else { ROOT };
    if address.get() == 0 { return Ok(address); }
    context.write_u16(address, index as u16)?;
    for offset in (2..=0x18).step_by(2) { context.write_u16(address.checked_add(offset)?, u16::MAX)?; }
    for (offset, value) in [(0x1c, 0), (0x20, 0x80), (0x24, 0), (0x28, 2)] { context.write_u32(address.checked_add(offset)?, value)?; }
    Ok(address)
}

fn acquire_resource<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: GuestAddress, field: u32, pool: u32, offset: u32, length: u32) -> Result<GuestAddress, CaptureResourceError> {
    let current = context.read_u16(handle.checked_add(field)?)? as i16;
    if current != -1 { return Ok(resolve(context, current, offset)?); }
    let allocated = acquire(context, POOLS.checked_add(pool * 0x30)?)?;
    context.write_u16(handle.checked_add(field)?, allocated as u16)?;
    let address = resolve(context, allocated, offset)?;
    if address.get() != 0 {
        for index in 0..length { context.write_u8(address.checked_add(index)?, 0)?; }
    }
    Ok(address)
}

pub fn helper_eb276c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: GuestAddress) -> Result<GuestAddress, CaptureResourceError> {
    acquire_resource(context, handle, 2, 1, 0x30, 0x90)
}

pub fn helper_eb288c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: GuestAddress) -> Result<GuestAddress, CaptureResourceError> {
    acquire_resource(context, handle, 4, 2, 0xc0, 0xc)
}

pub fn helper_eb2838<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: GuestAddress) -> Result<GuestAddress, MemoryError> {
    let index = context.read_u16(handle.checked_add(2)?)? as i16;
    resolve(context, index, 0x30)
}

pub fn helper_eb2958<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: GuestAddress) -> Result<GuestAddress, MemoryError> {
    let index = context.read_u16(handle.checked_add(4)?)? as i16;
    resolve(context, index, 0xc0)
}

pub fn helper_eb34bc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: GuestAddress) -> Result<GuestAddress, CaptureResourceError> {
    let current = context.read_u16(handle.checked_add(0x18)?)? as i16;
    if current != -1 { return Ok(GuestAddress::new(0x015b_73bc_u32.wrapping_add(i32::from(current).wrapping_mul(0x68) as u32))); }
    let allocated = acquire(context, POOLS.checked_add(12 * 0x30)?)?;
    context.write_u16(handle.checked_add(0x18)?, allocated as u16)?;
    if allocated == -1 { return Ok(GuestAddress::new(0)); }
    let address = GuestAddress::new(0x015b_73bc_u32.wrapping_add(i32::from(allocated).wrapping_mul(0x68) as u32));
    for index in 0..0x68 { context.write_u8(address.checked_add(index)?, 0)?; }
    Ok(address)
}

pub fn helper_eb35ec<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: GuestAddress) -> Result<GuestAddress, MemoryError> {
    let index = context.read_u16(handle.checked_add(0x18)?)? as i16;
    Ok(GuestAddress::new(if index == -1 { 0 } else { 0x015b_73bc_u32.wrapping_add(i32::from(index).wrapping_mul(0x68) as u32) }))
}
