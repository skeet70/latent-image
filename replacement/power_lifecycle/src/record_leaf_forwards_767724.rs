//! Leaf forwards and the selection-word byte comparison, image-4 entries 0x00767724-0x00f2e2e0.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

pub const SELECTION_BLOCK: u32 = 0x016b_7864;
const SELECTION_BYTE: u32 = 0x62;

pub fn helper_767724<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    let enabled = crate::record_profile_fields_75cbd4::helper_7718d8(context)?;
    if enabled == 0 { return Ok(0); }
    crate::record_profile_fields_75cbd4::helper_76a1d8(context, enabled)?;
    crate::record_profile_fields_75cbd4::helper_76a244(context, enabled)?;
    crate::record_profile_fields_75cbd4::helper_76a2c8(context, enabled)?;
    Ok(enabled)
}

pub fn helper_76dbc4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    crate::record_profile_fields_75cbd4::helper_76a328(context)
}

pub fn helper_fa7604<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    helper_76dbc4(context)
}

pub fn helper_f2e2e0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    let limit = crate::record_profile_fields_75cbd4::helper_ef6844(context)?;
    let value = context.read_u8(GuestAddress::new(SELECTION_BLOCK + SELECTION_BYTE))?;
    Ok(u32::from(u32::from(value) >= limit))
}

pub fn helper_f1af38<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, row: u32, column: u32, source: GuestAddress, layer: u32) -> Result<GuestAddress, MemoryError> {
    let third = u32::from(context.read_u8(source)?);
    let fourth = u32::from(context.read_u8(source.checked_add(1)?)?);
    crate::record_selection_buckets_f1a3cc::helper_f1ae20(context, row, column, third, fourth, layer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const SOURCE: u32 = 0x0080_0000;

    struct Memory {
        selection: [u8; 0x70],
        globals: [u8; 0x100],
        source: [u8; 2],
    }

    impl Memory {
        fn with_context<R>(&mut self, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
            let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
            let mut regions = [
                MemoryRegion::new(descriptor(SELECTION_BLOCK, 0x70), &mut self.selection).unwrap(),
                MemoryRegion::new(descriptor(0x0094_2ab0, 0x100), &mut self.globals).unwrap(),
                MemoryRegion::new(descriptor(SOURCE, 2), &mut self.source).unwrap(),
            ];
            let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
            body(&mut RecordContext::new(space, ()))
        }
    }

    fn memory() -> Memory { Memory { selection: [0; 0x70], globals: [0; 0x100], source: [0; 2] } }

    #[test]
    fn the_enabled_forward_runs_the_three_passthroughs() {
        let mut memory = memory();
        memory.with_context(|context| assert_eq!(helper_767724(context), Ok(1)));
    }

    #[test]
    fn the_selection_leaf_chain_reads_the_global_word() {
        let mut memory = memory();
        memory.globals[0xd8..0xdc].copy_from_slice(&0x0000_002a_u32.to_le_bytes());
        memory.with_context(|context| {
            assert_eq!(helper_76dbc4(context), Ok(0x2a));
            assert_eq!(helper_fa7604(context), Ok(0x2a));
        });
    }

    #[test]
    fn the_byte_comparison_uses_the_selection_limit() {
        let mut memory = memory();
        memory.with_context(|context| assert_eq!(helper_f2e2e0(context), Ok(0)));
        memory.selection[0x62] = 1;
        memory.with_context(|context| assert_eq!(helper_f2e2e0(context), Ok(1)));
        memory.selection[0x62] = 9;
        memory.with_context(|context| assert_eq!(helper_f2e2e0(context), Ok(1)));
    }

    #[test]
    fn the_index_table_reads_the_two_source_bytes() {
        let mut memory = memory();
        memory.source = [3, 1];
        memory.with_context(|context| {
            assert_eq!(helper_f1af38(context, 6, 3, GuestAddress::new(SOURCE), 0), Ok(GuestAddress::new(0x016a_d7a0 + 0xfc + 2 * 0x2f4)));
        });
    }
}
