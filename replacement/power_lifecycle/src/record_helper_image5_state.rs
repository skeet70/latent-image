//! Stock-shaped record helpers over the image-5 selection list block.
//!
//! Each body is the image-5 instruction sequence at the address in its name.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

pub const LISTS: u32 = 0x0169_e580;
pub const PROFILE_GLOBALS: u32 = 0x0094_2ab0;
pub const DISPLAY_STATE: u32 = 0x016b_5284;
pub const JOB_STATE: u32 = 0x016b_5428;

pub fn helper_f1608c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    let sample = crate::record_profile_globals::helper_76f340(context)?;
    context.write_u32(GuestAddress::new(LISTS + 0x1cc8), sample)
}

pub fn helper_f160ac<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, value: u32) -> Result<(), MemoryError> {
    crate::record_task43_selection_lists::helper_f160ac(context, value)
}

pub fn helper_f161f0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    for at in [0x1cd4, 0x1cd8, 0x1cdc] { context.write_u32(GuestAddress::new(LISTS + at), 0xff)?; }
    for at in [0x1ce4, 0x1ce8] { context.write_u32(GuestAddress::new(LISTS + at), 0)?; }
    let previous = crate::record_task43_selection_lists::helper_f160d0(context)? as u32;
    let current = crate::record_profile_globals::helper_76d9c8(context)?;
    if previous != current { context.write_u32(GuestAddress::new(LISTS + 0x1ce4), 1)?; }
    context.write_u32(GuestAddress::new(LISTS + 0x1cd4), current)
}

pub fn helper_f1fd84<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    context.write_u32(GuestAddress::new(DISPLAY_STATE + 0x18), 2)
}

pub fn helper_f21a3c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    context.read_u32(GuestAddress::new(JOB_STATE + 4))
}

#[cfg(target_arch = "arm")]
mod native {
    use super::*;
    use crate::record_helper_native_memory::InstalledMemory;

    extern "C" {
        fn xm1_power_record_helper_76f438() -> u32;
    }

    macro_rules! action_export {
        ($name:ident, $helper:ident) => {
            #[no_mangle]
            pub extern "C" fn $name() {
                let _ = super::$helper(&mut RecordContext::new(InstalledMemory, ()));
            }
        };
    }
    macro_rules! value_export {
        ($name:ident, $helper:ident) => {
            #[no_mangle]
            pub extern "C" fn $name(value: u32) {
                let _ = super::$helper(&mut RecordContext::new(InstalledMemory, ()), value);
            }
        };
    }
    macro_rules! getter_export {
        ($name:ident, $helper:ident) => {
            #[no_mangle]
            pub extern "C" fn $name() -> u32 {
                super::$helper(&mut RecordContext::new(InstalledMemory, ())).unwrap_or(0)
            }
        };
    }

    action_export!(xm1_power_record_helper_f1608c, helper_f1608c);
    value_export!(xm1_power_record_helper_f160ac, helper_f160ac);
    getter_export!(xm1_power_record_helper_f21a3c, helper_f21a3c);
    action_export!(xm1_power_record_helper_f1fd84, helper_f1fd84);

    /// The stock body discards the lens sample.
    #[no_mangle]
    pub extern "C" fn xm1_power_record_helper_f161f0() {
        unsafe { xm1_power_record_helper_76f438(); }
        let _ = super::helper_f161f0(&mut RecordContext::new(InstalledMemory, ()));
    }
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 bodies at 0x00f1608c-0x00f21a3c.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    struct Memory {
        lists: [u8; 0x2000],
        globals: [u8; 0x200],
        display: [u8; 0x40],
        job: [u8; 0x10],
    }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(LISTS, 0x2000), &mut memory.lists).unwrap(),
            MemoryRegion::new(descriptor(PROFILE_GLOBALS, 0x200), &mut memory.globals).unwrap(),
            MemoryRegion::new(descriptor(DISPLAY_STATE, 0x40), &mut memory.display).unwrap(),
            MemoryRegion::new(descriptor(JOB_STATE, 0x10), &mut memory.job).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    fn word(bytes: &[u8], at: usize) -> u32 { u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) }

    #[test]
    fn sample_and_list_writes_follow_the_stock_offsets() {
        let mut memory = Memory { lists: [0; 0x2000], globals: [0; 0x200], display: [0; 0x40], job: [0; 0x10] };
        memory.globals[0x138..0x13c].copy_from_slice(&0x1234_5678u32.to_le_bytes());
        with_memory(&mut memory, |context| {
            assert_eq!(helper_f1608c(context), Ok(()));
            assert_eq!(helper_f160ac(context, 0x9abc_def0), Ok(()));
            assert_eq!(helper_f1fd84(context), Ok(()));
        });
        assert_eq!(word(&memory.lists, 0x1cc8), 0x1234_5678);
        assert_eq!(word(&memory.lists, 0x1cd0), 0x9abc_def0);
        assert_eq!(word(&memory.display, 0x18), 2);
    }

    #[test]
    fn reset_records_the_mismatch_and_the_current_selection() {
        let mut memory = Memory { lists: [0; 0x2000], globals: [0; 0x200], display: [0; 0x40], job: [0; 0x10] };
        memory.globals[0..4].copy_from_slice(&0x8000_0000u32.to_le_bytes());
        memory.lists[0x1cd0..0x1cd4].copy_from_slice(&5u32.to_le_bytes());
        with_memory(&mut memory, |context| assert_eq!(helper_f161f0(context), Ok(())));
        assert_eq!(word(&memory.lists, 0x1cd4), 0);
        assert_eq!(word(&memory.lists, 0x1cd8), 0xff);
        assert_eq!(word(&memory.lists, 0x1cdc), 0xff);
        assert_eq!(word(&memory.lists, 0x1ce4), 1);
        assert_eq!(word(&memory.lists, 0x1ce8), 0);
    }

    #[test]
    fn reset_leaves_the_mismatch_flag_clear_when_the_selection_matches() {
        let mut memory = Memory { lists: [0; 0x2000], globals: [0; 0x200], display: [0; 0x40], job: [0; 0x10] };
        memory.globals[0..4].copy_from_slice(&0x8000_0000u32.to_le_bytes());
        with_memory(&mut memory, |context| assert_eq!(helper_f161f0(context), Ok(())));
        assert_eq!(word(&memory.lists, 0x1ce4), 0);
        assert_eq!(word(&memory.lists, 0x1cd4), 0);
    }

    #[test]
    fn job_getter_reads_the_second_word() {
        let mut memory = Memory { lists: [0; 0x2000], globals: [0; 0x200], display: [0; 0x40], job: [0; 0x10] };
        memory.job[4..8].copy_from_slice(&0xdead_beefu32.to_le_bytes());
        with_memory(&mut memory, |context| assert_eq!(helper_f21a3c(context), Ok(0xdead_beef)));
    }
}
