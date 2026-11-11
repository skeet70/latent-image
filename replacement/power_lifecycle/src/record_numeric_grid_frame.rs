use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_numeric_grid::ProfileRecord;

#[derive(Clone, Copy)]
pub struct GridScratch { base: GuestAddress }
impl GridScratch {
    pub fn new(base: GuestAddress, length: ByteCount) -> Result<Self, MemoryError> {
        if length.0 < 124 { return Err(MemoryError::RegionLength); }
        base.checked_add(123)?;
        Ok(Self { base })
    }
    pub fn input(self) -> GuestAddress { self.base }
    pub fn output(self) -> Result<GuestAddress, MemoryError> { self.base.checked_add(40) }
    pub fn samples(self) -> Result<GuestAddress, MemoryError> { self.base.checked_add(80) }
}

pub fn profile<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, scratch: GridScratch) -> Result<ProfileRecord, MemoryError> {
    crate::record_profile_shared_cpu::helper_7582c0(context, scratch.input())?;
    let mut record = ProfileRecord { words: [0; 10] };
    for (index, value) in record.words.iter_mut().enumerate() { *value = context.read_u32(scratch.input().checked_add(index as u32 * 4)?)?; }
    Ok(record)
}

pub fn publish<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: u32, scratch: GridScratch, record: &ProfileRecord) -> Result<(), MemoryError> {
    let output = scratch.output()?;
    for (index, value) in record.words.iter().enumerate() { context.write_u32(output.checked_add(index as u32 * 4)?, *value)?; }
    crate::record_profile_shared_cpu::helper_758300(context, profile, output)
}
