//! X-M1 IMAGE5 selection lists, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.
//! NONMATCHING list loops checked against id5.dis f15a48..f15d58.

use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext, StackAlignment};

const LISTS: GuestAddress = GuestAddress::new(0x169e580);
const CONTEXT: GuestAddress = GuestAddress::new(0x16a049c);
const SCRATCH: GuestAddress = GuestAddress::new(0x169d50c);
const LIST_BYTES: ByteCount = ByteCount(0x414);
const ENTRY_BYTES: u32 = 0x68;


#[derive(Clone, Copy, Eq, PartialEq)]
struct SelectionIdentifier(u32);

fn offset(address: GuestAddress, bytes: u32) -> Result<GuestAddress, MemoryError> { address.checked_add(bytes) }

pub fn helper_f15a48<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, identifier: u32, list: GuestAddress) -> Result<i32, MemoryError> {
    let identifier = SelectionIdentifier(identifier);
    let count = context.read_u8(list)?;
    for index in 0..count {
        if context.read_u32(offset(list, u32::from(index) * ENTRY_BYTES + 0x60)?)? == identifier.0 { return Ok(1); }
    }
    Ok(0)
}

pub fn helper_f15ab4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, lists: GuestAddress, count: u32, identifiers: GuestAddress, found: GuestAddress) -> Result<(), MemoryError> {
    let mut written = 0u8;
    let mut index = 0u8;
    loop {
        let first = GuestAddress::new(context.read_u32(lists)?);
        if index >= context.read_u8(first)? { break; }
        let id = SelectionIdentifier(context.read_u32(offset(first, u32::from(index) * ENTRY_BYTES + 0x60)?)?);
        if count > 1 {
            let mut bank = 1u8;
            let mut shared = true;
            loop {
                let list = GuestAddress::new(context.read_u32(offset(lists, u32::from(bank) * 4)?)?);
                if helper_f15a48(context, id.0, list)? == 0 { shared = false; break; }
                bank = bank.wrapping_add(1);
                if u32::from(bank) == count { break; }
            }
            if shared {
                context.write_u32(offset(identifiers, u32::from(written) * 4)?, id.0)?;
                written = written.wrapping_add(1);
            }
        }
        index = index.wrapping_add(1);
    }
    context.write_u8(found, written)
}

pub fn helper_f15b70<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, lists: GuestAddress, count: u32, identifiers: GuestAddress, id_count: u32) -> Result<(), MemoryError> {
    if count == 0 { return Ok(()); }
    let bank_count = u32::from(count.wrapping_sub(1) as u8) + 1;
    for bank in 0..bank_count {
        for index in 0..LIST_BYTES.0 { context.write_u8(offset(SCRATCH, index)?, 0)?; }
        let mut written = 0u8;
        if id_count != 0 {
            let requested = u32::from(id_count.wrapping_sub(1) as u8) + 1;
            for index in 0..requested {
                let list = GuestAddress::new(context.read_u32(offset(lists, bank * 4)?)?);
                let entries = context.read_u32(list)?;
                if entries as u8 == 0 { continue; }
                let id = SelectionIdentifier(context.read_u32(offset(identifiers, index * 4)?)?);
                let mut source_index = 0u32;
                let last = u32::from(entries.wrapping_sub(1) as u8);
                loop {
                    if context.read_u32(offset(list, source_index * ENTRY_BYTES + 0x60)?)? == id.0 {
                        context.copy_bytes(offset(SCRATCH, 4 + u32::from(written) * ENTRY_BYTES)?, offset(list, 4 + source_index * ENTRY_BYTES)?, ByteCount(ENTRY_BYTES))?;
                        written = written.wrapping_add(1);
                        break;
                    }
                    if source_index == last { break; }
                    source_index += 1;
                }
            }
        }
        context.write_u32(SCRATCH, u32::from(written))?;
        let destination = GuestAddress::new(context.read_u32(offset(lists, bank * 4)?)?);
        context.copy_bytes(destination, SCRATCH, LIST_BYTES)?;
    }
    Ok(())
}

pub fn helper_f15cbc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, use_three: u32) -> Result<(), MemoryError> {
    context.with_guest_frame(ByteCount(72), StackAlignment::Doubleword, |context, frame| {
        let lists = frame.at(ByteCount(48), ByteCount(16))?;
        for (index, bank_offset) in [0, 0x414, 0x828, 0xc3c].into_iter().enumerate() {
            context.write_u32(offset(lists, index as u32 * 4)?, offset(LISTS, bank_offset)?.get())?;
        }
        let identifiers = frame.at(ByteCount(8), ByteCount(40))?;
        let found = frame.at(ByteCount(7), ByteCount(1))?;
        let count = if use_three == 0 { 4 } else { 3 };
        helper_f15ab4(context, lists, count, identifiers, found)?;
        let id_count = u32::from(context.read_u8(found)?);
        helper_f15b70(context, lists, count, identifiers, id_count)
    })
}

pub fn helper_f156d4<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>) -> Result<(), MemoryError> { Ok(()) }

pub fn helper_f156e8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, item: u32) -> Result<(), MemoryError> { context.write_u32(CONTEXT, item) }

macro_rules! list_reader {
    ($name:ident, $offset:expr) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<i32, MemoryError> {
            Ok(context.read_u32(offset(LISTS, $offset)?)? as i32)
        }
    };
}
macro_rules! list_writer {
    ($name:ident, $offset:expr) => {
        pub fn $name<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<(), MemoryError> {
            context.write_u32(offset(LISTS, $offset)?, value)
        }
    };
}
list_writer!(helper_f160ac, 0x1cd0);
list_reader!(helper_f160d0, 0x1cd0);
list_reader!(helper_f160f4, 0x1cd4);
list_reader!(helper_f16118, 0x1cd8);
list_reader!(helper_f1613c, 0x1cdc);
list_writer!(helper_f16160, 0x1ce0);
list_reader!(helper_f16184, 0x1ce0);
list_reader!(helper_f161a8, 0x1ce4);
list_reader!(helper_f161cc, 0x1ce8);
list_writer!(helper_f16640, 0x1ccc);
list_writer!(helper_f17af8, 0x1f18);
list_reader!(helper_f17b1c, 0x1f18);

pub fn helper_f16664<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>) -> Result<i32, MemoryError> { Ok(1) }

pub fn helper_f17a4c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<i32, MemoryError> {
    let owner = GuestAddress::new(context.read_u32(offset(CONTEXT, 4)?)?);
    if owner.get() == 0 { return Ok(0); }
    Ok(i32::from(context.read_u32(offset(owner, 4)?)? != 6))
}

pub fn helper_f17abc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress) -> Result<(), MemoryError> {
    context.copy_bytes(destination, GuestAddress::new(0x4171783c), ByteCount(0x218))
}

pub fn helper_f17ad8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, source: GuestAddress) -> Result<(), MemoryError> {
    context.copy_bytes(GuestAddress::new(0x4171783c), source, ByteCount(0x218))
}

pub fn helper_f165fc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<i32, MemoryError> {
    Ok(i32::from(context.read_u32(GuestAddress::new(0x169d060))? == 5))
}

pub fn helper_f15708<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<i32, MemoryError> {
    let owner = GuestAddress::new(context.read_u32(offset(CONTEXT, 4)?)?);
    Ok(context.read_u32(offset(owner, 0x10)?)? as i32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_address_space::{GuestDomain, GuestStack, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    fn region(base: GuestAddress, bytes: &mut [u8]) -> MemoryRegion<'_> {
        let descriptor = RegionDescriptor::new(base, ByteCount(bytes.len() as u32), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        MemoryRegion::new(descriptor, bytes).unwrap()
    }

    #[test]
    fn membership_uses_count_and_ignores_stale_entries() {
        let base = GuestAddress::new(0x1000);
        let mut storage = [0u8; 0x414];
        let mut regions = [region(base, &mut storage)];
        let memory = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let mut context = RecordContext::new(memory, ());
        context.write_u32(base, 2).unwrap();
        context.write_u32(base.checked_add(0x60).unwrap(), 19).unwrap();
        context.write_u32(base.checked_add(0xc8).unwrap(), 37).unwrap();
        context.write_u32(base.checked_add(0x130).unwrap(), 53).unwrap();
        assert_eq!(helper_f15a48(&mut context, 19, base), Ok(1));
        assert_eq!(helper_f15a48(&mut context, 37, base), Ok(1));
        assert_eq!(helper_f15a48(&mut context, 53, base), Ok(0));
        context.write_u32(base, 0).unwrap();
        assert_eq!(helper_f15a48(&mut context, 19, base), Ok(0));
    }

    #[test]
    fn intersection_preserves_first_bank_order_and_duplicates() {
        let mut banks = [0u8; 0x1050];
        let mut scratch = [0u8; 0x414];
        let mut stack = [0u8; 0x100];
        let mut regions = [region(LISTS, &mut banks), region(SCRATCH, &mut scratch), region(GuestAddress::new(0x7f00), &mut stack)];
        let memory = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let mut context = RecordContext::with_stack(memory, (), GuestStack::new(GuestAddress::new(0x7f00), ByteCount(0x100)).unwrap());
        for (bank, identifiers) in [[19, 37, 19], [19, 53, 37], [37, 19, 71], [19, 37, 89]].into_iter().enumerate() {
            let address = LISTS.checked_add(bank as u32 * 0x414).unwrap();
            context.write_u32(address, 3).unwrap();
            for (index, identifier) in identifiers.into_iter().enumerate() {
                context.write_u32(address.checked_add(index as u32 * 0x68 + 0x60).unwrap(), identifier).unwrap();
            }
        }
        helper_f15cbc(&mut context, 0).unwrap();
        for bank in 0..4 {
            let address = LISTS.checked_add(bank * 0x414).unwrap();
            assert_eq!(context.read_u32(address), Ok(3));
            assert_eq!(context.read_u32(address.checked_add(0x60).unwrap()), Ok(19));
            assert_eq!(context.read_u32(address.checked_add(0xc8).unwrap()), Ok(37));
            assert_eq!(context.read_u32(address.checked_add(0x130).unwrap()), Ok(19));
        }
    }
}
