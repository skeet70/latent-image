//! X-M1 1.50: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; id5 f3aa94..f3af14.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_profile_accessors as profile;
use crate::record_profile_slotfields as slot;

pub(super) fn resource<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    context.read_u32(GuestAddress::new(0x016c0718))
}

fn pending<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, low: u32, high: u32) -> Result<u32, MemoryError> {
    let handle = resource(context)?;
    profile::helper_74e020(context, handle, 0, low, high)
}

fn alternate_pending<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, low: u32, high: u32) -> Result<u32, MemoryError> {
    let handle = context.read_u32(GuestAddress::new(0x016be01c))?;
    profile::helper_74e020(context, handle, 0, low, high)
}

pub fn helper_f3aa94<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, event: u32) -> Result<(), MemoryError> {
    context.write_u32(GuestAddress::new(0x016c0714), event)
}

pub fn helper_f3aab4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<i32, MemoryError> {
    if value != 13 { return Ok(0); }
    let a = pending(context, 0, 0x04000000)?;
    let b = pending(context, 0x100, 0)?;
    let c = pending(context, 0x400, 0)?;
    let d = pending(context, 0x40, 0)?;
    let e = pending(context, 0x80, 0)?;
    let f = pending(context, 0x40000000, 0)?;
    let g = pending(context, 0x80000000, 0)?;
    let h = pending(context, 0, 0x20)?;
    let i = pending(context, 0, 0x40)?;
    let j = pending(context, 2, 0)?;
    let k = alternate_pending(context, 0, 0x80)?;
    let l = alternate_pending(context, 0x800, 0)?;
    let m = alternate_pending(context, 1, 0x80000000)?;
    let n = alternate_pending(context, 0x100, 0x80000000)?;
    let o = alternate_pending(context, 0x04000000, 0)?;
    let p = alternate_pending(context, 0x80000, 0x80000000)?;
    let mut flags = u8::from((a | b | c | f | g | l | p) != 0);
    let mut result = i32::from(flags);
    context.write_u8(GuestAddress::new(0x016c071c), flags)?;
    let handle = resource(context)?;
    if profile::helper_74e5c0(context, handle)? == 1 {
        if d != 0 { flags |= 2; result = 2; context.write_u8(GuestAddress::new(0x016c071c), flags)?; }
        if h != 0 { flags |= 8; result = 4; context.write_u8(GuestAddress::new(0x016c071c), flags)?; }
    } else {
        if e != 0 { flags |= 2; result = 2; context.write_u8(GuestAddress::new(0x016c071c), flags)?; }
        for pending in [i, j, k, m, n, o] {
            if pending != 0 { flags |= 8; result = 4; context.write_u8(GuestAddress::new(0x016c071c), flags)?; }
        }
    }
    Ok(result)
}

pub fn helper_f3adbc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, query: u32) -> Result<i32, MemoryError> {
    if query == 0 { return Ok(0); }
    let shift = query.wrapping_sub(1) & 0xff;
    let clear = if shift < 32 { 1u32 << shift } else { 0 };
    let flags = u32::from(context.read_u8(GuestAddress::new(0x016c071c))?) & !clear;
    context.write_u8(GuestAddress::new(0x016c071c), flags as u8)?;
    Ok(if flags & 1 != 0 { 1 } else if flags & 2 != 0 { 2 } else if flags & 8 != 0 { 4 } else { 0 })
}

pub fn helper_f3ae2c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    let handle = resource(context)?;
    let value = context.read_u32(GuestAddress::new(0x0169d384))?;
    slot::helper_7527ac(context, handle, value)
}

pub fn helper_f3ae54<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<i32, MemoryError> {
    let handle = resource(context)?;
    let mode = slot::helper_754d9c(context, handle)?;
    let handle = resource(context)?;
    let active = slot::helper_754f8c(context, handle)?;
    let handle = context.read_u32(GuestAddress::new(0x016be01c))?;
    let alternate = profile::helper_7509c4(context, handle)?;
    Ok(i32::from((mode != 0 && mode != 3) || active != 0 || (mode == 3 && alternate == 0)))
}

pub fn helper_f3aec0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<i32, MemoryError> {
    let handle = resource(context)?;
    Ok(profile::helper_74e5c0(context, handle)? as i32)
}

pub fn helper_f3aedc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<i32, MemoryError> {
    Ok(i32::from(context.read_u32(GuestAddress::new(0x0169d2d8))? == 0))
}

pub fn helper_f3aef8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    let handle = resource(context)?;
    profile::helper_7501cc(context, handle)
}

pub fn helper_f3c0bc<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>) -> Result<(), MemoryError> { Ok(()) }

pub fn helper_f3c0d0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, request: u32) -> Result<(), MemoryError> {
    context.write_u32(GuestAddress::new(0x016c0b3c), request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    fn descriptor(base: u32, size: u32) -> RegionDescriptor {
        RegionDescriptor::new(GuestAddress::new(base), ByteCount(size), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap()
    }

    #[test]
    fn event_publication_preserves_neighboring_resource_and_flags() {
        let mut bytes = [0x71; 20];
        let mut regions = [MemoryRegion::new(descriptor(0x016c0714, 20), &mut bytes).unwrap()];
        let mut context = RecordContext::new(HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap(), ());
        helper_f3aa94(&mut context, 0xb1).unwrap();
        assert_eq!(context.read_u32(GuestAddress::new(0x016c0714)), Ok(0xb1));
        assert_eq!(context.read_u32(GuestAddress::new(0x016c0718)), Ok(0x71717171));
        assert_eq!(context.read_u8(GuestAddress::new(0x016c071c)), Ok(0x71));
    }

    #[test]
    fn consumption_removes_only_requested_flag_and_returns_remaining_priority() {
        let mut bytes = [0u8; 20];
        bytes[8] = 11;
        let mut regions = [MemoryRegion::new(descriptor(0x016c0714, 20), &mut bytes).unwrap()];
        let mut context = RecordContext::new(HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap(), ());
        assert_eq!(helper_f3adbc(&mut context, 0), Ok(0));
        assert_eq!(context.read_u8(GuestAddress::new(0x016c071c)), Ok(11));
        assert_eq!(helper_f3adbc(&mut context, 1), Ok(2));
        assert_eq!(context.read_u8(GuestAddress::new(0x016c071c)), Ok(10));
        assert_eq!(helper_f3adbc(&mut context, 2), Ok(4));
        assert_eq!(context.read_u8(GuestAddress::new(0x016c071c)), Ok(8));
        assert_eq!(helper_f3adbc(&mut context, 4), Ok(0));
        assert_eq!(context.read_u8(GuestAddress::new(0x016c071c)), Ok(0));
    }

    #[test]
    fn incomplete_selection_query_distinguishes_zero_from_nonzero() {
        let mut bytes = [0u8; 8];
        let mut regions = [MemoryRegion::new(descriptor(0x0169d2d4, 8), &mut bytes).unwrap()];
        let mut context = RecordContext::new(HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap(), ());
        assert_eq!(helper_f3aedc(&mut context), Ok(1));
        context.write_u32(GuestAddress::new(0x0169d2d8), 1).unwrap();
        assert_eq!(helper_f3aedc(&mut context), Ok(0));
        context.write_u32(GuestAddress::new(0x0169d2d8), u32::MAX).unwrap();
        assert_eq!(helper_f3aedc(&mut context), Ok(0));
    }

    #[test]
    fn publication_propagates_unmapped_memory_error() {
        let mut regions = [];
        let mut context = RecordContext::new(HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap(), ());
        assert_eq!(helper_f3aa94(&mut context, 10), Err(MemoryError::Unmapped(GuestAddress::new(0x016c0714))));
    }
}
