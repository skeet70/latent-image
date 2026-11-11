//! Shared-block halfword setters and staged-record copies, image-4 bodies
//! 0x0075_74e4-0x0075_7b70 over 0x0091_904c.
use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_helper_shared_state::SHARED_BLOCK;
use crate::record_profile_accessors::{can_write, ProfileField, ProfileHandle};

const PROFILE_FIELD: u32 = 27;

const RECORD_92B8: u32 = 0x0091_92b8;
const RECORD_96CC: u32 = 0x0091_96cc;
const RECORD_96F4: u32 = 0x0091_96f4;

fn block() -> GuestAddress { GuestAddress::new(SHARED_BLOCK) }

macro_rules! block_half_setter {
    ($name:ident, $at:literal) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, value: u32) -> Result<(), MemoryError> {
            if can_write(context, ProfileHandle(handle), ProfileField(PROFILE_FIELD))? {
                context.write_u16(block().checked_add($at)?, value as u16)?;
            }
            Ok(())
        }
    };
}

block_half_setter!(helper_7574e4, 0x246);
block_half_setter!(helper_757624, 0x24e);
block_half_setter!(helper_757674, 0x250);
block_half_setter!(helper_7576c4, 0x252);
block_half_setter!(helper_757714, 0x254);
block_half_setter!(helper_757764, 0x25e);
block_half_setter!(helper_7577b4, 0x260);
block_half_setter!(helper_75793c, 0x266);

/// The image-4 body at 0x0075_7800.
pub fn helper_757800<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, value: u32) -> Result<(), MemoryError> {
    if can_write(context, ProfileHandle(handle), ProfileField(PROFILE_FIELD))? {
        context.write_u32(block().checked_add(0x258)?, value)?;
    }
    Ok(())
}

/// Copies the record the caller staged at `staged` to `destination`.
///
/// Stock reads its own three argument words and the caller's staged buffer as
/// one run of bytes.
fn copy_staged<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>, handle: u32, staged: GuestAddress, destination: u32, bytes: u32,
) -> Result<(), MemoryError> {
    if can_write(context, ProfileHandle(handle), ProfileField(PROFILE_FIELD))? {
        context.copy_bytes(GuestAddress::new(destination), staged, ByteCount(bytes))?;
    }
    Ok(())
}

/// The image-4 body at 0x0075_7a2c.
pub fn helper_757a2c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, staged: GuestAddress) -> Result<(), MemoryError> {
    copy_staged(context, handle, staged, RECORD_92B8, 0x414)
}

/// The image-4 body at 0x0075_7ab4.
pub fn helper_757ab4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, staged: GuestAddress) -> Result<(), MemoryError> {
    copy_staged(context, handle, staged, RECORD_96CC, 0x414)
}

/// The image-4 body at 0x0075_7b30.
pub fn helper_757b30<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, staged: GuestAddress) -> Result<(), MemoryError> {
    copy_staged(context, handle, staged, RECORD_96F4, 80)
}

#[cfg(test)]
mod tests {
    //! Expected offsets and lengths are the image-4 stores at 0x0075_74e4-0x0075_7b70.
    use super::*;
    use crate::record_address_space::{GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const HANDLE: u32 = 0x00fe_0000;
    const SLOT_BASE: u32 = 0x0091_5d38;

    fn descriptor(base: u32, length: u32) -> RegionDescriptor {
        RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap()
    }

    fn admitted(slots: &mut [u8; 0x80]) {
        slots[0x08..0x0c].copy_from_slice(&0u32.to_le_bytes());
        slots[0x0c..0x10].copy_from_slice(&0u32.to_le_bytes());
        slots[0x14..0x18].copy_from_slice(&0u32.to_le_bytes());
        for field in 0..43 { slots[0x18 + field] = 3; }
    }

    #[test]
    fn halfword_and_word_setters_store_at_their_offsets() {
        let mut slots = [0u8; 0x80];
        admitted(&mut slots);
        let mut block = [0u8; 0x400];
        let mut regions = [
            MemoryRegion::new(descriptor(SLOT_BASE, 0x80), &mut slots).unwrap(),
            MemoryRegion::new(descriptor(SHARED_BLOCK, 0x400), &mut block).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let mut context = RecordContext::new(space, ());
        assert_eq!(helper_7574e4(&mut context, HANDLE, 0x1111_2222), Ok(()));
        assert_eq!(helper_757624(&mut context, HANDLE, 0x3333_4444), Ok(()));
        assert_eq!(helper_757674(&mut context, HANDLE, 0x5555_6666), Ok(()));
        assert_eq!(helper_7576c4(&mut context, HANDLE, 0x7777_8888), Ok(()));
        assert_eq!(helper_757714(&mut context, HANDLE, 0x9999_aaaa), Ok(()));
        assert_eq!(helper_757764(&mut context, HANDLE, 0xbbbb_cccc), Ok(()));
        assert_eq!(helper_7577b4(&mut context, HANDLE, 0xdddd_eeee), Ok(()));
        assert_eq!(helper_757800(&mut context, HANDLE, 0x0a0b_0c0d), Ok(()));
        assert_eq!(helper_75793c(&mut context, HANDLE, 0xf0f0_1234), Ok(()));
        assert_eq!(&block[0x246..0x248], &0x2222u16.to_le_bytes());
        assert_eq!(&block[0x24e..0x250], &0x4444u16.to_le_bytes());
        assert_eq!(&block[0x250..0x252], &0x6666u16.to_le_bytes());
        assert_eq!(&block[0x252..0x254], &0x8888u16.to_le_bytes());
        assert_eq!(&block[0x254..0x256], &0xaaaau16.to_le_bytes());
        assert_eq!(&block[0x25e..0x260], &0xccccu16.to_le_bytes());
        assert_eq!(&block[0x260..0x262], &0xeeeeu16.to_le_bytes());
        assert_eq!(&block[0x258..0x25c], &0x0a0b_0c0du32.to_le_bytes());
        assert_eq!(&block[0x266..0x268], &0x1234u16.to_le_bytes());
    }

    #[test]
    fn setters_ignore_a_handle_without_the_write_permission() {
        let mut slots = [0u8; 0x80];
        admitted(&mut slots);
        slots[0x18 + PROFILE_FIELD as usize] = 1;
        let mut block = [0u8; 0x400];
        let mut regions = [
            MemoryRegion::new(descriptor(SLOT_BASE, 0x80), &mut slots).unwrap(),
            MemoryRegion::new(descriptor(SHARED_BLOCK, 0x400), &mut block).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let mut context = RecordContext::new(space, ());
        assert_eq!(helper_7574e4(&mut context, HANDLE, 0x1111_2222), Ok(()));
        assert_eq!(helper_757800(&mut context, HANDLE, 0x3333_4444), Ok(()));
        assert_eq!(&block[0x246..0x248], &0u16.to_le_bytes());
        assert_eq!(&block[0x258..0x25c], &0u32.to_le_bytes());
    }

    #[test]
    fn staged_records_reach_their_three_destinations() {
        let mut slots = [0u8; 0x80];
        admitted(&mut slots);
        let mut records = [0u8; 0x828];
        let mut staged = [0u8; 0x414];
        for index in 0..0x414 { staged[index] = (index as u8) ^ 0x5a; }
        let mut regions = [
            MemoryRegion::new(descriptor(SLOT_BASE, 0x80), &mut slots).unwrap(),
            MemoryRegion::new(descriptor(RECORD_92B8, 0x828), &mut records).unwrap(),
            MemoryRegion::new(descriptor(0x0010_0000, 0x414), &mut staged).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let mut context = RecordContext::new(space, ());
        assert_eq!(helper_757a2c(&mut context, HANDLE, GuestAddress::new(0x0010_0000)), Ok(()));
        assert_eq!(helper_757ab4(&mut context, HANDLE, GuestAddress::new(0x0010_0000)), Ok(()));
        assert_eq!(helper_757b30(&mut context, HANDLE, GuestAddress::new(0x0010_0000)), Ok(()));
        assert_eq!(&records[0..0x414], &staged[..]);
        assert_eq!(&records[0x414..0x43c], &staged[..0x28]);
        assert_eq!(&records[0x43c..0x48c], &staged[..80]);
        assert_eq!(&records[0x48c..0x828], &staged[0x78..]);
    }
}
