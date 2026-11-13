use super::storage::{self, SettingsStorage, PreviousValues};
use xm1_kernel::{Kernel, SlotTable};

extern "C" {
    fn xm1_bank_current_selection() -> u32;
}

pub(crate) unsafe fn with_storage<'k, T>(
    kernel: &'k Kernel<SlotTable>,
    operation: impl FnOnce(&mut SettingsStorage<'_, storage::native_mirror::NativeStorageBackend<'k, SlotTable>>) -> T,
) -> T {
    let banks = unsafe { &mut *(0x96cc64 as *mut [u8; storage::BANK_WINDOW]) };
    let previous = PreviousValues::adopt(unsafe { &mut *(0x89ec00 as *mut [u8; storage::PREVIOUS_WINDOW]) });
    let mut storage = SettingsStorage::adopt(banks, previous, unsafe { storage::native_mirror::NativeStorageBackend::adopt(kernel) });
    operation(&mut storage)
}

/// `0x6439e0`: the slot the retained read and write wrappers pass to the service.
pub(crate) fn current_selection() -> storage::ProfileSelection {
    storage::ProfileSelection::from_abi(unsafe { xm1_bank_current_selection() }).unwrap_or(storage::ProfileSelection::Current)
}
