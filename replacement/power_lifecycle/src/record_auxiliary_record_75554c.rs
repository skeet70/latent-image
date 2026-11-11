//! Auxiliary-record accessors inside the profile resource, image-4 entries 0x0075554c-0x00756248.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_profile_accessors::{read_bank, resource_address, ProfileBank, ProfileField, ProfileHandle};

const AUXILIARY_RECORD: u32 = 0x20b0;
const AUXILIARY_CONFIGURATION: u32 = 0x187c;
const READ_FIELD: u32 = 13;
const CALLBACK_FIELD: u32 = 31;
const CONFIGURATION_FIELD: u32 = 30;

fn record<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, field: u32, offset: u32) -> Result<GuestAddress, MemoryError> {
    let bank: ProfileBank = read_bank(context, ProfileHandle(handle), ProfileField(field))?;
    resource_address(bank).checked_add(offset)
}

/// The image-4 body at 0x0075554c.
pub fn helper_75554c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<u32, MemoryError> {
    let record = record(context, handle, READ_FIELD, AUXILIARY_RECORD)?;
    crate::record_profile_aux_native::helper_75d8f0(context, record)
}

/// The image-4 body at 0x007555ec.
pub fn helper_7555ec<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress, handle: u32) -> Result<GuestAddress, MemoryError> {
    let record = record(context, handle, READ_FIELD, AUXILIARY_RECORD)?;
    crate::record_profile_aux_native::helper_75d920(context, destination, record)?;
    Ok(destination)
}

/// The image-4 body at 0x0075573c.
pub fn helper_75573c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<u32, MemoryError> {
    let record = record(context, handle, CALLBACK_FIELD, AUXILIARY_RECORD)?;
    crate::record_profile_aux_native::helper_75d998(context, record)
}

/// The image-4 body at 0x007561f8.
pub fn helper_7561f8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32, value: u32) -> Result<u32, MemoryError> {
    let record = record(context, handle, CONFIGURATION_FIELD, AUXILIARY_CONFIGURATION)?;
    crate::record_profile_aux_configuration::helper_75d650(context, record, value)
}

mod exports {
    use super::*;
    use crate::record_native_exports::stock_export;

    stock_export!(xm1_power_record_helper_75554c, helper_75554c_abi, (), (handle: u32) -> u32, (handle: u32), (handle), crate::record_auxiliary_record_75554c::helper_75554c, |value| value);
    stock_export!(
        xm1_power_record_helper_7555ec,
        helper_7555ec_abi,
        (),
        (destination: u32, handle: u32) -> (),
        (destination: GuestAddress, handle: u32),
        (GuestAddress::new(destination), handle),
        crate::record_auxiliary_record_75554c::helper_7555ec,
        |_| ()
    );
    stock_export!(xm1_power_record_helper_75573c, helper_75573c_abi, (), (handle: u32) -> u32, (handle: u32), (handle), crate::record_auxiliary_record_75554c::helper_75573c, |value| value);
    stock_export!(
        xm1_power_record_helper_7561f8,
        helper_7561f8_abi,
        (),
        (handle: u32, value: u32) -> u32,
        (handle: u32, value: u32),
        (handle, value),
        crate::record_auxiliary_record_75554c::helper_7561f8,
        |value| value
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const SLOT: u32 = 0x0091_5d44;
    const SELECTED: u32 = 0x0091_5d40;
    const RESOURCE: u32 = 0x0096_ebd0;
    const DESTINATION: u32 = 0x0080_0000;

    struct Memory {
        slots: [u8; 16 * 0x38],
        selected: [u8; 4],
        bank0: [u8; 0x2158],
        bank1: [u8; 0x2158],
        destination: [u8; 16],
    }

    impl Memory {
        fn new() -> Self {
            let mut memory = Memory { slots: [0; 16 * 0x38], selected: [0; 4], bank0: [0; 0x2158], bank1: [0; 0x2158], destination: [0; 16] };
            memory.slots[..4].copy_from_slice(&0x123u32.to_le_bytes());
            memory.slots[8..12].copy_from_slice(&1u32.to_le_bytes());
            memory
        }

        fn readable(&mut self, field: u32) {
            self.slots[12 + field as usize] |= 1;
        }

        fn with_context<R>(&mut self, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
            let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
            let mut regions = [
                MemoryRegion::new(descriptor(SLOT, 16 * 0x38), &mut self.slots).unwrap(),
                MemoryRegion::new(descriptor(SELECTED, 4), &mut self.selected).unwrap(),
                MemoryRegion::new(descriptor(RESOURCE, 0x2158), &mut self.bank0).unwrap(),
                MemoryRegion::new(descriptor(RESOURCE + 0x2158, 0x2158), &mut self.bank1).unwrap(),
                MemoryRegion::new(descriptor(DESTINATION, 16), &mut self.destination).unwrap(),
            ];
            let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
            body(&mut RecordContext::new(space, ()))
        }

        fn put(bank: &mut [u8; 0x2158], at: u32, value: u32) { bank[at as usize..at as usize + 4].copy_from_slice(&value.to_le_bytes()); }
        fn get(bank: &[u8; 0x2158], at: u32) -> u32 { u32::from_le_bytes(bank[at as usize..at as usize + 4].try_into().unwrap()) }
    }

    fn readable_handle() -> u32 { (63 << 18) | (0x123 << 4) }

    #[test]
    fn the_readable_handle_selects_its_slot_bank() {
        let mut memory = Memory::new();
        memory.readable(READ_FIELD);
        Memory::put(&mut memory.bank0, 0x20fc, 0x1111_1111);
        Memory::put(&mut memory.bank1, 0x20fc, 0x2222_2222);
        memory.with_context(|context| assert_eq!(helper_75554c(context, readable_handle()), Ok(0x2222_2222)));
    }

    #[test]
    fn an_unreadable_handle_falls_back_to_the_selected_bank() {
        let mut memory = Memory::new();
        Memory::put(&mut memory.bank0, 0x20fc, 0x1111_1111);
        Memory::put(&mut memory.bank1, 0x20fc, 0x2222_2222);
        memory.with_context(|context| assert_eq!(helper_75554c(context, readable_handle()), Ok(0x1111_1111)));
        memory.selected.copy_from_slice(&1u32.to_le_bytes());
        memory.with_context(|context| assert_eq!(helper_75554c(context, u32::MAX), Ok(0x2222_2222)));
    }

    #[test]
    fn the_extended_reader_uses_field_thirty_one() {
        let mut memory = Memory::new();
        memory.readable(CALLBACK_FIELD);
        Memory::put(&mut memory.bank1, 0x2114, 0x00fe_1234);
        memory.with_context(|context| assert_eq!(helper_75573c(context, readable_handle()), Ok(0x00fe_1234)));
    }

    #[test]
    fn the_copy_out_takes_four_words_of_the_auxiliary_record() {
        let mut memory = Memory::new();
        memory.readable(READ_FIELD);
        for index in 0..4u32 { Memory::put(&mut memory.bank1, 0x2100 + index * 4, 0x1000 + index); }
        memory.with_context(|context| assert_eq!(helper_7555ec(context, GuestAddress::new(DESTINATION), readable_handle()), Ok(GuestAddress::new(DESTINATION))));
        assert_eq!(memory.destination, [0, 0x10, 0, 0, 1, 0x10, 0, 0, 2, 0x10, 0, 0, 3, 0x10, 0, 0]);
    }

    #[test]
    fn the_configuration_writer_uses_field_thirty_and_returns_the_record() {
        let mut memory = Memory::new();
        memory.readable(CONFIGURATION_FIELD);
        memory.with_context(|context| assert_eq!(helper_7561f8(context, readable_handle(), 0x0000_0042), Ok(RESOURCE + 0x2158 + 0x187c)));
        assert_eq!(Memory::get(&memory.bank1, 0x20ac), 0x42);
        memory.selected.copy_from_slice(&0u32.to_le_bytes());
        memory.with_context(|context| assert_eq!(helper_7561f8(context, u32::MAX, 0x99), Ok(RESOURCE + 0x187c)));
        assert_eq!(Memory::get(&memory.bank0, 0x20ac), 0x99);
    }
}
