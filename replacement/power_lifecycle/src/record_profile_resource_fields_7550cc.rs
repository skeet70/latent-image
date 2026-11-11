//! Resource fields selected by a profile handle (image 4, 0x007550cc-0x00759180).
//!
//! Each body is the image-4 instruction sequence at the address in its name.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_profile_accessors::{helper_75a910, helper_75e1ec, helper_75e618, offset, resource_address, ProfileBank};
use crate::record_profile_aux_native::{helper_75d770, helper_75e0b0};

const RESOURCE_WORD: u32 = 0x20b0;

pub fn helper_7550cc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<u32, MemoryError> {
    let bank = if helper_75a910(context, handle, 13)? != 0 {
        helper_75e618(context, handle)?
    } else {
        helper_75e1ec(context)?
    };
    helper_75d770(context, offset(resource_address(ProfileBank(bank)), RESOURCE_WORD))
}

pub fn helper_759140<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<u32, MemoryError> {
    let bank = if helper_75a910(context, handle, 0)? != 0 {
        helper_75e618(context, handle)?
    } else {
        helper_75e1ec(context)?
    };
    helper_75e0b0(context, resource_address(ProfileBank(bank)))
}

mod exports {
    use crate::record_native_exports::stock_export;

    stock_export!(
        xm1_power_record_helper_7550cc,
        helper_7550cc_abi,
        (),
        (handle: u32) -> u32,
        (handle: u32),
        (handle),
        crate::record_profile_resource_fields_7550cc::helper_7550cc,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_759140,
        helper_759140_abi,
        (),
        (handle: u32) -> u32,
        (handle: u32),
        (handle),
        crate::record_profile_resource_fields_7550cc::helper_759140,
        |value| value
    );
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-4 bodies at 0x007550cc and 0x00759140.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const REGISTRY: u32 = 0x0091_5d40;
    const RESOURCES: u32 = 0x0096_ebd0;
    const HANDLE: u32 = (63 << 18) | (5 << 4);
    const RESOURCE_BYTES: u32 = 0x2158;

    struct Memory { registry: [u8; 0x40], resources: [u8; 0x4400] }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(REGISTRY, 0x40), &mut memory.registry).unwrap(),
            MemoryRegion::new(descriptor(RESOURCES, 0x4400), &mut memory.resources).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    fn slot(memory: &mut Memory, bank: u32, readable: &[u32]) {
        memory.registry[4..8].copy_from_slice(&5u32.to_le_bytes());
        memory.registry[0x0c..0x10].copy_from_slice(&bank.to_le_bytes());
        for field in readable { memory.registry[16 + *field as usize] = 1; }
    }

    fn selected(memory: &mut Memory, bank: u32) { memory.registry[0..4].copy_from_slice(&bank.to_le_bytes()); }

    #[test]
    fn the_word_follows_the_slot_bank_when_field_thirteen_is_readable() {
        let mut memory = Memory { registry: [0; 0x40], resources: [0; 0x4400] };
        slot(&mut memory, 1, &[13]);
        let at = (RESOURCE_BYTES + RESOURCE_WORD + 0x20) as usize;
        memory.resources[at..at + 4].copy_from_slice(&0xdead_beefu32.to_le_bytes());
        with_memory(&mut memory, |context| assert_eq!(helper_7550cc(context, HANDLE), Ok(0xdead_beef)));
    }

    #[test]
    fn the_word_follows_the_selected_bank_without_the_read_permission() {
        let mut memory = Memory { registry: [0; 0x40], resources: [0; 0x4400] };
        slot(&mut memory, 1, &[]);
        selected(&mut memory, 0);
        let at = (RESOURCE_WORD + 0x20) as usize;
        memory.resources[at..at + 4].copy_from_slice(&0x1234_5678u32.to_le_bytes());
        with_memory(&mut memory, |context| assert_eq!(helper_7550cc(context, HANDLE), Ok(0x1234_5678)));
    }

    #[test]
    fn the_header_byte_follows_the_slot_bank_when_field_zero_is_readable() {
        let mut memory = Memory { registry: [0; 0x40], resources: [0; 0x4400] };
        slot(&mut memory, 1, &[0]);
        memory.resources[(RESOURCE_BYTES + 0x59) as usize] = 0xab;
        with_memory(&mut memory, |context| assert_eq!(helper_759140(context, HANDLE), Ok(0xab)));
    }

    #[test]
    fn the_header_byte_follows_the_selected_bank_without_the_read_permission() {
        let mut memory = Memory { registry: [0; 0x40], resources: [0; 0x4400] };
        slot(&mut memory, 1, &[]);
        selected(&mut memory, 0);
        memory.resources[0x59] = 0xcd;
        with_memory(&mut memory, |context| assert_eq!(helper_759140(context, HANDLE), Ok(0xcd)));
    }
}
