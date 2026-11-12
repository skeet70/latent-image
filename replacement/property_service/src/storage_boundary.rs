use super::storage::{self, SettingsStorage, PreviousValues};
use xm1_kernel::{Kernel, SlotTable};

pub(crate) unsafe fn with_storage<'k, T>(
    kernel: &'k Kernel<SlotTable>,
    operation: impl FnOnce(&mut SettingsStorage<'_, storage::native_mirror::NativeHeaderBackend<'k, SlotTable>>) -> T,
) -> T {
    let banks = unsafe { &mut *(0x96cc64 as *mut [u8; storage::BANK_WINDOW]) };
    let previous = PreviousValues::adopt(unsafe { &mut *(0x89ec00 as *mut [u8; storage::PREVIOUS_WINDOW]) });
    let mut storage = SettingsStorage::adopt(banks, previous, unsafe { storage::native_mirror::NativeHeaderBackend::adopt(kernel) });
    operation(&mut storage)
}
