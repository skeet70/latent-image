use crate::{BankExchange, HeaderBackend, PreviousValues, SettingsBackend, SettingsBank, SettingsConfiguration, SettingsStorage, StartupResavePolicy, BANK_STRIDE, BANK_WINDOW, PREVIOUS_WINDOW};
use crate::mirror::{HeaderBackupDriver, RequestStatus, BODY_BYTES, CUSTOM_BYTES, HEADER_BYTES};
use crate::native_configuration::NativeConfiguration;
use crate::native_mirror::{AbiRequestDescriptor, NativeBackend, NativeBackupDriver};
use xm1_kernel::{Kernel, SlotTable};

#[repr(u32)]
enum SetterAssignment { Unassigned = 0, Assigned = 1 }
#[repr(i32)]
enum ShortRequestStatus { Success = 0, Failed = -63 }
extern "C" {
    fn xm1_bank_property_set(id: u32, value: u32, bank: u32, selection: u32) -> SetterAssignment;
    fn xm1_bank_current_selection() -> u32;
    fn xm1_bank_short_read(request: *const AbiRequestDescriptor) -> ShortRequestStatus;
}

#[inline(always)]
unsafe fn with_storage<T>(operation: impl FnOnce(&mut SettingsStorage<'_, NativeBackend<'_, SlotTable>>) -> T) -> T {
    let kernel = unsafe { Kernel::<SlotTable>::stock() };
    let banks = unsafe { &mut *(0x96cc64 as *mut [u8; BANK_WINDOW]) };
    let previous = PreviousValues::adopt(unsafe { &mut *(0x89ec00 as *mut [u8; PREVIOUS_WINDOW]) });
    let mut storage = SettingsStorage::adopt(banks, previous, unsafe { NativeBackend::adopt(&kernel) });
    operation(&mut storage)
}

/// # Safety
/// Both pointers must cover image-sized regions; the source must be readable and the destination writable and exclusively accessible.
#[no_mangle]
pub unsafe extern "C" fn xm1_storage_copy(destination: *mut u8, source: *const u8) {
    unsafe { core::ptr::copy(source, destination, BANK_STRIDE); }
}

/// # Safety
/// The X-M1 bank, previous-value and backup windows and callbacks must be mapped and exclusively accessible during this call.
#[no_mangle]
pub unsafe extern "C" fn xm1_storage_capture() {
    unsafe { with_storage(|storage| storage.capture_active()); }
}

/// # Safety
/// The requirements of `xm1_storage_capture` apply.
#[no_mangle]
pub unsafe extern "C" fn xm1_storage_restore() {
    #[cfg(storage_defect_restore_without_body_write)]
    unsafe { with_storage(|storage| storage.restore_active_image()); }
    #[cfg(not(storage_defect_restore_without_body_write))]
    unsafe { with_storage(|storage| { storage.restore_active_body(); }); }
}

/// # Safety
/// The requirements of `xm1_storage_capture` apply.
#[no_mangle]
pub unsafe extern "C" fn xm1_storage_exchange(direction: u32) {
    let direction = match direction { 0 => BankExchange::ActiveToAlternate, 1 => BankExchange::AlternateToActive, _ => return };
    unsafe { with_storage(|storage| { storage.exchange(direction); }); }
}

/// # Safety
/// The requirements of `xm1_storage_capture` apply, and the configuration window and owned property entry must be available.
#[no_mangle]
pub unsafe extern "C" fn xm1_storage_initialize() {
    unsafe { with_storage(|storage| { storage.load_images(); }); }
    let configuration = unsafe { NativeConfiguration::adopt() };
    if !cfg!(storage_defect_skip_startup_resave) && configuration.startup_resave_policy() == StartupResavePolicy::Resave {
        unsafe { xm1_storage_apply(0x49, 3, 0); }
    }
}

/// # Safety
/// The requirements of `xm1_storage_initialize` apply.
#[no_mangle]
#[inline(never)]
pub unsafe extern "C" fn xm1_storage_apply(id: u32, value: u32, bank: u32) -> u32 {
    let selection = unsafe { xm1_bank_current_selection() };
    let active = SettingsBank::from_abi(bank) == Some(SettingsBank::Active);
    if active && !cfg!(storage_defect_body_not_refreshed) {
        unsafe { with_storage(|storage| { storage.refresh_active_body(); }); }
    }
    let assignment = unsafe { xm1_bank_property_set(id, value, bank, selection) };
    if active && (matches!(assignment, SetterAssignment::Assigned) || cfg!(storage_defect_body_write_on_unassigned)) {
        unsafe { with_storage(|storage| { storage.write_active_body(); }); }
    }
    #[cfg(storage_defect_application_appends_bank_as_area3)]
    if active && matches!(assignment, SetterAssignment::Assigned) {
        unsafe { crate::native_mirror::append_bank_as_record(0x96cc64 as *const u8); }
    }
    #[cfg(storage_defect_commit_clears_previous)]
    unsafe {
        let previous = &mut *(0x89ec00 as *mut [u8; PREVIOUS_WINDOW]);
        for byte in previous { crate::memory::write_byte(byte, 0); }
    }
    assignment as u32
}

/// # Safety
/// The requirements of `NativeBackend::adopt` apply. `buffer` must cover a writable body disjoint from the backup windows.
#[no_mangle]
pub unsafe extern "C" fn xm1_storage_read_body(buffer: *mut u8) -> i32 {
    let kernel = unsafe { Kernel::<SlotTable>::stock() };
    let mut backend = unsafe { NativeBackend::adopt(&kernel) };
    backend.read_body(unsafe { &mut *buffer.cast::<[u8; BODY_BYTES]>() }).mapped.legacy_code()
}

/// # Safety
/// The requirements of `NativeBackend::adopt` apply. `buffer` must cover a readable body disjoint from the backup windows.
#[no_mangle]
pub unsafe extern "C" fn xm1_storage_write_body(buffer: *const u8) -> i32 {
    let kernel = unsafe { Kernel::<SlotTable>::stock() };
    let mut backend = unsafe { NativeBackend::adopt(&kernel) };
    backend.write_body(unsafe { &*buffer.cast::<[u8; BODY_BYTES]>() }).request.mapped.legacy_code()
}

/// # Safety
/// The requirements of `xm1_storage_capture` apply.
#[no_mangle]
pub unsafe extern "C" fn xm1_storage_write_active_body() -> i32 {
    unsafe { with_storage(|storage| storage.write_active_body().request.mapped.legacy_code()) }
}

/// # Safety
/// The requirements of `NativeBackend::adopt` apply. `buffer` must cover a writable header disjoint from the backup windows.
#[no_mangle]
pub unsafe extern "C" fn xm1_storage_read_header(buffer: *mut u8) -> i32 {
    let kernel = unsafe { Kernel::<SlotTable>::stock() };
    let mut backend = unsafe { NativeBackend::adopt(&kernel) };
    backend.read_header(unsafe { &mut *buffer.cast::<[u8; HEADER_BYTES]>() }).mapped.legacy_code()
}

/// # Safety
/// The requirements of `NativeBackend::adopt` apply. `buffer` must cover a readable header disjoint from the backup windows.
#[no_mangle]
pub unsafe extern "C" fn xm1_storage_write_header(buffer: *const u8) -> i32 {
    let kernel = unsafe { Kernel::<SlotTable>::stock() };
    let mut backend = unsafe { NativeBackend::adopt(&kernel) };
    backend.write_header(unsafe { &*buffer.cast::<[u8; HEADER_BYTES]>() }).request.mapped.legacy_code()
}

/// # Safety
/// The requirements of `NativeBackend::adopt` apply. The retained short-request dispatcher must be mapped, and `buffer` must cover a writable custom block disjoint from the backup windows.
#[no_mangle]
pub unsafe extern "C" fn xm1_storage_read_custom(buffer: *mut u8, index: u32) -> i32 {
    let request = AbiRequestDescriptor { buffer: buffer as u32, length: CUSTOM_BYTES as u32, offset: index.wrapping_mul(CUSTOM_BYTES as u32).wrapping_add(0x140) };
    let mapped = match unsafe { xm1_bank_short_read(&request) } { ShortRequestStatus::Success => RequestStatus::Success, ShortRequestStatus::Failed => RequestStatus::Failed };
    let kernel = unsafe { Kernel::<SlotTable>::stock() };
    let mut driver = NativeBackupDriver::new(&kernel);
    driver.store_request_status(mapped);
    if mapped == RequestStatus::Failed { driver.report_failure(mapped); }
    mapped.legacy_code()
}
