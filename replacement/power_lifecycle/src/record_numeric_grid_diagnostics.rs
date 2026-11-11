use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_numeric_grid::{MotionChannel, ProfileHandle};
use crate::record_numeric_grid_frame::{self as frame, GridScratch};

pub fn initialize_fields(memory: &mut impl GuestAddressSpace, kind: u32, speed: u32, exposure: u32, count: u32) -> Result<(), MemoryError> {
    let base = GuestAddress::new(0x016c73e0);
    memory.write_u32(base, kind)?;
    memory.write_u32(base.checked_add(4)?, speed)?;
    memory.write_u32(base.checked_add(12)?, exposure)?;
    memory.write_u32(base.checked_add(8)?, count)
}

pub fn publish_profile<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, profile: ProfileHandle, scratch: GridScratch) -> Result<(), MemoryError> {
    let mut record = frame::profile(context, scratch)?;
    record.words[2] = context.read_u32(GuestAddress::new(0x016c73e0))?;
    record.set_half(12, context.read_u32(GuestAddress::new(0x016c73e4))? as u16);
    record.words[6] = context.read_u32(GuestAddress::new(0x016c73e8))?;
    record.words[7] = context.read_u32(GuestAddress::new(0x016c73ec))?;
    frame::publish(context, profile.0, scratch, &record)?;
    Ok(())
}

pub fn copy_snapshot<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, _channel: MotionChannel) -> Result<(), MemoryError> {
    if crate::record_configuration::helper_69f8fc(context, 0xa8)? & 128 != 0 {
        context.copy_bytes(GuestAddress::new(0x5ffdb9ec), GuestAddress::new(0x5fead820), ByteCount(0xb598))?;
    }
    Ok(())
}

fn fill(memory: &mut impl GuestAddressSpace, base: GuestAddress, length: ByteCount, value: u8) -> Result<(), MemoryError> {
    for offset in 0..length.0 { memory.write_u8(base.checked_add(offset)?, value)?; }
    Ok(())
}

pub fn reset_snapshots(memory: &mut impl GuestAddressSpace) -> Result<u32, MemoryError> {
    for (base, first, second, third, marker) in [
        (0x1fea2280, 0x1fea2280, 0x1fea4e98, 0x1fead298, 0x1fead408),
        (0x5fead820, 0x5fead820, 0x5feb0438, 0x5feb8838, 0x5feb89a8),
    ] {
        fill(memory, GuestAddress::new(base), ByteCount(0xb598), 0)?;
        fill(memory, GuestAddress::new(first), ByteCount(0x2c00), 255)?;
        fill(memory, GuestAddress::new(second), ByteCount(0x8400), 255)?;
        fill(memory, GuestAddress::new(third), ByteCount(360), 255)?;
        memory.write_u32(GuestAddress::new(marker), 3)?;
    }
    Ok(0x5feb8838)
}
